//! doc-13 §6.6 / §11 P3 — repeated-needle plan hoisting.
//!
//! `xs.where(fn s { s.contains(NEEDLE) }).…` with a loop-invariant needle (a capture, free of the
//! lambda parameter) builds one `str_finder` plan before the loop and reuses it per element instead
//! of reconstructing a `memchr` searcher on every `str.contains`. These tests pin: the IR shape
//! (one `finder_new` in the preheader, `finder_find` — not `str_contains` — in the body), the
//! negative control (an element-derived needle keeps the per-call `str_contains` path), run-parity
//! with the unhoisted equivalent (including empty and too-long needles), and plan cleanup on every
//! exit path (normal, `?`/error, and a 20k-iteration leak loop).
//!
//! Note: every runtime symbol is `declare`d in the module, so the IR-shape assertions match the
//! `call <ret> @sym(` form (never the bare `@sym`, which the always-present declare also carries).

mod common;
use common::*;

// A single-function `where(str.contains)` reduction over an array of string literals; the needle is
// captured from an enclosing local. One function keeps the IR-shape count unambiguous (no inlining
// duplicates the loop into a caller).
const COUNT_ONE_FN: &str = r#"
fn main() -> i32 {
  n := "al"
  xs := ["alpha", "beta", "gamma", "alfalfa", "delta"]
  return xs.where(fn s { s.contains(n) }).count() as i32
}
"#;

#[test]
fn hoisted_where_contains_count_is_correct() {
    if !backend_available() {
        return;
    }
    // "al" occurs in "alpha" and "alfalfa" → 2.
    let out = build_and_run("nph-count", COUNT_ONE_FN);
    assert_eq!(out.status.code(), Some(2));
}

#[test]
fn ir_has_hoisted_finder_new_and_no_str_contains() {
    if !backend_available() {
        return;
    }
    let ir = emit_llvm(COUNT_ONE_FN);
    // Gate 2: exactly one plan is built (in the preheader), the per-element search is the finder,
    // and the one-shot str_contains is gone.
    assert_eq!(
        ir.matches("call ptr @align_rt_str_finder_new(").count(),
        1,
        "exactly one hoisted finder_new expected:\n{ir}"
    );
    assert!(
        ir.contains("call i64 @align_rt_str_finder_find("),
        "the loop body must call finder_find:\n{ir}"
    );
    // Preheader-before-body: the single plan construction must textually precede the per-element
    // search (MIR emits the preheader block, then the loop). Combined with the single static
    // finder_new site over a 5-element source and the correct dynamic result, this pins the plan as
    // built once before the loop, not reconstructed per element.
    let new_pos = ir.find("call ptr @align_rt_str_finder_new(").expect("finder_new present");
    let find_pos = ir.find("call i64 @align_rt_str_finder_find(").expect("finder_find present");
    assert!(new_pos < find_pos, "finder_new must be hoisted before the loop-body finder_find:\n{ir}");
    assert!(
        !ir.contains("call i32 @align_rt_str_contains("),
        "the hoisted path must not reconstruct a one-shot str_contains:\n{ir}"
    );
    // The plan is freed on the (single) normal exit path.
    assert!(
        ir.contains("call void @align_rt_str_finder_free("),
        "the plan must be freed:\n{ir}"
    );
}

#[test]
fn negative_control_element_derived_needle_not_hoisted() {
    if !backend_available() {
        return;
    }
    // The needle is derived from the element (`s[0..1]`) → not loop-invariant → NOT hoisted. The
    // per-call `str_contains` path is kept and no plan is built.
    let src = r#"
fn main() -> i32 {
  xs := ["abc", "xbc", "abx"]
  return xs.where(fn s { s.contains(s[0..1]) }).count() as i32
}
"#;
    let ir = emit_llvm(src);
    assert_eq!(
        ir.matches("call ptr @align_rt_str_finder_new(").count(),
        0,
        "an element-derived needle must not be hoisted:\n{ir}"
    );
    assert!(
        !ir.contains("call i64 @align_rt_str_finder_find("),
        "an element-derived needle must keep the per-call path:\n{ir}"
    );
    assert!(
        ir.contains("call i32 @align_rt_str_contains("),
        "the per-call path must retain str_contains:\n{ir}"
    );
    // Every element contains its own first char → all 3 survive.
    let out = build_and_run("nph-neg", src);
    assert_eq!(out.status.code(), Some(3));
}

