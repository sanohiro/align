//! `alignc explain-opt` CLI surface (`docs/impl/09-explain-opt.md`, Slice 3b). Driven through the
//! compiled `alignc` binary in a subprocess: `explain-opt` enables LLVM's process-global
//! `-pass-remarks*` state (via `LLVMParseCommandLineOptions`), which must never leak into the
//! in-process test harness — so, like `emit_llvm_stage.rs`, these tests run the real binary.

use std::path::{Path, PathBuf};
use std::process::Command;

/// A temp source file that removes itself on drop (survives an assertion panic).
struct TempFile(PathBuf);

impl TempFile {
    fn path(&self) -> &Path {
        &self.0
    }
}

impl Drop for TempFile {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.0);
    }
}

fn write_src(name: &str, body: &str) -> TempFile {
    let path = std::env::temp_dir().join(format!("align-explain-{}-{}.align", std::process::id(), name));
    std::fs::write(&path, body).expect("write src");
    TempFile(path)
}

fn alignc() -> Command {
    // Isolate from the (now default-ON) user cache so this test is deterministic and never
    // pollutes ~/.cache; these verbs assert exact codegen/IR, not caching behavior.
    let mut c = Command::new(env!("CARGO_BIN_EXE_alignc"));
    c.env("ALIGNC_CACHE", "off");
    c
}

const MAP_SUM: &str = "fn dbl(x: i64) -> i64 = x * 2\n\
     fn run(xs: slice<i64>) -> i64 = xs.map(dbl).sum()\n\
     fn main(args: array<str>) -> Result<(), Error> {\n  \
       a := [1, 2, 3, 4, 5, 6, 7, 8]\n  \
       s : slice<i64> := a[0..args.len()]\n  \
       print(run(s))\n  \
       return Ok(())\n\
     }\n";

const FP_SUM: &str = "fn add(a: f64, b: f64) -> f64 = a + b\n\
     fn run(xs: slice<f64>) -> f64 = xs.reduce(0.0, add)\n\
     fn main(args: array<str>) -> Result<(), Error> {\n  \
       a := [1.0, 2.0, 3.0, 4.0, 5.0, 6.0, 7.0, 8.0]\n  \
       s : slice<f64> := a[0..args.len()]\n  \
       print(run(s))\n  \
       return Ok(())\n\
     }\n";

/// A vectorizing pipeline → the success summary reflects it, no miss line, exit 0.
#[test]
fn vectorizing_pipeline_reports_success() {
    if !align_driver::backend_available() || !cfg!(target_arch = "x86_64") {
        return;
    }
    let src = write_src("vectorizing", MAP_SUM);
    let out = alignc()
        .args(["explain-opt"])
        .arg(src.path())
        .args(["--target-cpu", "x86-64-v3"])
        .output()
        .expect("run alignc");
    assert_eq!(out.status.code(), Some(0), "explain-opt exits 0 on success");
    let s = String::from_utf8_lossy(&out.stdout);
    assert!(s.contains("loop(s) vectorized"), "want a vectorized summary:\n{s}");
    assert!(!s.contains("not vectorized"), "no miss line expected:\n{s}");
}

/// An ordered floating-point reduction → a missed/actionable line with the FP-reorder reason, still
/// exit 0 (a missed optimization is not an error).
#[test]
fn fp_reduction_reports_an_actionable_miss() {
    if !align_driver::backend_available() || !cfg!(target_arch = "x86_64") {
        return;
    }
    let src = write_src("fp_miss", FP_SUM);
    let out = alignc()
        .args(["explain-opt"])
        .arg(src.path())
        .args(["--target-cpu", "x86-64-v3"])
        .output()
        .expect("run alignc");
    assert_eq!(out.status.code(), Some(0), "a missed optimization is not an error");
    let s = String::from_utf8_lossy(&out.stdout);
    // The actionable line is anchored to a real user source location and speaks the reason.
    assert!(s.contains(":1:"), "the miss should anchor to a source line:\n{s}");
    assert!(s.contains("not vectorized"), "want a miss line:\n{s}");
    assert!(s.contains("floating-point"), "want the FP-reorder reason (honest cause):\n{s}");
    // Honesty: an FP decline must not be dressed up as an aliasing story.
    assert!(!s.contains("overlap"), "must not fabricate an aliasing cause:\n{s}");
}

