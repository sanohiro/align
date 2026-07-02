//! Explicit numeric/char conversion — `x as T` (`draft.md` §3). The language's *only* conversion:
//! there is no implicit coercion, so widening an `i32` to `i64`, truncating, int↔float, and
//! char↔int code-point conversions all go through `as`. Conversions are zero-UB by design —
//! int↔int wraps/extends with defined two's-complement semantics, and float→int *saturates*
//! (out-of-range clamps to MIN/MAX, NaN → 0).

mod common;
use common::*;

#[test]
fn widen_and_truncate_round_trip() {
    if !backend_available() {
        return;
    }
    // i32 → i64 (widen, the canonical gap), arithmetic in i64, then i64 → i32 (truncate back).
    let src = concat!(
        "fn main() -> i32 {\n",
        "  a: i32 := 300\n",
        "  b: i64 := a as i64\n",
        "  c: i64 := b * 1000000\n",      // 300_000_000, fits i32
        "  return c as i32 - 299999997\n", // 300000000 - 299999997 = 3
        "}\n",
    );
    let out = build_and_run("cast-widen", src);
    assert_eq!(out.status.code(), Some(3));
}

#[test]
fn signed_widening_sign_extends() {
    if !backend_available() {
        return;
    }
    // A negative i32 widened to i64 must sign-extend (not zero-extend), so the round trip is -5.
    let src = concat!(
        "fn main() -> i32 {\n",
        "  neg: i32 := 0 - 5\n",
        "  w: i64 := neg as i64\n",
        "  return w as i32 + 12\n", // -5 + 12 = 7
        "}\n",
    );
    let out = build_and_run("cast-sext", src);
    assert_eq!(out.status.code(), Some(7));
}

#[test]
fn narrowing_wraps_two_s_complement() {
    if !backend_available() {
        return;
    }
    // 300 as u8 = 300 mod 256 = 44 (defined wrap, no UB).
    let src = "fn main() -> i32 {\n  big: i32 := 300\n  return big as u8 as i32\n}\n";
    let out = build_and_run("cast-wrap", src);
    assert_eq!(out.status.code(), Some(44));
}

#[test]
fn float_to_int_truncates_toward_zero() {
    if !backend_available() {
        return;
    }
    // 3.9 as i32 = 3 (truncation toward zero, like Rust's `as`).
    let src = "fn main() -> i32 {\n  x: f64 := 3.9\n  return x as i32\n}\n";
    let out = build_and_run("cast-f2i", src);
    assert_eq!(out.status.code(), Some(3));
}

#[test]
fn float_to_int_saturates_no_ub() {
    if !backend_available() {
        return;
    }
    // 1e20 is far beyond i32::MAX; the cast saturates to i32::MAX (2147483647) instead of UB.
    let src = "fn main() -> i32 {\n  big: f64 := 1e20\n  return big as i32 - 2147483640\n}\n";
    let out = build_and_run("cast-sat", src);
    assert_eq!(out.status.code(), Some(7)); // 2147483647 - 2147483640 = 7
}

#[test]
fn int_to_float_and_back() {
    if !backend_available() {
        return;
    }
    // i32 → f32 → f64 → i32 round trip of an exactly representable value.
    let src = concat!(
        "fn main() -> i32 {\n",
        "  a: i32 := 42\n",
        "  f: f32 := a as f32\n",
        "  g: f64 := f as f64\n",
        "  return g as i32\n",
        "}\n",
    );
    let out = build_and_run("cast-i2f", src);
    assert_eq!(out.status.code(), Some(42));
}

#[test]
fn char_and_int_code_point() {
    if !backend_available() {
        return;
    }
    // int → char → int via the code point: 65 ↔ 'A'.
    let src = concat!(
        "fn main() -> i32 {\n",
        "  n: i64 := 65\n",
        "  ch: char := n as char\n",
        "  return ch as i32\n",
        "}\n",
    );
    let out = build_and_run("cast-char", src);
    assert_eq!(out.status.code(), Some(65));
}

#[test]
fn cast_from_bool_rejected() {
    // `bool` is not numeric/char — `as` does not apply.
    let src = "fn main() -> i32 {\n  b: bool := true\n  return b as i32\n}\n";
    assert!(check_errs("cast-bool", src));
}

#[test]
fn cast_to_struct_rejected() {
    // The target must be a primitive numeric/char type.
    let src = "Point { x: i32 }\nfn main() -> i32 {\n  n: i32 := 1\n  p := n as Point\n  return 0\n}\n";
    assert!(check_errs("cast-struct", src));
}

#[test]
fn cast_char_to_float_rejected() {
    // `char` converts only through integers, never directly to/from a float.
    let src = "fn main() -> i32 {\n  c: char := 'A'\n  f: f64 := c as f64\n  return 0\n}\n";
    assert!(check_errs("cast-charfloat", src));
}

// --- Audit 1-1: unary `-` on an unsigned type is rejected (the sign would be silently lost:
//     `-5` typed `u32` prints `4294967291`). Only unary negation is caught — the defined u32
//     wrapping subtraction and an explicit `as u32` conversion are unchanged. ---

#[test]
fn negate_unsigned_literal_is_rejected() {
    assert!(check_errs("neg-u32-annot", "fn main() -> i32 {\n  x: u32 := -5\n  print(x)\n  return 0\n}\n"));
}

#[test]
fn negate_into_unsigned_param_is_rejected() {
    assert!(check_errs("neg-u32-param", "fn g(x: u32) -> u32 = x\nfn main() -> i32 {\n  print(g(-5))\n  return 0\n}\n"));
}

#[test]
fn negate_signed_is_fine() {
    if !backend_available() {
        return;
    }
    // `-5` at a signed type prints `-5` — the whole point (exit code carries the low byte).
    let out = build_and_run("neg-i32", "fn main() -> i32 {\n  x: i32 := -5\n  return 0 - x\n}\n");
    assert_eq!(out.status.code(), Some(5));
}

#[test]
fn explicit_cast_of_negative_to_unsigned_is_fine() {
    if !backend_available() {
        return;
    }
    // An *explicit* `(-5) as u32` is the sanctioned conversion (4294967291) — not caught, because the
    // inner `-5` is signed and only the cast changes its type. Round-trip back to a small i32.
    let out = build_and_run("neg-cast", "fn main() -> i32 {\n  x: i32 := -5\n  y := x as u32\n  return (y as i64 - 4294967290) as i32\n}\n");
    assert_eq!(out.status.code(), Some(1));
}

#[test]
fn unsigned_wrapping_subtraction_still_allowed() {
    if !backend_available() {
        return;
    }
    // `a - b` on `u32` is defined two's-complement wrap (a Binary op, not unary negation) — allowed.
    let out = build_and_run("u32-wrap-sub", "fn main() -> i32 {\n  a: u32 := 6\n  b: u32 := 1\n  return (a - b) as i32\n}\n");
    assert_eq!(out.status.code(), Some(5));
}

#[test]
fn negate_unsigned_constant_is_rejected() {
    // gemini #277: the const-fold path (`ConstEval`) is separate from `finalize_expr`, so a
    // top-level constant `X: u32 := -5` must be rejected there too (else it silently wraps at
    // compile time). A default-typed / signed constant negation stays fine.
    assert!(check_errs("neg-u32-const", "X: u32 := -5\nfn main() -> i32 { return 0 }\n"));
    assert!(!check_errs("neg-default-const", "X := -5\nY: i32 := -5\nfn main() -> i32 { return 0 }\n"));
}
