//! The no-shadowing rule (`draft.md` §4 Variables; `docs/open-questions.md` Settled 2026-07-09): a
//! name binds **once** per scope chain. A same-scope re-`:=`, an inner-scope shadow of a visible
//! binding/parameter, a duplicate parameter, a lambda parameter colliding with an enclosing binding,
//! a `match` arm binding shadowing an outer name, and a local shadowing a top-level constant are all
//! compile errors. Two *disjoint* sibling blocks (or `match` arms) may reuse a name — no point in
//! the program then sees two bindings.

mod common;
use common::*;

const SHADOW_MSG: &str = "no shadowing";

/// The rendered diagnostics must contain exactly one shadowing error.
fn shadow_errors(name: &str, src: &str) -> usize {
    check_diagnostics(name, src).matches(SHADOW_MSG).count()
}

// --- errors ---------------------------------------------------------------------------------

#[test]
fn same_scope_rebind_is_an_error() {
    let src = "fn main() -> i32 {\n  x := 1\n  x := 2\n  return x\n}\n";
    assert!(check_errs("shadow-same", src));
    assert!(check_diagnostics("shadow-same", src).contains(SHADOW_MSG));
}

#[test]
fn inner_block_shadow_is_an_error() {
    let src = "\
fn main() -> i32 {
  x := 1
  if true {
    x := 2
    return x
  }
  return x
}
";
    assert_eq!(shadow_errors("shadow-inner", src), 1);
}

#[test]
fn local_shadowing_a_parameter_is_an_error() {
    let src = "\
fn f(x: i32) -> i32 {
  x := 2
  return x
}
fn main() -> i32 { return f(1) }
";
    assert_eq!(shadow_errors("shadow-param", src), 1);
}

#[test]
fn duplicate_parameter_is_an_error() {
    let src = "\
fn f(x: i32, x: i32) -> i32 { return x }
fn main() -> i32 { return f(1, 2) }
";
    assert_eq!(shadow_errors("shadow-dup-param", src), 1);
}

#[test]
fn lambda_parameter_shadowing_an_enclosing_binding_is_an_error() {
    // The lambda param `x` collides with the enclosing local `x` (which it could capture).
    let src = "\
fn main() -> Result<(), Error> {
  x := 10
  print([1, 2, 3].map(fn x { x * 2 }).sum())
  return Ok(())
}
";
    assert_eq!(shadow_errors("shadow-lambda-enc", src), 1);
}

#[test]
fn match_binding_shadowing_an_outer_binding_is_an_error() {
    let src = "\
E { A(i32), B }
fn main() -> i32 {
  n := 5
  e := E.A(3)
  r := match e {
    A(n) => n
    B => 0
  }
  return r + n
}
";
    assert_eq!(shadow_errors("shadow-match-outer", src), 1);
}

#[test]
fn local_shadowing_a_top_level_constant_is_an_error() {
    let src = "\
MAX := 100
fn main() -> i32 {
  MAX := 5
  return MAX
}
";
    assert_eq!(shadow_errors("shadow-const", src), 1);
}

#[test]
fn tuple_destructuring_reusing_a_name_is_an_error() {
    // `(a, a) := …` binds `a` twice in one scope.
    let src = "\
fn main() -> i32 {
  (a, a) := (1, 2)
  return a
}
";
    assert_eq!(shadow_errors("shadow-tuple", src), 1);
}

// --- legal reuse ----------------------------------------------------------------------------

#[test]
fn mut_reassignment_is_not_shadowing() {
    let src = "\
fn main() -> i32 {
  mut count := 0
  count = count + 1
  return count
}
";
    assert!(!check_errs("shadow-mut", src));
}

#[test]
fn disjoint_sibling_blocks_may_reuse_a_name() {
    let src = "\
fn main() -> i32 {
  if true {
    y := 1
    return y
  }
  if false {
    y := 2
    return y
  }
  return 0
}
";
    assert!(!check_errs("shadow-sibling", src));
}

#[test]
fn sibling_match_arms_may_reuse_a_binding_name() {
    let src = "\
E { A(i32), B(i32) }
fn main() -> i32 {
  e := E.A(3)
  r := match e {
    A(v) => v
    B(v) => v + 1
  }
  return r
}
";
    assert!(!check_errs("shadow-match-sibling", src));
}

#[test]
fn sibling_lambdas_may_reuse_a_parameter_name() {
    // Two disjoint pipeline-stage lambdas: neither is visible in the other, so both may use `x`.
    let src = "\
fn main() -> Result<(), Error> {
  print([1, 2, 3, 4, 5].map(fn x { x * 10 }).where(fn x { x > 25 }).sum())
  return Ok(())
}
";
    assert!(!check_errs("shadow-lambda-siblings", src));
}

#[test]
fn a_lambda_may_capture_an_enclosing_binding_with_a_distinct_param() {
    let src = "\
fn main() -> Result<(), Error> {
  y := 10
  print([1, 2, 3].map(fn x { x + y }).sum())
  return Ok(())
}
";
    assert!(!check_errs("shadow-lambda-capture", src));
}

// --- no double diagnostics ------------------------------------------------------------------

#[test]
fn intra_pattern_duplicate_reports_once_as_a_duplicate_binding() {
    // `Rect(w, w)` is a duplicate *within* the pattern — it must stay the pattern's own
    // "duplicate binding" diagnostic, not also fire the shadowing error.
    let src = "\
E { Rect(i32, i32), Nil }
fn main() -> i32 {
  e := E.Rect(3, 4)
  r := match e {
    Rect(w, w) => w
    Nil => 0
  }
  return r
}
";
    let diags = check_diagnostics("shadow-dup-pattern", src);
    assert!(diags.contains("duplicate binding 'w' in pattern"), "got:\n{diags}");
    assert_eq!(diags.matches(SHADOW_MSG).count(), 0, "should not also report shadowing:\n{diags}");
}
