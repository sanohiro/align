//! Scalar math methods (`core.math`): `x.abs()`, `a.min(b)`, `a.max(b)` on numeric values. `abs`
//! uses `llvm.abs`/`llvm.fabs` (identity on unsigned); `min`/`max` use `llvm.{s,u}min`/`{s,u}max`
//! (int) / `llvm.minimum`/`maximum` (float, IEEE 754-2019: NaN-propagating). `a.min(b)` (pairwise) coexists with `arr.min()`
//! (reduction), dispatched by arity.

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
fn abs_min_max_int() {
    if !backend_available() {
        return;
    }
    let src = "fn main() -> Result<(), Error> {\n  x: i32 := -42\n  print(x.abs())\n  a: i32 := 3\n  b: i32 := 7\n  print(a.min(b))\n  print(a.max(b))\n  return Ok(())\n}\n";
    let out = build_and_run("sm-int", src);
    assert_eq!(out.status.code(), Some(0));
    assert_eq!(String::from_utf8_lossy(&out.stdout), "42\n3\n7\n");
}

#[test]
fn abs_min_max_float() {
    if !backend_available() {
        return;
    }
    let src = "fn main() -> Result<(), Error> {\n  f: f64 := -2.5\n  print(f.abs())\n  g: f64 := 1.5\n  print(f.min(g))\n  print(f.max(g))\n  return Ok(())\n}\n";
    let out = build_and_run("sm-float", src);
    assert_eq!(out.status.code(), Some(0));
    assert_eq!(String::from_utf8_lossy(&out.stdout), "2.5\n-2.5\n1.5\n");
}

#[test]
fn abs_unsigned_is_identity() {
    if !backend_available() {
        return;
    }
    let src = "fn main() -> Result<(), Error> {\n  u: u32 := 5\n  print(u.abs())\n  return Ok(())\n}\n";
    let out = build_and_run("sm-uabs", src);
    assert_eq!(out.status.code(), Some(0));
    assert_eq!(String::from_utf8_lossy(&out.stdout), "5\n");
}

#[test]
fn pairwise_min_coexists_with_array_reduction() {
    if !backend_available() {
        return;
    }
    // `a.min(b)` (one arg → pairwise) and `arr.min()` (no arg → reduction) coexist by arity.
    let src = "fn main() -> Result<(), Error> {\n  a: i64 := 8\n  print(a.min(5))\n  print([3, 1, 2].min())\n  return Ok(())\n}\n";
    let out = build_and_run("sm-coexist", src);
    assert_eq!(out.status.code(), Some(0));
    assert_eq!(String::from_utf8_lossy(&out.stdout), "5\n1\n");
}

#[test]
fn non_numeric_receiver_rejected() {
    // abs/min/max are numeric-only.
    assert!(check_errs("sm-bool", "fn main() -> i32 {\n  b := true\n  if b.abs() == 1 { return 1 }\n  return 0\n}\n"));
}
