//! Tuples / multi-value return (PR1 foundation): the anonymous product type `(T, U, ...)`,
//! tuple literals, destructuring `(a, b) := expr`, positional `.0`/`.1` access, and tuple
//! params/returns. PR1 cut: elements are primitive scalars (int/float/bool/char).

use align_driver::{backend_available, check, emit_object_file, link_executable, lower_to_mir};
use align_span::SourceMap;

fn build_and_run(name: &str, src: &str) -> std::process::Output {
    let mut sm = SourceMap::new();
    let checked = check(&mut sm, name, src);
    assert!(
        !checked.diags.has_errors(),
        "unexpected errors:\n{}",
        align_driver::format_diagnostics(&sm, &checked.diags)
    );
    let mir = lower_to_mir(&checked.hir);
    let dir = std::env::temp_dir();
    let obj = dir.join(format!("align-test-{name}.o"));
    let exe = dir.join(format!("align-test-{name}"));
    emit_object_file(&mir, &obj).expect("codegen");
    link_executable(&obj, &exe).expect("link");
    let out = std::process::Command::new(&exe).output().expect("run");
    let _ = std::fs::remove_file(&obj);
    let _ = std::fs::remove_file(&exe);
    out
}

fn check_errs(name: &str, src: &str) -> bool {
    let mut sm = SourceMap::new();
    check(&mut sm, name, src).diags.has_errors()
}

#[test]
fn multi_value_return_and_destructure() {
    if !backend_available() {
        return;
    }
    // A function returns a tuple; the caller destructures it. divmod(17,5) = (3, 2) → 3*10+2 = 32.
    let src = "fn divmod(a: i32, b: i32) -> (i32, i32) = (a / b, a % b)\nfn main() -> i32 {\n  (q, r) := divmod(17, 5)\n  return q * 10 + r\n}\n";
    let out = build_and_run("tup-divmod", src);
    assert_eq!(out.status.code(), Some(32));
}

#[test]
fn positional_index_and_ignore_binder() {
    if !backend_available() {
        return;
    }
    // `t.0`/`t.2` positional access, and `_` ignores an element. 3 + 3 + 5 == 11.
    let src = "fn main() -> i32 {\n  t := (3, 4, 5)\n  (a, _, c) := t\n  if t.0 + a + c == 11 { return 11 }\n  return 0\n}\n";
    let out = build_and_run("tup-index", src);
    assert_eq!(out.status.code(), Some(11));
}

#[test]
fn tuple_param_by_value() {
    if !backend_available() {
        return;
    }
    // A tuple is passed by value to a function that reads its fields. (40, 2) → 40 + 2 = 42.
    let src = "fn add(t: (i32, i32)) -> i32 = t.0 + t.1\nfn main() -> i32 {\n  return add((40, 2))\n}\n";
    let out = build_and_run("tup-param", src);
    assert_eq!(out.status.code(), Some(42));
}

#[test]
fn mixed_element_types() {
    if !backend_available() {
        return;
    }
    // A tuple of mixed primitive types `(i32, bool)`, destructured; the bool gates the result.
    let src = "fn flagged(n: i32) -> (i32, bool) = (n, n > 0)\nfn main() -> i32 {\n  (v, ok) := flagged(7)\n  if ok { return v }\n  return 0\n}\n";
    let out = build_and_run("tup-mixed", src);
    assert_eq!(out.status.code(), Some(7));
}

#[test]
fn tuple_value_through_local() {
    if !backend_available() {
        return;
    }
    // A tuple bound to a local, then destructured from the local (not the literal directly).
    // The elements default to i64 (no annotation), so compare in i64 and return a small i32.
    let src = "fn main() -> i32 {\n  t := (10, 32)\n  (a, b) := t\n  if a + b == 42 { return 42 }\n  return 0\n}\n";
    let out = build_and_run("tup-local", src);
    assert_eq!(out.status.code(), Some(42));
}

// --- diagnostics (rejected, no panic) ---

#[test]
fn destructure_arity_mismatch_errors() {
    // Binding 3 names against a 2-tuple is an error.
    assert!(check_errs("tup-arity", "fn main() -> i32 {\n  (a, b, c) := (1, 2)\n  return 0\n}\n"));
}

#[test]
fn index_out_of_range_errors() {
    assert!(check_errs("tup-oor", "fn main() -> i32 {\n  t := (1, 2)\n  return t.5\n}\n"));
}

