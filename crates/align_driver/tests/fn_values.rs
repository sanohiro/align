//! First-class function values (slice ①): a top-level function used as a value is a function
//! pointer (`Ty::Fn`), and calling such a local is an indirect call. Non-capturing only — no
//! environment yet (lambda-as-value and captures are later slices).

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
fn named_fn_as_value_and_call() {
    if !backend_available() {
        return;
    }
    let src = "fn double(x: i32) -> i32 = x * 2\n\nfn main() -> Result<(), Error> {\n  f := double\n  print(f(5))\n  print(f(21))\n  return Ok(())\n}\n";
    let out = build_and_run("fv-basic", src);
    assert_eq!(out.status.code(), Some(0));
    assert_eq!(String::from_utf8_lossy(&out.stdout), "10\n42\n");
}

#[test]
fn reassign_fn_value_same_signature() {
    if !backend_available() {
        return;
    }
    // add and sub share a signature → the same Ty::Fn → a `mut` fn value can hold either.
    let src = "fn add(a: i64, b: i64) -> i64 = a + b\nfn sub(a: i64, b: i64) -> i64 = a - b\n\nfn main() -> Result<(), Error> {\n  mut op := add\n  print(op(10, 3))\n  op = sub\n  print(op(10, 3))\n  return Ok(())\n}\n";
    let out = build_and_run("fv-reassign", src);
    assert_eq!(out.status.code(), Some(0));
    assert_eq!(String::from_utf8_lossy(&out.stdout), "13\n7\n");
}

#[test]
fn arg_type_mismatch_rejected() {
    // Calling a fn value with the wrong argument type is a type error.
    assert!(check_errs(
        "fv-argty",
        "fn takes_float(x: f64) -> f64 = x\n\nfn main() -> i32 {\n  f := takes_float\n  if f(1) > 0.0 { return 1 }\n  return 0\n}\n"
    ));
}

#[test]
fn lambda_as_value_and_call() {
    if !backend_available() {
        return;
    }
    // A lambda used as a value (typed params) is lifted to a function pointer (slice ②a).
    let src = "fn main() -> Result<(), Error> {\n  f := fn x: i32 { x * 2 }\n  print(f(5))\n  g := fn a: i64, b: i64 { a + b }\n  print(g(10, 32))\n  return Ok(())\n}\n";
    let out = build_and_run("fv-lambda", src);
    assert_eq!(out.status.code(), Some(0));
    assert_eq!(String::from_utf8_lossy(&out.stdout), "10\n42\n");
}

#[test]
fn untyped_lambda_param_rejected_as_value() {
    // A lambda value has no use site to infer from, so params need explicit types.
    assert!(check_errs(
        "fv-untyped",
        "fn main() -> Result<(), Error> {\n  f := fn x { x * 2 }\n  print(f(5))\n  return Ok(())\n}\n"
    ));
}

#[test]
fn capturing_closure_value_and_call() {
    if !backend_available() {
        return;
    }
    // A capturing lambda copies the captured value into a frame-local env (slice ②b-2).
    let src = "fn main() -> Result<(), Error> {\n  k: i32 := 100\n  f := fn x: i32 { x + k }\n  print(f(5))\n  print(f(20))\n  return Ok(())\n}\n";
    let out = build_and_run("fv-capture", src);
    assert_eq!(out.status.code(), Some(0));
    assert_eq!(String::from_utf8_lossy(&out.stdout), "105\n120\n");
}

#[test]
fn closure_multiple_captures() {
    if !backend_available() {
        return;
    }
    // Two captures (a, b) + two explicit params: (x+y)*a - b.
    let src = "fn main() -> Result<(), Error> {\n  a: i64 := 10\n  b: i64 := 3\n  g := fn x: i64, y: i64 { (x + y) * a - b }\n  print(g(1, 2))\n  return Ok(())\n}\n";
    let out = build_and_run("fv-multicap", src);
    assert_eq!(out.status.code(), Some(0));
    assert_eq!(String::from_utf8_lossy(&out.stdout), "27\n");
}

#[test]
fn non_scalar_signature_rejected_as_value() {
    // slice ①: only scalar params/return may become a function value.
    assert!(check_errs(
        "fv-nonscalar",
        "fn sum(xs: slice<i64>) -> i64 = 0\n\nfn main() -> i32 {\n  f := sum\n  return 0\n}\n"
    ));
}
