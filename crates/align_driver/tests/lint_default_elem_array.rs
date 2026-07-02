//! The "wasteful default element type" lint (`draft.md` §16): a large literal array whose element
//! type is left unconstrained falls to the `i64` / `f64` default (8 bytes/element) even when the
//! data would fit a narrower type — an 8× memory/bandwidth cost at scale. Above a size threshold
//! (`DEFAULT_ELEM_LITERAL_ARRAY_LEN`, 64 elements) this emits a **warning** pointing at a narrower
//! annotation; the program still type-checks. A small array, an element type constrained by context
//! (a typed pipeline stage), or a concrete element type stays silent.

mod common;
use common::*;

/// A `fn build()` returning `arr[0]` of an `n`-element literal array of `elem` values (`"0"` /
/// `"0.0"`), plus a `main`. The binding `arr := […]` is where the element type is inferred.
fn prog(elem: &str, ret: &str, n: usize) -> String {
    let elems: Vec<String> = (0..n).map(|i| format!("{}{elem}", i % 7)).collect();
    format!(
        "fn build() -> {ret} {{\n  arr := [{}]\n  return arr[0]\n}}\nfn main() -> i32 = 0\n",
        elems.join(", "),
    )
}

/// The formatted diagnostics for checking `src` (warnings included).
fn diags(name: &str, src: &str) -> String {
    let mut sm = SourceMap::new();
    let checked = check(&mut sm, name, src);
    align_driver::format_diagnostics(&sm, &checked.diags)
}

/// Whether checking `src` emits an "unconstrained element type" diagnostic.
fn warns_default(name: &str, src: &str) -> bool {
    diags(name, src).contains("unconstrained element type")
}

// --- positive: a large unconstrained literal array warns --------------------------------------

#[test]
fn large_int_array_defaults_and_warns() {
    // 64 integer literals with no constraining context → default `i64`.
    let d = diags("big-int", &prog("", "i64", 64));
    assert!(d.contains("unconstrained element type"), "expected a warning, got:\n{d}");
    assert!(d.contains("64-element"), "message should state the size:\n{d}");
    assert!(d.contains("defaults to `i64`"), "message should name the default:\n{d}");
    assert!(d.contains("narrower integer type"), "message should suggest a fix:\n{d}");
}

#[test]
fn large_float_array_defaults_and_warns() {
    // 64 float literals → default `f64`; the suggested narrower type is `f32`.
    let d = diags("big-float", &prog(".0", "f64", 80));
    assert!(d.contains("defaults to `f64`"), "message should name the float default:\n{d}");
    assert!(d.contains("`f32`"), "message should suggest `f32`:\n{d}");
}

// --- negative: silent below threshold / when constrained / concrete ---------------------------

#[test]
fn small_array_does_not_warn() {
    // 63 elements is below the 64-element threshold.
    assert!(!warns_default("small-int", &prog("", "i64", 63)));
}

#[test]
fn constrained_element_type_does_not_warn() {
    // A named `map` stage fixes the element type from its parameter (`i32`), so the literal is not
    // an unconstrained default — even at 100 elements.
    let elems: Vec<String> = (0..100).map(|i| (i % 7).to_string()).collect();
    let src = format!(
        "fn dbl(x: i32) -> i32 = x * 2\nfn build() -> i32 = [{}].map(dbl).sum()\nfn main() -> i32 = 0\n",
        elems.join(", "),
    );
    assert!(!warns_default("constrained", &src));
}

#[test]
fn the_lint_is_not_a_hard_error() {
    assert!(!check_errs("default-not-error", &prog("", "i64", 64)));
}

#[test]
fn a_defaulting_program_still_compiles_and_runs() {
    if !backend_available() {
        return;
    }
    // The warning is emitted, but the program builds and runs — arr[3] = 3 % 7 = 3, so exit 3.
    // (`arr[3] as i32` narrows i64 → i32, which also warns; both warnings are harmless.)
    let elems: Vec<String> = (0..64).map(|i| (i % 7).to_string()).collect();
    let src = format!(
        "fn main() -> i32 {{\n  arr := [{}]\n  return arr[3] as i32\n}}\n",
        elems.join(", "),
    );
    let out = build_and_run("default-run", &src);
    assert_eq!(out.status.code(), Some(3));
}
