//! Item-3 owners for the ordinary non-ThinLTO pipelined package driver.
//!
//! These tests assert artifacts, cache states, ordering, and failure shape rather than elapsed time.
//! The separate local benchmark owns the measured overlap/wall-time promise.

use align_driver::{
    build_package_pipelined, build_per_unit, codegen_units_parallel, ArtifactStage, BuildTarget,
    CacheContext, PackageCodegenError, PgoMode, PipelinedPackageBuild,
    PipelinedPackageComplete, Profile, UnitReuse,
};
use align_span::SourceMap;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};

static NONCE: AtomicU64 = AtomicU64::new(0);

struct Project {
    root: PathBuf,
    cache: PathBuf,
}

impl Project {
    fn new(tag: &str) -> Project {
        let nonce = NONCE.fetch_add(1, Ordering::Relaxed);
        let root = std::env::temp_dir().join(format!(
            "align-pipeline-{}-{nonce}-{tag}",
            std::process::id()
        ));
        std::fs::create_dir_all(&root).expect("create project");
        std::fs::write(
            root.join("leaf.align"),
            "module leaf\npub fn value() -> i64 = 20\n",
        )
        .expect("write leaf");
        std::fs::write(
            root.join("mid.align"),
            "module mid\nimport leaf\npub fn value() -> i64 = leaf.value() + 1\n",
        )
        .expect("write mid");
        std::fs::write(
            root.join("main.align"),
            "import mid\nfn main() {\n  print(mid.value() * 2)\n}\n",
        )
        .expect("write entry");
        let cache = root.join("cache");
        Project { root, cache }
    }

    fn entry(&self) -> PathBuf {
        self.root.join("main.align")
    }

    fn source(&self) -> String {
        std::fs::read_to_string(self.entry()).expect("read entry")
    }

    fn pipeline(&self, cache: CacheContext, reuse: UnitReuse, jobs: usize) -> PipelinedPackageBuild {
        let mut source_map = SourceMap::new();
        build_package_pipelined(
            &mut source_map,
            self.entry().to_str().expect("utf-8 entry"),
            &self.source(),
            cache,
            reuse,
            &BuildTarget::Baseline,
            Profile::Dev,
            false,
            jobs,
            &PgoMode::Off,
        )
    }
}

impl Drop for Project {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.root);
    }
}

fn complete(result: PipelinedPackageBuild) -> PipelinedPackageComplete {
    match result {
        PipelinedPackageBuild::Complete(build) => build,
        PipelinedPackageBuild::FrontendFailed { diags } => {
            panic!("unexpected frontend failure: {} diagnostic(s)", diags.len())
        }
        PipelinedPackageBuild::CodegenFailed { error, .. } => {
            panic!("unexpected codegen failure: {error}")
        }
    }
}

fn bytes(paths: impl Iterator<Item = PathBuf>) -> Vec<Vec<u8>> {
    paths
        .map(|path| std::fs::read(path).expect("read object"))
        .collect()
}

#[test]
fn pipeline_is_byte_identical_to_two_phase_build() {
    if !align_driver::backend_available() {
        return;
    }
    let project = Project::new("parity");
    let source = project.source();
    let entry = project.entry();

    let mut legacy_map = SourceMap::new();
    let legacy = build_per_unit(
        &mut legacy_map,
        entry.to_str().expect("utf-8 entry"),
        &source,
    );
    assert!(!legacy.diags.has_errors());
    let legacy_stage = ArtifactStage::temp("align-pipeline-legacy").expect("legacy stage");
    let legacy_paths: Vec<PathBuf> = (0..legacy.units.len())
        .map(|index| legacy_stage.path().join(format!("unit{index}.o")))
        .collect();
    let legacy_codegen = codegen_units_parallel(
        &legacy.units,
        &legacy_paths,
        &CacheContext::Disabled,
        &BuildTarget::Baseline,
        Profile::Dev,
        false,
        3,
        &PgoMode::Off,
    )
    .expect("legacy codegen");

    let pipelined = complete(project.pipeline(CacheContext::Disabled, UnitReuse::Forbidden, 3));
    assert_eq!(
        legacy.units.iter().map(|unit| unit.unit.as_str()).collect::<Vec<_>>(),
        pipelined.units.iter().map(|unit| unit.unit.as_str()).collect::<Vec<_>>()
    );
    assert_eq!(legacy_codegen.outcomes, pipelined.codegen.outcomes);
    assert_eq!(
        bytes(legacy_paths.into_iter()),
        bytes(pipelined.units.iter().map(|unit| unit.object().to_path_buf())),
        "scheduling must not change per-unit object bytes"
    );
}