#[test]
fn index_on_non_tuple_errors() {
    assert!(check_errs("tup-nontuple", "fn main() -> i32 {\n  x := 1\n  return x.0\n}\n"));
}

#[test]
fn destructure_non_tuple_errors() {
    assert!(check_errs("tup-destr-nontuple", "fn main() -> i32 {\n  (a, b) := 5\n  return 0\n}\n"));
}

#[test]
fn str_element_allowed() {
    if !backend_available() {
        return;
    }
    // `str` (a view) is a valid tuple element. A `(str, str)` of literals is region-0, returnable
    // and destructurable; "hello"+"world" lengths = 5 + 5 = 10.
    let src = "fn pair() -> (str, str) = (\"hello\", \"world\")\nfn main() -> i32 {\n  (a, b) := pair()\n  if a.len() + b.len() == 10 { return 10 }\n  return 0\n}\n";
    let out = build_and_run("tup-str", src);
    assert_eq!(out.status.code(), Some(10));
}

#[test]
fn arena_str_element_cannot_escape() {
    // A tuple holding an arena-backed `str` is region-tied to that arena and cannot be returned.
    let src = "fn bad() -> (str, i32) {\n  arena {\n    s := \"x\" + \"y\"\n    return (s, 1)\n  }\n}\nfn main() -> i32 = 0\n";
    assert!(check_errs("tup-arena-str", src));
}

// --- owned (Move) elements: temporaries only (returned / destructured) ---

#[test]
fn owned_array_tuple_return_and_destructure() {
    if !backend_available() {
        return;
    }
    // A function returns two owned arrays as a tuple; the caller destructures them. Each buffer
    // is freed exactly once (the returned source locals are nulled on the move). sum 6 + 30 = 36.
    let src = "fn split() -> (array<i64>, array<i64>) {\n  a := [1, 2, 3].to_array()\n  b := [10, 20].to_array()\n  return (a, b)\n}\nfn main() -> Result<(), Error> {\n  (xs, ys) := split()\n  print(xs.sum() + ys.sum())\n  return Ok(())\n}\n";
    let out = build_and_run("tup-owned-split", src);
    assert_eq!(out.status.code(), Some(0));
    assert_eq!(String::from_utf8_lossy(&out.stdout), "36\n");
}

#[test]
fn owned_tuple_literal_destructure() {
    if !backend_available() {
        return;
    }
    // `(x, y) := (a, b)` over owned-array locals: the literal moves a/b into the tuple, then the
    // destructure transfers them to x/y; a/b's slots are nulled so each buffer is freed once.
    let src = "fn main() -> Result<(), Error> {\n  a := [1, 2, 3].to_array()\n  b := [10, 20].to_array()\n  (x, y) := (a, b)\n  print(x.sum() + y.sum())\n  return Ok(())\n}\n";
    let out = build_and_run("tup-owned-lit", src);
    assert_eq!(out.status.code(), Some(0));
    assert_eq!(String::from_utf8_lossy(&out.stdout), "36\n");
}

#[test]
fn owned_string_tuple() {
    if !backend_available() {
        return;
    }
    // An owned `string` as a tuple element, returned and destructured (mixed with a scalar).
    let src = "fn named() -> (string, i64) {\n  b := builder()\n  b.write(\"align\")\n  return (b.to_string(), 7)\n}\nfn main() -> Result<(), Error> {\n  (name, n) := named()\n  print(name)\n  print(n)\n  return Ok(())\n}\n";
    let out = build_and_run("tup-owned-str", src);
    assert_eq!(out.status.code(), Some(0));
    assert_eq!(String::from_utf8_lossy(&out.stdout), "align\n7\n");
}

#[test]
fn owned_struct_array_tuple_element() {
    if !backend_available() {
        return;
    }
    // An owned `array<Struct>` (decoded) is a valid Move tuple element, destructured in scope.
    let src = "User { id: i64, score: i32 }\nfn main() -> Result<(), Error> {\n  arena {\n    users: array<User> := json.decode(\"[{\\\"id\\\":1,\\\"score\\\":10},{\\\"id\\\":2,\\\"score\\\":20}]\")?\n    (a, n) := (users, 5)\n    print(a.len())\n    print(n)\n  }\n  return Ok(())\n}\n";
    let out = build_and_run("tup-structarr", src);
    assert_eq!(out.status.code(), Some(0));
    assert_eq!(String::from_utf8_lossy(&out.stdout), "2\n5\n");
}

