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
fn elementary_exp_log_family_lowers_to_exact_scalar_intrinsics() {
    if !backend_available() {
        return;
    }
    let mut source = String::new();
    for bits in [32, 64] {
        for operation in ["exp", "exp2", "log", "log2", "log10"] {
            source.push_str(&format!(
                "pub fn {operation}_{bits}(x: f{bits}) -> f{bits} = x.{operation}()\n"
            ));
        }
    }
    source.push_str("fn main() -> i32 = 0\n");
    let ir = raw_ir("sm-elementary-ir", &source);
    for bits in [32, 64] {
        for operation in ["exp", "exp2", "log", "log2", "log10"] {
            assert!(
                ir.contains(&format!("@llvm.{operation}.f{bits}")),
                "missing llvm.{operation}.f{bits}:\n{ir}",
            );
        }
    }
    assert!(!ir.contains("@llvm.pow"), "elementary functions must not decompose through pow:\n{ir}");
}

#[test]
fn elementary_exp_log_special_value_classes() {
    if !backend_available() {
        return;
    }
    let source = concat!(
        "fn main() -> i32 {\n",
        "  zero: f64 := 0.0\n",
        "  neg_zero: f64 := -0.0\n",
        "  one: f64 := 1.0\n",
        "  inf := one / zero\n",
        "  neg_inf := zero - inf\n",
        "  nan := zero / zero\n",
        "  high: f64 := 10000.0\n",
        "  low := zero - high\n",
        "  if zero.exp() != one || neg_zero.exp() != one || neg_inf.exp().to_bits() != 0 || high.exp() != inf || low.exp() < zero { return 1 }\n",
        "  if zero.exp2() != one || neg_zero.exp2() != one || neg_inf.exp2().to_bits() != 0 || high.exp2() != inf || low.exp2() < zero { return 2 }\n",
        "  if one.log() != zero || zero.log() != neg_inf || neg_zero.log() != neg_inf || inf.log() != inf || !neg_inf.log().is_nan() { return 3 }\n",
        "  if one.log2() != zero || zero.log2() != neg_inf || neg_zero.log2() != neg_inf || inf.log2() != inf || !neg_inf.log2().is_nan() { return 4 }\n",
        "  if one.log10() != zero || zero.log10() != neg_inf || neg_zero.log10() != neg_inf || inf.log10() != inf || !neg_inf.log10().is_nan() { return 5 }\n",
        "  if !(zero - one).log().is_nan() || !nan.exp().is_nan() || !nan.exp2().is_nan() || !nan.log().is_nan() || !nan.log2().is_nan() || !nan.log10().is_nan() { return 6 }\n",
        "  return 0\n",
        "}\n",
    );
    assert_eq!(build_and_run("sm-elementary-special", source).status.code(), Some(0));
}

