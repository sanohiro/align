//! `pkg.web.multipart` — RFC 7578 multipart/form-data bodies (uploads).
//!
//! A **public** submodule (not `internal`): `Part`/`Step` are types an application names when it
//! walks an upload. It is deliberately not part of `pkg.web`'s core surface — an application feeds
//! it `pkg.web.header(c, "content-type")` and `pkg.web.body(c)`, so the module stays dependency-free
//! and every value it returns is a zero-copy view into those two inputs.

mod common;
use common::*;

const MULTIPART: &str = include_str!("../../../apps/web/pkg/web/multipart.align");

/// The walk every test drives, printing one `name|filename|content_type|data` line per part and
/// then the terminal outcome (`DONE` for the close-delimiter, `INVALID` for a body that does not
/// parse). This is the iteration protocol the module header documents, verbatim.
const WALK: &str = r#"
fn walk(text: str, bd: str) -> () {
  body := text.bytes()
  mut off := 0
  loop {
    match pkg.web.multipart.next(body, bd, off) {
      Found(p) => {
        data := match p.data.as_str() {
          Ok(d) => d
          Err(_) => "<binary>"
        }
        print(template "{p.name}|{p.filename}|{p.content_type}|{data}")
        off = p.next
      }
      Done => {
        print("DONE")
        break
      }
      Invalid => {
        print("INVALID")
        break
      }
    }
  }
}
"#;

fn run_multipart(name: &str, entry_main: &str) -> std::process::Output {
    build_and_run_multi(
        name,
        &[("pkg/web/multipart.align", MULTIPART), ("main.align", entry_main)],
        "main.align",
    )
}

/// `module main` + the import + the shared walk helper + a `main` body.
fn program(body: &str) -> String {
    format!("module main\nimport pkg.web.multipart\n{WALK}\nfn main() -> Result<(), Error> {{\n{body}\n  return Ok(())\n}}\n")
}

#[test]
fn walks_a_text_field_and_a_file_part() {
    if !backend_available() {
        return;
    }
    // The everyday shape an HTML form posts: one text field, then a file part carrying `filename`
    // and its own `Content-Type`. Absent parameters read as "" — the part is not a file, or
    // declares no type — never as an error.
    let main = program(
        r#"  ct := "multipart/form-data; boundary=----WebKitFormBoundary7MA4YWxk"
  bd := pkg.web.multipart.boundary(ct)
  print(bd)
  walk("------WebKitFormBoundary7MA4YWxk\r\nContent-Disposition: form-data; name=\"title\"\r\n\r\nholiday\r\n------WebKitFormBoundary7MA4YWxk\r\nContent-Disposition: form-data; name=\"photo\"; filename=\"beach.txt\"\r\nContent-Type: text/plain\r\n\r\nsand\r\n------WebKitFormBoundary7MA4YWxk--\r\n", bd)"#,
    );
    let out = run_multipart("multipart-basic", &main);
    assert_eq!(out.status.code(), Some(0));
    assert_eq!(
        String::from_utf8_lossy(&out.stdout),
        "----WebKitFormBoundary7MA4YWxk\n\
title|||holiday\n\
photo|beach.txt|text/plain|sand\n\
DONE\n"
    );
}

#[test]
fn zero_length_part_and_immediate_close_are_legal() {
    if !backend_available() {
        return;
    }
    // A part with an empty body is well-formed (an empty text input posts exactly this), and a form
    // with no parts at all is just the close-delimiter — `Done`, not `Invalid`.
    let main = program(
        r#"  walk("--SEP\r\nContent-Disposition: form-data; name=\"empty\"\r\n\r\n\r\n--SEP\r\nContent-Disposition: form-data; name=\"after\"\r\n\r\nx\r\n--SEP--\r\n", "SEP")
  walk("--SEP--\r\n", "SEP")"#,
    );
    let out = run_multipart("multipart-empty", &main);
    assert_eq!(out.status.code(), Some(0));
    assert_eq!(String::from_utf8_lossy(&out.stdout), "empty|||\nafter|||x\nDONE\nDONE\n");
}