#[test]
fn bound_owned_tuple_then_destructure() {
    if !backend_available() {
        return;
    }
    // An owned tuple bound to a variable, then destructured: each buffer freed once. 6 + 30 = 36.
    let src = "fn split() -> (array<i64>, array<i64>) {\n  a := [1, 2, 3].to_array()\n  b := [10, 20].to_array()\n  return (a, b)\n}\nfn main() -> Result<(), Error> {\n  t := split()\n  (a, b) := t\n  print(a.sum() + b.sum())\n  return Ok(())\n}\n";
    let out = build_and_run("tup-owned-bound", src);
    assert_eq!(out.status.code(), Some(0));
    assert_eq!(String::from_utf8_lossy(&out.stdout), "36\n");
}

#[test]
fn bound_owned_tuple_unused_is_dropped() {
    if !backend_available() {
        return;
    }
    // A bound owned tuple that is never used: its `Drop` frees both owned elements (no leak,
    // no double-free) — the program runs and exits cleanly.
    let src = "fn split() -> (array<i64>, array<i64>) {\n  a := [1, 2, 3].to_array()\n  b := [10, 20].to_array()\n  return (a, b)\n}\nfn main() -> Result<(), Error> {\n  t := split()\n  return Ok(())\n}\n";
    let out = build_and_run("tup-owned-unused", src);
    assert_eq!(out.status.code(), Some(0));
}

#[test]
fn bound_owned_tuple_copy_field_read() {
    if !backend_available() {
        return;
    }
    // Reading a *Copy* element (`t.1` = i64) of a bound Move tuple is fine; the owned element is
    // still dropped at scope exit.
    let src = "fn f() -> (array<i64>, i64) {\n  a := [1, 2, 3].to_array()\n  return (a, 5)\n}\nfn main() -> Result<(), Error> {\n  t := f()\n  print(t.1)\n  return Ok(())\n}\n";
    let out = build_and_run("tup-owned-copyfield", src);
    assert_eq!(out.status.code(), Some(0));
    assert_eq!(String::from_utf8_lossy(&out.stdout), "5\n");
}

#[test]
fn owned_tuple_use_after_move_rejected() {
    // Consuming a Move tuple twice (destructure, then destructure again) is a use-after-move —
    // must be a compile error, not a runtime double-free.
    let src = "fn split() -> (array<i64>, array<i64>) {\n  a := [1].to_array()\n  b := [2].to_array()\n  return (a, b)\n}\nfn main() -> Result<(), Error> {\n  t := split()\n  (a, b) := t\n  (c, d) := t\n  print(a.sum())\n  return Ok(())\n}\n";
    assert!(check_errs("tup-uam", src));
}

#[test]
fn owned_tuple_use_after_pass_rejected() {
    // Passing a Move tuple to a function consumes it; using it afterwards is a use-after-move.
    let src = "fn split() -> (array<i64>, array<i64>) {\n  a := [1].to_array()\n  b := [2].to_array()\n  return (a, b)\n}\nfn take(t: (array<i64>, array<i64>)) -> i64 = 0\nfn main() -> Result<(), Error> {\n  t := split()\n  print(take(t))\n  (a, b) := t\n  print(a.sum())\n  return Ok(())\n}\n";
    assert!(check_errs("tup-uap", src));
}

#[test]
fn owned_tuple_field_partial_move() {
    if !backend_available() {
        return;
    }
    // `a := t.0` moves the owned element out of the bound tuple; the other element stays usable.
    // MIR nulls the moved field so the tuple's exit `Drop` frees null there (no double-free).
    let src = "fn main() -> Result<(), Error> {\n  a := [1, 2, 3].to_array()\n  b := [10, 20].to_array()\n  t := (a, b)\n  x := t.0\n  print(x.sum())\n  print(t.1.sum())\n  return Ok(())\n}\n";
    let out = build_and_run("tup-partial-move", src);
    assert_eq!(out.status.code(), Some(0));
    assert_eq!(String::from_utf8_lossy(&out.stdout), "6\n30\n");
}

#[test]
fn owned_tuple_both_fields_moved_individually() {
    if !backend_available() {
        return;
    }
    // Each owned element can be moved out independently; each buffer is freed exactly once.
    let src = "fn main() -> Result<(), Error> {\n  t := ([1, 2, 3].to_array(), [10, 20].to_array())\n  x := t.0\n  y := t.1\n  print(x.sum() + y.sum())\n  return Ok(())\n}\n";
    let out = build_and_run("tup-both-moved", src);
    assert_eq!(out.status.code(), Some(0));
    assert_eq!(String::from_utf8_lossy(&out.stdout), "36\n");
}

