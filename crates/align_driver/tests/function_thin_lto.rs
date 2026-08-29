//! Function-partition ThinLTO owner: formation, cache frontier, sealed completion, and support.

mod common;
use common::*;

const V1: &str = "\
fn leaf(n: i64) -> i64 = n + 1
fn sibling(n: i64) -> i64 = n * 2
fn main() -> i32 { print(leaf(41) + sibling(2)); return 0 }
";

const V2: &str = "\
fn leaf(n: i64) -> i64 = n + 2
fn sibling(n: i64) -> i64 = n * 2
fn main() -> i32 { print(leaf(41) + sibling(2)); return 0 }
";

const V3: &str = "\
fn leaf(n: i64) -> i64 = n + 1
fn sibling(n: i64) -> i64 = n * 3
fn main() -> i32 { print(leaf(41) + sibling(2)); return 0 }
";

fn walk(proj: &Proj) -> PerUnitWalk {
    let entry = proj.dir.join(&proj.entry);
    let source = std::fs::read_to_string(&entry).expect("read entry");
    let mut source_map = SourceMap::new();
    let walk = build_per_unit(&mut source_map, &entry.display().to_string(), &source);
    assert!(
        !walk.diags.has_errors(),
        "unexpected build errors:\n{}",
        align_driver::format_diagnostics(&source_map, &walk.diags)
    );
    walk
}

fn function_prelinks(
    observations: &[FunctionThinLtoObservation],
) -> std::collections::BTreeMap<String, bool> {
    observations
        .iter()
        .filter_map(|observation| match observation {
            FunctionThinLtoObservation::Partitioned {
                source:
                    ThinPartitionSource {
                        partition: PartitionKey::Function(function),
                        ..
                    },
                prelink,
                ..
            } => Some((function.as_str().to_owned(), prelink.hit)),
            _ => None,
        })
        .collect()
}

fn function_backends(
    observations: &[FunctionThinLtoObservation],
) -> std::collections::BTreeMap<String, bool> {
    observations
        .iter()
        .filter_map(|observation| match observation {
            FunctionThinLtoObservation::Partitioned {
                source:
                    ThinPartitionSource {
                        partition: PartitionKey::Function(function),
                        ..
                    },
                backend,
                ..
            } => Some((function.as_str().to_owned(), backend.hit)),
            _ => None,
        })
        .collect()
}

#[test]
fn function_partition_cache_edit_revert_and_completion() {
    if !backend_available() {
        return;
    }
    let proj = Proj::new("function-cache", &[("main.align", V1)], "main.align");
    let cache = proj.cache();

    let cold_walk = walk(&proj);
    let cold = build_function_thin_lto(
        &cold_walk.units,
        &cache,
        &BuildTarget::Baseline,
        Profile::Release,
        &[],
        false,
        2,
    )
    .expect("cold function ThinLTO build");
    assert_eq!(cold.mode(), FunctionThinLtoMode::Partitioned);
    assert_eq!(
        function_prelinks(cold.observations()),
        [
            ("leaf".to_owned(), false),
            ("main".to_owned(), false),
            ("sibling".to_owned(), false)
        ]
        .into_iter()
        .collect()
    );
    let executable = proj.dir.join("function-thin");
    cold.link_and_publish(&executable).expect("link cold build");
    let output = std::process::Command::new(&executable)
        .output()
        .expect("run cold build");
    assert!(output.status.success());
    assert_eq!(output.stdout, b"46\n");

    let hot_walk = walk(&proj);
    let hot = build_function_thin_lto(
        &hot_walk.units,
        &cache,
        &BuildTarget::Baseline,
        Profile::Release,
        &[],
        false,
        0,
    )
    .expect("hot function ThinLTO build");
    assert!(
        function_prelinks(hot.observations())
            .values()
            .all(|hit| *hit)
    );
    drop(hot);

    proj.write("main.align", V2);
    let edited_walk = walk(&proj);
    let edited = build_function_thin_lto(
        &edited_walk.units,
        &cache,
        &BuildTarget::Baseline,
        Profile::Release,
        &[],
        false,
        2,
    )
    .expect("edited function ThinLTO build");
    assert_eq!(
        function_prelinks(edited.observations()),
        [
            ("leaf".to_owned(), false),
            ("main".to_owned(), true),
            ("sibling".to_owned(), true)
        ]
        .into_iter()
        .collect()
    );
    assert_eq!(
        function_backends(edited.observations()),
        [
            ("leaf".to_owned(), false),
            ("main".to_owned(), false),
            ("sibling".to_owned(), true)
        ]
        .into_iter()
        .collect(),
        "the caller backend must bind the edited leaf partition digest"
    );
    drop(edited);

    proj.write("main.align", V1);
    let reverted_walk = walk(&proj);
    let reverted = build_function_thin_lto(
        &reverted_walk.units,
        &cache,
        &BuildTarget::Baseline,
        Profile::Release,
        &[],
        false,
        1,
    )
    .expect("reverted function ThinLTO build");
    assert!(
        function_prelinks(reverted.observations())
            .values()
            .all(|hit| *hit)
    );
    assert!(
        function_backends(reverted.observations())
            .values()
            .all(|hit| *hit)
    );
    drop(reverted);

    proj.write("main.align", V3);
    let sibling_walk = walk(&proj);
    let sibling = build_function_thin_lto(
        &sibling_walk.units,
        &cache,
        &BuildTarget::Baseline,
        Profile::Release,
        &[],
        false,
        2,
    )
    .expect("sibling-edited function ThinLTO build");
    assert_eq!(
        function_prelinks(sibling.observations()),
        [
            ("leaf".to_owned(), true),
            ("main".to_owned(), true),
            ("sibling".to_owned(), false)
        ]
        .into_iter()
        .collect()
    );
    assert_eq!(
        function_backends(sibling.observations()),
        [
            ("leaf".to_owned(), true),
            ("main".to_owned(), false),
            ("sibling".to_owned(), false)
        ]
        .into_iter()
        .collect(),
        "the caller backend must bind the independently edited sibling partition digest"
    );
}