#[test]
fn preamble_epilogue_and_transport_padding_are_ignored() {
    if !backend_available() {
        return;
    }
    // RFC 2046 §5.1.1: bytes before the first delimiter (the preamble) and after the close one (the
    // epilogue) exist to be ignored, and SP/HTAB may pad a delimiter line before its CRLF.
    let main = program(
        r#"  walk("MIME preamble for non-MIME readers\r\n--SEP \t\r\nContent-Disposition: form-data; name=\"a\"\r\n\r\nv\r\n--SEP-- \t\r\ntrailing epilogue\r\n", "SEP")
  walk("--SEP\r\nContent-Disposition: form-data; name=\"a\"\r\n\r\nv\r\n--SEP--", "SEP")"#,
    );
    let out = run_multipart("multipart-preamble", &main);
    assert_eq!(out.status.code(), Some(0));
    // The second body ends at the close-delimiter with no trailing CRLF at all — also legal.
    assert_eq!(String::from_utf8_lossy(&out.stdout), "a|||v\nDONE\na|||v\nDONE\n");
}

#[test]
fn header_and_parameter_names_are_case_insensitive() {
    if !backend_available() {
        return;
    }
    // Field names and parameter attributes fold ASCII case (RFC 9110 §5.1, §5.6.6), values do not.
    // Both the quoted and the bare-token parameter form are accepted, and an unknown header is
    // skipped rather than rejected.
    let main = program(
        r#"  walk("--SEP\r\nX-Trace: 42\r\nCONTENT-DISPOSITION: Form-Data; NAME=\"a\"; FileName=b.bin\r\ncontent-type: Application/Octet-Stream\r\n\r\nz\r\n--SEP--", "SEP")
  print(pkg.web.multipart.boundary("Multipart/Form-Data; BOUNDARY=abc"))"#,
    );
    let out = run_multipart("multipart-case", &main);
    assert_eq!(out.status.code(), Some(0));
    assert_eq!(String::from_utf8_lossy(&out.stdout), "a|b.bin|Application/Octet-Stream|z\nDONE\nabc\n");
}

#[test]
fn malformed_parts_are_invalid() {
    if !backend_available() {
        return;
    }
    // Every one of these is a body an application must answer 400 for. `name` is REQUIRED
    // (RFC 7578 §4.2), the disposition must be `form-data`, obs-fold continuations are gone from
    // HTTP (RFC 9110 §5.2) and are not guessed at, and a field line needs a ':'.
    let main = program(
        r#"  walk("--SEP\r\nContent-Disposition: form-data; filename=\"x\"\r\n\r\nz\r\n--SEP--", "SEP")
  walk("--SEP\r\nContent-Type: text/plain\r\n\r\nz\r\n--SEP--", "SEP")
  walk("--SEP\r\nContent-Disposition: attachment; name=\"a\"\r\n\r\nz\r\n--SEP--", "SEP")
  walk("--SEP\r\nContent-Disposition: form-data;\r\n name=\"a\"\r\n\r\nz\r\n--SEP--", "SEP")
  walk("--SEP\r\nContent-Disposition: form-data; name=\"a\"\r\nbroken\r\n\r\nz\r\n--SEP--", "SEP")
  walk("nothing here resembles a multipart body", "SEP")"#,
    );
    let out = run_multipart("multipart-malformed", &main);
    assert_eq!(out.status.code(), Some(0));
    assert_eq!(String::from_utf8_lossy(&out.stdout), "INVALID\nINVALID\nINVALID\nINVALID\nINVALID\nINVALID\n");
}

#[test]
fn an_obs_fold_continuation_is_refused_not_interpreted() {
    if !backend_available() {
        return;
    }
    // The discriminating body for the obs-fold guard alone: every header here is otherwise
    // well-formed, and the folded line belongs to a header nothing reads. Without the guard the
    // continuation parses as its own field line and the part comes back as `a|||1`; RFC 9110 §5.2
    // removed obs-fold, and reading a folded header as a fresh one is how two parsers end up
    // disagreeing about a request.
    let main = program(
        r#"  walk("--SEP\r\nContent-Disposition: form-data; name=\"a\"\r\n X-Fold: 1\r\n\r\nz\r\n--SEP--", "SEP")"#,
    );
    let out = run_multipart("multipart-obs-fold", &main);
    assert_eq!(out.status.code(), Some(0));
    assert_eq!(String::from_utf8_lossy(&out.stdout), "INVALID\n");
}

