//! M11 std.http Slice 1 — request/response types + HTTP/1.1 serialize/parse (NO sockets; the
//! network client is Slice 2). `http.request(method, url)` builds a Move `http request` (`r.header`/
//! `r.body` mutate it in place; a CR/LF/NUL in a header **aborts** — request-smuggling defence, P6);
//! `http.parse(bytes)` parses a response buffer into a Move `http response` -> `Result<response,
//! Error>` (zero-copy offset table, R1). `resp.status()` reads the code, `resp.header(name)` is a
//! case-insensitive `Option<str>` **view**, `resp.body()` a `slice<u8>` **view** — both region-bound
//! to `resp` (an escape past its `Drop` is a compile error, #297). A 4xx/5xx status is data, not an
//! error (P2). All ops Pure (no sockets in this slice). The serialize codec + exact wire bytes are
//! runtime-unit-tested in `align_runtime` (Slice 2's client calls `align_rt_http_serialize`).
//! (`docs/impl/std-design/http.md`.)

mod common;
use common::*;

// --- request builder ---------------------------------------------------------------------------

/// A request builds, takes headers + a body, and drops cleanly (no leak, no crash). The request
/// builder has no language-observable output in this slice (serialize is an internal codec — Slice
/// 2's client renders + sends it), so the observable behaviour is that it compiles, runs, and the
/// Move handle drops without a double-free.
#[test]
fn request_builds_and_drops() {
    if !backend_available() {
        return;
    }
    let prog = "\
import std.http
pub fn main() -> Result<(), Error> {
  r := http.request(\"POST\", \"http://example.com/submit\")
  r.header(\"Accept\", \"application/json\")
  r.header(\"X-Trace\", \"abc\")
  r.body(\"{\\\"k\\\":1}\")
  print(1)
  return Ok(())
}
";
    let out = build_and_run("m11-http-req-build", prog);
    assert_eq!(out.status.code(), Some(0), "stderr: {}", String::from_utf8_lossy(&out.stderr));
    assert_eq!(String::from_utf8_lossy(&out.stdout), "1\n");
}

/// P6: a CR/LF in a header name or value **aborts** (header injection is a programmer error —
/// request smuggling). A NUL likewise. Each is checked in `align_rt_http_header` at build time.
#[test]
fn header_crlf_or_nul_injection_aborts() {
    if !backend_available() {
        return;
    }
    let inject = "\
import std.http
pub fn main() -> Result<(), Error> {
  r := http.request(\"GET\", \"http://a/\")
  r.header(\"X-Evil\", \"value\\r\\nInjected: 1\")
  print(1)
  return Ok(())
}
";
    let out = build_and_run("m11-http-inject", inject);
    assert!(!out.status.success(), "a CR/LF in a header value must abort (request smuggling)");
    // A bare-LF in the header *name* aborts too.
    let inject_name = "\
import std.http
pub fn main() -> Result<(), Error> {
  r := http.request(\"GET\", \"http://a/\")
  r.header(\"Bad\\nName\", \"v\")
  print(1)
  return Ok(())
}
";
    let out2 = build_and_run("m11-http-inject-name", inject_name);
    assert!(!out2.status.success(), "a LF in a header name must abort");
}

/// The request handle is Move: after a re-binding move, the old binding is dead — using it is a
/// compile error (no double-free of the owned header list / body buffer).
#[test]
fn request_use_after_move_rejected() {
    let src = "\
import std.http
pub fn main() -> Result<(), Error> {
  r := http.request(\"GET\", \"http://a/\")
  s := r
  r.header(\"X\", \"1\")
  return Ok(())
}
";
    assert!(check_errs("m11-http-uam", src), "using a moved-out http request must be rejected");
}

/// v1 bound-receiver gate: a method on an unbound owned-request temporary is rejected (the handle is
/// not dropped yet) — bind it to a local first.
#[test]
fn request_unbound_receiver_rejected() {
    let src = "\
import std.http
pub fn main() -> Result<(), Error> {
  http.request(\"GET\", \"http://a/\").header(\"X\", \"1\")
  return Ok(())
}
";
    assert!(check_errs("m11-http-unbound", src), "a method on an unbound http request temporary must be rejected");
}

/// A `request` is an owned handle bound to one local — it cannot be collected into an array (a
/// copied handle would double-free its buffers), rejected at construction like the cli / net handles.
#[test]
fn request_rejected_as_array_element() {
    let src = "\
import std.http
pub fn main() -> i32 {
  r := http.request(\"GET\", \"http://a/\")
  xs := [r, r]
  return 0
}
";
    assert!(check_errs("m11-http-req-array", src), "an http request cannot be an array element");
}

/// `http.request` requires `import std.http`.
#[test]
fn http_request_requires_import() {
    let src = "\
pub fn main() -> Result<(), Error> {
  r := http.request(\"GET\", \"http://a/\")
  return Ok(())
}
";
    assert!(check_errs("m11-http-req-noimport", src), "http.request without import std.http must be rejected");
}

// --- response parse + getters ------------------------------------------------------------------

/// The headline round-trip: parse a known response, read its status, a header (case-insensitively),
/// and its body — the body is a zero-copy view written straight to stdout.
#[test]
fn parse_status_header_body_round_trip() {
    if !backend_available() {
        return;
    }
    let prog = "\
import std.http
import std.io
pub fn main() -> Result<(), Error> {
  resp := http.parse(\"HTTP/1.1 200 OK\\r\\nContent-Type: text/plain\\r\\nContent-Length: 5\\r\\n\\r\\nhello\")?
  print(resp.status())
  match resp.header(\"content-TYPE\") {
    Some(v) => io.stdout.write(v)?,
    None => io.stdout.write(\"none\")?,
  }
  io.stdout.write(\"\\n\")?
  io.stdout.write(resp.body())?
  io.stdout.write(\"\\n\")?
  return Ok(())
}
";
    let out = build_and_run("m11-http-parse-roundtrip", prog);
    assert_eq!(out.status.code(), Some(0), "stderr: {}", String::from_utf8_lossy(&out.stderr));
    assert_eq!(String::from_utf8_lossy(&out.stdout), "200\ntext/plain\nhello\n");
}

/// P2 (status-is-data): a 404 response is `Ok(response with status 404)`, NOT `Err`. A missing
/// header is `None`.
#[test]
fn parse_404_is_ok_not_err() {
    if !backend_available() {
        return;
    }
    let prog = "\
import std.http
pub fn main() -> Result<(), Error> {
  resp := http.parse(\"HTTP/1.1 404 Not Found\\r\\nContent-Length: 0\\r\\n\\r\\n\")?
  print(resp.status())
  match resp.header(\"X-Absent\") {
    Some(v) => print(1),
    None => print(0),
  }
  return Ok(())
}
";
    let out = build_and_run("m11-http-404", prog);
    assert_eq!(out.status.code(), Some(0), "stderr: {}", String::from_utf8_lossy(&out.stderr));
    assert_eq!(String::from_utf8_lossy(&out.stdout), "404\n0\n");
}

/// A malformed response (bad status line / non-numeric status / header without `:` / chunked /
/// oversized framing) is `Error.Invalid` — a recoverable `Err`, never an abort.
#[test]
fn parse_malformed_is_err_not_abort() {
    if !backend_available() {
        return;
    }
    let prog = "\
import std.http
pub fn main() -> Result<(), Error> {
  match http.parse(\"not a valid response\\r\\n\\r\\n\") {
    Ok(resp) => print(1),
    Err(e) => print(0),
  }
  return Ok(())
}
";
    let out = build_and_run("m11-http-malformed", prog);
    assert_eq!(out.status.code(), Some(0), "stderr: {}", String::from_utf8_lossy(&out.stderr));
    assert_eq!(String::from_utf8_lossy(&out.stdout), "0\n", "a malformed response is a recoverable Err, not an abort");
}

/// P3 (#297): `resp.body()` is a `slice<u8>` view into the response buffer — returning it past the
/// response's `Drop` is a compile error (a use-after-free of freed memory). The response is
/// constructed inline (its view cannot ride a `Result` payload, so the escape is exercised through a
/// bare-`slice<u8>`-returning function that match-unwraps the parse).
#[test]
fn resp_body_view_cannot_escape_response() {
    let src = "\
import std.http
fn steal(bytes: slice<u8>) -> slice<u8> {
  match http.parse(bytes) {
    Ok(resp) => resp.body(),
    Err(e) => bytes,
  }
}
pub fn main() -> i32 {
  return 0
}
";
    let d = check_diagnostics("m11-http-body-escape", src);
    assert!(d.contains("cannot return a slice that views a local"), "resp.body() must not escape the response:\n{d}");
}

/// P3 (#297): `resp.header(name)` is an `Option<str>` whose `str` views the response buffer —
/// returning it past the response's `Drop` is a compile error.
#[test]
fn resp_header_view_cannot_escape_response() {
    let src = "\
import std.http
fn steal(bytes: slice<u8>) -> Option<str> {
  match http.parse(bytes) {
    Ok(resp) => resp.header(\"X-A\"),
    Err(e) => None,
  }
}
pub fn main() -> i32 {
  return 0
}
";
    let d = check_diagnostics("m11-http-header-escape", src);
    assert!(d.contains("cannot return a view that borrows local storage"), "resp.header() view must not escape the response:\n{d}");
}

/// The response handle is Move: using a moved-out response is a compile error.
#[test]
fn response_use_after_move_rejected() {
    let src = "\
import std.http
pub fn main() -> Result<(), Error> {
  resp := http.parse(\"HTTP/1.1 200 OK\\r\\nContent-Length: 0\\r\\n\\r\\n\")?
  other := resp
  print(resp.status())
  return Ok(())
}
";
    assert!(check_errs("m11-http-resp-uam", src), "using a moved-out http response must be rejected");
}

/// A `response` cannot be collected into an array (a copied handle would double-free), rejected like
/// the request / cli / net handles.
#[test]
fn response_rejected_as_array_element() {
    let src = "\
import std.http
pub fn main() -> Result<(), Error> {
  resp := http.parse(\"HTTP/1.1 200 OK\\r\\nContent-Length: 0\\r\\n\\r\\n\")?
  xs := [resp, resp]
  return Ok(())
}
";
    assert!(check_errs("m11-http-resp-array", src), "an http response cannot be an array element");
}

/// `http.parse` requires `import std.http`.
#[test]
fn http_parse_requires_import() {
    let src = "\
pub fn main() -> Result<(), Error> {
  match http.parse(\"HTTP/1.1 200 OK\\r\\n\\r\\n\") {
    Ok(r) => print(1),
    Err(e) => print(0),
  }
  return Ok(())
}
";
    assert!(check_errs("m11-http-parse-noimport", src), "http.parse without import std.http must be rejected");
}
