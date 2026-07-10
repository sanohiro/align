//! `else` on `Result` (settled 2026-07-09). `v := f() else fallback` yields `Ok`'s value or, on
//! `Err`, deliberately discards the error and takes the fallback — completing the intent triangle
//! `?` propagates / `else` falls back / `match` inspects. The error is discarded, so the
//! unhandled-`Result` lint never fires on it. Symmetric with the existing `Option` `else`. The error
//! must be a Copy scalar today (every `Result` error is — the `Error` enum / a user error enum); a
//! Move error is deferred (rejected in sema, exercised in the `align_sema` unit tests). These are
//! end-to-end run tests; each returns a small (`< 256`) exit code the harness reads back.

mod common;
use common::*;

fn run_code(name: &str, src: &str) -> i32 {
    let out = build_and_run(name, src);
    out.status.code().expect("process exited with a code")
}

#[test]
fn ok_passes_through() {
    let src = "\
fn parse(s: str) -> Result<i32, Error> {
  if s == \"42\" { return Ok(42) }
  return Err(Error.Invalid)
}
fn main() -> i32 {
  return parse(\"42\") else 0
}
";
    assert_eq!(run_code("else-res-ok", src), 42);
}

#[test]
fn err_takes_fallback() {
    let src = "\
fn parse(s: str) -> Result<i32, Error> {
  if s == \"42\" { return Ok(42) }
  return Err(Error.Invalid)
}
fn main() -> i32 {
  return parse(\"nope\") else 99
}
";
    assert_eq!(run_code("else-res-err", src), 99);
}

#[test]
fn nested_else_chain() {
    // `a else b else c`: parse("x") is Err -> try parse("y") (also Err) -> fall back to 7.
    let src = "\
fn parse(s: str) -> Result<i32, Error> = if s == \"1\" { Ok(1) } else { Err(Error.Invalid) }
fn main() -> i32 {
  return parse(\"x\") else parse(\"y\") else 7
}
";
    assert_eq!(run_code("else-res-chain", src), 7);
    // And the same chain short-circuiting on the first Ok.
    let src2 = "\
fn parse(s: str) -> Result<i32, Error> = if s == \"1\" { Ok(1) } else { Err(Error.Invalid) }
fn main() -> i32 {
  return parse(\"x\") else parse(\"1\") else 7
}
";
    assert_eq!(run_code("else-res-chain2", src2), 1);
}

#[test]
fn move_ok_payload_ok_branch_no_double_free() {
    // `Result<string, Error> else <owned fallback>`: the Ok branch moves the owned `string` out of
    // the result and returns its length; a clean exit (not a SIGABRT) confirms no double-free / leak
    // of the moved-out buffer.
    let src = "\
fn get(ok: bool) -> Result<string, Error> {
  if ok { return Ok(\"hello\".clone()) }
  return Err(Error.NotFound)
}
fn main() -> i32 {
  s := get(true) else \"fallback\".clone()
  return s.len() as i32
}
";
    assert_eq!(run_code("else-res-move-ok", src), 5);
}

#[test]
fn move_ok_payload_err_branch_uses_fallback() {
    // The Err branch discards the (Copy) error and takes the owned fallback; its length is 8.
    let src = "\
fn get(ok: bool) -> Result<string, Error> {
  if ok { return Ok(\"hello\".clone()) }
  return Err(Error.NotFound)
}
fn main() -> i32 {
  s := get(false) else \"fallback\".clone()
  return s.len() as i32
}
";
    assert_eq!(run_code("else-res-move-err", src), 8);
}

#[test]
fn fallback_type_mismatch_is_a_clean_error() {
    // The fallback must unify with Ok's payload type — a `str` fallback for an `i32` payload errors.
    let bad = check_errs(
        "else-res-mismatch",
        "fn parse(s: str) -> Result<i32, Error> = if s == \"1\" { Ok(1) } else { Err(Error.Invalid) }\nfn main() -> i32 {\n  return parse(\"a\") else \"oops\"\n}\n",
    );
    assert!(bad, "a fallback that doesn't unify with Ok's payload must be a clean error");
}

#[test]
fn move_error_is_deferred() {
    // A `Result<T, string>` (a Move error) is rejected — its discarded buffer would leak.
    let bad = check_errs(
        "else-res-move-error",
        "fn f() -> Result<i32, string> = Err(\"x\".clone())\nfn main() -> i32 {\n  return f() else 0\n}\n",
    );
    assert!(bad, "else on a Result with a Move error must be rejected (deferred)");
}