#[test]
fn a_duplicate_is_refused_never_resolved() {
    if !backend_available() {
        return;
    }
    // One rule for ambiguity: a repeat is `Invalid`. Picking the first (Go's choice) or the last
    // are both guesses, they disagree with each other, and whatever parser stands in front of us
    // may have made the other one — the shape of a smuggling differential. Covered here: a second
    // Content-Disposition, a second Content-Type, a repeated `name`, a repeated `filename`, and a
    // repeated `boundary` parameter (which makes the request unwalkable, so `boundary` answers "").
    let main = program(
        r#"  walk("--SEP\r\nContent-Disposition: form-data; name=\"a\"\r\nContent-Disposition: form-data; name=\"b\"\r\n\r\nz\r\n--SEP--", "SEP")
  walk("--SEP\r\nContent-Disposition: form-data; name=\"a\"\r\nContent-Type: text/plain\r\nContent-Type: text/html\r\n\r\nz\r\n--SEP--", "SEP")
  walk("--SEP\r\nContent-Disposition: form-data; name=\"a\"; name=\"b\"\r\n\r\nz\r\n--SEP--", "SEP")
  walk("--SEP\r\nContent-Disposition: form-data; name=\"a\"; filename=\"f\"; filename=\"g\"\r\n\r\nz\r\n--SEP--", "SEP")
  print(pkg.web.multipart.boundary("multipart/form-data; boundary=a; boundary=a").len())
  walk("--SEP\r\nContent-Disposition: form-data; name=\"a\"\r\nContent-Type: text/plain\r\n\r\nz\r\n--SEP--", "SEP")"#,
    );
    let out = run_multipart("multipart-duplicates", &main);
    assert_eq!(out.status.code(), Some(0));
    // The last row is the same shape without a repeat — the rule rejects duplicates, not headers.
    assert_eq!(
        String::from_utf8_lossy(&out.stdout),
        "INVALID\nINVALID\nINVALID\nINVALID\n0\na||text/plain|z\nDONE\n"
    );
}

#[test]
fn a_bare_parameter_value_must_be_a_token() {
    if !backend_available() {
        return;
    }
    // RFC 9110 §5.6.6: an unquoted parameter value is a `token`. Taking "everything up to the next
    // ';'" instead would accept `boundary=A,B` and `boundary=abc def` — which Go's `mime` rejects
    // outright, so a proxy in front of us would see a different boundary (or no valid header) than
    // we do, and the two of us would disagree about where the parts are. A QUOTED value may hold
    // those bytes (`"A,B"`, `"a b c"`) — that is what quoting is for. A boundary may not END in
    // SP/HTAB either (RFC 2046 `bcharsnospace`): transport padding follows a delimiter, so a
    // trailing space makes "boundary or padding?" ambiguous.
    let main = program(
        r#"  print(pkg.web.multipart.boundary("multipart/form-data; boundary=ok-9_x"))
  print(pkg.web.multipart.boundary("multipart/form-data; boundary=\"A,B\""))
  print(pkg.web.multipart.boundary("multipart/form-data; boundary=\"a b c\""))
  print(pkg.web.multipart.boundary("multipart/form-data; boundary=A,B").len())
  print(pkg.web.multipart.boundary("multipart/form-data; boundary=abc def").len())
  print(pkg.web.multipart.boundary("multipart/form-data; boundary=a\"b").len())
  print(pkg.web.multipart.boundary("multipart/form-data; boundary=\"abc \"").len())
  print(pkg.web.multipart.boundary("multipart/form-data; boundary=\"abc\t\"").len())
  walk("--SEP\r\nContent-Disposition: form-data; name=a b\r\n\r\nz\r\n--SEP--", "SEP")
  walk("--SEP\r\nContent-Disposition: form-data; name=a-b_9\r\n\r\nz\r\n--SEP--", "SEP")"#,
    );
    let out = run_multipart("multipart-token", &main);
    assert_eq!(out.status.code(), Some(0));
    assert_eq!(
        String::from_utf8_lossy(&out.stdout),
        "ok-9_x\nA,B\na b c\n0\n0\n0\n0\n0\nINVALID\na-b_9|||z\nDONE\n"
    );
}

