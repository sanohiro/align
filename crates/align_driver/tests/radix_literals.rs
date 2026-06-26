//! Radix-prefixed integer literals — `0x..` (hex), `0o..` (octal), `0b..` (binary) (`draft.md` §3).
//! They are ordinary integer literals (same `i128` value, width inferred from context, `_`
//! separators allowed), so they truncate to a binding's width by the defined wrap rule like any
//! literal. They pair naturally with the bitwise/shift operators.

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
fn hex_uppercase_value_truncates_to_width() {
    if !backend_available() {
        return;
    }
    // 0xFFFFFFFF stored as i128 4294967295, narrowed to i32 = -1 (defined wrap). -1 + 43 = 42.
    let src = "fn main() -> i32 {\n  m: i32 := 0xFFFFFFFF\n  return m + 43\n}\n";
    let out = build_and_run("radix-trunc", src);
    assert_eq!(out.status.code(), Some(42));
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
