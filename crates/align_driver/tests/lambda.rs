//! Inline lambdas (`fn x { … }`) as pipeline-stage arguments (`draft.md` §11). Slice ①: a
//! non-capturing lambda in `map` / `where` is lifted to a synthetic top-level function, so it
//! flows through the existing fused-loop lowering — optimized identically to a named function.

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
fn map_lambda() {
    if !backend_available() {
        return;
    }
    let src = "fn main() -> Result<(), Error> {\n  print([1, 2, 3].map(fn x { x * 2 }).sum())\n  return Ok(())\n}\n";
    let out = build_and_run("lam-map", src);
    assert_eq!(out.status.code(), Some(0));
    assert_eq!(String::from_utf8_lossy(&out.stdout), "12\n");
}

#[test]
fn where_lambda() {
    if !backend_available() {
        return;
    }
    let src = "fn main() -> Result<(), Error> {\n  print([1, 2, 3, 4, 5].where(fn x { x > 2 }).sum())\n  return Ok(())\n}\n";
    let out = build_and_run("lam-where", src);
    assert_eq!(out.status.code(), Some(0));
    assert_eq!(String::from_utf8_lossy(&out.stdout), "12\n");
}

#[test]
fn map_and_where_lambdas_fuse() {
    if !backend_available() {
        return;
    }
    // map *10, keep >25 → 30+40+50 = 120. Both lambdas lift and fuse into one loop.
    let src = "fn main() -> Result<(), Error> {\n  print([1, 2, 3, 4, 5].map(fn x { x * 10 }).where(fn x { x > 25 }).sum())\n  return Ok(())\n}\n";
    let out = build_and_run("lam-fuse", src);
    assert_eq!(out.status.code(), Some(0));
    assert_eq!(String::from_utf8_lossy(&out.stdout), "120\n");
}

#[test]
fn lambda_lifts_to_a_called_function() {
    // The lambda becomes a synthetic top-level function the fused loop calls (so LLVM inlines it
    // exactly like a named stage function).
    let src = "fn main() -> Result<(), Error> {\n  print([1, 2, 3].map(fn x { x * 2 }).sum())\n  return Ok(())\n}\n";
    let mut sm = SourceMap::new();
    let mir = lower_to_mir(&check(&mut sm, "m", src).hir);
    let text = align_mir::print::program_to_string(&mir);
    assert!(text.contains("$lambda"), "lambda should be lifted to a synthetic function:\n{text}");
    assert!(text.contains("call main$lambda0"), "the fused loop should call the lifted lambda:\n{text}");
}

#[test]
fn lambda_can_call_a_named_function() {
    if !backend_available() {
        return;
    }
    // A lambda body may reference top-level functions (resolved via signatures), just not
    // enclosing locals (capture is a follow-up slice).
    let src = "fn inc(x: i64) -> i64 = x + 1\nfn main() -> Result<(), Error> {\n  print([1, 2, 3].map(fn x { inc(x) * 2 }).sum())\n  return Ok(())\n}\n";
    let out = build_and_run("lam-callnamed", src);
    assert_eq!(out.status.code(), Some(0));
    // (2+4+6)*... → (1+1)*2 + (2+1)*2 + (3+1)*2 = 4+6+8 = 18
    assert_eq!(String::from_utf8_lossy(&out.stdout), "18\n");
}

#[test]
fn bare_lambda_rejected() {
    // A lambda value outside a stage argument is not yet a first-class value.
    let src = "fn main() -> Result<(), Error> {\n  f := fn x { x * 2 }\n  return Ok(())\n}\n";
    assert!(check_errs("lam-bare", src));
}

#[test]
fn lambda_capture_rejected_for_now() {
    // Slice ① is non-capturing: referencing an enclosing local is an undefined-name error.
    let src = "fn main() -> Result<(), Error> {\n  factor := 3\n  print([1, 2, 3].map(fn x { x * factor }).sum())\n  return Ok(())\n}\n";
    assert!(check_errs("lam-capture", src));
}

#[test]
fn lambda_wrong_arity_rejected() {
    // A `map` lambda takes exactly one parameter.
    let src = "fn main() -> Result<(), Error> {\n  print([1, 2, 3].map(fn x, y { x + y }).sum())\n  return Ok(())\n}\n";
    assert!(check_errs("lam-arity", src));
}
