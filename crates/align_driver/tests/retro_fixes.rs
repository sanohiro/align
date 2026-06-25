//! Regression tests for bugs surfaced by the retroactive Gemini reviews of M0–M5-A
//! (PRs #18–#36, all closed unmerged — the code was already in `main`). Each test pins a
//! specific finding that was still live in `main` at review time.


mod common;
use common::*;

/// Type-check `src` and return whether it reported an error. The point of the panic-guard
/// tests is that this returns (with `true`) instead of crashing the compiler.
// --- #18a: a `//` comment line must not be read as a `/` line-continuation ---

#[test]
fn comment_line_between_statements_terminates() {
    if !backend_available() {
        return;
    }
    // The comment line sits between two statements; before the fix its leading `/` was
    // mistaken for a division continuation, gluing `return x` onto `x := 42`.
    let src = "fn main() -> i32 {\n  x := 42\n  // a comment on its own line\n  return x\n}\n";
    let out = build_and_run("comment-line", src);
    assert_eq!(out.status.code(), Some(42));
}

#[test]
fn division_continuation_still_works() {
    if !backend_available() {
        return;
    }
    // A genuine `/`-led continuation line must still join (no false comment match).
    let src = "fn main() -> i32 {\n  return 84\n    / 2\n}\n";
    let out = build_and_run("div-cont", src);
    assert_eq!(out.status.code(), Some(42));
}

// --- #22: float `!=` must use UNE so `NaN != NaN` is true ---

#[test]
fn float_ne_lowers_to_une() {
    if !backend_available() {
        return;
    }
    let mut sm = SourceMap::new();
    let src = "fn main() -> i32 {\n  a := 1.0\n  b := 2.0\n  if a != b { return 1 }\n  return 0\n}\n";
    let checked = check(&mut sm, "fne.align", src);
    assert!(!checked.diags.has_errors());
    let ir = emit_llvm_ir(&lower_to_mir(&checked.hir)).expect("llvm ir");
    assert!(
        ir.contains("fcmp une"),
        "float != must lower to `fcmp une` (IEEE 754 NaN-correct), got:\n{ir}"
    );
    assert!(!ir.contains("fcmp one"), "must not use ONE for !=");
}

// --- #34: an arbitrary array-valued expression as a pipeline / slice source is rejected
//          cleanly (it used to panic the MIR lowering with `unreachable!`) ---

#[test]
fn if_array_pipeline_source_is_rejected_not_panicked() {
    let src = "fn main() -> i32 {\n  return (if true { [1, 2, 3] } else { [4, 5, 6] }).sum()\n}\n";
    assert!(check_errs("if-arr-pipe", src));
}

#[test]
fn if_array_coerced_to_slice_is_rejected_not_panicked() {
    let src = "fn total(xs: slice<i32>) -> i32 = xs.sum()\nfn main() -> i32 {\n  return total(if true { [1, 2, 3] } else { [4, 5, 6] })\n}\n";
    assert!(check_errs("if-arr-slice", src));
}

// --- #33: a struct-array pipeline stage that has nothing scalar loaded is rejected cleanly
//          (it used to panic `cur.take().expect("map before ...")`). `map(f)` over a whole
//          struct is now *supported* (loaded by value); the cases below are the ones that stay
//          rejected: a `.field` projection after `map` (it reads the source, not the map result),
//          and `where(structfn)` over a whole struct (use `where(.field)` or project first). ---

#[test]
fn field_projection_after_map_is_rejected_not_panicked() {
    let src = "Point { x: i32, y: i32 }\nfn bump(p: Point) -> Point = p\nfn main() -> i32 {\n  return [Point { x: 1, y: 2 }].map(bump).x.sum()\n}\n";
    assert!(check_errs("map-struct", src));
}

#[test]
fn where_over_struct_element_now_compiles() {
    if !backend_available() {
        return;
    }
    // `where(structfn)` over a whole struct element is now a supported feature (a multi-field
    // predicate), no longer the rejected/panicking case it was at #33. `keep` is always true, so
    // the element survives; `.x` projects (where leaves the element unchanged) → sum = 1.
    let src = "Point { x: i32, y: i32 }\nfn keep(p: Point) -> bool = true\nfn main() -> i32 {\n  return [Point { x: 1, y: 2 }].where(keep).x.sum()\n}\n";
    let out = build_and_run("where-struct", src);
    assert_eq!(out.status.code(), Some(1));
}

// --- #19/#22: unifying two unconstrained int vars links them, so constraining one later
//              constrains both (no i32/i64 divergence in codegen) ---

#[test]
fn linked_int_vars_resolve_together() {
    if !backend_available() {
        return;
    }
    // `a` and `b` start as unconstrained int vars; `a + b` unifies (links) them, and the
    // `-> i32` return then constrains the linked set to i32. Before union-find the two
    // vars could resolve independently (one to the i64 default), mismatching in codegen.
    let src = "fn main() -> i32 {\n  a := 20\n  b := 22\n  return a + b\n}\n";
    let out = build_and_run("linked-vars", src);
    assert_eq!(out.status.code(), Some(42));
}
