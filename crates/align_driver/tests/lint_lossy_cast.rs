//! The "lossy conversion" lint (`draft.md` §16): a narrowing / precision-losing / saturating `as`
//! is **defined behavior** (no UB — int↔int wraps, float→int saturates), so it is never an error,
//! but silent loss of information is worth surfacing. This lint emits a **warning** for it; the
//! program still type-checks and runs. Lossless conversions (widening, same-width, a same-width
//! sign change like `u8 as i8` that keeps every bit) stay silent, and an unconstrained literal
//! source (`1 as i8`, an explicit annotation) is not flagged — the lint targets *typed* values.

mod common;
use common::*;

/// The formatted diagnostics for checking `src` (warnings included).
fn diags(name: &str, src: &str) -> String {
    let mut sm = SourceMap::new();
    let checked = check(&mut sm, name, src);
    align_driver::format_diagnostics(&sm, &checked.diags)
}

/// Whether checking `src` emits a "lossy conversion" diagnostic.
fn warns_lossy(name: &str, src: &str) -> bool {
    diags(name, src).contains("lossy conversion")
}

// --- positive: the four lossy categories warn -------------------------------------------------

#[test]
fn narrowing_int_to_int_warns() {
    // i64 → i8 truncates the high bits.
    let d = diags("narrow-int", "fn f(x: i64) -> i8 = x as i8\nfn main() -> i32 = 0\n");
    assert!(d.contains("lossy conversion"), "expected a warning, got:\n{d}");
    assert!(d.contains("`i64 as i8`"), "message should name the conversion:\n{d}");
    assert!(d.contains("truncates the high bits"), "message should explain the loss:\n{d}");
    assert!(d.contains("defined behavior, not an error"), "message should reassure it is defined:\n{d}");
}

#[test]
fn float_to_int_warns() {
    // f64 → i32 drops the fractional part and saturates out-of-range values.
    let d = diags("float-to-int", "fn f(x: f64) -> i32 = x as i32\nfn main() -> i32 = 0\n");
    assert!(d.contains("`f64 as i32`"), "message should name the conversion:\n{d}");
    assert!(d.contains("truncates the fractional part"), "message should explain the loss:\n{d}");
}

#[test]
fn wide_int_to_float_warns() {
    // i64 → f32: 64 bits > the f32 mantissa (24), so large values lose precision.
    let d = diags("int-to-f32", "fn f(x: i64) -> f32 = x as f32\nfn main() -> i32 = 0\n");
    assert!(d.contains("`i64 as f32`"), "message should name the conversion:\n{d}");
    assert!(d.contains("wider than the float's mantissa"), "message should explain the loss:\n{d}");
}

#[test]
fn narrowing_float_to_float_warns() {
    // f64 → f32 narrows a float and may lose precision.
    let d = diags("f64-to-f32", "fn f(x: f64) -> f32 = x as f32\nfn main() -> i32 = 0\n");
    assert!(d.contains("`f64 as f32`"), "message should name the conversion:\n{d}");
    assert!(d.contains("may lose precision"), "message should explain the loss:\n{d}");
}

#[test]
fn char_to_narrow_int_warns() {
    // char (32-bit code point) → i8 truncates the high bits.
    let d = diags("char-narrow", "fn f(c: char) -> i8 = c as i8\nfn main() -> i32 = 0\n");
    assert!(d.contains("`char as i8`"), "message should name the conversion:\n{d}");
    assert!(d.contains("truncates the high bits"), "message should explain the loss:\n{d}");
}

// --- negative: lossless conversions stay silent -----------------------------------------------

#[test]
fn widening_int_does_not_warn() {
    // i32 → i64 sign-extends; every value is preserved.
    assert!(!warns_lossy("widen-int", "fn f(x: i32) -> i64 = x as i64\nfn main() -> i32 = 0\n"));
}

#[test]
fn same_width_sign_change_does_not_warn() {
    // u8 → i8 keeps every bit (only the interpretation changes) — treated as lossless.
    assert!(!warns_lossy("sign-change", "fn f(x: u8) -> i8 = x as i8\nfn main() -> i32 = 0\n"));
}

#[test]
fn narrow_int_to_wide_float_does_not_warn() {
    // i32 → f64: 32 bits fit the f64 mantissa (53), so no precision is lost.
    assert!(!warns_lossy("i32-to-f64", "fn f(x: i32) -> f64 = x as f64\nfn main() -> i32 = 0\n"));
}

#[test]
fn narrow_int_to_char_does_not_warn() {
    // u8 → char widens an 8-bit value into a 32-bit code point (lossless).
    assert!(!warns_lossy("u8-to-char", "fn f(x: u8) -> char = x as char\nfn main() -> i32 = 0\n"));
}

#[test]
fn literal_source_is_not_flagged() {
    // `1 as i8` is an explicit annotation on an unconstrained literal, not a loss — stays silent.
    assert!(!warns_lossy("lit-annot", "fn main() -> i32 {\n  x := 1 as i8\n  return 0\n}\n"));
}

// --- the lint is a warning, not a hard error --------------------------------------------------

#[test]
fn the_lint_is_not_a_hard_error() {
    assert!(!check_errs("lossy-not-error", "fn f(x: i64) -> i8 = x as i8\nfn main() -> i32 = 0\n"));
}

#[test]
fn a_lossy_program_still_compiles_and_runs() {
    if !backend_available() {
        return;
    }
    // 300 (i64) as u8 = 44 — the warning is emitted, the program builds and runs (exit 44).
    let src = concat!(
        "fn main() -> i32 {\n",
        "  big: i64 := 300\n",
        "  return big as u8 as i32\n",
        "}\n",
    );
    let out = build_and_run("lossy-run", src);
    assert_eq!(out.status.code(), Some(44));
}
