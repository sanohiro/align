//! `alignc emit-llvm --stage raw|optimized` CLI surface (`docs/impl/09-explain-opt.md`, Slice 3a).
//! Drives the built `alignc` binary: `raw` (the default) prints pre-optimization IR, `optimized`
//! runs the `-O2` pipeline first, and any other `--stage` value is a clean argument error (exit 1
//! with a diagnostic), never a panic.

use std::path::{Path, PathBuf};
use std::process::Command;

/// A source file under `std::env::temp_dir()` that removes itself on drop — so a test file is
/// cleaned up even if an assertion panics partway through (a bare `let _ =
/// std::fs::remove_file(...)` right after the process runs never gets a chance to execute then).
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

fn write_src(test_name: &str) -> TempFile {
    let dir = std::env::temp_dir();
    let path = dir.join(format!("align-stage-{}-{}.align", std::process::id(), test_name));
    std::fs::write(
        &path,
        "fn dbl(x: i64) -> i64 = x * 2\n\
         fn run(xs: slice<i64>) -> i64 = xs.map(dbl).sum()\n\
         fn main(args: array<str>) -> Result<(), Error> {\n  \
           a := [1, 2, 3, 4, 5, 6, 7, 8]\n  \
           s : slice<i64> := a[0..args.len()]\n  \
           print(run(s))\n  \
           return Ok(())\n\
         }\n",
    )
    .expect("write src");
    TempFile(path)
}

fn alignc() -> Command {
    // Isolate from the (now default-ON) user cache so this test is deterministic and never
    // pollutes ~/.cache; these verbs assert exact codegen/IR, not caching behavior.
    let mut c = Command::new(env!("CARGO_BIN_EXE_alignc"));
    c.env("ALIGNC_CACHE", "off");
    c
}

#[test]
fn stage_optimized_runs_the_pipeline() {
    if !align_driver::backend_available() || !cfg!(target_arch = "x86_64") {
        return;
    }
    let src = write_src("stage_optimized_runs_the_pipeline");
    let out = alignc()
        .args(["emit-llvm"])
        .arg(src.path())
        .args(["--stage", "optimized", "--target-cpu", "x86-64-v3"])
        .output()
        .expect("run alignc");
    assert!(out.status.success(), "exit: {:?}", out.status.code());
    let ir = String::from_utf8_lossy(&out.stdout);
    // Optimized: the loop vectorizer has run.
    assert!(ir.contains("vector.body"), "optimized IR should be vectorized:\n{ir}");
}

#[test]
fn stage_raw_is_the_default_and_unoptimized() {
    if !align_driver::backend_available() {
        return;
    }
    let src = write_src("stage_raw_is_the_default_and_unoptimized");
    // No `--stage` flag → default `raw`.
    let out = alignc().args(["emit-llvm"]).arg(src.path()).output().expect("run alignc");
    assert!(out.status.success(), "exit: {:?}", out.status.code());
    let ir = String::from_utf8_lossy(&out.stdout);
    assert!(ir.contains("define"), "expected LLVM IR on stdout:\n{ir}");
    assert!(!ir.contains("vector.body"), "raw (default) IR must not be vectorized:\n{ir}");
}

#[test]
fn stage_unknown_value_is_a_diagnostic_not_a_panic() {
    let src = write_src("stage_unknown_value_is_a_diagnostic_not_a_panic");
    let out = alignc()
        .args(["emit-llvm"])
        .arg(src.path())
        .args(["--stage", "bogus"])
        .output()
        .expect("run alignc");
    assert_eq!(out.status.code(), Some(1), "a bad --stage value must fail cleanly");
    let err = String::from_utf8_lossy(&out.stderr);
    assert!(err.contains("unknown --stage"), "want a stage diagnostic:\n{err}");
    assert!(err.contains("bogus"), "the diagnostic should echo the bad value:\n{err}");
    // A panic would print a backtrace / "panicked at"; this path must be a plain diagnostic.
    assert!(!err.contains("panicked"), "must not panic:\n{err}");
}

// ---- Inspection roots: a `main`-less unit reports its OWN code (issue 1086) ---------------------

/// A library unit: `pub fn`s, one private helper, no `main`. Under the link-roots model every one of
/// these is internal, dead, and eliminated by the optimized lens.
const LIB: &str = "pub fn k1(x: i64) -> i64 = helper(x) + 1\n\
     pub fn k2(xs: slice<i64>) -> i64 = xs.map(dbl).sum()\n\
     fn dbl(x: i64) -> i64 = x * 2\n\
     fn helper(x: i64) -> i64 = x + 10\n";

fn write_named(test_name: &str, body: &str) -> TempFile {
    let path = std::env::temp_dir().join(format!("align-stage-{}-{}.align", std::process::id(), test_name));
    std::fs::write(&path, body).expect("write src");
    TempFile(path)
}