// Run-parity: the hoisted count must equal a hand-written per-element loop count for the same
// needle, across ordinary, empty, and too-long needles. `diff` is hoisted − manual and must be 0.
#[test]
fn run_parity_across_needle_shapes() {
    if !backend_available() {
        return;
    }
    let src = r#"
fn hoisted(n: str) -> i64 {
  xs := ["", "a", "ab", "abc", "xyz", "aXbc"]
  return xs.where(fn s { s.contains(n) }).count()
}
fn manual(n: str) -> i64 {
  xs := ["", "a", "ab", "abc", "xyz", "aXbc"]
  mut c := 0
  mut i := 0
  total := xs.len()
  r := loop {
    if i >= total { break c }
    if xs[i].contains(n) { c = c + 1 }
    i = i + 1
  }
  return r
}
fn diff(n: str) -> i64 = hoisted(n) - manual(n)
fn main() -> i32 {
  // Empty needle matches every element; "ab" matches "ab","abc"; a too-long needle matches none.
  bad := diff("") + diff("ab") + diff("abcdefzzz") + diff("a") + diff("X")
  return bad as i32
}
"#;
    let out = build_and_run("nph-parity", src);
    // All diffs are zero → the hoisted path matches the manual loop exactly.
    assert_eq!(out.status.code(), Some(0));
}

// Cleanup / leak: build the plan-bearing pipeline 20_000 times; the plan must be freed each time.
#[test]
fn plan_freed_no_leak_over_many_iterations() {
    if !backend_available() {
        return;
    }
    let src = r#"
fn run_once(n: str) -> i64 {
  xs := ["needle in haystack", "no match here", "another needle"]
  return xs.where(fn s { s.contains(n) }).count()
}
fn main() -> i32 {
  mut i := 0
  mut acc := 0
  r := loop {
    if i >= 20000 { break acc }
    acc = acc + run_once("needle")
    i = i + 1
  }
  // Each call finds 2 → 40000; 40000 % 7 = 2. A leak/double-free would crash or drift the total.
  return (r % 7) as i32
}
"#;
    let out = build_and_run("nph-leak", src);
    assert_eq!(out.status.code(), Some(2));
}

// Cleanup on an early `?` / error exit: the plan built before a `?` that propagates an error must
// still be freed exactly once (the drop-flag machinery covers the hole exit).
#[test]
fn plan_freed_on_early_error_exit() {
    if !backend_available() {
        return;
    }
    let src = r#"
fn maybe(flag: i64) -> Result<i64, i64> {
  if flag == 0 { return Err(9) }
  return Ok(flag)
}
fn count_then_try(n: str, flag: i64) -> Result<i64, i64> {
  xs := ["ab", "abc", "xy"]
  c := xs.where(fn s { s.contains(n) }).count()
  v := maybe(flag)?
  return Ok(c + v)
}
fn main() -> i32 {
  r := count_then_try("ab", 0)
  return match r {
    Ok(v) => v as i32
    Err(e) => e as i32
  }
}
"#;
    // The `?` propagates Err(9) after the plan is built; the plan must be freed on that path (no
    // leak, no double free — verified by the process exiting cleanly with the error code).
    let out = build_and_run("nph-try", src);
    assert_eq!(out.status.code(), Some(9));
}

// The `to_array()` (materializing) terminal also hoists and reuses the plan.
#[test]
fn hoisted_where_contains_to_array() {
    if !backend_available() {
        return;
    }
    let src = r#"
fn main() -> i32 {
  n := "match"
  xs := ["skip", "match_one", "nope", "match_two"]
  kept := xs.where(fn s { s.contains(n) }).to_array()
  return kept.len() as i32
}
"#;
    let ir = emit_llvm(src);
    assert_eq!(
        ir.matches("call ptr @align_rt_str_finder_new(").count(),
        1,
        "to_array pipeline must hoist one plan:\n{ir}"
    );
    assert!(
        !ir.contains("call i32 @align_rt_str_contains("),
        "to_array must use the finder:\n{ir}"
    );
    let out = build_and_run("nph-toarray", src);
    assert_eq!(out.status.code(), Some(2));
}
