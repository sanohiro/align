//! The "huge struct copy" lint (`draft.md` §16): a struct passed or returned **by value** is copied
//! in full at every call boundary. Above the threshold (two cache lines) this is a data-oriented
//! smell — narrow the struct (split hot/cold fields, `draft.md` §9) or pass a `slice`/view. It is a
//! **warning** (a perf hint), not a hard error, so the program still compiles and runs. The size is
//! a deterministic, profile-independent structural property, so the lint needs no `--profile` data.

mod common;
use common::*;

/// Whether checking `src` emits a "huge struct copy" diagnostic.
fn warns_huge(name: &str, src: &str) -> bool {
    let mut sm = SourceMap::new();
    let checked = check(&mut sm, name, src);
    align_driver::format_diagnostics(&sm, &checked.diags).contains("huge struct copy")
}

/// A struct of `n` `i64` fields (`8 * n` bytes) named `Big`.
fn big_struct(n: usize) -> String {
    let fields: Vec<String> = (0..n).map(|i| format!("  f{i}: i64,\n")).collect();
    format!("Big {{\n{}}}\n", fields.concat())
}

#[test]
fn a_huge_struct_param_warns() {
    // 17 × 8 = 136 bytes > the 128-byte threshold.
    let src = format!("{}fn sum0(b: Big) -> i64 = b.f0\nfn main() -> i32 = 0\n", big_struct(17));
    assert!(warns_huge("huge-param", &src));
}

#[test]
fn a_huge_struct_return_warns() {
    let src = format!(
        "{}fn make() -> Big = Big {{ {} }}\nfn main() -> i32 = 0\n",
        big_struct(17),
        (0..17).map(|i| format!("f{i}: 0")).collect::<Vec<_>>().join(", "),
    );
    assert!(warns_huge("huge-return", &src));
}

#[test]
fn a_small_struct_does_not_warn() {
    // 2 × 8 = 16 bytes — well under the threshold.
    let src = "Small { x: i64, y: i64 }\nfn dist(s: Small) -> i64 = s.x + s.y\nfn main() -> i32 = 0\n";
    assert!(!warns_huge("small-param", src));
}

#[test]
fn a_struct_at_the_threshold_does_not_warn() {
    // Exactly 128 bytes (16 × 8) is not "above" the threshold — the boundary is exclusive.
    let src = format!("{}fn use0(b: Big) -> i64 = b.f0\nfn main() -> i32 = 0\n", big_struct(16));
    assert!(!warns_huge("threshold-param", &src));
}

#[test]
fn the_lint_is_not_a_hard_error() {
    // A warning never fails compilation.
    let src = format!("{}fn sum0(b: Big) -> i64 = b.f0\nfn main() -> i32 = 0\n", big_struct(17));
    assert!(!check_errs("huge-not-error", &src));
}

#[test]
fn a_huge_struct_program_still_compiles_and_runs() {
    if !backend_available() {
        return;
    }
    // The warning is emitted but the program builds and runs normally (exit 18).
    let src = format!(
        concat!(
            "{}",
            "fn total(b: Big) -> i64 = b.f0 + b.f16\n",
            "fn main() -> i32 {{\n",
            "  b := Big {{ {} }}\n",
            "  return total(b) as i32\n",
            "}}\n",
        ),
        big_struct(17),
        (0..17).map(|i| format!("f{i}: {}", if i == 0 || i == 16 { i + 1 } else { 0 })).collect::<Vec<_>>().join(", "),
    );
    // f0 = 1, f16 = 17, sum = 18.
    let out = build_and_run("huge-run", &src);
    assert_eq!(out.status.code(), Some(18));
}