#[test]
fn elementary_exp_log_diagnostics_check_receiver_before_arity() {
    let diagnostic = |name: &str, source: &str| {
        let mut sm = SourceMap::new();
        let checked = check(&mut sm, name, source);
        assert!(checked.diags.has_errors());
        align_driver::format_diagnostics(&sm, &checked.diags)
    };
    let receiver = diagnostic(
        "sm-elementary-receiver",
        "fn main() -> i32 {\n  x: i32 := 1\n  return x.exp(2.0) as i32\n}\n",
    );
    assert!(receiver.contains("'exp' needs a float"), "{receiver}");
    assert!(!receiver.contains("takes 0 argument"), "receiver error must win:\n{receiver}");

    let arity = diagnostic(
        "sm-elementary-arity",
        "fn main() -> i32 {\n  x: f64 := 1.0\n  return x.log10(2.0) as i32\n}\n",
    );
    assert!(arity.contains("'log10' takes 0 argument(s), got 1"), "{arity}");

    let array = diagnostic(
        "sm-elementary-array",
        "fn bad(a: array<f64>) -> f64 = a.log()\nfn main() -> i32 = 0\n",
    );
    assert!(array.contains("'log' needs a float"), "{array}");

    let mask = diagnostic(
        "sm-elementary-mask",
        "fn bad(m: mask4<f32>) -> mask4<f32> = m.exp2()\nfn main() -> i32 = 0\n",
    );
    assert!(mask.contains("'exp2' needs a float"), "{mask}");
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
fn float_relaxation_scope_emits_only_its_named_permissions() {
    if !backend_available() {
        return;
    }
    let ir = raw_ir(
        "sm-float-scope-ir",
        concat!(
            "pub fn strict(a: f64, b: f64, c: f64) -> f64 = a * b + c\n",
            "pub fn reassociated(a: f64, b: f64, c: f64) -> f64 = float(reassoc) { a + b + c }\n",
            "pub fn contracted(a: f64, b: f64, c: f64) -> f64 = float(contract) { a * b + c }\n",
            "pub fn plus_right(a: f64, b: f64, c: f64) -> f64 = float(contract) { c + a * b }\n",
            "pub fn minus_right(a: f64, b: f64, c: f64) -> f64 = float(contract) { a * b - c }\n",
            "pub fn both(a: f64, b: f64, c: f64) -> f64 = float(contract, reassoc) { c - a * b }\n",
            "pub fn vector_minus_right(v: vec4<f64>) -> vec4<f64> = float(contract) { v * 2.0 - 1.0 }\n",
            "pub fn vector_minus_left(v: vec4<f64>, w: vec4<f64>) -> vec4<f64> = float(contract) { v - w * 2.0 }\n",
            "fn main() -> i32 { return 0 }\n",
        ),
    );
    assert!(
        ir.contains("fadd reassoc double"),
        "reassoc was not emitted:\n{ir}"
    );
    assert_eq!(
        ir.matches("@llvm.fma.f64").count(),
        5,
        "four calls plus one declaration must name the explicit FMA intrinsic:\n{ir}",
    );
    assert_eq!(
        ir.matches("fmul double").count(),
        1,
        "only the strict function may retain a multiply; contracted pairs must not emit dead multiplies:\n{ir}",
    );
    assert!(
        ir.lines()
            .any(|line| line.contains("call double @llvm.fma.f64")),
        "contract-only FMA unexpectedly gained reassoc:\n{ir}",
    );
    assert!(
        ir.lines()
            .any(|line| line.contains("call reassoc double @llvm.fma.f64")),
        "the combined scope did not retain reassoc on its explicit FMA:\n{ir}",
    );
    assert!(
        ir.contains("@llvm.fma.v4f64"),
        "mixed vector/scalar subtraction did not preserve a valid vector FMA:\n{ir}",
    );
    assert!(
        !ir.lines()
            .any(|line| line.contains("fmul contract") || line.contains("fadd contract")),
        "raw LLVM contract must never be emitted:\n{ir}",
    );
    for forbidden in [" nnan", " ninf", " nsz", " arcp", " afn", " fast"] {
        assert!(
            !ir.contains(forbidden),
            "float scopes emitted the unnamed fast-math flag {forbidden:?}:\n{ir}",
        );
    }
    assert!(
        ir.lines()
            .any(|line| line.contains("fmul double") && !line.contains("reassoc")),
        "strict multiplication disappeared or gained a relaxed flag:\n{ir}",
    );

    let callable = raw_ir(
        "sm-float-scope-callable-root",
        concat!(
            "pub fn run(x: f64) -> f64 = float(contract) {\n",
            "  f := fn y: f64 { y * y + 1.0 }\n",
            "  f(x)\n",
            "}\n",
            "fn main() -> i32 { return 0 }\n",
        ),
    );
    assert!(
        !callable.contains("@llvm.fma"),
        "a lifted callable inherited its declaration-site float mode:\n{callable}",
    );

    let boundaries = raw_ir(
        "sm-float-scope-contract-boundaries",
        concat!(
            "pub fn strict_product(a: f64, b: f64, c: f64) -> f64 {\n",
            "  product := a * b\n",
            "  return float(contract) { product + c }\n",
            "}\n",
            "pub fn strict_consumer(a: f64, b: f64, c: f64) -> f64 {\n",
            "  product := float(contract) { a * b }\n",
            "  return product + c\n",
            "}\n",
            "pub fn indirect(a: f64, b: f64, c: f64) -> f64 = float(contract) {\n",
            "  product := a * b\n",
            "  product + c\n",
            "}\n",
            "fn main() -> i32 { return 0 }\n",
        ),
    );
    assert!(
        !boundaries.contains("@llvm.fma"),
        "strict or non-immediate multiplication crossed the contraction boundary:\n{boundaries}",
    );

    let restored = raw_ir(
        "sm-float-scope-restoration",
        concat!(
            "pub fn run(a: f64, b: f64, c: f64) -> f64 {\n",
            "  inside := float(reassoc) { a + b }\n",
            "  return inside + c\n",
            "}\n",
            "pub fn nested(a: f64, b: f64, c: f64) -> f64 = float(reassoc) {\n",
            "  float(contract) { a * b + c }\n",
            "}\n",
            "fn main() -> i32 { return 0 }\n",
        ),
    );
    assert!(restored.contains("fadd reassoc double"), "scope entry was lost:\n{restored}");
    assert!(
        restored.lines().any(|line| line.contains("fadd double") && !line.contains("reassoc")),
        "the mode was not restored after leaving the scope:\n{restored}",
    );
    assert!(
        restored
            .lines()
            .any(|line| line.contains("call reassoc double @llvm.fma.f64")),
        "nested scopes did not union their permissions:\n{restored}",
    );
}

#[test]
fn float_relaxation_keeps_strict_bits_and_nan_infinity_defined() {
    if !backend_available() {
        return;
    }
    let source = concat!(
        "fn left() -> f64 {\n  print(1)\n  return 2.0\n}\n",
        "fn middle() -> f64 {\n  print(2)\n  return 3.0\n}\n",
        "fn right() -> f64 {\n  print(3)\n  return 4.0\n}\n",
        "fn main() -> i32 {\n",
        "  ordered := float(contract) { left() * middle() + right() }\n",
        "  if ordered != 10.0 { return 4 }\n",
        "  strict := (10000000000000000.0 + (0.0 - 10000000000000000.0)) + 1.0\n",
        "  if strict.to_bits() != 4607182418800017408 { return 1 }\n",
        "  zero := 0.0\n",
        "  nan := zero / zero\n",
        "  if !float(reassoc) { nan + 1.0 }.is_nan() { return 2 }\n",
        "  infinity := 1.0 / zero\n",
        "  if !float(reassoc, contract) { infinity * 1.0 + zero }.is_infinite() { return 3 }\n",
        "  return 0\n",
        "}\n",
    );
    let output = build_and_run("sm-float-scope-defined", source);
    assert_eq!(output.status.code(), Some(0));
    assert_eq!(String::from_utf8_lossy(&output.stdout), "1\n2\n3\n");
}

#[test]
fn float_reduction_scopes_reach_sum_and_dot_owners() {
    if !backend_available() {
        return;
    }
    let ir = raw_ir(
        "sm-float-scope-reductions",
        concat!(
            "import core.json\n",
            "FloatRow { score: f64 }\n",
            "pub fn sum(borrow xs: slice<f64>) -> f64 = float(reassoc) { xs.sum() }\n",
            "pub fn vector(a: vec4<f64>, b: vec4<f64>) -> f64 = float(contract) { dot(a, b) }\n",
            "pub fn vector_sum(a: vec4<f64>) -> f64 = float(reassoc) { a.sum() }\n",
            "pub fn vector_sum_where(a: vec4<f64>, b: vec4<f64>) -> f64 = float(reassoc) { a.sum_where(a > b) }\n",
            "pub fn fixed() -> f64 = float(contract) { [1.0, 2.0].dot([3.0, 4.0]) }\n",
            "pub fn scanner() -> Result<f64, Error> {\n",
            "  rows: json.scanner<FloatRow> := json.scan(\"[{\\\"score\\\":1.0}]\")\n",
            "  return float(reassoc) { rows.score.sum() }\n",
            "}\n",
            "fn main() -> i32 { return 0 }\n",
        ),
    );
    assert!(
        ir.contains("fadd reassoc double"),
        "sum lost reassoc:\n{ir}"
    );
    assert!(
        ir.matches("call double @llvm.fma.f64").count() >= 5,
        "vector and fixed-array dot must select explicit per-term FMA:\n{ir}",
    );
    assert!(
        !ir.lines()
            .any(|line| line.contains("fmul contract") || line.contains("fadd contract")),
        "a reduction emitted raw LLVM contract:\n{ir}",
    );
}

#[test]
fn float_relaxation_diagnostics_have_stable_precedence() {
    let cases = [
        (
            "fn f() -> f64 = float() { missing }\n",
            "`float(...)` needs at least one option",
        ),
        (
            "fn f() -> f64 = float(reassoc, reassoc, mystery) { 0.0 }\n",
            "unknown floating-point relaxation option 'mystery'",
        ),
        (
            "fn f() -> f64 = float(mystery, contract, contract) { 0.0 }\n",
            "unknown floating-point relaxation option 'mystery'",
        ),
        (
            "fn f() -> f64 = float(contract, contract) { 0.0 }\n",
            "duplicate floating-point relaxation option 'contract'",
        ),
    ];
    for (source, expected_first) in cases {
        let mut sources = SourceMap::new();
        let checked = check(&mut sources, "sm-float-scope-error", source);
        let first = checked
            .diags
            .iter()
            .next()
            .expect("float scope must be rejected");
        assert!(
            first.message.contains(expected_first),
            "wrong first diagnostic: {:?}",
            checked
                .diags
                .iter()
                .map(|diagnostic| &diagnostic.message)
                .collect::<Vec<_>>(),
        );
    }
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