/// `--verbose` surfaces the raw LLVM remarks that the default view only counts, explicitly marked
/// `[llvm …]` (a machine string is never dressed as an Align diagnostic).
#[test]
fn verbose_shows_raw_passthrough() {
    if !align_driver::backend_available() || !cfg!(target_arch = "x86_64") {
        return;
    }
    let src = write_src("verbose", MAP_SUM);
    let default = alignc()
        .args(["explain-opt"])
        .arg(src.path())
        .args(["--target-cpu", "x86-64-v3"])
        .output()
        .expect("run alignc");
    let verbose = alignc()
        .args(["explain-opt"])
        .arg(src.path())
        .args(["--target-cpu", "x86-64-v3", "--verbose"])
        .output()
        .expect("run alignc");
    let d = String::from_utf8_lossy(&default.stdout);
    let v = String::from_utf8_lossy(&verbose.stdout);
    assert!(!d.contains("[llvm]"), "default view must not print raw machine strings:\n{d}");
    assert!(v.contains("[llvm]"), "verbose should show raw [llvm …] passthrough:\n{v}");
    // The compiler-internal (`<unknown>`) remarks are suppressed by default, shown labeled in verbose.
    assert!(!d.contains("<unknown>"), "internal remarks are suppressed by default:\n{d}");
    assert!(v.contains("compiler-internal"), "verbose labels the internal remarks:\n{v}");
}

#[test]
fn current_plan_rows_precede_llvm_and_do_not_change_under_verbose() {
    if !align_driver::backend_available() || !cfg!(target_arch = "x86_64") {
        return;
    }
    let source = "fn chunk_sum(xs: slice<i64>) -> i64 = xs.sum()\n\
        fn run() -> i64 = [1, 2, 3, 4].chunks(2).par_map(chunk_sum).sum()\n\
        fn main() -> Result<(), Error> { print(run()); return Ok(()) }\n";
    let src = write_src("current-plan", source);
    let run = |target_cpu: &str, verbose: bool| {
        let mut command = alignc();
        command
            .args(["explain-opt"])
            .arg(src.path())
            .args(["--target-cpu", target_cpu]);
        if verbose {
            command.arg("--verbose");
        }
        command.output().expect("run alignc")
    };
    let default = run("x86-64-v3", false);
    let verbose = run("x86-64-v3", true);
    assert!(default.status.success(), "default failed: {}", String::from_utf8_lossy(&default.stderr));
    assert!(verbose.status.success(), "verbose failed: {}", String::from_utf8_lossy(&verbose.stderr));
    let plan_lines = |bytes: &[u8]| {
        String::from_utf8_lossy(bytes)
            .lines()
            .filter(|line| line.contains("current plan"))
            .map(str::to_owned)
            .collect::<Vec<_>>()
    };
    let default_plan = plan_lines(&default.stdout);
    assert_eq!(default_plan, plan_lines(&verbose.stdout));
    let baseline = run("baseline", false);
    let native = run("native", false);
    assert!(baseline.status.success());
    assert!(native.status.success());
    assert_eq!(
        plan_lines(&baseline.stdout),
        plan_lines(&native.stdout),
        "current-plan decisions must be target-independent"
    );
    assert_eq!(default_plan.len(), 2);
    assert!(default_plan[0].contains(
        "current plan `run` #1 chunks: selected `virtual-range-views` — the direct explicit-parallel consumer derives borrowed chunk views in its range kernel"
    ));
    assert!(default_plan[1].contains(
        "current plan `run` #2 par-map: runtime-selected `range-reduce` — the runtime chooses caller-only or shared-pool range reduction from the input length, element layouts, conservative work hint, and process-lifetime worker availability"
    ));
    let stdout = String::from_utf8_lossy(&default.stdout);
    assert!(
        stdout.find("current plan").unwrap() < stdout.find("loop(s) vectorized").unwrap(),
        "current-plan rows must precede LLVM output:\n{stdout}"
    );
}

#[test]
fn current_plan_measurement_override_banner_is_exact_and_first() {
    if !align_driver::backend_available() || !cfg!(target_arch = "x86_64") {
        return;
    }
    let src = write_src("current-plan-override", MAP_SUM);
    let out = alignc()
        .env("ALIGN_BUFFER_DONATE", "off")
        .args(["explain-opt"])
        .arg(src.path())
        .args(["--target-cpu", "x86-64-v3"])
        .output()
        .expect("run alignc");
    assert!(out.status.success(), "explain-opt failed: {}", String::from_utf8_lossy(&out.stderr));
    assert_eq!(
        String::from_utf8_lossy(&out.stdout).lines().next(),
        Some(
            "current-plan measurement override: buffer donation is disabled; donation rows are not the default plan"
        )
    );
}

