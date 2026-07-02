//! Radix-prefixed integer literals — `0x..` (hex), `0o..` (octal), `0b..` (binary) (`draft.md` §3).
//! They are ordinary integer literals (same `i128` value, width inferred from context, `_`
//! separators allowed), so — like any literal — a value that provably does not fit the inferred
//! type is a hard error (not a silent wrap; `draft.md` §5). They pair naturally with the
//! bitwise/shift operators.

mod common;
use common::*;

#[test]
fn hex_octal_binary_denote_the_same_value() {
    if !backend_available() {
        return;
    }
    let src = concat!(
        "fn main() -> i32 {\n",
        "  h: i32 := 0x2A\n",   // 42
        "  o: i32 := 0o52\n",   // 42
        "  b: i32 := 0b101010\n", // 42
        "  if h == o && o == b { return h }\n",
        "  return 0\n",
        "}\n",
    );
    let out = build_and_run("radix-eq", src);
    assert_eq!(out.status.code(), Some(42));
}

#[test]
fn underscores_separate_radix_digits() {
    if !backend_available() {
        return;
    }
    // 0xFF_FF = 65535; & 0x2A = 42.
    let src = "fn main() -> i32 {\n  m: i32 := 0xFF_FF\n  return m & 0x2A\n}\n";
    let out = build_and_run("radix-sep", src);
    assert_eq!(out.status.code(), Some(42));
}

#[test]
fn hex_pairs_with_bitwise() {
    if !backend_available() {
        return;
    }
    let src = "fn main() -> i32 {\n  flags := 0x1 | 0x8 | 0x20\n  return flags + 1\n}\n"; // 41 + 1
    let out = build_and_run("radix-bitwise", src);
    assert_eq!(out.status.code(), Some(42));
}

#[test]
fn hex_uppercase_digits_parse() {
    if !backend_available() {
        return;
    }
    // Uppercase hex digits (`0xAB`) parse to the same value as lowercase; 0xAB = 171 fits i32.
    let src = "fn main() -> i32 {\n  m: i32 := 0xAB\n  return m - 129\n}\n"; // 171 - 129 = 42
    let out = build_and_run("radix-upper", src);
    assert_eq!(out.status.code(), Some(42));
}

#[test]
fn hex_literal_overflowing_its_type_is_rejected() {
    // 0xFFFFFFFF = 4294967295 does not fit i32 — a provably-out-of-range literal is a hard error
    // (it would otherwise silently wrap to -1). Write `0xFFFFFFFF as i32` for the bit pattern.
    assert!(check_errs(
        "radix-overflow",
        "fn main() -> i32 {\n  m: i32 := 0xFFFFFFFF\n  return m\n}\n"
    ));
}

#[test]
fn an_invalid_digit_is_an_error() {
    // `2` is not a binary digit.
    assert!(check_errs("radix-bad", "fn main() -> i32 {\n  x := 0b12\n  return 0\n}\n"));
}

#[test]
fn an_empty_radix_literal_is_an_error() {
    assert!(check_errs("radix-empty", "fn main() -> i32 {\n  x := 0x\n  return 0\n}\n"));
}

#[test]
fn a_literal_suffix_is_rejected_with_a_hint() {
    // Align has no literal suffix (`10i32`); `as` is the one expression-position form. The lexer
    // rejects the suffix attempt rather than silently lexing `10` then an identifier `i32`.
    let mut sm = SourceMap::new();
    let checked = check(&mut sm, "suffix", "fn main() -> i32 {\n  z := 10i32\n  return z\n}\n");
    assert!(checked.diags.has_errors());
    let rendered = align_driver::format_diagnostics(&sm, &checked.diags);
    assert!(rendered.contains("10 as i32"), "expected an `as` hint, got: {rendered}");
}

#[test]
fn a_radix_literal_suffix_is_rejected_with_a_hint() {
    // `0x10i32` is a suffix attempt on a radix literal: the same `as` hint (not the generic
    // "invalid hex" error). `0xFFf64` stays a valid hex number (every char is a hex digit).
    let mut sm = SourceMap::new();
    let checked = check(&mut sm, "radix-suffix", "fn main() -> i32 {\n  z := 0x10i32\n  return 0\n}\n");
    assert!(checked.diags.has_errors());
    let rendered = align_driver::format_diagnostics(&sm, &checked.diags);
    assert!(rendered.contains("0x10 as i32"), "expected an `as` hint, got: {rendered}");
}
