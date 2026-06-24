//! Error type — slice 4b-4 (structured, position-bearing errors). A user error enum whose variant
//! carries a plain-data struct payload models a parse/validation error that *carries its position*
//! (line/column, offset) as structured data — the Align way of attaching context to an error
//! (structured sum-type payloads), rather than free-form string chaining. This works on the
//! 4b-1 (user error enums) + S2 (plain-struct variant payloads) foundation, no new mechanism.

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

#[test]
fn position_bearing_error_matched() {
    if !backend_available() {
        return;
    }
    // A parse error carries its position as a struct payload; the caller matches and reads it.
    let src = concat!(
        "Pos { line: i32, col: i32 }\n",
        "ParseError { BadToken(Pos), Eof }\n",
        "fn parse(n: i32) -> Result<i32, ParseError> {\n",
        "  if n < 0 { return Err(ParseError.BadToken(Pos { line: 3, col: 7 })) }\n",
        "  if n == 0 { return Err(ParseError.Eof) }\n",
        "  return Ok(n * 2)\n",
        "}\n",
        "fn main() -> i32 {\n",
        "  return match parse(-1) {\n",
        "    Ok(v) => v\n",
        "    Err(e) => match e {\n",
        "      BadToken(p) => p.line * 10 + p.col\n",
        "      Eof => 99\n",
        "    }\n",
        "  }\n",
        "}\n",
    );
    let out = build_and_run("structured-err-pos", src);
    assert_eq!(out.status.code(), Some(37)); // line 3 * 10 + col 7
}

#[test]
fn position_bearing_error_propagated_via_question() {
    if !backend_available() {
        return;
    }
    // `?` propagates the structured error unchanged (same E); the success path returns the value.
    let src = concat!(
        "Span { off: i32 }\n",
        "LexError { Unexpected(Span) }\n",
        "fn lex(n: i32) -> Result<i32, LexError> {\n",
        "  if n == 0 { return Err(LexError.Unexpected(Span { off: 42 })) }\n",
        "  return Ok(n)\n",
        "}\n",
        "fn run(n: i32) -> Result<i32, LexError> {\n",
        "  v := lex(n)?\n", // propagate the structured error
        "  return Ok(v + 1)\n",
        "}\n",
        "fn main() -> i32 {\n",
        "  return match run(0) {\n",
        "    Ok(v) => v\n",
        "    Err(e) => match e { Unexpected(s) => s.off }\n",
        "  }\n",
        "}\n",
    );
    let out = build_and_run("structured-err-prop", src);
    assert_eq!(out.status.code(), Some(42)); // the propagated Span.off
}
