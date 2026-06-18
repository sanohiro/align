//! M2 end-to-end: Option / `else`-unwrap (and, later, Result / `?`).
//! Requires LLVM/cc, so skip where they are absent.

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
    std::process::Command::new(&exe).output().expect("run")
}

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
