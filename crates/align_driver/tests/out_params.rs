//! Mutable element writes `place[i] = v` (a `mut` array local or an `out` slice parameter) and
//! `out` parameters — a writable output buffer the callee fills (the write mechanism; the
//! no-alias / `noalias` optimization is a follow-up). Stores are bounds-checked.

use align_driver::{backend_available, check, emit_object_file, link_executable, lower_to_mir};
use align_span::SourceMap;

fn build_and_run(name: &str, src: &str) -> std::process::Output {
    let mut sm = SourceMap::new();
    let checked = check(&mut sm, name, src);
    assert!(
        !checked.diags.has_errors(),
        "unexpected errors:\n{}",
        align_driver::format_diagnostics(&sm, &checked.diags)
    );
    let mir = lower_to_mir(&checked.hir);
    let dir = std::env::temp_dir();
    let obj = dir.join(format!("align-test-{name}.o"));
    let exe = dir.join(format!("align-test-{name}"));
    emit_object_file(&mir, &obj).expect("codegen");
    link_executable(&obj, &exe).expect("link");
    let out = std::process::Command::new(&exe).output().expect("run");
    let _ = std::fs::remove_file(&obj);
    let _ = std::fs::remove_file(&exe);
    out
}

fn check_errs(name: &str, src: &str) -> bool {
    let mut sm = SourceMap::new();
    check(&mut sm, name, src).diags.has_errors()
}

#[test]
fn mut_local_array_element_write() {
    if !backend_available() {
        return;
    }
    // `mut a := [...]; a[1] = 32` — write an element of a mutable local array. 10 + 32 == 42.
    let src = "fn main() -> i32 {\n  mut a := [10, 0, 0]\n  a[1] = 32\n  if a[0] + a[1] == 42 { return 42 }\n  return 0\n}\n";
    let out = build_and_run("w-mut-local", src);
    assert_eq!(out.status.code(), Some(42));
}

#[test]
fn out_slice_param_write() {
    if !backend_available() {
        return;
    }
    // A callee fills an `out slice` buffer; the caller (which passed its array) sees the writes.
    let src = "fn put(out dst: slice<i64>) {\n  dst[0] = 10\n  dst[1] = 32\n}\nfn main() -> i32 {\n  mut a := [0, 0, 0]\n  put(a)\n  if a[0] + a[1] == 42 { return 42 }\n  return 0\n}\n";
    let out = build_and_run("w-out", src);
    assert_eq!(out.status.code(), Some(42));
}

#[test]
fn out_write_computed_index() {
    if !backend_available() {
        return;
    }
    // A computed (non-constant) subscript on the write side.
    let src = "fn put(out dst: slice<i64>, k: i64) {\n  dst[k + 1] = 42\n}\nfn main() -> i32 {\n  mut a := [0, 0, 0]\n  put(a, 1)\n  return a[2]\n}\n";
    let out = build_and_run("w-out-idx", src);
    assert_eq!(out.status.code(), Some(42));
}

#[test]
fn out_write_out_of_bounds_aborts() {
    if !backend_available() {
        return;
    }
    // An out-of-range element store aborts (the bounds check fires), like an out-of-range read.
    let src = "fn put(out dst: slice<i64>) {\n  dst[9] = 1\n}\nfn main() -> i32 {\n  mut a := [0, 0]\n  put(a)\n  return 0\n}\n";
    let out = build_and_run("w-oob", src);
    assert_ne!(out.status.code(), Some(0), "an out-of-bounds store must abort");
}

// --- diagnostics ---

#[test]
fn write_to_immutable_array_rejected() {
    assert!(check_errs("w-immut", "fn main() -> i32 {\n  a := [1, 2, 3]\n  a[0] = 9\n  return 0\n}\n"));
}

#[test]
fn element_write_after_move_rejected() {
    // Writing an element of an owned array that was moved away is a use-after-move (it would
    // write through a nulled slot) — must be a compile error, not a runtime fault.
    let src = "fn main() -> Result<(), Error> {\n  mut ys := [1, 2, 3].to_array()\n  zs := ys\n  ys[0] = 4\n  print(zs.sum())\n  return Ok(())\n}\n";
    assert!(check_errs("w-uam", src));
}

#[test]
fn out_on_non_slice_rejected() {
    assert!(check_errs("w-out-nonslice", "fn f(out x: i32) -> i32 = x\nfn main() -> i32 = 0\n"));
}
