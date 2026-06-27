//! Error type — slice 4b-4 (structured, position-bearing errors). A user error enum whose variant
//! carries a plain-data struct payload models a parse/validation error that *carries its position*
//! (line/column, offset) as structured data — the Align way of attaching context to an error
//! (structured sum-type payloads), rather than free-form string chaining. This works on the
//! 4b-1 (user error enums) + S2 (plain-struct variant payloads) foundation, no new mechanism.


mod common;
use common::*;

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

// ---- match on an owned-payload Result/Option must not double-free the moved buffer ----
// (Gemini M2 report Part 2 §4: matching `Ok(users)` moved the array out but the scrutinee's
// exit Drop still freed it → double-free crash. The arm-binding now nulls the scrutinee.)

#[test]
fn match_owned_result_payload_no_double_free() {
    if !backend_available() {
        return;
    }
    // Bind + consume the owned array via `.len()` in the Ok arm; the scrutinee `res` must not also
    // free it. (Before the fix this aborted with "double free detected".)
    let src = concat!(
        "import core.json\n",
        "User { id: i64 }\n",
        "fn main() -> i32 {\n  arena {\n",
        "    res: Result<array<User>, Error> := json.decode(\"[{\\\"id\\\":1},{\\\"id\\\":2}]\")\n",
        "    n := match res {\n      Ok(users) => users.len(),\n      Err(_) => 0 - 1,\n    }\n",
        "    print(n)\n  }\n  return 0\n}\n",
    );
    let out = build_and_run("match-owned-nodf", src);
    assert_eq!(out.status.code(), Some(0));
    assert_eq!(String::from_utf8_lossy(&out.stdout), "2\n");
}

#[test]
fn match_returns_owned_payload_no_double_free() {
    if !backend_available() {
        return;
    }
    // The Ok arm *returns* the owned array (moves it out of the match result); neither the
    // scrutinee nor the arm binding may free the buffer the caller now owns.
    let src = concat!(
        "import core.json\n",
        "fn pick(data: str) -> array<i64> {\n",
        "  res: Result<array<i64>, Error> := json.decode(data)\n",
        "  return match res {\n    Ok(xs) => xs,\n    Err(_) => [0].to_array(),\n  }\n",
        "}\n",
        "fn main() -> i32 {\n  ys := pick(\"[10, 20, 12]\")\n  return ys.sum() as i32\n}\n",
    );
    let out = build_and_run("match-return-owned-nodf", src);
    assert_eq!(out.status.code(), Some(42));
}

#[test]
fn match_wildcard_owned_result_frees_once() {
    if !backend_available() {
        return;
    }
    // A wildcard arm binds nothing, so the scrutinee keeps ownership and frees the buffer exactly
    // once (no double-free, no leak/crash).
    let src = concat!(
        "import core.json\n",
        "User { id: i64 }\n",
        "fn main() -> i32 {\n  arena {\n",
        "    res: Result<array<User>, Error> := json.decode(\"[{\\\"id\\\":1}]\")\n",
        "    print(match res { Ok(_) => 7, Err(_) => 9 })\n  }\n  return 0\n}\n",
    );
    let out = build_and_run("match-wild-owned", src);
    assert_eq!(out.status.code(), Some(0));
    assert_eq!(String::from_utf8_lossy(&out.stdout), "7\n");
}
