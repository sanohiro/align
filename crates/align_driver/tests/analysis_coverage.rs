//! Regression tests for coverage holes in the escape / effect / move analyses — cases where a
//! value that must not escape (or an impurity that must be seen) slipped through because a
//! per-`ExprKind` hand-written traversal was missing an arm. Each is a program that used to
//! type-check (allowing a use-after-free or a purity bypass) and must now be rejected, plus the
//! matching false-positive fix. Found by an external multi-agent audit (report 2026-07-02).

mod common;
use common::*;

// --- 1-2: an arena value escaping through a `match` arm (region_of lacked `Match`) ---
#[test]
fn arena_value_escaping_via_match_arm_is_rejected() {
    let src = "\
Tag { A, B }
fn main() -> i32 {
  v := Tag.A
  s := arena {
    n := 42
    t := template \"n={n}\"
    match v { A => t, B => t }
  }
  print(s.len())
  return 0
}
";
    assert!(check_errs("match-escape", src), "arena str escaping via a match arm must be rejected");
}

// --- NEW-1: an arena value escaping through an indirect call (region_of lacked `CallFnValue`) ---
#[test]
fn arena_value_escaping_via_indirect_call_is_rejected() {
    let src = "\
fn idstr(s: str) -> str = s
fn main() -> i32 {
  g := idstr
  out := arena {
    n := 5
    t := template \"n={n}\"
    g(t)
  }
  print(out.len())
  return 0
}
";
    assert!(check_errs("callfnvalue-escape", src), "arena str escaping via an indirect call must be rejected");
}

#[test]
fn arena_capture_returned_by_zero_arg_closure_is_rejected() {
    let src = "\
fn main() -> i32 {
  v := arena {
    n := 7
    s := template \"hello {n}\"
    f := fn { s }
    f()
  }
  return v.len() as i32
}
";
    assert!(
        check_errs("callfnvalue-capture-result-escape", src),
        "an indirect call result must not outlive the closure environment it can borrow"
    );
}

// A direct call of the same borrow-returning function is (and stays) rejected — the control that
// proves the indirect path was the only gap.
#[test]
fn arena_value_escaping_via_direct_call_is_rejected() {
    let src = "\
fn idstr(s: str) -> str = s
fn main() -> i32 {
  out := arena {
    n := 5
    t := template \"n={n}\"
    idstr(t)
  }
  print(out.len())
  return 0
}
";
    assert!(check_errs("call-escape", src), "arena str escaping via a direct call must be rejected");
}

// --- 1-5: a slice viewing a local array, returned (slice_is_local lacked `SliceRange`) ---
#[test]
fn returning_range_slice_of_local_array_is_rejected() {
    let src = "\
fn f() -> slice<i64> {
  xs := [1, 2, 3]
  return xs[0..2]
}
fn main() -> i32 { return 0 }
";
    assert!(check_errs("slicerange-return", src), "returning a range slice of a local array must be rejected");
}

// --- 1-6: an arena str stored into an outer array element (AssignIndex lacked a region check) ---
#[test]
fn storing_arena_str_into_outer_array_element_is_rejected() {
    let src = "\
fn main() -> i32 {
  mut arr := [\"aa\", \"bb\"]
  arena {
    n := 5
    t := template \"n={n}\"
    arr[0] = t
  }
  print(arr[0].len())
  return 0
}
";
    assert!(check_errs("elem-assign-escape", src), "storing an arena str into an outer array element must be rejected");
}

// --- 1-4: an impure function laundered through a fn value, used in par_map (EffectScan lacked
//          the `FnValue` call edge) ---
#[test]
fn impure_fn_via_fn_value_rejected_in_par_map() {
    let src = "\
fn loud(x: i64) -> i64 {
  print(x)
  return x
}
fn sneaky(x: i64) -> i64 {
  g := loud
  return g(x)
}
fn main() -> Result<(), Error> {
  ys := [1, 2, 3].par_map(sneaky)
  print(ys.sum())
  return Ok(())
}
";
    assert!(check_errs("parmap-fnvalue-purity", src), "an impure fn laundered through a fn value must be rejected by par_map");
}

#[test]
fn impure_capturing_closure_edge_rejected_in_par_map() {
    let src = "\
fn worker(x: i64) -> i64 {
  k := 100
  f := fn y: i64 {
    print(y + k)
    y
  }
  return x
}
fn main() -> Result<(), Error> {
  ys := [1, 2, 3].par_map(worker)
  print(ys.sum())
  return Ok(())
}
";
    assert!(
        check_errs("parmap-capturing-closure-purity", src),
        "an Impure lifted closure must contribute an effect edge even before an indirect call"
    );
}

#[test]
fn unknown_higher_order_effect_rejected_in_par_map() {
    let src = "\
fn loud(x: i64) -> i64 {
  print(x)
  return x
}
fn apply(f: fn(i64) -> i64, x: i64) -> i64 = f(x)
fn main() -> Result<(), Error> {
  f := loud
  ys := [1, 2, 3].par_map(fn x { apply(f, x) })
  print(ys.sum())
  return Ok(())
}
";
    let diagnostics = check_diagnostics("parmap-hof-unknown-effect", src);
    assert!(
        diagnostics.contains("calls a function value whose effect is not statically known"),
        "a higher-order target with no function-type effect must fail closed at par_map:\n{diagnostics}"
    );
}

#[test]
fn pure_higher_order_call_remains_legal_sequentially() {
    if !backend_available() {
        return;
    }
    let src = "\
fn inc(x: i64) -> i64 = x + 1
fn apply(f: fn(i64) -> i64, x: i64) -> i64 = f(x)
fn main() -> i32 {
  return apply(inc, 4) as i32
}
";
    assert!(!check_errs("sequential-hof-unknown-effect", src), "sequential higher-order calls remain legal");
    assert_eq!(build_and_run("sequential-hof-unknown-effect", src).status.code(), Some(5));
}

// --- NEW-3: the MoveCheck false positive — the same move value consumed in mutually-exclusive
//            match arms must be accepted (arms now clone+join like if/else). ---
#[test]
fn same_move_value_in_exclusive_match_arms_is_accepted() {
    if !backend_available() {
        return;
    }
    let src = "\
Tag { A, B }
fn main() -> i32 {
  arena {
    v := Tag.A
    b := heap.new(5)
    r := match v {
      A => { c := b
             c.get() }
      B => { d := b
             d.get() }
    }
    print(r)
  }
  return 0
}
";
    assert!(!check_errs("match-move-join", src), "moving the same value in exclusive match arms must be accepted");
    let out = build_and_run("match-move-join", src);
    assert_eq!(out.status.code(), Some(0));
}

// --- gemini #270 review: a `task_group {}` opens a region (like `arena {}`), so a task/box value
//     must not escape it (region_of / slice_is_local gained the `TaskGroup` block-wrapping arms). ---
#[test]
fn task_group_value_cannot_escape() {
    let src = "\
fn main() -> i32 {
  t := task_group {
    a := spawn(fn { 5 })
    wait()
    a
  }
  return 0
}
";
    assert!(check_errs("task-group-escape", src), "a task value must not escape its task_group");
}

#[test]
fn lambda_capturing_arena_view_cannot_escape() {
    let src = "\
fn main() -> i32 {
  f := arena {
    n := 5
    v := template \"hello {n}\"
    fn { v.len() as i32 }
  }
  return f()
}
";
    assert!(check_errs("lambda-capture-escape", src), "a lambda capturing an arena view must not escape the arena");
}
