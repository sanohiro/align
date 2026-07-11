//! `alignc emit-llvm --stage raw|optimized` CLI surface (`docs/impl/09-explain-opt.md`, Slice 3a).
//! Drives the built `alignc` binary: `raw` (the default) prints pre-optimization IR, `optimized`
//! runs the `-O2` pipeline first, and any other `--stage` value is a clean argument error (exit 1
//! with a diagnostic), never a panic.

use std::process::Command;

fn write_src(test_name: &str) -> std::path::PathBuf {
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
    path
}

fn alignc() -> Command {
    Command::new(env!("CARGO_BIN_EXE_alignc"))
}

#[test]
fn stage_optimized_runs_the_pipeline() {
    if !align_driver::backend_available() || !cfg!(target_arch = "x86_64") {
        return;
    }
    let src = write_src("stage_optimized_runs_the_pipeline");
    let out = alignc()
        .args(["emit-llvm"])
        .arg(&src)
        .args(["--stage", "optimized", "--target-cpu", "x86-64-v3"])
        .output()
        .expect("run alignc");
    let _ = std::fs::remove_file(&src);
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
    let out = alignc().args(["emit-llvm"]).arg(&src).output().expect("run alignc");
    let _ = std::fs::remove_file(&src);
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
        .arg(&src)
        .args(["--stage", "bogus"])
        .output()
        .expect("run alignc");
    let _ = std::fs::remove_file(&src);
    assert_eq!(out.status.code(), Some(1), "a bad --stage value must fail cleanly");
    let err = String::from_utf8_lossy(&out.stderr);
    assert!(err.contains("unknown --stage"), "want a stage diagnostic:\n{err}");
    assert!(err.contains("bogus"), "the diagnostic should echo the bad value:\n{err}");
    // A panic would print a backtrace / "panicked at"; this path must be a plain diagnostic.
    assert!(!err.contains("panicked"), "must not panic:\n{err}");
}