#[test]
fn cache_product_selects_exact_work() {
    if !align_driver::backend_available() {
        return;
    }
    let project = Project::new("cache-product");

    let cold = complete(project.pipeline(
        CacheContext::at(project.cache.clone()),
        UnitReuse::Allowed,
        3,
    ));
    assert!(cold.units.iter().all(|unit| unit.frontend.as_ref().is_some_and(|outcome| !outcome.hit)));
    assert!(cold.codegen.outcomes.iter().all(|outcome| !outcome.hit));
    drop(cold);

    let all_hit = complete(project.pipeline(
        CacheContext::at(project.cache.clone()),
        UnitReuse::Allowed,
        3,
    ));
    assert!(all_hit.units.iter().all(|unit| unit.frontend.as_ref().is_some_and(|outcome| outcome.hit)));
    assert!(all_hit.codegen.outcomes.iter().all(|outcome| outcome.hit));
    drop(all_hit);

    // Preserve frontend entries while removing the object namespace. This is the one product cell
    // that requires serial rehydration before a miss can enter codegen.
    let _ = std::fs::remove_dir_all(project.cache.join("actions/codegen"));
    let _ = std::fs::remove_dir_all(project.cache.join("index/codegen"));
    let _ = std::fs::remove_dir_all(project.cache.join("cas"));
    let rehydrated = complete(project.pipeline(
        CacheContext::at(project.cache.clone()),
        UnitReuse::Allowed,
        3,
    ));
    assert!(rehydrated.units.iter().all(|unit| unit.frontend.as_ref().is_some_and(|outcome| outcome.hit)));
    assert!(rehydrated.codegen.outcomes.iter().all(|outcome| !outcome.hit));
    assert!(rehydrated.units.iter().all(|unit| unit.object().is_file()));
}

#[test]
fn concurrent_pipelines_own_distinct_stages() {
    if !align_driver::backend_available() {
        return;
    }
    let project = Project::new("stage-owner");
    let entry = project.entry();
    let source = project.source();
    let build = move || {
        let mut source_map = SourceMap::new();
        complete(build_package_pipelined(
            &mut source_map,
            entry.to_str().expect("utf-8 entry"),
            &source,
            CacheContext::Disabled,
            UnitReuse::Forbidden,
            &BuildTarget::Baseline,
            Profile::Dev,
            false,
            2,
            &PgoMode::Off,
        ))
    };
    let left = std::thread::spawn(build);

    let entry = project.entry();
    let source = project.source();
    let right = std::thread::spawn(move || {
        let mut source_map = SourceMap::new();
        complete(build_package_pipelined(
            &mut source_map,
            entry.to_str().expect("utf-8 entry"),
            &source,
            CacheContext::Disabled,
            UnitReuse::Forbidden,
            &BuildTarget::Baseline,
            Profile::Dev,
            false,
            2,
            &PgoMode::Off,
        ))
    });
    let left = left.join().expect("left pipeline");
    let right = right.join().expect("right pipeline");
    let left_stage = left.units[0].object().parent().expect("left stage").to_path_buf();
    let right_stage = right.units[0].object().parent().expect("right stage").to_path_buf();
    assert_ne!(left_stage, right_stage);
    assert!(left.units.iter().all(|unit| unit.object().is_file()));
    assert!(right.units.iter().all(|unit| unit.object().is_file()));
    drop(left);
    assert!(!left_stage.exists(), "a complete record owns and removes only its stage");
    assert!(right_stage.exists(), "dropping a sibling must not remove this stage");
    drop(right);
    assert!(!right_stage.exists());
}

