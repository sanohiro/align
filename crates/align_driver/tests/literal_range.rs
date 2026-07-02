//! Out-of-range compile-time integer literals are a hard error (`draft.md` §5, "Integer Literals").
//! When a literal's value and its type are both known at compile time, a value that does not fit the
//! type would otherwise silently two's-complement wrap — hidden data corruption, at odds with
//! "nothing hidden". Runtime arithmetic overflow still wraps (unchanged); this is a *static* check of
//! literals only, in every context where a literal is directly typed (let, argument, field, array
//! element, return). A negated literal (`-128`) is checked at its effective value, so the signed
//! minimum is accepted while the same magnitude as a positive literal is rejected.

mod common;
use common::*;

// --- rejected: a value that overflows its resolved type ---

#[test]
fn u8_let_out_of_range_is_rejected() {
    assert!(check_errs("lit-u8-300", "fn main() -> i32 {\n  x: u8 := 300\n  return 0\n}\n"));
}

#[test]
fn i8_let_out_of_range_is_rejected() {
    // 200 > i8::MAX (127).
    assert!(check_errs("lit-i8-200", "fn main() -> i32 {\n  x: i8 := 200\n  return 0\n}\n"));
}

#[test]
fn u32_let_two_pow_32_is_rejected() {
    // 2^32 = 4294967296 = u32::MAX + 1.
    assert!(check_errs("lit-u32-2p32", "fn main() -> i32 {\n  x: u32 := 4294967296\n  return 0\n}\n"));
}

#[test]
fn u8_one_past_max_is_rejected() {
    assert!(check_errs("lit-u8-256", "fn main() -> i32 {\n  x: u8 := 256\n  return 0\n}\n"));
}

#[test]
fn i8_positive_min_magnitude_is_rejected() {
    // `128` as a *positive* literal overflows i8 (max 127); only `-128` is in range (see below).
    assert!(check_errs("lit-i8-128", "fn main() -> i32 {\n  x: i8 := 128\n  return 0\n}\n"));
}

#[test]
fn out_of_range_function_argument_is_rejected() {
    let src = "fn g(a: u8) -> i32 = 0\nfn main() -> i32 {\n  return g(300)\n}\n";
    assert!(check_errs("lit-arg-u8", src));
}

#[test]
fn out_of_range_struct_field_is_rejected() {
    let src = "P { x: u8 }\nfn main() -> i32 {\n  p := P{x: 300}\n  return 0\n}\n";
    assert!(check_errs("lit-field-u8", src));
}

#[test]
fn out_of_range_array_element_is_rejected() {
    // The `.sum()` terminal's expected type (u8) drives the element type, so `300` is an out-of-range
    // u8 element. The in-range sibling below confirms the construct itself is valid.
    let bad = "fn s() -> u8 {\n  return [1, 2, 300].sum()\n}\nfn main() -> i32 = 0\n";
    assert!(check_errs("lit-arr-u8", bad));
    let ok = "fn s() -> u8 {\n  return [1, 2, 3].sum()\n}\nfn main() -> i32 = 0\n";
    assert!(!check_errs("lit-arr-u8-ok", ok));
}

#[test]
fn out_of_range_return_value_is_rejected() {
    assert!(check_errs("lit-ret-u8", "fn f() -> u8 = 300\nfn main() -> i32 = 0\n"));
}

// --- accepted: values at the exact type boundaries ---

#[test]
fn signed_boundaries_are_accepted() {
    // i64::MIN and i64::MAX, i8::MIN (`-128`), i8::MAX.
    let src = concat!(
        "fn main() -> i32 {\n",
        "  a: i64 := -9223372036854775808\n",
        "  b: i64 := 9223372036854775807\n",
        "  c: i8 := -128\n",
        "  d: i8 := 127\n",
        "  return 0\n",
        "}\n",
    );
    assert!(!check_errs("lit-signed-bounds", src));
}

#[test]
fn unsigned_boundaries_are_accepted() {
    // u8::MAX and u64::MAX.
    let src = concat!(
        "fn main() -> i32 {\n",
        "  a: u8 := 255\n",
        "  b: u64 := 18446744073709551615\n",
        "  return 0\n",
        "}\n",
    );
    assert!(!check_errs("lit-unsigned-bounds", src));
}

#[test]
fn unconstrained_literal_defaults_to_i64_and_is_accepted() {
    // `x := 300` has no annotation, so it defaults to i64 — 300 fits, no error.
    assert!(!check_errs("lit-default-i64", "fn main() -> i32 {\n  x := 300\n  return 0\n}\n"));
}

#[test]
fn negated_literal_below_signed_min_is_rejected() {
    // `-200` < i8::MIN (-128); the effective (negated) value is range-checked.
    assert!(check_errs("lit-i8-neg200", "fn main() -> i32 {\n  x: i8 := -200\n  return 0\n}\n"));
}

// --- regression: the pre-existing negative-into-unsigned rejection still fires (and only once) ---

#[test]
fn negative_into_unsigned_still_rejected() {
    assert!(check_errs("lit-neg-u8", "fn main() -> i32 {\n  x: u8 := -1\n  return 0\n}\n"));
}