/// Roots for *inspecting* a unit are not roots for *linking* an executable. A unit with no `main`
/// and no `--export` is reported through its own `pub` surface, so `emit-llvm --stage optimized`
/// emits that unit's bodies instead of an empty module — and says so on stderr.
#[test]
fn a_main_less_unit_is_reported_through_its_own_pub_functions() {
    if !align_driver::backend_available() {
        return;
    }
    let src = write_named("inspection_roots", LIB);
    let out = alignc()
        .args(["emit-llvm"])
        .arg(src.path())
        .args(["--stage", "optimized"])
        .output()
        .expect("run alignc");
    assert!(out.status.success(), "exit: {:?}", out.status.code());
    let ir = String::from_utf8_lossy(&out.stdout);
    let err = String::from_utf8_lossy(&out.stderr);
    // A `define` line, not merely a mention: a call site or a declaration would satisfy a bare
    // `contains` while the body was still eliminated, which is exactly the defect.
    let defined: Vec<&str> = ir
        .lines()
        .map(str::trim_start)
        .filter(|l| l.starts_with("define "))
        .collect();
    // The roots keep their ENCODED symbols. Seeding marks `exportable` (external linkage, encoded
    // symbol), not an `--export` root, which would additionally rename the symbol to the raw source
    // name — see `a_pub_function_named_like_a_runtime_symbol_still_reports`.
    for root in [encoded("k1"), encoded("k2")] {
        assert!(
            defined.iter().any(|l| l.contains(&format!("@\"{root}\"("))),
            "the requested unit's own `pub` body {root} must be defined, not eliminated:\n{ir}"
        );
    }
    assert!(
        !ir.contains("define i64 @k1("),
        "an inspection root must not be renamed to its raw source symbol:\n{ir}"
    );
    // The seeded set is stated, not applied silently, and it goes to stderr so a redirected IR
    // stream is unchanged.
    assert!(err.contains("defines no `main`"), "the seeded roots must be stated:\n{err}");
    assert!(err.contains("2 `pub` function(s)"), "the note names the count:\n{err}");
    assert!(!ir.contains("defines no `main`"), "the note must not pollute stdout:\n{ir}");
}

/// An explicit `--export` still narrows the set exactly as before: it wins over the seeded roots,
/// and every other function keeps the default `internal` linkage.
#[test]
fn an_explicit_export_narrows_the_inspection_roots() {
    if !align_driver::backend_available() {
        return;
    }
    let src = write_named("inspection_roots_narrowed", LIB);
    let out = alignc()
        .args(["emit-llvm"])
        .arg(src.path())
        .args(["--stage", "optimized", "--export", "k1"])
        .output()
        .expect("run alignc");
    assert!(out.status.success(), "exit: {:?}", out.status.code());
    let ir = String::from_utf8_lossy(&out.stdout);
    let err = String::from_utf8_lossy(&out.stderr);
    let defines = ir.lines().filter(|l| l.trim_start().starts_with("define ")).count();
    assert_eq!(defines, 1, "only the named root survives:\n{ir}");
    assert!(ir.contains("@k1("), "the named root is the one emitted:\n{ir}");
    assert!(
        !err.contains("defines no `main`"),
        "an explicit --export seeds nothing, so there is nothing to state:\n{err}"
    );
}

/// A unit that defines `main` already has its root: it is reported exactly as it builds, with no
/// seeded roots and no note. The `{main}` link-roots model is untouched.
#[test]
fn a_unit_with_main_is_unchanged_and_says_nothing() {
    if !align_driver::backend_available() {
        return;
    }
    let src = write_src("main_unit_unchanged");
    let out = alignc()
        .args(["emit-llvm"])
        .arg(src.path())
        .args(["--stage", "optimized"])
        .output()
        .expect("run alignc");
    assert!(out.status.success(), "exit: {:?}", out.status.code());
    let err = String::from_utf8_lossy(&out.stderr);
    assert!(!err.contains("defines no `main`"), "a unit with `main` seeds nothing:\n{err}");
}

/// The encoded, collision-free LLVM symbol for a program function (mirrors `symbol_name`'s
/// non-export path). Duplicated from `export_roots.rs`: each integration test file is its own
/// crate.
fn encoded(sym: &str) -> String {
    let hex = sym.as_bytes().iter().map(|byte| format!("{byte:02x}")).collect::<String>();
    format!("align_fn${}${hex}", sym.len())
}

/// An inspection root gets `external` linkage, NOT a renamed symbol.
///
/// `--export` deliberately does both: it roots the function AND replaces the encoded symbol with
/// the raw source name, because a linkable object needs a C-ABI name. Seeding inspection roots
/// through that same mechanism would rename a `pub fn align_rt_print_i64` onto a reserved runtime
/// symbol, and `callable_preflight` would reject a unit that compiled fine before — an inspection
/// verb must never fail on a program a build accepts. Both verbs are pinned, because both seed.
#[test]
fn a_pub_function_named_like_a_runtime_symbol_still_reports() {
    if !align_driver::backend_available() {
        return;
    }
    let src = write_named(
        "reserved_runtime_name",
        "pub fn align_rt_print_i64(x: i64) -> i64 = x + 1\npub fn ordinary(x: i64) -> i64 = x * 2\n",
    );
    let ir_run = alignc()
        .args(["emit-llvm"])
        .arg(src.path())
        .args(["--stage", "optimized"])
        .output()
        .expect("run alignc");
    let err = String::from_utf8_lossy(&ir_run.stderr);
    assert!(
        ir_run.status.success(),
        "a `pub` name that matches a reserved runtime symbol must still report: {err}"
    );
    assert!(
        !err.contains("external identity collision"),
        "seeding a root must not rename it onto a reserved runtime symbol:\n{err}"
    );
    let ir = String::from_utf8_lossy(&ir_run.stdout);
    assert!(
        ir.contains(&format!("@\"{}\"(", encoded("align_rt_print_i64"))),
        "the root keeps its encoded symbol:\n{ir}"
    );

    let explain = alignc()
        .arg("explain-opt")
        .arg(src.path())
        .output()
        .expect("run alignc");
    assert!(
        explain.status.success(),
        "explain-opt seeds the same roots and must not fail either: {}",
        String::from_utf8_lossy(&explain.stderr)
    );
}
