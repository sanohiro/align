use align_driver::cache::{CacheContext, CacheStage};
use align_driver::{
    ArtifactStage, BuildTarget, FunctionThinLtoObservation, PerUnitWalk, Profile,
    build_function_thin_lto, build_per_unit, build_thin_lto, link_objects,
};
use align_span::SourceMap;
use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, Instant};

const EDIT_OLD: &str = "  if now < 0 || budget_ns <= 0 || now > 9223372036854775807 - budget_ns {";
const EDIT_NEW: &str = "  if budget_ns <= 0 || now < 0 || now > 9223372036854775807 - budget_ns {";

static TEMP_SEQUENCE: AtomicU64 = AtomicU64::new(0);

struct TempRoot(PathBuf);

impl TempRoot {
    fn new(label: &str) -> Result<Self, String> {
        let sequence = TEMP_SEQUENCE.fetch_add(1, Ordering::Relaxed);
        let path = std::env::temp_dir().join(format!(
            "align-function-incremental-{label}-{}-{sequence}",
            std::process::id()
        ));
        std::fs::create_dir(&path)
            .map_err(|error| format!("cannot create benchmark temp root: {error}"))?;
        Ok(Self(path))
    }

    fn path(&self) -> &Path {
        &self.0
    }
}

impl Drop for TempRoot {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn copy_tree(source: &Path, destination: &Path) -> Result<(), String> {
    std::fs::create_dir_all(destination)
        .map_err(|error| format!("cannot create corpus directory: {error}"))?;
    for entry in std::fs::read_dir(source)
        .map_err(|error| format!("cannot read corpus directory: {error}"))?
    {
        let entry = entry.map_err(|error| format!("cannot read corpus entry: {error}"))?;
        let kind = entry
            .file_type()
            .map_err(|error| format!("cannot inspect corpus entry: {error}"))?;
        let target = destination.join(entry.file_name());
        if kind.is_dir() {
            copy_tree(&entry.path(), &target)?;
        } else if kind.is_file() {
            std::fs::copy(entry.path(), target)
                .map_err(|error| format!("cannot copy corpus file: {error}"))?;
        } else {
            return Err("benchmark corpus contains a non-file entry".to_owned());
        }
    }
    Ok(())
}

fn repo_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(Path::parent)
        .expect("benchmark lives two levels below the repository root")
        .to_path_buf()
}

fn build_walk(root: &Path) -> Result<PerUnitWalk, String> {
    let entry = root.join("main.align");
    let source = std::fs::read_to_string(&entry)
        .map_err(|error| format!("cannot read benchmark entry: {error}"))?;
    let mut source_map = SourceMap::new();
    let walk = build_per_unit(&mut source_map, &entry.display().to_string(), &source);
    if walk.diags.has_errors() {
        return Err(align_driver::format_diagnostics(&source_map, &walk.diags));
    }
    Ok(walk)
}

struct Corpus {
    _root: TempRoot,
    original: PerUnitWalk,
    edited: PerUnitWalk,
}

impl Corpus {
    fn new() -> Result<Self, String> {
        let root = TempRoot::new("corpus")?;
        let original_root = root.path().join("original");
        let edited_root = root.path().join("edited");
        let source = repo_root().join("apps/db");
        copy_tree(&source, &original_root)?;
        copy_tree(&source, &edited_root)?;
        let edited_path = edited_root.join("pkg/db/internal/resource.align");
        let text = std::fs::read_to_string(&edited_path)
            .map_err(|error| format!("cannot read edit owner: {error}"))?;
        if text.matches(EDIT_OLD).count() != 1 {
            return Err("pkg.db edit owner no longer has the fixed private leaf body".to_owned());
        }
        std::fs::write(&edited_path, text.replacen(EDIT_OLD, EDIT_NEW, 1))
            .map_err(|error| format!("cannot write edited corpus: {error}"))?;
        let original = build_walk(&original_root)?;
        let edited = build_walk(&edited_root)?;
        Ok(Self {
            _root: root,
            original,
            edited,
        })
    }
}

fn object_paths(stage: &ArtifactStage, units: usize) -> Vec<PathBuf> {
    (0..units)
        .map(|index| stage.path().join(format!("unit{index}.o")))
        .collect()
}

fn run_unit(walk: &PerUnitWalk, cache: &CacheContext, jobs: usize) -> Result<UnitRun, String> {
    let stage = ArtifactStage::temp("align-function-bench-unit")
        .map_err(|error| format!("cannot create unit ThinLTO stage: {error}"))?;
    let objects = object_paths(&stage, walk.units.len());
    let build = build_thin_lto(
        &walk.units,
        &objects,
        cache,
        &BuildTarget::Baseline,
        Profile::Release,
        &[],
        false,
        stage.path(),
        jobs,
    )?;
    Ok(UnitRun {
        stage,
        objects,
        prelink_misses: build
            .outcomes
            .iter()
            .filter(|outcome| outcome.stage == CacheStage::ThinLtoPrelink && !outcome.hit)
            .count(),
        backend_misses: build
            .outcomes
            .iter()
            .filter(|outcome| outcome.stage == CacheStage::ThinLtoBackend && !outcome.hit)
            .count(),
    })
}

