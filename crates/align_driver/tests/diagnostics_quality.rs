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

/// A rejected operand or target must yield the error sentinel, never its own type. Later passes
/// classify unary, binary, and cast results as borrowing nothing, so propagating a view-bearing
/// type builds a node the borrow-provenance walk cannot explain — in a debug build that is a
/// compiler panic on malformed input, which is what the nightly fuzz sweep hit at seed 2561.
/// These are the whole class, not just the reported cell: unary, arithmetic, bitwise, and casts.
#[test]
fn a_rejected_operand_does_not_propagate_a_borrowing_type() {
    for (label, src, expected) in [
        (
            "unary minus",
            "fn main() -> i32 {\n  s := \"text\"\n  v := -s\n  return 0\n}\n",
            "unary '-' expects a number",
        ),
        (
            "unary bit-not",
            "fn main() -> i32 {\n  s := \"text\"\n  v := ~s\n  return 0\n}\n",
            "unary '~' expects an integer",
        ),
        (
            "unary over a slice",
            "fn main() -> i32 {\n  xs := [1, 2]\n  v := -xs[..]\n  return 0\n}\n",
            "unary '-' expects a number",
        ),
        (
            "bitwise over strings",
            "fn main() -> i32 {\n  s := \"text\"\n  v := s & s\n  return 0\n}\n",
            "bitwise and shift operators expect integers",
        ),
        (
            "arithmetic over slices",
            "fn main() -> i32 {\n  xs := [1, 2]\n  v := xs[..] - xs[..]\n  return 0\n}\n",
            "arithmetic expects numbers",
        ),
        (
            "cast to a view type",
            "fn main() -> i32 {\n  v := 1 as str\n  return 0\n}\n",
            "cannot cast to `str`",
        ),
    ] {
        let diagnostics = check_diagnostics("rejected-operand-sentinel", src);
        assert!(
            diagnostics.contains(expected),
            "{label}: expected `{expected}`:\n{diagnostics}"
        );
    }
}
