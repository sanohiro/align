//! `loop` — the one sequential-control construct (`draft.md` §4). An expression: `break expr`
//! ends the innermost loop and yields `expr`, unified across breaks like `match` arms; a bare
//! `break` yields `()`; a `loop` with no `break` diverges. No `for`/`while`/`continue`/labels;
//! `?`/`return` exit the function, `break` is the only loop exit and cannot cross a lambda. Per-
//! iteration owned locals drop each pass; a loop-back move of an enclosing owned local is a
//! use-after-move; a `break` value may not borrow per-iteration storage (the escape rule).

mod common;
use common::*;

#[test]
fn loop_yields_its_break_value() {
    if !backend_available() {
        return;
    }
    // Accumulate 0+1+2+3+4 = 10, breaking with the running total.
    let src = "fn main() -> i32 {\n  mut total := 0\n  mut i := 0\n  n := loop {\n    if i >= 5 { break total }\n    total = total + i\n    i = i + 1\n  }\n  return n\n}\n";
    let out = build_and_run("loop-value", src);
    assert_eq!(out.status.code(), Some(10));
}

#[test]
fn a_bare_break_ends_a_statement_loop() {
    if !backend_available() {
        return;
    }
    // A `loop` used as a statement, exited by a bare `break` (value `()`).
    let src = "fn main() -> i32 {\n  mut i := 0\n  loop {\n    i = i + 1\n    if i >= 7 { break }\n  }\n  return i\n}\n";
    let out = build_and_run("loop-bare", src);
    assert_eq!(out.status.code(), Some(7));
}

#[test]
fn nested_loops_break_the_innermost() {
    if !backend_available() {
        return;
    }
    // The inner `break` exits only the inner loop; the outer loop keeps its own `break`.
    let src = "fn main() -> i32 {\n  mut o := 0\n  r := loop {\n    mut inner := 0\n    s := loop {\n      if inner >= 3 { break inner }\n      inner = inner + 1\n    }\n    o = o + s\n    if o >= 9 { break o }\n  }\n  return r\n}\n";
    let out = build_and_run("loop-nested", src);
    assert_eq!(out.status.code(), Some(9));
}

#[test]
fn a_question_mark_in_a_loop_exits_the_function() {
    if !backend_available() {
        return;
    }
    // A break-less loop diverges; `?` inside it early-returns the enclosing function's `Err`.
    let src = "fn get(i: i64) -> Result<i64, i64> {\n  if i > 3 { return Err(99) }\n  Ok(i * 2)\n}\nfn run() -> Result<i64, i64> {\n  mut i := 0\n  mut sum := 0\n  loop {\n    v := get(i)?\n    sum = sum + v\n    i = i + 1\n  }\n}\nfn main() -> i32 {\n  r := match run() {\n    Ok(v) => v\n    Err(e) => e\n  }\n  return r as i32\n}\n";
    let out = build_and_run("loop-try", src);
    assert_eq!(out.status.code(), Some(99));
}

#[test]
fn a_diverging_loop_body_is_a_function_result() {
    if !backend_available() {
        return;
    }
    // A `loop` with no `break` diverges: it satisfies the `i64` return type, and control leaves
    // only via the `return` inside it.
    let src = "fn spin() -> i64 {\n  mut i := 0\n  loop {\n    i = i + 1\n    if i == 42 { return i }\n  }\n}\nfn main() -> i32 {\n  return spin() as i32\n}\n";
    let out = build_and_run("loop-diverge", src);
    assert_eq!(out.status.code(), Some(42));
}

#[test]
fn a_per_iteration_owned_string_is_freed_each_pass() {
    if !backend_available() {
        return;
    }
    // A fresh owned `string` is built and dropped on every iteration (no leak across the back-edge);
    // the program prints it three times and exits cleanly.
    let src = "fn make() -> string {\n  mut b := builder()\n  b.write(\"x\")\n  b.to_string()\n}\nfn main() -> i32 {\n  mut i := 0\n  loop {\n    s := make()\n    print(s)\n    i = i + 1\n    if i >= 3 { break }\n  }\n  return 0\n}\n";
    let out = build_and_run("loop-perdrop", src);
    assert_eq!(out.status.code(), Some(0));
    assert_eq!(String::from_utf8_lossy(&out.stdout), "x\nx\nx\n");
}

#[test]
fn a_per_iteration_owned_local_nested_in_an_expression_is_freed_each_pass() {
    if !backend_available() {
        return;
    }
    // The per-iteration owned `string` is declared inside a block that is itself a *call argument*
    // (a value position, not a top-level body statement). Its drop must still fire each pass — the
    // loop's per-iteration drop set is the body's declared-local range, so it captures a `let` at
    // any nesting depth, not only body-level statements. (Before the range-based fix, the
    // `ExprKind`-walking collector missed this `let` and the slot leaked/corrupted each iteration.)
    let src = "fn make() -> string {\n  mut b := builder()\n  b.write(\"hello\")\n  b.to_string()\n}\nfn take(n: i64) -> i64 = n\nfn main() -> i32 {\n  mut i := 0\n  loop {\n    r := take({ s := make(); s.len() })\n    print(r)\n    i = i + 1\n    if i >= 3 { break }\n  }\n  return 0\n}\n";
    let out = build_and_run("loop-nested-drop", src);
    assert_eq!(out.status.code(), Some(0), "a per-iteration owned local nested in a call arg must drop each pass, not leak/corrupt");
    assert_eq!(String::from_utf8_lossy(&out.stdout), "5\n5\n5\n");
}