#[test]
fn a_truncated_body_is_invalid_not_done() {
    if !backend_available() {
        return;
    }
    // The distinction `Step` exists for: a body cut off mid-upload has no closing delimiter, and
    // must not read as a well-formed end of iteration — that is how a short file gets stored whole.
    // The first part of the second body is still handed back before the truncation is reported.
    let main = program(
        r#"  walk("--SEP\r\nContent-Disposition: form-data; name=\"a\"\r\n\r\nhalf a fi", "SEP")
  walk("--SEP\r\nContent-Disposition: form-data; name=\"a\"\r\n\r\nv\r\n--SEP\r\nContent-Disposition: form-data; name=\"b\"\r\n", "SEP")
  walk("--SEP\r\nContent-Disposition: form-data; name=\"a\"\r\n\r\n", "SEP")"#,
    );
    let out = run_multipart("multipart-truncated", &main);
    assert_eq!(out.status.code(), Some(0));
    assert_eq!(String::from_utf8_lossy(&out.stdout), "INVALID\na|||v\nINVALID\nINVALID\n");
}

#[test]
fn the_boundary_may_appear_inside_a_part() {
    if !backend_available() {
        return;

    }
    // A delimiter is a whole LINE — CRLF, `--`, the boundary, padding, CRLF (RFC 2046 §5.1.1). So
    // the boundary text mid-line is data, and so is a longer line that merely STARTS with it
    // (`--SEPARATE` when the boundary is `SEP`): splitting there would corrupt legal content.
    let main = program(
        r#"  walk("--SEP\r\nContent-Disposition: form-data; name=\"a\"\r\n\r\nsee --SEP inline\r\n--SEPARATE\r\n--SEP--junk\r\nend\r\n--SEP--\r\n", "SEP")"#,
    );
    let out = run_multipart("multipart-inline-boundary", &main);
    assert_eq!(out.status.code(), Some(0));
    assert_eq!(
        String::from_utf8_lossy(&out.stdout),
        "a|||see --SEP inline\r\n--SEPARATE\r\n--SEP--junk\r\nend\nDONE\n"
    );
}

#[test]
fn part_data_is_a_verbatim_byte_view() {
    if !backend_available() {
        return;
    }
    // `data` is a `slice<u8>` view into the caller's body — no copy, no decoding, no UTF-8
    // requirement. The blob here carries NUL, 0xFF and a bare CRLF, and comes back byte-identical.
    let main = r#"module main
import std.encoding
import pkg.web.multipart

// The body is assembled from bytes rather than written as a literal: it carries NUL, 0xFF and a
// bare CRLF, none of which a `str` literal can hold.
fn joined(head: str, blob: slice<u8>, tail: str) -> array<u8> {
  mut out: array_builder<u8> := array_builder()
  hb := head.bytes()
  mut i := 0
  loop {
    if i >= hb.len() { break }
    out.push(hb[i])
    i = i + 1
  }
  mut j := 0
  loop {
    if j >= blob.len() { break }
    out.push(blob[j])
    j = j + 1
  }
  tb := tail.bytes()
  mut k := 0
  loop {
    if k >= tb.len() { break }
    out.push(tb[k])
    k = k + 1
  }
  out.build()
}

fn main() -> Result<(), Error> {
  raw := encoding.hex_decode("00ff0d0a2d2d80")?
  blob := raw.bytes()
  owned := joined("--SEP\r\nContent-Disposition: form-data; name=\"bin\"; filename=\"a.bin\"\r\nContent-Type: application/octet-stream\r\n\r\n", blob, "\r\n--SEP--\r\n")
  body := owned[0..owned.len()]
  match pkg.web.multipart.next(body, "SEP", 0) {
    Found(p) => {
      print(p.name)
      print(p.filename)
      print(p.content_type)
      print(p.data.len())
      print(p.data[0] as i64)
      print(p.data[1] as i64)
      print(p.data[6] as i64)
      match pkg.web.multipart.next(body, "SEP", p.next) {
        Found(_) => print("MORE")
        Done => print("DONE")
        Invalid => print("INVALID")
      }
    }
    Done => print("DONE")
    Invalid => print("INVALID")
  }
  return Ok(())
}
"#;
    let out = run_multipart("multipart-binary", main);
    assert_eq!(out.status.code(), Some(0));
    assert_eq!(
        String::from_utf8_lossy(&out.stdout),
        "bin\na.bin\napplication/octet-stream\n7\n0\n255\n128\nDONE\n"
    );
}

