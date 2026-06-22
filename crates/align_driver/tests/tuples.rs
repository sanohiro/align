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
fn non_primitive_element_rejected() {
    // PR1 cut: a `str` element is not allowed in a tuple yet.
    assert!(check_errs("tup-str", "fn main() -> i32 {\n  t := (\"hi\", 1)\n  return 0\n}\n"));
}