#[test]
fn whole_unit_and_unknown_export_are_fail_closed() {
    let proj = Proj::new(
        "function-whole",
        &[("main.align", "fn main() -> i32 { return 0 }\n")],
        "main.align",
    );
    let walk = walk(&proj);
    let build = build_function_thin_lto(
        &walk.units,
        &CacheContext::Disabled,
        &BuildTarget::Baseline,
        Profile::Release,
        &[],
        false,
        1,
    )
    .expect("whole-unit build");
    assert_eq!(build.mode(), FunctionThinLtoMode::WholeUnit);
    assert!(matches!(
        build.observations(),
        [FunctionThinLtoObservation::WholeUnit {
            source: ThinPartitionSource {
                partition: PartitionKey::WholeUnit,
                ..
            },
            codegen,
        }] if codegen.stage == CacheStage::Codegen
    ));

    let error = match function_partitions(&walk.units, &["missing".to_owned()]) {
        Ok(_) => panic!("unknown export unexpectedly formed partitions"),
        Err(error) => error,
    };
    assert_eq!(error, "unknown ThinLTO export roots:1:7:6d697373696e67");
}

#[test]
fn wrapped_and_direct_main_symbols_conflict_during_formation() -> Result<(), String> {
    let proj = Proj::new(
        "function-duplicate-main",
        &[
            ("dep.align", "module dep\npub fn helper() {}\n"),
            (
                "main.align",
                "module main\nimport dep\nfn main() -> i32 { return 0 }\n",
            ),
        ],
        "main.align",
    );
    let mut built = walk(&proj);
    let dependency = built
        .units
        .iter_mut()
        .find(|unit| !unit.is_entry)
        .ok_or_else(|| "dependency fixture unit missing".to_owned())?;
    let helper = dependency
        .mir
        .fns
        .first_mut()
        .ok_or_else(|| "dependency fixture function missing".to_owned())?;
    helper.name = align_mir::ProgramCall::try_from_logical("main")
        .map_err(|error| format!("cannot form main fixture identity: {error:?}"))?;
    helper.exportable = false;

    let error = match function_partitions(&built.units, &[]) {
        Ok(_) => return Err("duplicate C main symbols formed partitions".to_owned()),
        Err(error) => error,
    };
    assert_eq!(error, "duplicate ThinLTO root symbol:4:6d61696e");
    Ok(())
}