#[test]
fn boundary_accepts_and_rejects() {
    if !backend_available() {
        return;
    }
    // "" is the single "this body is not walkable" answer, so an application checks once. Rejected:
    // a non-multipart media type, a `multipart/*` that is not `form-data`, no `boundary` parameter,
    // an empty or over-70-byte value (RFC 2046 §5.1.1), a control byte, and an unterminated quote.
    let main = program(
        r#"  print(pkg.web.multipart.boundary("multipart/form-data; boundary=abc"))
  print(pkg.web.multipart.boundary("multipart/form-data; boundary=\"a b c\""))
  print(pkg.web.multipart.boundary("multipart/form-data; charset=utf-8; boundary=xyz"))
  print(pkg.web.multipart.boundary("multipart/form-data ; boundary = spaced "))
  print(pkg.web.multipart.boundary("multipart/form-data; boundary=1234567890123456789012345678901234567890123456789012345678901234567890"))
  print(pkg.web.multipart.boundary("application/json").len())
  print(pkg.web.multipart.boundary("multipart/mixed; boundary=abc").len())
  print(pkg.web.multipart.boundary("multipart/form-data").len())
  print(pkg.web.multipart.boundary("multipart/form-data; boundary=").len())
  print(pkg.web.multipart.boundary("multipart/form-data; boundary").len())
  print(pkg.web.multipart.boundary("multipart/form-data; boundary=\"unterminated").len())
  print(pkg.web.multipart.boundary("multipart/form-data; boundary=\"a\tb\"").len())
  print(pkg.web.multipart.boundary("multipart/form-data; boundary=12345678901234567890123456789012345678901234567890123456789012345678901").len())"#,
    );
    let out = run_multipart("multipart-boundary", &main);
    assert_eq!(out.status.code(), Some(0));
    assert_eq!(
        String::from_utf8_lossy(&out.stdout),
        "abc\na b c\nxyz\nspaced\n1234567890123456789012345678901234567890123456789012345678901234567890\n\
0\n0\n0\n0\n0\n0\n0\n0\n"
    );
}

#[test]
fn quoted_pair_escapes_are_refused_never_guessed() {
    if !backend_available() {
        return;
    }
    // A quoted value carrying a `\` escape is NOT the bytes on the wire, so it cannot be handed back
    // zero-copy — and returning the still-escaped form would silently give the caller a wrong
    // filename. Both surfaces refuse instead: `boundary` answers "", `next` answers `Invalid`.
    // An escape inside some OTHER parameter is scanned over correctly and skipped.
    let main = program(
        r#"  print(pkg.web.multipart.boundary("multipart/form-data; boundary=\"a\\\"b\"").len())
  walk("--SEP\r\nContent-Disposition: form-data; name=\"a\"; filename=\"quo\\\"te.txt\"\r\n\r\nz\r\n--SEP--", "SEP")
  walk("--SEP\r\nContent-Disposition: form-data; other=\"esc\\\"aped\"; name=\"a\"\r\n\r\nz\r\n--SEP--", "SEP")
  walk("--SEP\r\nContent-Disposition: form-data; name\r\n\r\nz\r\n--SEP--", "SEP")"#,
    );
    let out = run_multipart("multipart-escapes", &main);
    assert_eq!(out.status.code(), Some(0));
    assert_eq!(String::from_utf8_lossy(&out.stdout), "0\nINVALID\na|||z\nDONE\nINVALID\n");
}

#[test]
fn a_semicolon_inside_a_quoted_filename_does_not_split_the_parameter() {
    if !backend_available() {
        return;
    }
    // The parameter scanner is quote-aware, so a ';' inside a quoted-string is part of the value
    // (RFC 9110 §5.6.4) rather than a separator — otherwise the `name` after it would go missing.
    let main = program(
        r#"  walk("--SEP\r\nContent-Disposition: form-data; filename=\"a;b=c.txt\"; name=\"f\"\r\n\r\nz\r\n--SEP--", "SEP")"#,
    );
    let out = run_multipart("multipart-quoted-semi", &main);
    assert_eq!(out.status.code(), Some(0));
    assert_eq!(String::from_utf8_lossy(&out.stdout), "f|a;b=c.txt||z\nDONE\n");
}

