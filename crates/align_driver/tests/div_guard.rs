//! Integer division / remainder guards. A raw LLVM `sdiv`/`udiv`/`srem`/`urem` is UB on a zero
//! divisor and on the signed `INT_MIN / -1` overflow, so MIR (`lower_int_div`) emits guards:
//! division by zero aborts (the settled "never silent" rule, via `align_rt_div_fail`), and
//! `INT_MIN / -1` wraps to the defined two's-complement result (`INT_MIN` for `/`, `0` for `%`).
//!
//! The `divide`/`modulo` helpers force a *runtime* division: a division of two literals is folded
//! at compile time (a constant `/0` is a compile error, `INT_MIN/-1` folds to `INT_MIN`), so the
//! runtime guard path is only reached when an operand is a function parameter.

mod common;
use common::*;

/// Lower `src` to MIR and render it as text (for asserting the guard is / isn't emitted).
fn mir_text(src: &str) -> String {
    let mut sm = SourceMap::new();
    let checked = check(&mut sm, "div.align", src);
    assert!(
        !checked.diags.has_errors(),
        "unexpected errors:\n{}",
        align_driver::format_diagnostics(&sm, &checked.diags)
    );
    align_mir::print::program_to_string(&lower_to_mir(&checked.hir))
}

#[test]
fn div_by_zero_aborts() {
    if !backend_available() {
        return;
    }
    let src = "fn divide(a: i32, b: i32) -> i32 = a / b\nfn main() -> i32 {\n  return divide(10, 0)\n}\n";
    let out = build_and_run("div-zero", src);
    // `std::process::abort()` kills the process with SIGABRT → no normal exit code.
    assert_eq!(out.status.code(), None, "division by zero must abort, not exit normally");
    assert!(
        String::from_utf8_lossy(&out.stderr).contains("division by zero"),
        "expected a division-by-zero panic message, got: {}",
        String::from_utf8_lossy(&out.stderr)
    );
}

#[test]
fn rem_by_zero_aborts() {
    if !backend_available() {
        return;
    }
    let src = "fn modulo(a: i32, b: i32) -> i32 = a % b\nfn main() -> i32 {\n  return modulo(10, 0)\n}\n";
    let out = build_and_run("rem-zero", src);
    assert_eq!(out.status.code(), None);
    assert!(String::from_utf8_lossy(&out.stderr).contains("division by zero"));
}

#[test]
fn unsigned_div_by_zero_aborts() {
    if !backend_available() {
        return;
    }
    let src = "fn divide(a: u32, b: u32) -> u32 = a / b\nfn main() -> i32 {\n  print(divide(10, 0))\n  return 0\n}\n";
    let out = build_and_run("udiv-zero", src);
    assert_eq!(out.status.code(), None);
    assert!(String::from_utf8_lossy(&out.stderr).contains("division by zero"));
}

#[test]
fn int_min_div_neg_one_wraps() {
    if !backend_available() {
        return;
    }
    // `INT_MIN / -1` overflows; it wraps to `INT_MIN` (like any defined two's-complement overflow),
    // and `INT_MIN % -1 == 0`. `1 << 31` is the i32 INT_MIN; `0 - 1` is -1. Both go through the
    // runtime helpers so the select-based wrap is exercised (not const-folded away).
    let src = "fn divide(a: i32, b: i32) -> i32 = a / b\nfn modulo(a: i32, b: i32) -> i32 = a % b\nfn main() -> Result<(), Error> {\n  print(divide(1 << 31, 0 - 1))\n  print(modulo(1 << 31, 0 - 1))\n  return Ok(())\n}\n";
    let out = build_and_run("imin-div", src);
    assert_eq!(out.status.code(), Some(0));
    assert_eq!(String::from_utf8_lossy(&out.stdout), "-2147483648\n0\n");
}

#[test]
fn normal_division_regression() {
    if !backend_available() {
        return;
    }
    // Ordinary (non-zero, non-overflowing) division must be unaffected by the guards. Signed
    // truncates toward zero; unsigned is a plain divide. All routed through runtime helpers.
    let src = "fn sdiv(a: i32, b: i32) -> i32 = a / b\nfn srem(a: i32, b: i32) -> i32 = a % b\nfn udiv(a: u32, b: u32) -> u32 = a / b\nfn main() -> Result<(), Error> {\n  print(sdiv(7, 2))\n  print(srem(7, 2))\n  print(sdiv(0 - 7, 2))\n  print(srem(0 - 7, 2))\n  print(udiv(100, 7))\n  return Ok(())\n}\n";
    let out = build_and_run("div-normal", src);
    assert_eq!(out.status.code(), Some(0));
    assert_eq!(String::from_utf8_lossy(&out.stdout), "3\n1\n-3\n-1\n14\n");
}