#[test]
fn owned_tuple_field_borrow_keeps_tuple() {
    if !backend_available() {
        return;
    }
    // A *borrowing* read (`t.0.sum()`) does not move the field — the tuple stays whole afterwards.
    let src = "fn main() -> Result<(), Error> {\n  t := ([1, 2, 3].to_array(), [10, 20].to_array())\n  print(t.0.sum())\n  print(t.1.sum())\n  (a, b) := t\n  print(a.sum() + b.sum())\n  return Ok(())\n}\n";
    let out = build_and_run("tup-field-borrow", src);
    assert_eq!(out.status.code(), Some(0));
    assert_eq!(String::from_utf8_lossy(&out.stdout), "6\n30\n36\n");
}

#[test]
fn copy_field_usable_after_owned_field_moved() {
    if !backend_available() {
        return;
    }
    // Moving an owned field must NOT poison a *Copy* field read: `t` mixes an owned array and a
    // scalar; after `x := t.0`, reading the Copy field `t.1` is still valid (per-field tracking).
    let src = "fn main() -> Result<(), Error> {\n  t := ([1, 2, 3].to_array(), 99)\n  x := t.0\n  print(x.sum())\n  print(t.1)\n  return Ok(())\n}\n";
    let out = build_and_run("tup-copy-after-move", src);
    assert_eq!(out.status.code(), Some(0));
    assert_eq!(String::from_utf8_lossy(&out.stdout), "6\n99\n");
}

#[test]
fn owned_tuple_field_reuse_after_move_rejected() {
    // Moving the same owned field twice is use-after-move.
    let src = "fn main() -> Result<(), Error> {\n  t := ([1, 2, 3].to_array(), [4, 5].to_array())\n  x := t.0\n  y := t.0\n  print(x.sum())\n  return Ok(())\n}\n";
    assert!(check_errs("tup-field-reuse", src));
}

#[test]
fn owned_tuple_whole_use_after_field_move_rejected() {
    // After a field is moved out, the tuple can no longer be used as a whole (destructured).
    let src = "fn main() -> Result<(), Error> {\n  t := ([1, 2, 3].to_array(), [4, 5].to_array())\n  x := t.0\n  (p, q) := t\n  print(x.sum())\n  return Ok(())\n}\n";
    assert!(check_errs("tup-whole-after-field", src));
}

#[test]
fn owned_tuple_field_index_on_temporary_rejected() {
    // Indexing an owned element out of a *temporary* tuple would orphan the other owned elements —
    // bind it to a variable (or destructure) first.
    let src = "fn mk() -> (array<i64>, i64) {\n  return ([1, 2, 3].to_array(), 9)\n}\nfn main() -> Result<(), Error> {\n  x := mk().0\n  print(x.sum())\n  return Ok(())\n}\n";
    assert!(check_errs("tup-owned-temp-index", src));
}

#[test]
fn owned_tuple_param_destructured() {
    if !backend_available() {
        return;
    }
    // An owned tuple passed as a parameter and destructured in the callee; each buffer freed once.
    let src = "fn split() -> (array<i64>, array<i64>) {\n  a := [1, 2, 3].to_array()\n  b := [10, 20].to_array()\n  return (a, b)\n}\nfn sumboth(t: (array<i64>, array<i64>)) -> i64 {\n  (a, b) := t\n  return a.sum() + b.sum()\n}\nfn main() -> Result<(), Error> {\n  print(sumboth(split()))\n  return Ok(())\n}\n";
    let out = build_and_run("tup-param-destr", src);
    assert_eq!(out.status.code(), Some(0));
    assert_eq!(String::from_utf8_lossy(&out.stdout), "36\n");
}

#[test]
fn owned_tuple_param_dropped_in_callee() {
    if !backend_available() {
        return;
    }
    // An owned tuple parameter that the callee never consumes is dropped (its owned elements
    // freed) at the callee's exit — no leak, no double-free.
    let src = "fn split() -> (array<i64>, array<i64>) {\n  a := [1].to_array()\n  b := [2].to_array()\n  return (a, b)\n}\nfn ignore(t: (array<i64>, array<i64>)) -> i64 = 7\nfn main() -> Result<(), Error> {\n  print(ignore(split()))\n  return Ok(())\n}\n";
    let out = build_and_run("tup-param-drop", src);
    assert_eq!(out.status.code(), Some(0));
    assert_eq!(String::from_utf8_lossy(&out.stdout), "7\n");
}