struct UnitRun {
    stage: ArtifactStage,
    objects: Vec<PathBuf>,
    prelink_misses: usize,
    backend_misses: usize,
}

fn function_misses(build: &align_driver::FunctionThinLtoBuild) -> (usize, usize) {
    build.observations().iter().fold(
        (0, 0),
        |(prelink, backend), observation| match observation {
            FunctionThinLtoObservation::Partitioned {
                prelink: prelink_outcome,
                backend: backend_outcome,
                ..
            } => (
                prelink + usize::from(!prelink_outcome.hit),
                backend + usize::from(!backend_outcome.hit),
            ),
            FunctionThinLtoObservation::WholeUnit { codegen, .. } => {
                (prelink + usize::from(!codegen.hit), backend)
            }
        },
    )
}

fn median(samples: &mut [Duration]) -> Duration {
    samples.sort_unstable();
    samples[samples.len() / 2]
}

fn samples_from_env() -> Result<usize, String> {
    let raw = std::env::var("ALIGN_FUNCTION_SAMPLES").unwrap_or_else(|_| "7".to_owned());
    let samples = raw
        .parse::<usize>()
        .map_err(|_| "ALIGN_FUNCTION_SAMPLES must be an odd integer >= 3".to_owned())?;
    if samples < 3 || samples % 2 == 0 {
        return Err("ALIGN_FUNCTION_SAMPLES must be an odd integer >= 3".to_owned());
    }
    Ok(samples)
}

fn measure_unit_edit(
    walk: &PerUnitWalk,
    cache: &CacheContext,
    jobs: usize,
) -> Result<(Duration, usize, usize), String> {
    let started = Instant::now();
    let run = run_unit(walk, cache, jobs)?;
    Ok((started.elapsed(), run.prelink_misses, run.backend_misses))
}

fn measure_function_edit(
    walk: &PerUnitWalk,
    cache: &CacheContext,
    jobs: usize,
) -> Result<(Duration, usize, usize), String> {
    let started = Instant::now();
    let build = build_function_thin_lto(
        &walk.units,
        cache,
        &BuildTarget::Baseline,
        Profile::Release,
        &[],
        false,
        jobs,
    )?;
    let elapsed = started.elapsed();
    let misses = function_misses(&build);
    Ok((elapsed, misses.0, misses.1))
}

fn run_edit() -> Result<(), String> {
    let corpus = Corpus::new()?;
    let samples = samples_from_env()?;
    let jobs = 4;
    let mut unit_times = Vec::with_capacity(samples);
    let mut function_times = Vec::with_capacity(samples);
    let mut frontier = None;

    for sample in 0..samples {
        let roots = TempRoot::new(&format!("sample-{sample}"))?;
        let unit_cache = CacheContext::at(roots.path().join("unit-cache"));
        let function_cache = CacheContext::at(roots.path().join("function-cache"));
        drop(run_unit(&corpus.original, &unit_cache, jobs)?);
        drop(build_function_thin_lto(
            &corpus.original.units,
            &function_cache,
            &BuildTarget::Baseline,
            Profile::Release,
            &[],
            false,
            jobs,
        )?);

        let (unit, function) = if sample % 2 == 0 {
            (
                measure_unit_edit(&corpus.edited, &unit_cache, jobs)?,
                measure_function_edit(&corpus.edited, &function_cache, jobs)?,
            )
        } else {
            let function = measure_function_edit(&corpus.edited, &function_cache, jobs)?;
            let unit = measure_unit_edit(&corpus.edited, &unit_cache, jobs)?;
            (unit, function)
        };
        unit_times.push(unit.0);
        function_times.push(function.0);
        frontier.get_or_insert((unit.1, unit.2, function.1, function.2));
    }

    let unit_median = median(&mut unit_times);
    let function_median = median(&mut function_times);
    let ratio = function_median.as_secs_f64() / unit_median.as_secs_f64();
    let frontier = frontier.expect("at least three samples");
    println!(
        "unit edit seconds: {:?}",
        unit_times
            .iter()
            .map(Duration::as_secs_f64)
            .collect::<Vec<_>>()
    );
    println!(
        "function edit seconds: {:?}",
        function_times
            .iter()
            .map(Duration::as_secs_f64)
            .collect::<Vec<_>>()
    );
    println!("unit edit median: {:.6} s", unit_median.as_secs_f64());
    println!(
        "function edit median: {:.6} s",
        function_median.as_secs_f64()
    );
    println!("function/unit ratio: {ratio:.4}");
    println!(
        "edit frontier: unit prelink/backend miss={}/{}, function prelink/backend miss={}/{}",
        frontier.0, frontier.1, frontier.2, frontier.3
    );
    if frontier.0 != 1 || frontier.2 != 1 {
        return Err("the private leaf edit did not miss exactly one prelink partition".to_owned());
    }
    if ratio > 0.75 {
        return Err(format!("function edit ratio {ratio:.4} exceeds 0.75"));
    }
    Ok(())
}

