//! Regression tests for two parser ambiguities found by the 2026-07-02 audit:
//! - 2-1: a type-annotated `let` at an `if`/`match` condition-body head was misread as a struct
//!   literal (`if flag { x: i32 := 5 }` → "expected field name").
//! - 2-2: `x as u32 < 5` failed to parse because `parse_type` greedily ate `<` as generic args.

mod common;
use common::*;

// --- 2-1: struct-literal recognition is suppressed at an `if` condition's top level ---
#[test]
fn type_annotated_let_at_if_body_head_parses() {
    if !backend_available() {
        return;
    }
    let src = "\
fn main() -> i32 {
  flag := true
  if flag { x: i32 := 5
    return x }
  return 0
}
";
    let out = build_and_run("if-head-let", src);
    assert_eq!(out.status.code(), Some(5));
}

// A genuine struct literal must still work as a plain value, and inside a condition's delimiters
// (call args) where there is no block ambiguity.
#[test]
fn struct_literal_still_works_as_value_and_in_condition_call_args() {
    if !backend_available() {
        return;
    }
    let src = "\
P { a: i32, b: i32 }
fn use2(p: P) -> i32 = p.a + p.b
fn main() -> i32 {
  p := P { a: 1, b: 2 }
  if use2(P { a: 3, b: 4 }) > 5 { return use2(p) }
  return 0
}
";
    // p = {1,2}; use2(P{3,4}) = 7 > 5 → return use2(p) = 3.
    let out = build_and_run("struct-lit-in-cond-args", src);
    assert_eq!(out.status.code(), Some(3));
}

// A bare struct literal in a condition (no parens) is not silently misparsed into a wrong AST —
// it is rejected (the value must be parenthesized). The point is a clean error, not a crash.
#[test]
fn bare_struct_literal_in_condition_is_rejected_cleanly() {
    let src = "\
P { ok: bool }
fn main() -> i32 {
  if P { ok: true } { return 1 }
  return 0
}
";
    assert!(check_errs("bare-struct-in-cond", src));
}

// --- 2-2: a cast target is a concrete primitive, so `<` after it is a comparison ---
#[test]
fn cast_then_comparison_parses() {
    if !backend_available() {
        return;
    }
    let src = "\
fn f(x: i32) -> bool = x as u32 < 5
fn main() -> i32 {
  if f(3) { return 7 }
  return 0
}
";
    let out = build_and_run("cast-cmp", src);
    assert_eq!(out.status.code(), Some(7));
}

// Chained casts still parse (`x as u8 as i64`).
#[test]
fn chained_cast_still_parses() {
    if !backend_available() {
        return;
    }
    let src = "\
fn main() -> i32 {
  x := 300
  y := x as u8 as i64
  if y > 0 { return 2 }
  return 0
}
";
    let out = build_and_run("chained-cast", src);
    assert_eq!(out.status.code(), Some(2));
}
