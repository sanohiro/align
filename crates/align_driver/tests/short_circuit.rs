//! `&&` / `||` are short-circuit (audit 3-1): the right operand is evaluated only when the left
//! doesn't already decide the result. Before the fix MIR lowered them as a strict `Rvalue::Bin`,
//! so both sides always ran — a correctness bug (a guarded index still trapped) and an
//! observable-side-effect bug.

mod common;
use common::*;

#[test]
fn and_skips_rhs_when_lhs_false() {
    // f() returns false and prints 2; t() prints 1. `f() && t()` must not run t().
    let src = "\
fn t() -> bool {
  print(1)
  return true
}
fn f() -> bool {
  print(2)
  return false
}
fn main() -> i32 {
  if f() && t() { return 9 }
  return 0
}
";
    let out = build_and_run("sc-and", src);
    assert_eq!(String::from_utf8_lossy(&out.stdout).trim(), "2", "t() must be skipped");
}

#[test]
fn or_skips_rhs_when_lhs_true() {
    // t() returns true and prints 1; f() prints 2. `t() || f()` must not run f().
    let src = "\
fn t() -> bool {
  print(1)
  return true
}
fn f() -> bool {
  print(2)
  return false
}
fn main() -> i32 {
  if t() || f() { return 0 }
  return 9
}
";
    let out = build_and_run("sc-or", src);
    assert_eq!(String::from_utf8_lossy(&out.stdout).trim(), "1", "f() must be skipped");
}

#[test]
fn short_circuit_guard_avoids_out_of_bounds_trap() {
    // i is out of range; `i < 3 && xs[i] > 0` must not evaluate xs[i] (which would abort on the
    // bounds check). Reaching `return 7` proves the index was skipped.
    let src = "\
fn main() -> i32 {
  xs := [10, 20, 30]
  i := 5
  if i < 3 && xs[i] > 0 { return 1 }
  return 7
}
";
    let out = build_and_run("sc-guard", src);
    assert_eq!(out.status.code(), Some(7));
}

#[test]
fn boolean_truth_table_is_correct() {
    // Each of the 6 combinations contributes a distinct decade so a wrong one is identifiable.
    // Expected: T&&T (1) + T||F (1000) + F||T (100000) = 101001; exit code is that mod 256 = 137.
    let src = "\
fn main() -> i32 {
  mut n := 0
  if true && true { n = n + 1 }
  if true && false { n = n + 10 }
  if false && true { n = n + 100 }
  if true || false { n = n + 1000 }
  if false || false { n = n + 10000 }
  if false || true { n = n + 100000 }
  return n
}
";
    let out = build_and_run("sc-truth", src);
    assert_eq!(out.status.code(), Some(101001 % 256));
}