fn run_cold(function: bool) -> Result<(), String> {
    let corpus = Corpus::new()?;
    let root = TempRoot::new(if function {
        "cold-function"
    } else {
        "cold-unit"
    })?;
    let cache = CacheContext::at(root.path().join("cache"));
    let started = Instant::now();
    if function {
        let build = build_function_thin_lto(
            &corpus.original.units,
            &cache,
            &BuildTarget::Baseline,
            Profile::Release,
            &[],
            false,
            4,
        )?;
        println!("partitions={}", build.observations().len());
    } else {
        let build = run_unit(&corpus.original, &cache, 4)?;
        println!("partitions={}", build.objects.len());
    }
    println!("backend_seconds={:.6}", started.elapsed().as_secs_f64());
    Ok(())
}

fn link_libs(walk: &PerUnitWalk) -> Vec<String> {
    let mut seen = BTreeSet::new();
    let mut result = Vec::new();
    for unit in &walk.units {
        for library in &unit.mir.link_libs {
            if seen.insert(library.clone()) {
                result.push(library.clone());
            }
        }
    }
    result
}

fn run_executable(path: &Path) -> Result<std::process::Output, String> {
    Command::new(path)
        .output()
        .map_err(|error| format!("cannot run benchmark executable: {error}"))
}

fn run_parity() -> Result<(), String> {
    let corpus = Corpus::new()?;
    let root = TempRoot::new("parity")?;
    let libraries = link_libs(&corpus.original);

    let unit_cache = CacheContext::at(root.path().join("unit-cache"));
    let unit = run_unit(&corpus.original, &unit_cache, 4)?;
    let unit_exe = root.path().join("unit");
    let unit_refs = unit
        .objects
        .iter()
        .map(PathBuf::as_path)
        .collect::<Vec<_>>();
    link_objects(&unit_refs, &unit_exe, &libraries, Profile::Release)?;

    let function_cache = CacheContext::at(root.path().join("function-cache"));
    let function_exe = root.path().join("function");
    build_function_thin_lto(
        &corpus.original.units,
        &function_cache,
        &BuildTarget::Baseline,
        Profile::Release,
        &[],
        false,
        4,
    )?
    .link_and_publish(&function_exe)?;

    let off_exe = root.path().join("function-cache-off");
    build_function_thin_lto(
        &corpus.original.units,
        &CacheContext::Disabled,
        &BuildTarget::Baseline,
        Profile::Release,
        &[],
        false,
        4,
    )?
    .link_and_publish(&off_exe)?;
    let hot_exe = root.path().join("function-hot");
    build_function_thin_lto(
        &corpus.original.units,
        &function_cache,
        &BuildTarget::Baseline,
        Profile::Release,
        &[],
        false,
        4,
    )?
    .link_and_publish(&hot_exe)?;

    let unit_output = run_executable(&unit_exe)?;
    let function_output = run_executable(&function_exe)?;
    if unit_output.status != function_output.status
        || unit_output.stdout != function_output.stdout
        || unit_output.stderr != function_output.stderr
    {
        return Err("unit/function executable behavior differs".to_owned());
    }
    let function_bytes = std::fs::read(&function_exe)
        .map_err(|error| format!("cannot read cold function executable: {error}"))?;
    if std::fs::read(&off_exe).ok().as_deref() != Some(function_bytes.as_slice())
        || std::fs::read(&hot_exe).ok().as_deref() != Some(function_bytes.as_slice())
    {
        return Err("function cache-off/cold/hot executables differ".to_owned());
    }
    let unit_bytes = std::fs::metadata(&unit_exe)
        .map_err(|error| format!("cannot stat unit executable: {error}"))?
        .len();
    let function_size = u64::try_from(function_bytes.len()).expect("executable size fits u64");
    let size_ratio = function_size as f64 / unit_bytes as f64;
    println!("unit executable bytes={unit_bytes}");
    println!("function executable bytes={function_size}");
    println!("function/unit size ratio={size_ratio:.4}");
    println!("cache-off/cold/hot bytes=identical");
    println!("unit/function runtime output=identical");
    if !(0.95..=1.05).contains(&size_ratio) {
        return Err(format!(
            "function executable size ratio {size_ratio:.4} exceeds ±5%"
        ));
    }
    drop(unit.stage);
    Ok(())
}

fn real_main() -> Result<(), String> {
    match std::env::args().nth(1).as_deref() {
        None | Some("edit") => run_edit(),
        Some("cold-unit") => run_cold(false),
        Some("cold-function") => run_cold(true),
        Some("parity") => run_parity(),
        Some(other) => Err(format!(
            "unknown mode `{other}`; expected edit, cold-unit, cold-function, or parity"
        )),
    }
}

fn main() {
    if let Err(error) = real_main() {
        eprintln!("function incremental benchmark: {error}");
        std::process::exit(1);
    }
}
