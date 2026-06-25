//! Scalar math methods (`core.math`): `x.abs()`, `a.min(b)`, `a.max(b)` on numeric values. `abs`
//! uses `llvm.abs`/`llvm.fabs` (identity on unsigned); `min`/`max` use `llvm.{s,u}min`/`{s,u}max`
//! (int) / `llvm.minimum`/`maximum` (float, IEEE 754-2019: NaN-propagating). `a.min(b)` (pairwise) coexists with `arr.min()`
//! (reduction), dispatched by arity.


mod common;
use common::*;

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
fn float_transcendentals() {
    if !backend_available() {
        return;
    }
    // sqrt(2)=1.414…, floor(3.7)=3, ceil(3.2)=4, round(2.5)=3 (away from zero), trunc(3.9)=3,
    // pow(2,10)=1024.
    let src = "fn main() -> Result<(), Error> {\n  print((2.0).sqrt())\n  print((3.7).floor())\n  print((3.2).ceil())\n  print((2.5).round())\n  print((3.9).trunc())\n  print((2.0).pow(10.0))\n  return Ok(())\n}\n";
    let out = build_and_run("sm-float-fns", src);
    assert_eq!(out.status.code(), Some(0));
    assert_eq!(String::from_utf8_lossy(&out.stdout), "1.4142135623730951\n3.0\n4.0\n3.0\n3.0\n1024.0\n");
}

#[test]
fn sqrt_on_int_rejected() {
    // The transcendentals are float-only.
    assert!(check_errs("sm-int-sqrt", "fn main() -> i32 {\n  x: i32 := 4\n  return x.sqrt()\n}\n"));
}

#[test]
fn non_numeric_receiver_rejected() {
    // abs/min/max are numeric-only.
    assert!(check_errs("sm-bool", "fn main() -> i32 {\n  b := true\n  if b.abs() == 1 { return 1 }\n  return 0\n}\n"));
}
