//! Control-flow regression gates for escape provenance. Region and local-slice facts are may-state:
//! mutually exclusive branches join at their continuation, and loop back-edges iterate to a
//! fixpoint before a later return is checked.

mod common;
use common::*;

#[test]
fn loop_region_fixpoint_rejects_second_iteration_frame_borrow() {
    // The first modeled iteration only copies `b` (Static) into `a`, then makes `b` borrow `s`.
    // On the second iteration the Frame provenance reaches `a`. A one-pass loop walk misses that
    // transitive back-edge and would allow `a` to escape after `s` is dropped.
    let src = r#"
fn f() -> str {
  s := "owned".clone()
  mut a: str := "static-a"
  mut b: str := "static-b"
  mut i := 0
  loop {
    if i >= 2 { break 0 }
    a = b
    b = s
    i = i + 1
  }
  return a
}

fn main() -> i32 {
  print(f())
  return 0
}
"#;

    let diagnostics = check_diagnostics("region-loop-fixpoint.align", src);
    assert!(
        diagnostics.contains("cannot return a view that borrows local storage"),
        "the second-iteration Frame borrow must not escape:\n{diagnostics}"
    );
}

#[test]
fn diverging_if_branch_does_not_taint_fallthrough_region() {
    // The only path that assigns the Frame borrow to `v` returns from inside that branch. It never
    // reaches the final `return v`, whose sole predecessor is the else path retaining the literal.
    let src = r#"
fn f(cond: bool) -> str {
  s := "owned".clone()
  mut v: str := "static"
  if cond {
    v = s
    return "early"
  }
  return v
}

fn main() -> i32 {
  print(f(false))
  return 0
}
"#;

    let diagnostics = check_diagnostics("region-diverging-if.align", src);
    assert!(diagnostics.is_empty(), "a diverging branch must not taint fallthrough:\n{diagnostics}");
}

#[test]
fn straight_line_overwrite_replaces_obsolete_frame_region() {
    // This is a strong update, not a join: after the literal overwrite no path still holds the
    // borrow of `s`, so returning `v` is safe.
    let src = r#"
fn f() -> str {
  s := "owned".clone()
  mut v: str := s
  v = "static"
  return v
}

fn main() -> i32 {
  print(f())
  return 0
}
"#;

    let diagnostics = check_diagnostics("region-strong-update.align", src);
    assert!(diagnostics.is_empty(), "a straight-line overwrite must replace old provenance:\n{diagnostics}");
}

#[test]
fn loop_local_slice_fixpoint_rejects_second_iteration_borrow() {
    // Local-slice provenance is a second component of the same flow state. It must traverse the
    // back-edge just like Region::Frame rather than relying on a single monotone syntax walk.
    let src = r#"
fn f(input: slice<i64>) -> slice<i64> {
  xs := [1, 2, 3]
  mut a: slice<i64> := input
  mut b: slice<i64> := input
  mut i := 0
  loop {
    if i >= 2 { break 0 }
    a = b
    b = xs
    i = i + 1
  }
  return a
}

fn main() -> i32 {
  xs := [4, 5, 6]
  print(f(xs).len())
  return 0
}
"#;

    let diagnostics = check_diagnostics("slice-loop-fixpoint.align", src);
    assert!(
        diagnostics.contains("cannot return a slice that views a local array"),
        "the second-iteration local slice must not escape:\n{diagnostics}"
    );
}

#[test]
fn diverging_match_arm_does_not_taint_fallthrough_region() {
    let src = r#"
Choice { Early, Continue }

fn f(choice: Choice) -> str {
  s := "owned".clone()
  mut v: str := "static"
  match choice {
    Early => {
      v = s
      return "early"
    },
    Continue => {},
  }
  return v
}

fn main() -> i32 {
  print(f(Choice.Continue))
  return 0
}
"#;

    let diagnostics = check_diagnostics("region-diverging-match.align", src);
    assert!(diagnostics.is_empty(), "a diverging match arm must not taint fallthrough:\n{diagnostics}");
}

#[test]
fn diverging_else_fallback_does_not_taint_success_path() {
    let src = r#"
fn maybe(ok: bool) -> Option<i32> {
  if ok { return Some(1) }
  return None
}

fn f(ok: bool) -> str {
  s := "owned".clone()
  mut v: str := "static"
  n := maybe(ok) else {
    v = s
    return "fallback"
  }
  print(n)
  return v
}

fn main() -> i32 {
  print(f(true))
  return 0
}
"#;

    let diagnostics = check_diagnostics("region-diverging-else.align", src);
    assert!(diagnostics.is_empty(), "a diverging fallback must not taint success:\n{diagnostics}");
}

#[test]
fn diverging_branch_keeps_arena_owned_drop_classification() {
    if !backend_available() {
        return;
    }
    // Although this branch does not join the continuation, its arena-owned local still exists on
    // the early-return path. It must remain excluded from individual function-exit drops and be
    // freed only by the arena cleanup.
    let src = r#"
fn main() -> i32 {
  if true {
    arena {
      xs := [7, 8, 9].to_array()
      return xs[0] as i32
    }
  }
  return 0
}
"#;

    assert_eq!(build_and_run("diverging-arena-drop-region", src).status.code(), Some(7));
}

#[test]
fn break_edge_captures_state_at_the_terminator() {
    // The CFG keeps lowering unreachable syntax for diagnostics, but the break edge must leave
    // before that syntax mutates provenance. Otherwise the dead assignment would falsely taint the
    // only state that reaches the return after the loop.
    let src = r#"
fn f(input: slice<i64>) -> slice<i64> {
  xs := [1, 2, 3]
  mut out: slice<i64> := input
  loop {
    break 0
    out = xs
  }
  return out
}

fn main() -> i32 {
  xs := [4, 5, 6]
  print(f(xs).len())
  return 0
}
"#;

    let diagnostics = check_diagnostics("region-break-edge.align", src);
    assert!(
        diagnostics.is_empty(),
        "unreachable syntax after break must not change the break edge:\n{diagnostics}"
    );
}

#[test]
fn loop_exit_joins_all_reachable_break_predecessors() {
    // One break path carries a frame-local slice and the other retains the caller slice. The
    // compact CFG's loop exit must join both explicit predecessors before checking the return.
    let src = r#"
fn f(input: slice<i64>, local: bool) -> slice<i64> {
  xs := [1, 2, 3]
  mut out: slice<i64> := input
  loop {
    if local {
      out = xs
      break 0
    } else {
      break 0
    }
  }
  return out
}

fn main() -> i32 {
  xs := [4, 5, 6]
  print(f(xs, false).len())
  return 0
}
"#;

    let diagnostics = check_diagnostics("region-break-join.align", src);
    assert!(
        diagnostics.contains("cannot return a slice that views a local array"),
        "every reachable break predecessor must contribute to the loop exit:\n{diagnostics}"
    );
}
