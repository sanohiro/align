//! `==` / `!=` (and the ordering operators) are defined for **scalars and strings only** — numbers,
//! `bool`, `char`, and `str` — with **no structural comparison** (`draft.md` §5 "Equality and
//! Ordering"). Before this was enforced in sema, comparing a struct / tuple / array / sum value
//! slipped through to `align_codegen_llvm`, which fed the aggregate to the integer-compare path and
//! panicked ("expected the IntValue variant") — a compiler ICE on ordinary user input. sema now
//! rejects every non-scalar / non-string operand with a clean diagnostic (a *positive* allow-list —
//! the exact `Eq` / `Ord` bound predicate — never a fail-open pass-through).

mod common;
use common::*;

fn check_msg(name: &str, src: &str) -> String {
    check_diagnostics(name, src)
}

// --- rejected: no structural equality (each used to ICE in codegen) ---

#[test]
fn struct_equality_rejected() {
    let src = "\
Point { x: i32, y: i32 }
fn main() -> i32 {
  a := Point { x: 1, y: 2 }
  b := Point { x: 1, y: 2 }
  if a == b { return 1 }
  return 0
}
";
    let text = check_msg("struct-eq", src);
    assert!(text.contains("has no equality"), "struct `==` must be rejected, got:\n{text}");
    // The user's type name, not the compiler-internal `struct#N` placeholder.
    assert!(text.contains("Point"), "message should name the struct, got:\n{text}");
    assert!(!text.contains("struct#"), "must not leak the internal `struct#N` name, got:\n{text}");
}

#[test]
fn struct_inequality_rejected() {
    let src = "\
Point { x: i32, y: i32 }
fn main() -> i32 {
  a := Point { x: 1, y: 2 }
  b := Point { x: 3, y: 4 }
  if a != b { return 1 }
  return 0
}
";
    assert!(check_msg("struct-ne", src).contains("has no equality"));
}

#[test]
fn struct_ordering_rejected() {
    let src = "\
Point { x: i32, y: i32 }
fn main() -> i32 {
  a := Point { x: 1, y: 2 }
  if a < a { return 1 }
  return 0
}
";
    let text = check_msg("struct-lt", src);
    assert!(text.contains("has no ordering"), "struct `<` must be rejected, got:\n{text}");
    assert!(text.contains("Point") && !text.contains("struct#"), "should name the struct, got:\n{text}");
}

#[test]
fn tuple_equality_rejected() {
    let src = "\
fn main() -> i32 {
  a := (1, 2)
  if a == (3, 4) { return 1 }
  return 0
}
";
    assert!(check_msg("tuple-eq", src).contains("has no equality"));
}

#[test]
fn array_equality_rejected() {
    let src = "\
fn main() -> i32 {
  a := [1, 2, 3]
  if a == [1, 2, 3] { return 1 }
  return 0
}
";
    assert!(check_msg("array-eq", src).contains("has no equality"));
}

#[test]
fn sum_equality_rejected() {
    // A sum value is inspected with `match`, never `==` (one way: `match` = variants).
    let src = "\
Color { Red, Green, Blue }
fn main() -> i32 {
  a := Color.Red
  if a == Color.Green { return 1 }
  return 0
}
";
    let text = check_msg("sum-eq", src);
    assert!(text.contains("has no equality"), "sum `==` must be rejected, got:\n{text}");
    assert!(text.contains("Color") && !text.contains("enum#"), "should name the sum type, got:\n{text}");
}

#[test]
fn owned_string_equality_rejected() {
    // Owned `string` comparison is not implemented yet (only the `str` view is comparable); it would
    // otherwise ICE on the same integer-compare path. A dedicated "not yet" message, not the generic
    // structural-equality one.
    let src = "\
fn main() -> i32 {
  a := \"x\".clone()
  b := \"y\".clone()
  if a == b { return 1 }
  return 0
}
";
    assert!(check_msg("string-eq", src).contains("not directly comparable yet"));
}

#[test]
fn bool_ordering_rejected() {
    // `bool` has equality but not ordering (ordering = numbers + char + str).
    let src = "\
fn main() -> i32 {
  a := true
  if a < false { return 1 }
  return 0
}
";
    assert!(check_msg("bool-lt", src).contains("has no ordering"));
}

// --- allowed: scalars + strings still compile (regression guard) ---

#[test]
fn scalar_and_string_comparisons_still_ok() {
    // int / float / bool / char equality, numeric+char ordering, and `str` equality all type-check.
    let src = "\
fn main() -> i32 {
  i := 1 == 2
  f := 1.5 < 2.5
  b := true == false
  c := 'a' < 'b'
  s := \"x\" == \"y\"
  if i && f && b && c && s { return 1 }
  return 0
}
";
    assert!(!check_errs("scalar-cmp-ok", src), "scalar/string comparisons must still type-check");
}

#[test]
fn struct_equality_does_not_ice_end_to_end() {
    // The original bug: `check` passed and `run` reached codegen and panicked. Guard the full
    // pipeline — a clean compile error, never a panic.
    if !backend_available() {
        return;
    }
    let src = "\
Point { x: i32, y: i32 }
fn main() -> i32 {
  a := Point { x: 1, y: 2 }
  b := Point { x: 1, y: 2 }
  if a == b { return 1 }
  return 0
}
";
    assert!(check_errs("struct-eq-e2e", src), "struct `==` must be a compile error, not an ICE");
}