#[test]
fn a_per_iteration_owned_move_is_allowed() {
    if !backend_available() {
        return;
    }
    // Moving a *per-iteration* owned local (declared inside the body) is fine: it is fresh each
    // pass, so there is no back-edge use-after-move.
    let src = "fn make() -> string {\n  mut b := builder()\n  b.write(\"hi\")\n  b.to_string()\n}\nfn take(s: string) -> i64 = 1\nfn main() -> i32 {\n  mut i := 0\n  loop {\n    s := make()\n    print(take(s))\n    i = i + 1\n    if i >= 2 { break }\n  }\n  return 0\n}\n";
    let out = build_and_run("loop-periter-move", src);
    assert_eq!(out.status.code(), Some(0));
}

#[test]
fn a_break_moves_an_owned_value_out_once() {
    if !backend_available() {
        return;
    }
    // An unconditional `break s` moves an enclosing owned local out exactly once (the loop runs a
    // single pass) — not a loop-back use-after-move.
    let src = "fn make() -> string {\n  mut b := builder()\n  b.write(\"hi\")\n  b.to_string()\n}\nfn take(s: string) -> i64 = 5\nfn main() -> i32 {\n  s := make()\n  r := loop {\n    break s\n  }\n  return take(r) as i32\n}\n";
    let out = build_and_run("loop-break-move", src);
    assert_eq!(out.status.code(), Some(5));
}

// --- negative: type checking ------------------------------------------------------------------

#[test]
fn breaks_with_different_types_are_rejected() {
    let src = "fn main() {\n  x := loop {\n    if true { break 1 }\n    break false\n  }\n  print(x)\n}\n";
    let out = check_diagnostics("loop-mismatch", src);
    assert!(out.contains("type mismatch"), "break types must unify:\n{out}");
}

#[test]
fn a_bare_break_conflicts_with_a_valued_break() {
    let src = "fn main() {\n  x := loop {\n    if true { break 5 }\n    break\n  }\n  print(x)\n}\n";
    // A bare `break` yields `()`, which must unify with the other break's `i64` — a mismatch.
    assert!(check_errs("loop-bare-mismatch", src), "bare break vs valued break must mismatch");
}

#[test]
fn a_bare_array_literal_break_value_is_a_clean_error_not_a_panic() {
    // A bare `[…]` has no free-value MIR lowering (only a `let` initializer / pipeline source), so a
    // `break [1, 2, 3]` must be a sema diagnostic, never a compiler panic.
    let src = "fn main() {\n  x := loop {\n    break [1, 2, 3]\n  }\n  print(x[0])\n}\n";
    let out = check_diagnostics("loop-bare-array", src);
    assert!(out.contains("bare array literal cannot be a `break` value"), "expected a clean bare-array-literal error:\n{out}");
}

#[test]
fn a_break_outside_a_loop_is_an_error() {
    let src = "fn main() {\n  break 5\n}\n";
    let out = check_diagnostics("break-outside", src);
    assert!(out.contains("`break` outside of a `loop`"), "expected break-outside error:\n{out}");
}

#[test]
fn a_break_cannot_cross_a_lambda_boundary() {
    let src = "fn main() {\n  xs := [1, 2, 3]\n  loop {\n    total := xs.map(fn x { break x }).sum()\n    print(total)\n    break\n  }\n}\n";
    let out = check_diagnostics("break-lambda", src);
    assert!(out.contains("`break` outside of a `loop`"), "a break inside a lambda must not bind to the enclosing loop:\n{out}");
}

// --- negative: move / escape ------------------------------------------------------------------

#[test]
fn a_loop_back_use_after_move_is_rejected() {
    // An *enclosing* owned local moved by one iteration is already moved at the start of the next.
    let src = "fn make() -> string {\n  mut b := builder()\n  b.write(\"hi\")\n  b.to_string()\n}\nfn take(s: string) -> i64 = 1\nfn main() {\n  s := make()\n  mut i := 0\n  loop {\n    n := take(s)\n    i = i + 1\n    if i >= 2 { break }\n  }\n}\n";
    let out = check_diagnostics("loop-uam", src);
    assert!(out.contains("use of moved value"), "loop-back use-after-move must be caught:\n{out}");
}

#[test]
fn breaking_a_view_of_a_per_iteration_local_is_rejected() {
    // A `str` view of a per-iteration owned `string` (dropped at the `break`) would dangle.
    let src = "fn make() -> string {\n  mut b := builder()\n  b.write(\"hello\")\n  b.to_string()\n}\nfn main() {\n  found := loop {\n    s := make()\n    break s[0..2]\n  }\n  print(found)\n}\n";
    let out = check_diagnostics("loop-escape", src);
    assert!(out.contains("cannot `break`"), "a break value must not borrow per-iteration storage:\n{out}");
}

// --- negative: the banned C/Rust control constructs --------------------------------------------

#[test]
fn while_does_not_exist() {
    let src = "fn main() {\n  while true { print(1) }\n}\n";
    let out = check_diagnostics("kw-while", src);
    assert!(out.contains("`while` does not exist in Align"), "`while` must be a clear error:\n{out}");
}

#[test]
fn for_does_not_exist() {
    let src = "fn main() {\n  for i := 0 { print(i) }\n}\n";
    let out = check_diagnostics("kw-for", src);
    assert!(out.contains("`for` does not exist in Align"), "`for` must be a clear error:\n{out}");
}

#[test]
fn continue_does_not_exist() {
    let src = "fn main() {\n  loop {\n    continue\n  }\n}\n";
    let out = check_diagnostics("kw-continue", src);
    assert!(out.contains("`continue` does not exist in Align"), "`continue` must be a clear error:\n{out}");
}
