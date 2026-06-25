//! M2 end-to-end: Option / `else`-unwrap and Result / `?` (incl. Result-returning main).
//! Requires LLVM/cc, so skip where they are absent.


mod common;
use common::*;

const CHOOSE: &str =
    "fn choose(b: bool) -> Option<i32> {\n  if b { return Some(7) }\n  return None\n}\n";

#[test]
fn option_else_unwrap_value_fallback() {
    if !backend_available() {
        return;
    }
    // Some(7) → 7; None → 99 fallback. 7 + 99 = 106.
    let src = format!("{CHOOSE}fn main() -> i32 {{\n  x := choose(true) else 99\n  y := choose(false) else 99\n  return x + y\n}}\n");
    let out = build_and_run("opt-value", &src);
    assert_eq!(out.status.code(), Some(106));
}

#[test]
fn option_else_unwrap_diverging_fallback() {
    if !backend_available() {
        return;
    }
    // None path runs the diverging `else { return 42 }`.
    let src = format!("{CHOOSE}fn main() -> i32 {{\n  x := choose(false) else {{ return 42 }}\n  return x\n}}\n");
    let out = build_and_run("opt-diverge", &src);
    assert_eq!(out.status.code(), Some(42));
}

const TRY_GET: &str =
    "fn try_get(n: i32) -> Result<i32, Error> {\n  if n < 0 { return Err(error(7)) }\n  return Ok(n)\n}\n";

#[test]
fn result_question_propagates_err_from_main() {
    if !backend_available() {
        return;
    }
    // try_get(-1) → Err(7); `?` propagates it out of main → reported, exit 7.
    let src = format!("{TRY_GET}pub fn main() -> Result<(), Error> {{\n  x := try_get(-1)?\n  return Ok(())\n}}\n");
    let out = build_and_run("res-err", &src);
    assert_eq!(out.status.code(), Some(7));
    assert!(
        String::from_utf8_lossy(&out.stderr).contains("code 7"),
        "stderr: {}",
        String::from_utf8_lossy(&out.stderr)
    );
}

#[test]
fn result_question_ok_path_exits_zero() {
    if !backend_available() {
        return;
    }
    let src = format!("{TRY_GET}pub fn main() -> Result<(), Error> {{\n  x := try_get(5)?\n  return Ok(())\n}}\n");
    let out = build_and_run("res-ok", &src);
    assert_eq!(out.status.code(), Some(0));
}

#[test]
fn result_struct_payload_ok_and_err() {
    if !backend_available() {
        return;
    }
    // Result<Pt, Error>: `?` unwraps a struct on Ok; an Err propagates the code.
    let ok_src = "Pt { x: i32, y: i32 }\nfn make() -> Result<Pt, Error> {\n  p := Pt{x: 40, y: 2}\n  return Ok(p)\n}\nfn main() -> Result<(), Error> {\n  q := make()?\n  print(q.x + q.y)\n  return Ok(())\n}\n";
    let ok = build_and_run("res-struct-ok", ok_src);
    assert_eq!(ok.status.code(), Some(0));
    assert_eq!(String::from_utf8_lossy(&ok.stdout), "42\n");

    let err_src = "Pt { x: i32, y: i32 }\nfn make() -> Result<Pt, Error> = Err(error(5))\nfn main() -> Result<(), Error> {\n  q := make()?\n  print(q.x)\n  return Ok(())\n}\n";
    let err = build_and_run("res-struct-err", err_src);
    assert_eq!(err.status.code(), Some(5), "Err propagates through a struct-payload Result");
}

#[test]
fn option_struct_payload_else_unwrap() {
    if !backend_available() {
        return;
    }
    // Option<Pt>: `else` unwraps the struct (Some) or runs the diverging fallback (None).
    let src = "Pt { x: i32, y: i32 }\nfn pick(b: bool) -> Option<Pt> {\n  if b {\n    p := Pt{x: 30, y: 12}\n    return Some(p)\n  }\n  return None\n}\nfn main() -> i32 {\n  q := pick(true) else { return 99 }\n  return q.x + q.y\n}\n";
    let out = build_and_run("opt-struct", src);
    assert_eq!(out.status.code(), Some(42));
}

#[test]
fn err_code_zero_still_exits_nonzero() {
    if !backend_available() {
        return;
    }
    // Err(0) must not be mistaken for success: the exit code is clamped to nonzero.
    let src = "fn f() -> Result<i32, Error> {\n  return Err(error(0))\n}\npub fn main() -> Result<(), Error> {\n  x := f()?\n  return Ok(())\n}\n";
    let out = build_and_run("res-zero", src);
    assert_eq!(out.status.code(), Some(1), "Err(0) clamps to exit 1");
}