/// A compile error → exit 1 (not a report), with a diagnostic, never a panic.
#[test]
fn compile_error_exits_one() {
    let src = write_src("bad", "fn main() -> i32 = no_such_fn()\n");
    let out = alignc().args(["explain-opt"]).arg(src.path()).output().expect("run alignc");
    assert_eq!(out.status.code(), Some(1), "a compile error must fail cleanly");
    let err = String::from_utf8_lossy(&out.stderr);
    assert!(err.contains("error"), "want a diagnostic:\n{err}");
    assert!(!err.contains("panicked"), "must not panic:\n{err}");
}

/// A missing file → exit 1 with a read diagnostic.
#[test]
fn missing_file_exits_one() {
    let out = alignc()
        .args(["explain-opt", "/nonexistent/definitely-not-here.align"])
        .output()
        .expect("run alignc");
    assert_eq!(out.status.code(), Some(1));
    let err = String::from_utf8_lossy(&out.stderr);
    assert!(err.contains("cannot read"), "want a read diagnostic:\n{err}");
}

// ---- Inspection roots: `explain-opt` reports the unit it was pointed at (issue 1086) ------------

/// A library unit with a vectorizable pipeline and an inlinable helper — and no `main`. Under the
/// link-roots model nothing here is a root, so the lens used to report "no opportunities" for code
/// it had never compiled.
const LIB_PIPELINE: &str = "pub fn k1(x: i64) -> i64 = helper(x) + 1\n\
     pub fn k2(xs: slice<i64>) -> i64 = xs.map(dbl).sum()\n\
     fn dbl(x: i64) -> i64 = x * 2\n\
     fn helper(x: i64) -> i64 = x + 10\n";

/// No verb may report "no opportunities" for code it did not compile. A `main`-less unit is rooted
/// at its own `pub` functions, so the remarks describe that unit — and the seeded set is stated on
/// stderr rather than applied silently.
#[test]
fn a_main_less_unit_reports_its_own_optimizer_decisions() {
    if !align_driver::backend_available() {
        return;
    }
    let src = write_src("library_unit", LIB_PIPELINE);
    let out = alignc().arg("explain-opt").arg(src.path()).output().expect("run alignc");
    assert!(out.status.success(), "exit: {:?}", out.status.code());
    let report = String::from_utf8_lossy(&out.stdout);
    let err = String::from_utf8_lossy(&out.stderr);
    assert!(
        !report.contains("no vectorization or inlining opportunities were reported"),
        "the unit's own code was compiled, so it must not be reported as opportunity-free:\n{report}"
    );
    assert!(
        report.contains("inlined"),
        "the intra-unit helper call inlines and must be reported:\n{report}"
    );
    assert!(err.contains("defines no `main`"), "the seeded roots must be stated:\n{err}");
    assert!(err.contains("explain-opt"), "the note names the verb:\n{err}");
}

/// `--export` is valid on every verb that has roots, and narrows `explain-opt`'s the same way it
/// narrows `emit-llvm`'s. An unknown name stays a hard, listed error — never a silent no-op.
#[test]
fn explain_opt_takes_explicit_export_roots_and_rejects_unknown_ones() {
    if !align_driver::backend_available() {
        return;
    }
    let src = write_src("library_unit_narrowed", LIB_PIPELINE);
    let narrowed = alignc()
        .arg("explain-opt")
        .arg(src.path())
        .args(["--export", "k2"])
        .output()
        .expect("run alignc");
    assert!(narrowed.status.success(), "exit: {:?}", narrowed.status.code());
    let err = String::from_utf8_lossy(&narrowed.stderr);
    assert!(
        !err.contains("defines no `main`"),
        "an explicit --export seeds nothing, so there is nothing to state:\n{err}"
    );

    let unknown = alignc()
        .arg("explain-opt")
        .arg(src.path())
        .args(["--export", "nosuch"])
        .output()
        .expect("run alignc");
    assert_eq!(unknown.status.code(), Some(1), "a typo'd export must fail cleanly");
    let unknown_err = String::from_utf8_lossy(&unknown.stderr);
    assert!(unknown_err.contains("unknown export(s): nosuch"), "{unknown_err}");
    assert!(!unknown_err.contains("panicked"), "must not panic:\n{unknown_err}");
}

/// A unit that defines `main` is reported exactly as it builds: no seeded roots, no note.
#[test]
fn a_unit_with_main_seeds_no_inspection_roots() {
    if !align_driver::backend_available() {
        return;
    }
    let src = write_src("entry_unit_unchanged", MAP_SUM);
    let out = alignc().arg("explain-opt").arg(src.path()).output().expect("run alignc");
    assert!(out.status.success(), "exit: {:?}", out.status.code());
    let err = String::from_utf8_lossy(&out.stderr);
    assert!(!err.contains("defines no `main`"), "a unit with `main` seeds nothing:\n{err}");
}
