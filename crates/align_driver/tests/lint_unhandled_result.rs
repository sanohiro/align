//! The "unhandled `Result`" lint (`draft.md` §16): discarding a `Result` as a statement silently
//! drops a possible error, against Align's "errors are visible / handled" stance — so it is a
//! compile error. Propagate with `?`, branch with `match` / `else`, or bind it (`r := …`).

mod common;
use common::*;

#[test]
fn discarded_result_rejected() {
    let src = "fn f() -> Result<i32, Error> = Ok(1)\nfn main() -> i32 {\n  f()\n  return 0\n}\n";
    assert!(check_errs("unh-discard", src));
}

#[test]
fn bound_result_is_handled() {
    if !backend_available() {
        return;
    }
    // Binding the Result handles it (the binding is checked / dropped normally).
    let src = "fn f() -> Result<i32, Error> = Ok(7)\nfn main() -> i32 {\n  r := f()\n  return match r { Ok(x) => x, Err(e) => 0 }\n}\n";
    let out = build_and_run("unh-bound", src);
    assert_eq!(out.status.code(), Some(7));
}

#[test]
fn propagated_result_is_handled() {
    if !backend_available() {
        return;
    }
    // `?` propagation is not a discard.
    let src = concat!(
        "fn f() -> Result<i32, Error> = Ok(5)\n",
        "fn run() -> Result<i32, Error> {\n",
        "  v := f()?\n",
        "  return Ok(v + 1)\n",
        "}\n",
        "fn main() -> i32 = match run() { Ok(x) => x, Err(e) => 0 }\n",
    );
    let out = build_and_run("unh-prop", src);
    assert_eq!(out.status.code(), Some(6));
}

#[test]
fn non_result_statement_is_fine() {
    if !backend_available() {
        return;
    }
    // A non-`Result` statement expression (here `print`, returning unit) is not flagged.
    let src = "fn main() -> i32 {\n  print(1)\n  return 0\n}\n";
    let out = build_and_run("unh-nonresult", src);
    assert_eq!(out.status.code(), Some(0));
}
