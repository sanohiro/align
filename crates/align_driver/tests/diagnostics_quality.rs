//! Diagnostic-quality regressions from the 2026-07-02 audit (2-7): a type-mismatch message must
//! name the user's type (`MyErr`), not the compiler-internal placeholder (`enum#0`).

mod common;
use common::*;
use align_span::SourceMap;

#[test]
fn enum_name_not_leaked_in_type_mismatch() {
    // Returning a bare enum value where a `Result` is expected (forgot to wrap in `Err(...)`).
    let src = "\
MyErr { NotFound }
fn f() -> Result<i32, MyErr> {
  return MyErr.NotFound
}
fn main() -> i32 { return 0 }
";
    let mut sm = SourceMap::new();
    let checked = check(&mut sm, "enum-name", src);
    let text = align_driver::format_diagnostics(&sm, &checked.diags);
    assert!(text.contains("MyErr"), "type-mismatch should name the enum, got:\n{text}");
    assert!(!text.contains("enum#"), "must not leak the internal `enum#N` name, got:\n{text}");
}