#[test]
fn size_verb_uses_function_partitions() {
    if !backend_available() {
        return;
    }
    let proj = Proj::new("function-size", &[("main.align", V1)], "main.align");
    let output = std::process::Command::new(env!("CARGO_BIN_EXE_alignc"))
        .args([
            "size",
            "main.align",
            "--thin-lto",
            "--cache-stats",
            "--profile",
            "release",
        ])
        .current_dir(&proj.dir)
        .env("ALIGNC_CACHE", proj.cache_root())
        .output()
        .expect("run size --thin-lto");
    assert!(
        output.status.success(),
        "size --thin-lto failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("prelink: 0 hit, 3 miss") && stderr.contains("backend: 0 hit, 3 miss"),
        "size did not use the three function partitions:\n{stderr}"
    );
}

#[test]
fn resource_owner_forms_one_support_partition_and_runs_once() {
    if !backend_available() {
        return;
    }
    let internal = "module pkg.db.internal.resource\npub fn drop_conn(handle: raw) { unsafe { raw.free(handle) } }\n";
    let root = "module pkg.db\nimport pkg.db.internal.resource\npub resource conn = pkg.db.internal.resource.drop_conn\npub fn open() -> conn { unsafe { return resource.from_raw(raw.alloc(8)) } }\n";
    let entry =
        "module main\nimport pkg.db\nfn main() -> i32 { owner := pkg.db.open(); return 0 }\n";
    let built = build_per_unit_multi(
        "function-resource",
        &[
            ("pkg/db/internal/resource.align", internal),
            ("pkg/db.align", root),
            ("main.align", entry),
        ],
        "main.align",
    );
    let partitions = function_partitions(&built.walk.units, &[]).expect("resource partitions");
    let supports = partitions
        .iter()
        .filter(|partition| matches!(partition.view, PartitionCodegenView::Support { .. }))
        .count();
    assert_eq!(
        supports,
        1,
        "exactly one producer support partition: {:?}",
        built
            .walk
            .units
            .iter()
            .map(|unit| (
                &unit.unit,
                unit.mir
                    .resources
                    .iter()
                    .map(|resource| (&resource.drop_hook, &resource.drop_thunk))
                    .collect::<Vec<_>>(),
                unit.mir
                    .fns
                    .iter()
                    .map(|function| function.name.as_str())
                    .collect::<Vec<_>>(),
            ))
            .collect::<Vec<_>>()
    );

    let executable = built.dir.join("resource-function-thin");
    let build = build_function_thin_lto(
        &built.walk.units,
        &CacheContext::Disabled,
        &BuildTarget::Baseline,
        Profile::Release,
        &[],
        false,
        2,
    )
    .expect("resource function ThinLTO build");
    build.link_and_publish(&executable).expect("resource link");
    let output = std::process::Command::new(&executable)
        .output()
        .expect("resource run");
    assert!(
        output.status.success(),
        "resource executable failed: {output:?}"
    );
}

#[test]
fn shared_codegen_table_change_invalidates_every_function_partition() {
    let before = Proj::new(
        "function-shared-before",
        &[(
            "main.align",
            "Marker { value: i64 }\nfn leaf(n: i64) -> i64 = n + 1\nfn main() -> i32 { print(leaf(1)); return 0 }\n",
        )],
        "main.align",
    );
    let after = Proj::new(
        "function-shared-after",
        &[(
            "main.align",
            "Marker { value: u64 }\nfn leaf(n: i64) -> i64 = n + 1\nfn main() -> i32 { print(leaf(1)); return 0 }\n",
        )],
        "main.align",
    );
    let hashes = |proj: &Proj| {
        function_partitions(&walk(proj).units, &[])
            .expect("form shared-table partitions")
            .into_iter()
            .filter_map(|partition| match partition.view {
                PartitionCodegenView::Function { selected, .. } => {
                    Some((selected.name.as_str().to_owned(), partition.impl_hash))
                }
                PartitionCodegenView::Support { .. } => None,
            })
            .collect::<std::collections::BTreeMap<_, _>>()
    };
    let before_hashes = hashes(&before);
    let after_hashes = hashes(&after);
    assert_eq!(
        before_hashes.keys().collect::<Vec<_>>(),
        after_hashes.keys().collect::<Vec<_>>()
    );
    assert!(
        before_hashes
            .iter()
            .all(|(function, hash)| after_hashes[function] != *hash),
        "one shared-table change must invalidate every function partition"
    );
}
