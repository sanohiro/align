//! Integer bitwise & shift operators — `& | ^ << >>` and unary `~` (`draft.md` §5). Integer-only
//! (no implicit coercion; the shift amount shares the value's type). Precedence follows Go: shifts
//! and `&` bind like `*`, `|`/`^` like `+` — so every bitwise/shift operator binds tighter than a
//! comparison (no C `a & b == c` footgun). A shift amount is masked mod the value's bit width
//! (defined, zero-cost), `>>` is arithmetic on a signed value and logical on an unsigned one.

mod common;
use common::*;

#[test]
fn and_or_xor() {
    if !backend_available() {
        return;
    }
    let src = concat!(
        "fn main() -> i32 {\n",
        "  a: i32 := 12\n",
        "  b: i32 := 10\n",
        "  return (a & b) + (a | b) + (a ^ b)\n", // 8 + 14 + 6 = 28
        "}\n",
    );
    let out = build_and_run("bit-and-or-xor", src);
    assert_eq!(out.status.code(), Some(28));
}

#[test]
fn shifts() {
    if !backend_available() {
        return;
    }
    let src = concat!(
        "fn main() -> i32 {\n",
        "  return (1 << 5) + (64 >> 2)\n", // 32 + 16 = 48
        "}\n",
    );
    let out = build_and_run("bit-shifts", src);
    assert_eq!(out.status.code(), Some(48));
}

#[test]
fn complement() {
    if !backend_available() {
        return;
    }
    // ~0 = -1 (all bits set); ~5 = -6. (~0) & 0xFF = 255.
    let src = concat!(
        "fn main() -> i32 {\n",
        "  x: i32 := 5\n",
        "  return (~x) + 48\n", // -6 + 48 = 42
        "}\n",
    );
    let out = build_and_run("bit-complement", src);
    assert_eq!(out.status.code(), Some(42));
}

#[test]
fn bitwise_binds_tighter_than_comparison() {
    if !backend_available() {
        return;
    }
    // `a & b == 4` must parse as `(a & b) == 4` (Go precedence), not `a & (b == 4)`.
    let src = concat!(
        "fn main() -> i32 {\n",
        "  a: i32 := 6\n",
        "  b: i32 := 4\n",
        "  if a & b == 4 { return 7 }\n", // 6 & 4 = 4, == 4 → true
        "  return 0\n",
        "}\n",
    );
    let out = build_and_run("bit-precedence", src);
    assert_eq!(out.status.code(), Some(7));
}

#[test]
fn arithmetic_right_shift_sign_extends() {
    if !backend_available() {
        return;
    }
    // -16 >> 2 = -4 (arithmetic shift on a signed value); -4 + 46 = 42.
    let src = concat!(
        "fn main() -> i32 {\n",
        "  x: i32 := 0 - 16\n",
        "  return (x >> 2) + 46\n",
        "}\n",
    );
    let out = build_and_run("bit-ashr", src);
    assert_eq!(out.status.code(), Some(42));
}

#[test]
fn logical_right_shift_on_unsigned() {
    if !backend_available() {
        return;
    }
    // u32 0xFFFFFFF8 >> 1 = 0x7FFFFFFC (zero-fill, not sign-extend).
    let src = concat!(
        "fn main() -> i32 {\n",
        "  x: u32 := 4294967288\n",
        "  y: u32 := x >> 1\n",
        "  return y as i32 - 2147483644\n", // 0x7FFFFFFC - 0x7FFFFFFC = 0
        "}\n",
    );
    let out = build_and_run("bit-lshr", src);
    assert_eq!(out.status.code(), Some(0));
}

#[test]
fn a_bitwise_constant_is_folded() {
    if !backend_available() {
        return;
    }
    // Constants fold the bitwise/shift expression at compile time.
    let src = concat!(
        "FLAGS: i32 := 1 << 3 | 1\n", // 9
        "MASK: i32 := ~0\n",          // -1 (all bits set)
        "fn main() -> i32 {\n",
        "  return FLAGS + (MASK & 33)\n", // 9 + 33 = 42
        "}\n",
    );
    let out = build_and_run("bit-const", src);
    assert_eq!(out.status.code(), Some(42));
}

#[test]
fn bitwise_on_a_float_is_rejected() {
    assert!(check_errs(
        "bit-float",
        "fn main() -> i32 {\n  x: f64 := 1.0\n  y := x & x\n  return 0\n}\n",
    ));
}

#[test]
fn bitwise_on_a_bool_is_rejected() {
    // `&` is bitwise (integers); logical-and is `&&`. A bool needs `&&`.
    assert!(check_errs(
        "bit-bool",
        "fn main() -> i32 {\n  b := true & false\n  return 0\n}\n",
    ));
}

#[test]
fn complement_on_a_bool_is_rejected() {
    assert!(check_errs(
        "bit-not-bool",
        "fn main() -> i32 {\n  b := ~true\n  return 0\n}\n",
    ));
}

#[test]
fn nested_generics_still_close_with_adjacent_gt() {
    // Regression: `>>` is not a single token, so `Pair<Pair<i32>>` still parses (the shift is only
    // formed in expression position). This compiles past parsing (the field-type error is unrelated).
    let mut sm = SourceMap::new();
    let src = "Pair<T> { a: T, b: T }\nfn f(p: Pair<Pair<i32>>) -> i32 { return 0 }\nfn main() -> i32 { return 0 }\n";
    let checked = check(&mut sm, "nested", src);
    // Parsing must succeed; any diagnostics here are semantic, never "expected '>'".
    let rendered = align_driver::format_diagnostics(&sm, &checked.diags);
    assert!(!rendered.contains("expected '>'"), "nested generics failed to parse: {rendered}");
}
