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

#[test]
fn reserved_identifiers_report_the_word_without_dependent_cascades() {
    for (word, src) in [
        ("arena", "fn total(borrow arena: slice<u8>) -> i64 {\n return arena.len()\n}\nfn main() {\n data: buffer := buffer(8)\n view := data.bytes()\n print(total(view))\n}\n"),
        ("arena", "fn main() {\n arena := 3\n print(arena)\n}\n"),
        ("unsafe", "fn total(borrow unsafe: i64) -> i64 { return unsafe }\nfn main() { value := 3\n print(total(value)) }\n"),
        ("arena", "fn main() { mut arena: i64 := 3\n print(arena) }"),
        ("mut", "fn main() { mut := 3\n print(mut) }"),
        ("arena", "fn main() { (arena, value) := (3, 4)\n print(arena + value) }"),
        ("unsafe", "Data { unsafe: i64 }\nfn main() { value := Data { unsafe: 3 }\n print(value.unsafe) }"),
        ("unsafe", "fn unsafe() -> i64 = 3\nfn main() { print(unsafe()) }"),
    ] {
        for per_unit in [false, true] {
            let mut sm = SourceMap::new();
            let diags = if per_unit {
                align_driver::check_per_unit(&mut sm, "reserved.align", src).diags
            } else {
                check(&mut sm, "reserved.align", src).diags
            };
            let rendered = align_driver::format_diagnostics(&sm, &diags);
            assert_eq!(diags.error_count(), 1, "per_unit={per_unit}: {src}\n{rendered}");
            let error = diags.iter().find(|d| d.severity == align_diag::Severity::Error)
                .expect("one error");
            assert!(error.message.contains(&format!("`{word}` is a reserved word")), "{rendered}");
            let span = error.span.expect("word span");
            assert_eq!(&src[span.lo as usize..span.hi as usize], word, "{rendered}");
            assert_eq!(Some(span.lo as usize), src.find(word), "{rendered}");
        }
    }
}

#[test]
fn reserved_identifier_recovery_does_not_hide_an_independent_borrow_error() {
    // Request 51's original parameter repro also passes a temporary to a Borrow parameter.
    // Preserve that independent error; a named-view control is covered above.
    let src = "fn total(borrow arena: slice<u8>) -> i64 { return arena.len() }\nfn main() { data: buffer := buffer(8)\n print(total(data.bytes())) }";
    for per_unit in [false, true] {
        let mut sm = SourceMap::new();
        let diags = if per_unit {
            align_driver::check_per_unit(&mut sm, "reserved-borrow.align", src).diags
        } else {
            check(&mut sm, "reserved-borrow.align", src).diags
        };
        let rendered = align_driver::format_diagnostics(&sm, &diags);
        assert_eq!(diags.error_count(), 2, "per_unit={per_unit}: {rendered}");
        assert!(rendered.contains("`arena` is a reserved word"), "{rendered}");
        assert!(rendered.contains("must be a stable named local or field"), "{rendered}");
    }
}
