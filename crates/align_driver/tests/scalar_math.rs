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

// ---------------------------------------------------------------------------------------------
// One min/max lowering (#1082 Part 1). The scalar method, the explicit vector lane reduction and
// the pipeline `min`/`max` terminal are the same operation, so they emit the same intrinsic:
// `llvm.{s,u}{min,max}` for integers and `llvm.minimum`/`llvm.maximum` (IEEE 754-2019) for floats.
// The pipeline terminal previously lowered to `fcmp ogt` + `select`, a different operation with no
// NaN semantics attached — which silently skipped NaN elements and left LLVM unable to form a
// reduction from it.
// ---------------------------------------------------------------------------------------------

/// Raw (pre-optimization) IR for one source, so the assertion pins what codegen EMITTED rather
/// than what the target's optimizer happened to do with it. Architecture-independent.
fn raw_ir(name: &str, src: &str) -> String {
    let mut sm = SourceMap::new();
    let checked = check(&mut sm, name, src);
    assert!(
        !checked.diags.has_errors(),
        "{}",
        align_driver::format_diagnostics(&sm, &checked.diags)
    );
    let mir = lower_to_mir(&checked.hir);
    emit_llvm_ir(&mir, BuildTarget::Baseline, Profile::Release, false, &[], false)
        .expect("emit llvm ir")
}

#[test]
fn the_pipeline_terminal_emits_the_same_intrinsic_as_the_scalar_method() {
    if !backend_available() {
        return;
    }
    let float = raw_ir(
        "sm-pipe-max-f32",
        "pub fn run(borrow xs: slice<f32>) -> f32 = xs.max()\nfn main() -> i32 { return 0 }\n",
    );
    assert!(
        float.contains("llvm.maximum.f32"),
        "the float pipeline terminal must lower to `llvm.maximum`:\n{float}"
    );
    assert!(
        !float.contains("fcmp ogt"),
        "the compare+select spelling must be gone — it is a different operation:\n{float}"
    );
    let float_min = raw_ir(
        "sm-pipe-min-f64",
        "pub fn run(borrow xs: slice<f64>) -> f64 = xs.min()\nfn main() -> i32 { return 0 }\n",
    );
    assert!(
        float_min.contains("llvm.minimum.f64"),
        "the float pipeline terminal must lower to `llvm.minimum`:\n{float_min}"
    );
    // Integers use the same MathFn path; signedness picks `smax` vs `umax`.
    let signed = raw_ir(
        "sm-pipe-max-i64",
        "pub fn run(borrow xs: slice<i64>) -> i64 = xs.max()\nfn main() -> i32 { return 0 }\n",
    );
    assert!(
        signed.contains("llvm.smax.i64"),
        "a signed integer terminal must lower to `llvm.smax`:\n{signed}"
    );
    let unsigned = raw_ir(
        "sm-pipe-min-u32",
        "pub fn run(borrow xs: slice<u32>) -> u32 = xs.min()\nfn main() -> i32 { return 0 }\n",
    );
    assert!(
        unsigned.contains("llvm.umin.i32"),
        "an unsigned integer terminal must lower to `llvm.umin`:\n{unsigned}"
    );
}

#[test]
fn a_masked_pipeline_terminal_uses_the_same_intrinsic_after_its_lane_select() {
    if !backend_available() {
        return;
    }
    // `where` still selects a rejected element to the fold seed (the extreme that can never win)
    // and then reduces with the one min/max operation.
    let ir = raw_ir(
        "sm-pipe-where-max",
        concat!(
            "fn big(x: f32) -> bool = x > 2.0\n",
            "pub fn run(borrow xs: slice<f32>) -> f32 = xs.where(big).max()\n",
            "fn main() -> i32 { return 0 }\n",
        ),
    );
    assert!(
        ir.contains("llvm.maximum.f32"),
        "a masked float terminal must also lower to `llvm.maximum`:\n{ir}"
    );
}

#[test]
fn pipeline_and_scalar_min_max_agree_on_nan_and_signed_zero() {
    if !backend_available() {
        return;
    }
    // The unified semantics, observable: `llvm.minimum`/`llvm.maximum` propagate NaN and order ±0
    // deterministically, and the pipeline terminal now says exactly what `a.max(b)` says.
    // `min(-0.0, +0.0)` is `-0.0`, whose f64 bits printed as i64 are i64::MIN.
    let src = concat!(
        "fn pmax(borrow xs: slice<f64>) -> f64 = xs.max()\n",
        "fn pmin(borrow xs: slice<f64>) -> f64 = xs.min()\n",
        "fn main() -> Result<(), Error> {\n",
        "  zero := 0.0\n",
        "  nan := zero / zero\n",
        "  a := [1.0, nan, 3.0]\n",
        "  s : slice<f64> := a[0..3]\n",
        "  print(pmax(s).is_nan())\n",
        "  print(pmin(s).is_nan())\n",
        "  print((1.0).max(nan).is_nan())\n",
        "  allnan := [nan, nan]\n",
        "  t : slice<f64> := allnan[0..2]\n",
        "  print(pmax(t).is_nan())\n",
        "  z := [-0.0, 0.0]\n",
        "  u : slice<f64> := z[0..2]\n",
        "  print(pmax(u).to_bits() as i64)\n",
        "  print(pmin(u).to_bits() as i64)\n",
        "  return Ok(())\n",
        "}\n",
    );
    let out = build_and_run("sm-pipe-nan", src);
    assert_eq!(out.status.code(), Some(0));
    assert_eq!(
        String::from_utf8_lossy(&out.stdout),
        "true\ntrue\ntrue\ntrue\n0\n-9223372036854775808\n"
    );
}

#[test]
fn pipeline_min_max_values_are_unchanged_for_ordinary_numbers() {
    if !backend_available() {
        return;
    }
    // The ordinary (NaN-free) results, including the empty-pipeline fold seed, are exactly what
    // they were before the unification.
    let src = concat!(
        "fn pmax(borrow xs: slice<i64>) -> i64 = xs.max()\n",
        "fn pmin(borrow xs: slice<i64>) -> i64 = xs.min()\n",
        "fn fmax(borrow xs: slice<f64>) -> f64 = xs.max()\n",
        "fn main() -> Result<(), Error> {\n",
        "  a := [3, -7, 11, 0]\n",
        "  s : slice<i64> := a[0..4]\n",
        "  print(pmax(s))\n",
        "  print(pmin(s))\n",
        "  e : slice<i64> := a[0..0]\n",
        "  print(pmax(e))\n",
        "  print(pmin(e))\n",
        "  f := [1.5, -2.5, 9.25]\n",
        "  g : slice<f64> := f[0..3]\n",
        "  print(fmax(g))\n",
        "  return Ok(())\n",
        "}\n",
    );
    let out = build_and_run("sm-pipe-ordinary", src);
    assert_eq!(out.status.code(), Some(0));
    assert_eq!(
        String::from_utf8_lossy(&out.stdout),
        "11\n-7\n-9223372036854775808\n9223372036854775807\n9.25\n"
    );
}
