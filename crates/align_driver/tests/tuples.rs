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
fn bound_owned_tuple_rejected() {
    // Cut: an owned tuple may not be bound to a variable — it must be destructured directly.
    let src = "fn split() -> (array<i64>, array<i64>) {\n  a := [1].to_array()\n  b := [2].to_array()\n  return (a, b)\n}\nfn main() -> i32 {\n  t := split()\n  return 0\n}\n";
    assert!(check_errs("tup-owned-bound", src));
}

#[test]
fn owned_tuple_param_rejected() {
    // Cut: an owned tuple may not be a function parameter (it would need callee-side drop).
    let src = "fn f(t: (array<i64>, array<i64>)) -> i32 = 0\nfn main() -> i32 = 0\n";
    assert!(check_errs("tup-owned-param", src));
}
