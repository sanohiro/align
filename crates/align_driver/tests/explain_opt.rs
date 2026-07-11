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
    Command::new(env!("CARGO_BIN_EXE_alignc"))
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

/// A compile error → exit 1 (not a report), with a diagnostic, never a panic.
#[test]
fn compile_error_exits_one() {
    let src = write_src("bad", "fn main() -> i64 = no_such_fn()\n");
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