#[test]
fn frontend_diagnostics_precede_shallow_setup_failure() {
    let project = Project::new("failure-order");
    let bad_profile = project.root.join("bad.profdata");
    std::fs::write(&bad_profile, b"not a profile").expect("write bad profile");
    std::fs::write(project.entry(), "fn main( {\n").expect("write invalid entry");
    let mut source_map = SourceMap::new();
    let result = build_package_pipelined(
        &mut source_map,
        project.entry().to_str().expect("utf-8 entry"),
        &project.source(),
        CacheContext::Disabled,
        UnitReuse::Allowed,
        &BuildTarget::Baseline,
        Profile::Dev,
        false,
        4,
        &PgoMode::Use(bad_profile),
    );
    assert!(matches!(result, PipelinedPackageBuild::FrontendFailed { .. }));
}

#[test]
fn shallow_profile_failure_is_codegen_failure_with_no_paths() {
    let project = Project::new("pgo-shallow");
    let bad_profile = project.root.join("bad.profdata");
    std::fs::write(&bad_profile, b"not a profile").expect("write bad profile");
    let mut source_map = SourceMap::new();
    let result = build_package_pipelined(
        &mut source_map,
        project.entry().to_str().expect("utf-8 entry"),
        &project.source(),
        CacheContext::Disabled,
        UnitReuse::Allowed,
        &BuildTarget::Baseline,
        Profile::Dev,
        false,
        4,
        &PgoMode::Use(bad_profile),
    );
    match result {
        PipelinedPackageBuild::CodegenFailed {
            error: PackageCodegenError::Failed(message),
            ..
        } => assert!(message.contains("bad magic"), "unexpected error: {message}"),
        PipelinedPackageBuild::CodegenFailed { error, .. } => {
            panic!("unexpected typed error: {error}")
        }
        PipelinedPackageBuild::FrontendFailed { .. } => panic!("frontend was valid"),
        PipelinedPackageBuild::Complete(_) => panic!("bad profile unexpectedly completed"),
    }
}

#[test]
fn excluded_verbs_keep_their_existing_driver() {
    // The public pipeline has no ThinLTO parameter. Keep a source-level guard so adding one cannot
    // silently absorb the separately scheduled ThinLTO path.
    let source = include_str!("../src/lib.rs");
    let signature = &source[source.find("pub fn build_package_pipelined").expect("pipeline fn")..];
    let signature = &signature[..signature.find(") -> PipelinedPackageBuild").expect("return type")];
    assert!(!signature.contains("thin_lto"));
}

#[test]
fn only_complete_results_lend_objects() {
    if !align_driver::backend_available() {
        return;
    }
    let project = Project::new("object-lifetime");
    let build = complete(project.pipeline(CacheContext::Disabled, UnitReuse::Forbidden, 1));
    let paths = build
        .units
        .iter()
        .map(|unit| unit.object().to_path_buf())
        .collect::<Vec<_>>();
    assert!(paths.iter().all(|path| path.is_file()));
    let parent = paths[0].parent().expect("stage parent").to_path_buf();
    assert!(paths.iter().all(|path| path.parent() == Some(parent.as_path())));
    drop(build);
    assert!(!parent.exists());
}

#[test]
fn object_publication_waits_for_validation_commit() {
    if !align_driver::backend_available() {
        return;
    }
    let project = Project::new("publication-commit");
    // The dependency is clean and becomes ready, but the entry fails after it. A speculative object
    // may be produced privately; no codegen action or CAS blob may become persistent.
    std::fs::write(
        project.entry(),
        "import mid\nfn main() {\n  missing_name()\n}\n",
    )
    .expect("invalid entry");
    let result = project.pipeline(
        CacheContext::at(project.cache.clone()),
        UnitReuse::Allowed,
        4,
    );
    assert!(matches!(result, PipelinedPackageBuild::FrontendFailed { .. }));
    assert!(
        !project.cache.join("actions/codegen").exists(),
        "a frontend failure must publish no speculative object action"
    );
    assert!(
        !project.cache.join("index/codegen").exists(),
        "a frontend failure must publish no speculative object slot"
    );
}