#[test]
fn an_unusable_boundary_argument_never_walks() {
    if !backend_available() {
        return;
    }
    // `boundary()` returns "" for a request that is not a walkable multipart form; an empty
    // boundary would make `--` alone a delimiter, so `next` refuses it rather than splitting the
    // body at nonsense positions. An out-of-range `from` is refused for a harder reason: integer
    // overflow WRAPS in Align, so `i64::MAX` reached a `i + n > len` bounds test inside the scan,
    // wrapped negative, passed it, and aborted the process on the index — one bad number, no
    // server. What this row pins is `next`'s own `from` range check, the guard that keeps such a
    // value out of the scan at all; the wrap-safe form of that inner test is unreachable behind it
    // and deliberately not claimed to be pinned here. Every out-of-range `from` reads as `Invalid`,
    // and the walk that survives them prints last.
    let main = program(
        r#"  walk("--SEP\r\nContent-Disposition: form-data; name=\"a\"\r\n\r\nz\r\n--SEP--", "")
  body := "--SEP\r\nContent-Disposition: form-data; name=\"a\"\r\n\r\nz\r\n--SEP--".bytes()
  offsets := [-1, 9999, 9223372036854775807, -9223372036854775808, body.len()]
  mut i := 0
  loop {
    if i >= offsets.len() { break }
    match pkg.web.multipart.next(body, "SEP", offsets[i]) {
      Found(_) => print("FOUND")
      Done => print("DONE")
      Invalid => print("INVALID")
    }
    i = i + 1
  }
  walk("--SEP\r\nContent-Disposition: form-data; name=\"a\"\r\n\r\nz\r\n--SEP--", "SEP")"#,
    );
    let out = run_multipart("multipart-guards", &main);
    assert_eq!(out.status.code(), Some(0));
    assert_eq!(
        String::from_utf8_lossy(&out.stdout),
        "INVALID\nINVALID\nINVALID\nINVALID\nINVALID\nINVALID\na|||z\nDONE\n"
    );
}

const WEB_ROOT: &str = include_str!("../../../apps/web/pkg/web.align");
const TYPES: &str = include_str!("../../../apps/web/pkg/web/types.align");
const ROUTER: &str = include_str!("../../../apps/web/pkg/web/internal/router.align");
const QUERY: &str = include_str!("../../../apps/web/pkg/web/internal/query.align");
const DESIGN_DOC: &str = include_str!("../../../docs/impl/pkg-design/web.md");

/// The first ```` ```align ```` block after `marker` in the design doc — the documented example
/// itself, not a copy of it that can drift away from what readers are told to write.
fn doc_example(marker: &str) -> String {
    let after = DESIGN_DOC.split_once(marker).expect("design-doc section").1;
    let body = after.split_once("```align\n").expect("align fence").1;
    body.split_once("```").expect("fence close").0.to_string()
}

#[test]
fn the_documented_handler_example_compiles_against_the_real_pkg_web() {
    if !backend_available() {
        return;
    }
    // The doc's `upload` handler is EXTRACTED from `pkg-design/web.md` and built against the real
    // `apps/web/pkg/**` sources. A documented example that does not compile is worse than none —
    // and the first draft of this one did not: it called `pkg.web.status(c, 400)` (the responder
    // takes no `Ctx`) and used `pkg.web.header(...)` as a `str` when it returns `Option<str>`.
    let main = format!(
        "module main\nimport pkg.web\nimport pkg.web.types\nimport pkg.web.multipart\n\n{}\nfn main() -> Result<(), Error> {{\n  routes := [pkg.web.post(\"/upload\", upload)]\n  print(routes.len())\n  return Ok(())\n}}\n",
        doc_example("## multipart/form-data")
    );
    let out = build_and_run_multi(
        "multipart-doc-example",
        &[
            ("pkg/web.align", WEB_ROOT),
            ("pkg/web/types.align", TYPES),
            ("pkg/web/internal/router.align", ROUTER),
            ("pkg/web/internal/query.align", QUERY),
            ("pkg/web/multipart.align", MULTIPART),
            ("main.align", &main),
        ],
        "main.align",
    );
    assert_eq!(out.status.code(), Some(0));
    assert_eq!(String::from_utf8_lossy(&out.stdout), "1\n");
}
