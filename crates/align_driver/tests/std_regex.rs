//! Post-M15 `std.regex` first slice — `regex.compile` + borrowed `is_match` / `find` / `find_at`,
//! returning the builtin Copy `regex_match { start, end }` (half-open UTF-8 byte span).
//!
//! The compiled `regex` is an owned Move handle (no implicit cache); the enclosing `Result` owns it
//! and `Drop`s via `align_rt_regex_free`. Coverage: import gating, arity/type diagnostics, the
//! reserved `regex` / `regex_match` names, the bound-receiver restriction, valid/invalid patterns,
//! ASCII + Unicode matching, empty matches, end-of-input `find_at`, UTF-8 byte offsets, the
//! programmer-error aborts (negative / past-end / mid-code-point start), the pattern-length limit,
//! the `Result` Err path, and Move / Drop soundness (no double-free / leak). (roadmap Post-M15
//! std.regex; `draft.md` §18.2; `docs/impl/std-design/regex.md`.)

mod common;
use common::*;

// --- import gating ------------------------------------------------------------------------------

/// `regex.compile` requires `import std.regex`; without it the call is a diagnostic.
#[test]
fn compile_requires_the_import() {
    let missing = "\
pub fn main() -> Result<(), Error> {
  re := regex.compile(\"a\")?
  print(re.is_match(\"a\"))
  return Ok(())
}
";
    assert!(check_errs("regex-no-import", missing), "regex.compile without `import std.regex` must error");

    let present = "\
import std.regex
pub fn main() -> Result<(), Error> {
  re := regex.compile(\"a\")?
  print(re.is_match(\"a\"))
  return Ok(())
}
";
    assert!(!check_errs("regex-with-import", present), "the imported form must type-check");
}

// --- arity / type diagnostics -------------------------------------------------------------------

/// `compile` takes exactly one pattern; the search methods have fixed arities and a typed start.
#[test]
fn arity_and_type_diagnostics() {
    let cases = [
        ("import std.regex\npub fn main() -> Result<(), Error> {\n  re := regex.compile(\"a\", \"b\")?\n  return Ok(())\n}\n", "compile arity"),
        ("import std.regex\npub fn main() -> Result<(), Error> {\n  re := regex.compile(\"a\")?\n  print(re.is_match())\n  return Ok(())\n}\n", "is_match arity"),
        ("import std.regex\npub fn main() -> Result<(), Error> {\n  re := regex.compile(\"a\")?\n  print(re.find(\"a\", 0))\n  return Ok(())\n}\n", "find arity"),
        ("import std.regex\npub fn main() -> Result<(), Error> {\n  re := regex.compile(\"a\")?\n  match re.find_at(\"a\", \"x\") { Some(m) => print(m.start), None => print(-1) }\n  return Ok(())\n}\n", "find_at start type"),
        ("import std.regex\npub fn main() -> Result<(), Error> {\n  re := regex.compile(42)?\n  return Ok(())\n}\n", "compile pattern type"),
    ];
    for (src, what) in cases {
        assert!(check_errs("regex-arity", src), "expected a diagnostic for: {what}");
    }
}

// --- reserved names -----------------------------------------------------------------------------

/// `regex` and `regex_match` are builtin reserved types: nameable in signatures, but a user
/// declaration of `regex_match` is rejected, and `regex` resolves to the handle type.
#[test]
fn reserved_names_and_type_resolution() {
    // `regex_match` names the builtin span struct in a pattern.
    let uses = "\
import std.regex
pub fn main() -> Result<(), Error> {
  re := regex.compile(\"[0-9]+\")?
  match re.find(\"x42\") {
    Some(m) => print(m.end - m.start),
    None => print(-1),
  }
  return Ok(())
}
";
    let out = build_and_run("regex-match-type", uses);
    assert_eq!(out.status.code(), Some(0), "stderr: {}", String::from_utf8_lossy(&out.stderr));
    assert_eq!(String::from_utf8_lossy(&out.stdout), "2\n");

    // Redeclaring the reserved `regex_match` struct is an error.
    let redecl = "\
import std.regex
regex_match { a: i64 }
pub fn main() -> Result<(), Error> { return Ok(()) }
";
    assert!(check_errs("regex-match-redecl", redecl), "declaring `regex_match` must be rejected");

    // `regex` is nameable as a parameter type (a compiled plan passed between functions).
    let param = "\
import std.regex
fn run(re: regex, s: str) -> bool = re.is_match(s)
pub fn main() -> Result<(), Error> {
  re := regex.compile(\"^a\")?
  print(run(re, \"abc\"))
  return Ok(())
}
";
    let out = build_and_run("regex-param-type", param);
    assert_eq!(out.status.code(), Some(0), "stderr: {}", String::from_utf8_lossy(&out.stderr));
    assert_eq!(String::from_utf8_lossy(&out.stdout), "true\n");
}

// --- bound Move receiver restriction ------------------------------------------------------------

/// A search must go through a bound local — an unbound Move-temporary receiver is rejected (its
/// drop is still deferred).
#[test]
fn search_requires_a_bound_receiver() {
    let temp = "\
import std.regex
pub fn main() -> Result<(), Error> {
  print(regex.compile(\"a\")?.is_match(\"a\"))
  return Ok(())
}
";
    assert!(check_errs("regex-temp-recv", temp), "a search on a temporary compiled handle must be rejected");
}

// --- valid / invalid patterns + is_match --------------------------------------------------------

/// A syntactically invalid pattern is `Error.Invalid` (the `Result` Err path), a valid one matches.
#[test]
fn invalid_pattern_is_err_valid_matches() {
    let prog = "\
import std.regex
pub fn main() -> Result<(), Error> {
  match regex.compile(\"[\") {
    Ok(_) => print(\"unexpected-ok\"),
    Err(e) => print(\"invalid\"),
  }
  re := regex.compile(\"^[a-z]+$\")?
  print(re.is_match(\"hello\"))
  print(re.is_match(\"Hello\"))
  print(re.is_match(\"\"))
  return Ok(())
}
";
    let out = build_and_run("regex-invalid-valid", prog);
    assert_eq!(out.status.code(), Some(0), "stderr: {}", String::from_utf8_lossy(&out.stderr));
    assert_eq!(String::from_utf8_lossy(&out.stdout), "invalid\ntrue\nfalse\nfalse\n");
}

// --- find / find_at: ASCII, Unicode, byte offsets, empty match, end-of-input --------------------

/// `find` reports half-open UTF-8 byte offsets that slice the text; Unicode-width prefixes shift
/// the offsets; `find_at` resumes at a byte offset; an empty match at end-of-input is valid.
#[test]
fn find_and_find_at_semantics() {
    let prog = "\
import std.regex
pub fn main() -> Result<(), Error> {
  digits := regex.compile(\"[0-9]+\")?
  // 'π' is two UTF-8 bytes, so the digit run starts at byte 3.
  s := \"π=314\"
  match digits.find(s) {
    Some(m) => {
      print(m.start)
      print(m.end)
      print(s[m.start .. m.end])
    }
    None => print(\"none\"),
  }
  // find_at resumes past the first match.
  a := regex.compile(\"a\")?
  match a.find_at(\"a_a\", 1) {
    Some(m) => print(m.start),
    None => print(-1),
  }
  // An empty match at end-of-input.
  empty := regex.compile(\"x*\")?
  match empty.find_at(\"xx\", 2) {
    Some(m) => {
      print(m.start)
      print(m.end)
    }
    None => print(-1),
  }
  // No match after the offset.
  match a.find_at(\"a__\", 1) {
    Some(m) => print(m.start),
    None => print(\"no-more\"),
  }
  return Ok(())
}
";
    let out = build_and_run("regex-find", prog);
    assert_eq!(out.status.code(), Some(0), "stderr: {}", String::from_utf8_lossy(&out.stderr));
    assert_eq!(String::from_utf8_lossy(&out.stdout), "3\n6\n314\n2\n2\n2\nno-more\n");
}

/// `find_at(text, k)` anchors against the TRUE start of `text`, not against `k`: `^` / `\A` / `\b`
/// resolve to position 0 (and the byte at `k-1` for `\b`), so it is NOT `find(text[k..])`. This
/// pins the documented semantics gap (`docs/impl/std-design/regex.md`).
#[test]
fn find_at_anchors_against_the_true_start() {
    let prog = "\
import std.regex
pub fn main() -> Result<(), Error> {
  // `^a` only matches at position 0; starting at 1 finds nothing even though byte 2 is 'a'.
  caret := regex.compile(\"^a\")?
  match caret.find_at(\"aXa\", 1) {
    Some(m) => print(m.start),
    None => print(-1),
  }
  // `\\bword` uses the byte before `start` as boundary context: 'word' after the space matches.
  wb := regex.compile(\"\\\\bword\")?
  match wb.find_at(\"a word\", 1) {
    Some(m) => print(m.start),
    None => print(-1),
  }
  return Ok(())
}
";
    let out = build_and_run("regex-find-at-anchor", prog);
    assert_eq!(out.status.code(), Some(0), "stderr: {}", String::from_utf8_lossy(&out.stderr));
    assert_eq!(String::from_utf8_lossy(&out.stdout), "-1\n2\n");
}

// --- programmer-error aborts (checked like a range slice) ---------------------------------------

/// A negative start aborts (SIGABRT), never a silent clamp or `None`.
#[test]
fn find_at_negative_start_aborts() {
    let prog = "\
import std.regex
pub fn main() -> Result<(), Error> {
  re := regex.compile(\"a\")?
  match re.find_at(\"abc\", -1) { Some(m) => print(m.start), None => print(-1) }
  return Ok(())
}
";
    let out = build_and_run("regex-find-neg", prog);
    assert_eq!(out.status.code(), None, "a negative start must abort (SIGABRT), not exit normally");
}

/// A start past the end of the input aborts.
#[test]
fn find_at_past_end_aborts() {
    let prog = "\
import std.regex
pub fn main() -> Result<(), Error> {
  re := regex.compile(\"a\")?
  match re.find_at(\"abc\", 4) { Some(m) => print(m.start), None => print(-1) }
  return Ok(())
}
";
    let out = build_and_run("regex-find-past", prog);
    assert_eq!(out.status.code(), None, "a past-end start must abort");
}

/// A start in the middle of a multi-byte code point aborts (matches the slice-boundary model).
#[test]
fn find_at_mid_codepoint_aborts() {
    // 'π' is bytes 0..2; offset 1 is interior.
    let prog = "\
import std.regex
pub fn main() -> Result<(), Error> {
  re := regex.compile(\"x\")?
  match re.find_at(\"π\", 1) { Some(m) => print(m.start), None => print(-1) }
  return Ok(())
}
";
    let out = build_and_run("regex-find-midchar", prog);
    assert_eq!(out.status.code(), None, "a non-char-boundary start must abort");
}

// --- resource limit -----------------------------------------------------------------------------

/// A pattern whose compiled automaton exceeds the 10 MiB size limit is `Error.Invalid`, not a
/// crash. (The exact 64 KiB source-length boundary is pinned by the runtime unit test, which can
/// build an over-length string directly; here we exercise the compiled-size cap end-to-end.)
#[test]
fn oversized_pattern_is_err() {
    let prog = "\
import std.regex
pub fn main() -> Result<(), Error> {
  // A million-wide counted repetition compiles to far more than 10 MiB of program.
  match regex.compile(\"[a-z]{1000000}\") {
    Ok(_) => print(\"ok\"),
    Err(_) => print(\"err\"),
  }
  return Ok(())
}
";
    let out = build_and_run("regex-oversize", prog);
    assert_eq!(out.status.code(), Some(0), "stderr: {}", String::from_utf8_lossy(&out.stderr));
    // Rejected by the compiled-size limit → Err, never an abort.
    assert_eq!(String::from_utf8_lossy(&out.stdout), "err\n");
}

// --- Move / Drop soundness ----------------------------------------------------------------------

/// A compiled handle is Move: using it after it is moved is a diagnostic.
#[test]
fn handle_is_move_use_after_move_rejected() {
    let prog = "\
import std.regex
fn consume(re: regex) -> bool = re.is_match(\"a\")
pub fn main() -> Result<(), Error> {
  re := regex.compile(\"a\")?
  print(consume(re))
  print(re.is_match(\"a\"))
  return Ok(())
}
";
    assert!(check_errs("regex-uaf", prog), "using a moved regex handle must be rejected");
}

/// Compiling and dropping many handles in a loop exits cleanly — no double-free, no leak. The
/// `Result` unwrap (`?`) drops the Err path's nothing and the Ok handle at scope end.
#[test]
fn drop_in_a_loop_is_clean() {
    let prog = "\
import std.regex
pub fn main() -> Result<(), Error> {
  mut i := 0
  mut hits := 0
  loop {
    if i >= 1000 { break }
    re := regex.compile(\"[a-z]+[0-9]+\")?
    if re.is_match(\"abc123\") { hits = hits + 1 }
    i = i + 1
  }
  print(hits)
  return Ok(())
}
";
    let out = build_and_run("regex-drop-loop", prog);
    // A clean exit (not SIGABRT) confirms no double-free; 1000 hits confirms each handle worked.
    assert_eq!(out.status.code(), Some(0), "stderr: {}", String::from_utf8_lossy(&out.stderr));
    assert_eq!(String::from_utf8_lossy(&out.stdout), "1000\n");
}
