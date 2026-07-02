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