#[test]
fn constant_div_by_zero_is_compile_error() {
    // In a *constant* initializer (folded at compile time), division by zero is a compile error
    // rather than a deferred runtime abort — still "never silent", just caught earlier. (In a
    // function body the same `1 / 0` is a runtime division and aborts, as tested above.)
    assert!(check_errs("const-div-zero", "BAD: i32 := 1 / 0\nfn main() -> i32 { return BAD }\n"));
}

// --- constant-divisor fast path: a known non-zero divisor needs no runtime guard ---

#[test]
fn constant_divisor_skips_the_runtime_guard() {
    // `x / 2` and `x % 10` have a compile-time-known non-zero, non-(-1) divisor, so no zero guard
    // (`div_fail`) or `-1` remap is emitted — just the raw op.
    let cst = mir_text("fn f(x: i32) -> i32 = x / 2\nfn g(x: i32) -> i32 = x % 10\nfn main() -> i32 { return f(9) + g(9) }\n");
    assert!(!cst.contains("div_fail"), "constant divisor must not emit a runtime guard:\n{cst}");

    // A runtime (parameter) divisor still gets the guard, for contrast.
    let dyn_ = mir_text("fn f(x: i32, y: i32) -> i32 = x / y\nfn main() -> i32 { return f(9, 3) }\n");
    assert!(dyn_.contains("div_fail"), "a runtime divisor must still emit the guard:\n{dyn_}");
}

#[test]
fn constant_neg_one_divisor_skips_the_guard() {
    // A constant `-1` divisor (from a folded top-level constant reference — a literal `-1` in a
    // body is a `Unary` node, not a constant) is folded directly (`x / -1 == -x`, `x % -1 == 0`)
    // with no zero guard and no `-1` remap select.
    let text = mir_text("NEG: i32 := -1\nfn f(x: i32) -> i32 = x / NEG\nfn g(x: i32) -> i32 = x % NEG\nfn main() -> i32 { return f(5) + g(5) }\n");
    assert!(!text.contains("div_fail"), "constant -1 divisor must not emit a guard:\n{text}");
}

#[test]
fn constant_divisor_computes_correctly() {
    if !backend_available() {
        return;
    }
    // Constant divisors across widths (the argument is a runtime parameter so the division is real,
    // not const-folded). Signed truncates toward zero.
    let src = "fn di8(x: i8) -> i8 = x / 3\nfn ri8(x: i8) -> i8 = x % 3\nfn di32(x: i32) -> i32 = x / 10\nfn ri32(x: i32) -> i32 = x % 10\nfn di64(x: i64) -> i64 = x / 1000\nfn du32(x: u32) -> u32 = x / 7\nfn main() -> Result<(), Error> {\n  print(di8(100))\n  print(ri8(100))\n  print(di32(0 - 105))\n  print(ri32(0 - 105))\n  print(di64(123456))\n  print(du32(100))\n  return Ok(())\n}\n";
    let out = build_and_run("div-const", src);
    assert_eq!(out.status.code(), Some(0));
    // 100/3=33, 100%3=1, -105/10=-10, -105%10=-5, 123456/1000=123, 100/7=14.
    assert_eq!(String::from_utf8_lossy(&out.stdout), "33\n1\n-10\n-5\n123\n14\n");
}

#[test]
fn constant_neg_one_divisor_folds_correctly() {
    if !backend_available() {
        return;
    }
    // A constant `-1` divisor: `x / -1 == -x` (INT_MIN wraps back to INT_MIN), `x % -1 == 0`.
    let src = "NEG: i32 := -1\nfn dneg(x: i32) -> i32 = x / NEG\nfn mneg(x: i32) -> i32 = x % NEG\nfn main() -> Result<(), Error> {\n  print(dneg(1 << 31))\n  print(dneg(42))\n  print(mneg(1 << 31))\n  print(mneg(42))\n  return Ok(())\n}\n";
    let out = build_and_run("div-const-neg1", src);
    assert_eq!(out.status.code(), Some(0));
    // INT_MIN / -1 = INT_MIN, 42 / -1 = -42, INT_MIN % -1 = 0, 42 % -1 = 0.
    assert_eq!(String::from_utf8_lossy(&out.stdout), "-2147483648\n-42\n0\n0\n");
}
