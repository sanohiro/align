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
//!
//! M11 std.http Slice 2 — the plaintext HTTP/1.1 client. `http.client()` opens a Move `http client`;
//! `cl.get(url)` / `cl.post(url, body)` / `cl.request(req)` each perform ONE request over one fresh
//! `tcp_conn` (connect → TCP_NODELAY → one write of the serialized request → stream the response to
//! Content-Length → parse → close), reusing the net rail + the Slice-1 codec/parse engine. A 4xx/5xx
//! is `Ok(response)` (P2). Requests are Impure. Round-trips run against an in-process Rust server; the
//! Gate-1 rejections (client unbound receiver / array element / use-after-move of a `request` / view
//! escape / import) are compile checks. (`docs/impl/std-design/http.md` Slice 2.)
//!
//! M11 std.http Slice 5 — HTTPS/TLS on the client (OpenSSL libssl). `https://` now routes to a
//! verified TLS connection through the SAME `cl.get/post/request` + `cl.get_many` surface (no new
//! user surface); certs are verified against the system trust store with mandatory hostname binding,
//! so a verify failure → `Error.Denied`. The positive round-trip is unit-tested in `align_runtime`
//! (a local TLS server + the `#[cfg(test)]` trust hook); the driver test here proves the ROUTING
//! change (`https://` connects instead of being rejected pre-connect). (`docs/impl/std-design/http.md`
//! Slice 5.)

mod common;
use common::*;

/// A one-shot in-process HTTP server on an ephemeral loopback port: accept ONE connection, read the
/// whole request (head + any `Content-Length` body), write `response`, close. Returns
/// `(port, handle)`; the handle yields the exact request bytes the client sent (for wire assertions).
fn spawn_http_server(response: Vec<u8>) -> (u16, std::thread::JoinHandle<Vec<u8>>) {
    use std::io::{Read, Write};
    let listener = std::net::TcpListener::bind("127.0.0.1:0").expect("bind loopback");
    let port = listener.local_addr().unwrap().port();
    let handle = std::thread::spawn(move || {
        let mut req: Vec<u8> = Vec::new();
        if let Ok((mut sock, _)) = listener.accept() {
            let mut tmp = [0u8; 512];
            let mut want: Option<usize> = None; // total request length once the head is parsed
            loop {
                if let Some(t) = want {
                    if req.len() >= t {
                        break;
                    }
                } else if let Some(p) = req.windows(4).position(|w| w == b"\r\n\r\n") {
                    let head = String::from_utf8_lossy(&req[..p]).to_ascii_lowercase();
                    let cl = head
                        .lines()
                        .find_map(|l| l.strip_prefix("content-length:").map(|v| v.trim().parse::<usize>().unwrap_or(0)))
                        .unwrap_or(0);
                    want = Some(p + 4 + cl);
                    if req.len() >= p + 4 + cl {
                        break;
                    }
                }
                match sock.read(&mut tmp) {
                    Ok(0) | Err(_) => break,
                    Ok(n) => req.extend_from_slice(&tmp[..n]),
                }
            }
            let _ = sock.write_all(&response);
        }
        req
    });
    (port, handle)
}

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

/// P3 (#297), the general pattern-binding region gap (the confirmed UAF): unwrapping the
/// `Option<str>` view through a **`match` arm binding** (`Some(v) => v`) must ALSO carry the region —
/// the bound `v` views `resp`'s buffer, so returning it past `resp`'s `Drop` is a use-after-free.
/// (This is the codebase's first `Option<borrowed-view>`; the fix threads the scrutinee's non-Static
/// region into every arm-payload binding, closing the gap for every future `Option<view>` /
/// `Result<view>`, not just http — `env.get`'s `Option<string>` is owned, so it never exposed it.)
#[test]
fn resp_header_view_cannot_escape_via_match_binding() {
    let src = "\
import std.http
fn steal(bytes: slice<u8>) -> Result<str, Error> {
  resp := http.parse(bytes)?
  h := match resp.header(\"X-A\") { Some(v) => v, None => \"x\" }
  return Ok(h)
}
pub fn main() -> i32 {
  return 0
}
";
    let d = check_diagnostics("m11-http-header-match-escape", src);
    assert!(
        d.contains("cannot return a view that borrows local storage"),
        "a header view unwrapped through a match arm must not escape the response:\n{d}"
    );
}

/// The `slice<u8>` body view likewise cannot escape when bound out of a `match` arm and then
/// returned (the same pattern-binding region path).
#[test]
fn resp_body_view_cannot_escape_via_match_binding() {
    let src = "\
import std.http
fn steal(bytes: slice<u8>) -> slice<u8> {
  match http.parse(bytes) {
    Ok(resp) => {
      b := resp.body()
      b
    }
    Err(e) => bytes,
  }
}
pub fn main() -> i32 {
  return 0
}
";
    assert!(check_errs("m11-http-body-match-escape", src), "a match-bound body view returned out of the frame must be rejected");
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

// --- Slice 2: the plaintext HTTP/1.1 client ----------------------------------------------------

/// `cl.get()` round-trips against a local plaintext server: a 200 with a body parses to a response
/// whose `status()`/`body()` are correct, and the request went out as a well-formed GET. The URL
/// (with the OS-assigned ephemeral port) is passed as a `--url` flag.
#[test]
fn client_get_round_trip_200() {
    if !backend_available() {
        return;
    }
    let resp = b"HTTP/1.1 200 OK\r\nContent-Length: 5\r\nContent-Type: text/plain\r\n\r\nhello".to_vec();
    let (port, server) = spawn_http_server(resp);
    let prog = "\
import std.http
import std.io
import std.cli
pub fn main(args: array<str>) -> Result<(), Error> {
  c := cli.command(\"get\")
  c.flag_str(\"url\", \"\")
  p := c.parse(args)?
  cl := http.client()
  resp := cl.get(p.get_str(\"url\"))?
  print(resp.status())
  io.stdout.write(resp.body())?
  return Ok(())
}
";
    let url = format!("http://127.0.0.1:{port}/path");
    let out = build_and_run_args("m11-http-get-200", prog, &["--url", &url]);
    let req = String::from_utf8_lossy(&server.join().unwrap()).into_owned();
    assert_eq!(out.status.code(), Some(0), "stderr: {}", String::from_utf8_lossy(&out.stderr));
    assert_eq!(String::from_utf8_lossy(&out.stdout), "200\nhello", "status + zero-copy body view");
    assert!(req.starts_with("GET /path HTTP/1.1\r\n"), "request line: {req:?}");
    assert!(req.contains(&format!("Host: 127.0.0.1:{port}\r\n")), "auto Host header: {req:?}");
}

/// P2: a 404 is a valid `Ok(response)` with status 404 — NOT an `Err`. The program branches on the
/// status and exits 0.
#[test]
fn client_get_404_is_ok_not_err() {
    if !backend_available() {
        return;
    }
    let (port, server) = spawn_http_server(b"HTTP/1.1 404 Not Found\r\nContent-Length: 0\r\n\r\n".to_vec());
    let prog = "\
import std.http
import std.cli
pub fn main(args: array<str>) -> Result<(), Error> {
  c := cli.command(\"get\")
  c.flag_str(\"url\", \"\")
  p := c.parse(args)?
  cl := http.client()
  resp := cl.get(p.get_str(\"url\"))?
  print(resp.status())
  return Ok(())
}
";
    let url = format!("http://127.0.0.1:{port}/missing");
    let out = build_and_run_args("m11-http-get-404", prog, &["--url", &url]);
    let _ = server.join();
    assert_eq!(out.status.code(), Some(0), "a 404 is Ok, not an error (P2); stderr: {}", String::from_utf8_lossy(&out.stderr));
    assert_eq!(String::from_utf8_lossy(&out.stdout), "404\n");
}

/// `cl.post()` sends the body with an auto `Content-Length`; the server receives exactly those bytes.
#[test]
fn client_post_sends_content_length_and_body() {
    if !backend_available() {
        return;
    }
    let (port, server) = spawn_http_server(b"HTTP/1.1 200 OK\r\nContent-Length: 2\r\n\r\nok".to_vec());
    let prog = "\
import std.http
import std.cli
pub fn main(args: array<str>) -> Result<(), Error> {
  c := cli.command(\"post\")
  c.flag_str(\"url\", \"\")
  p := c.parse(args)?
  cl := http.client()
  resp := cl.post(p.get_str(\"url\"), \"payload\")?
  print(resp.status())
  return Ok(())
}
";
    let url = format!("http://127.0.0.1:{port}/submit");
    let out = build_and_run_args("m11-http-post", prog, &["--url", &url]);
    let req = String::from_utf8_lossy(&server.join().unwrap()).into_owned();
    assert_eq!(out.status.code(), Some(0), "stderr: {}", String::from_utf8_lossy(&out.stderr));
    assert_eq!(String::from_utf8_lossy(&out.stdout), "200\n");
    assert!(req.starts_with("POST /submit HTTP/1.1\r\n"), "request line: {req:?}");
    assert!(req.contains("Content-Length: 7\r\n"), "auto Content-Length: {req:?}");
    assert!(req.ends_with("\r\npayload"), "body sent: {req:?}");
}

/// Slice 5: a `https://` URL now ROUTES to a verified TLS connection (retiring the old DC-1
/// pre-connect rejection). We can't drive a positive TLS round-trip from the driver harness — the
/// runtime's test-only trust hook is `#[cfg(test)]`, compiled OUT of the runtime linked into a
/// driver-built executable, so a self-signed local server can't be trusted here (that positive path
/// is covered by the `align_runtime` `https_*` unit tests). Instead we prove the ROUTING change:
/// point `https://` at a local plaintext server and assert it observes a connection attempt (a TLS
/// ClientHello). Under the old behavior `https://` was rejected pre-connect → ZERO accepts; now it is
/// a real TLS connection → exactly ONE. The handshake then fails (a plaintext peer), so `main` still
/// exits non-zero — but the connection was made, which is the point.
#[test]
fn client_https_routes_to_tls_not_preconnect_reject() {
    if !backend_available() {
        return;
    }
    // A plaintext server that counts a single accept then closes (the client's ClientHello arrives as
    // garbage; the closed peer makes the client's handshake fail fast rather than hang).
    let listener = std::net::TcpListener::bind("127.0.0.1:0").expect("bind loopback");
    let port = listener.local_addr().unwrap().port();
    listener.set_nonblocking(true).unwrap();
    let handle = std::thread::spawn(move || {
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
        while std::time::Instant::now() < deadline {
            match listener.accept() {
                Ok((_s, _)) => return 1usize, // one TLS connection attempt observed; `_s` drops → close
                Err(ref e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                    std::thread::sleep(std::time::Duration::from_millis(1));
                }
                Err(_) => break,
            }
        }
        0usize
    });
    let prog = format!(
        "\
import std.http
pub fn main() -> Result<(), Error> {{
  cl := http.client()
  resp := cl.get(\"https://127.0.0.1:{port}/\")?
  print(resp.status())
  return Ok(())
}}
"
    );
    let out = build_and_run("m11-http-https-routes", &prog);
    // The handshake against a plaintext peer fails → `?` propagates → non-zero exit. The routing is
    // what we assert: the server saw the connection.
    assert!(!out.status.success(), "https to a plaintext peer fails the handshake (still an Err)");
    assert_eq!(handle.join().unwrap(), 1, "https:// now routes to a TLS connection (old: rejected pre-connect, 0 accepts)");
}

/// A malformed URL (no scheme / no host) is `Error.Invalid` at request time; the `?` propagates.
#[test]
fn client_malformed_url_is_error() {
    if !backend_available() {
        return;
    }
    let prog = "\
import std.http
pub fn main() -> Result<(), Error> {
  cl := http.client()
  resp := cl.get(\"not-a-url\")?
  print(resp.status())
  return Ok(())
}
";
    let out = build_and_run("m11-http-badurl", prog);
    assert!(!out.status.success(), "a malformed URL must be an error");
}

/// `cl.request(req)` consumes the Move `http request`: using `req` afterwards is a compile error.
#[test]
fn client_request_consumes_request_use_after_move_rejected() {
    let src = "\
import std.http
pub fn main() -> Result<(), Error> {
  cl := http.client()
  req := http.request(\"POST\", \"http://127.0.0.1/\")
  resp := cl.request(req)?
  req.header(\"X-After\", \"1\")
  return Ok(())
}
";
    assert!(check_errs("m11-http-req-uam", src), "using a moved-out http request after cl.request(req) must be rejected");
}

/// P4: an `http client` is an owned Move handle — an unbound temporary (`http.client().get(...)`) is
/// not dropped yet, so it cannot be a method receiver in v1. Bind it first.
#[test]
fn client_unbound_temporary_receiver_rejected() {
    let src = "\
import std.http
pub fn main() -> Result<(), Error> {
  resp := http.client().get(\"http://127.0.0.1/\")?
  return Ok(())
}
";
    assert!(check_errs("m11-http-client-unbound", src), "a method on an unbound client temporary must be rejected (bind first)");
}

/// A `client` is an owned handle bound to one local — it cannot be collected into an array (a copied
/// client would double-free), rejected at construction like the request / response / net handles.
#[test]
fn client_rejected_as_array_element() {
    let src = "\
import std.http
pub fn main() -> Result<(), Error> {
  xs := [http.client(), http.client()]
  print(xs.len())
  return Ok(())
}
";
    assert!(check_errs("m11-http-client-array", src), "an http client cannot be an array element");
}

/// `http.client()` requires `import std.http`.
#[test]
fn client_requires_import() {
    let src = "\
pub fn main() -> Result<(), Error> {
  cl := http.client()
  return Ok(())
}
";
    assert!(check_errs("m11-http-client-noimport", src), "http.client without import std.http must be rejected");
}

/// P3 (#297), via the client path: a `resp.body()` view from `cl.get()` cannot escape the response's
/// `Drop` — returning it out of the frame is a use-after-free (the same region rule as the Slice-1
/// `http.parse` path, exercised through the client).
#[test]
fn client_response_body_view_cannot_escape() {
    let src = "\
import std.http
fn steal(fallback: slice<u8>) -> slice<u8> {
  cl := http.client()
  match cl.get(\"http://127.0.0.1/\") {
    Ok(resp) => resp.body(),
    Err(e) => fallback,
  }
}
pub fn main() -> i32 {
  return 0
}
";
    let d = check_diagnostics("m11-http-client-body-escape", src);
    assert!(d.contains("cannot return a slice that views a local"), "a client response body view must not escape:\n{d}");
}

/// A persistent loopback keepalive server: accepts up to `max_conns` connections, handling every
/// request on each conn (HTTP/1.1 keepalive) until the client closes it. Returns the number of
/// ACCEPTED connections — the observable that shows the client reused one conn across gets (Slice-3
/// R3). Non-blocking accept + a per-read timeout bound the thread so a pool regression fails the count
/// rather than hanging the test.
fn spawn_keepalive_server(response: Vec<u8>, max_conns: usize) -> (u16, std::thread::JoinHandle<usize>) {
    use std::io::{Read, Write};
    let listener = std::net::TcpListener::bind("127.0.0.1:0").expect("bind loopback");
    let port = listener.local_addr().unwrap().port();
    listener.set_nonblocking(true).unwrap();
    let handle = std::thread::spawn(move || {
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
        let mut accepted = 0usize;
        while accepted < max_conns && std::time::Instant::now() < deadline {
            let mut sock = match listener.accept() {
                Ok((s, _)) => s,
                Err(ref e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                    std::thread::sleep(std::time::Duration::from_millis(1));
                    continue;
                }
                Err(_) => break,
            };
            accepted += 1;
            sock.set_nonblocking(false).unwrap();
            sock.set_read_timeout(Some(std::time::Duration::from_secs(5))).unwrap();
            let mut req: Vec<u8> = Vec::new();
            loop {
                let mut want: Option<usize> = None;
                let got_one = loop {
                    if let Some(t) = want {
                        if req.len() >= t {
                            break true;
                        }
                    } else if let Some(p) = req.windows(4).position(|w| w == b"\r\n\r\n") {
                        let head = String::from_utf8_lossy(&req[..p]).to_ascii_lowercase();
                        let cl = head
                            .lines()
                            .find_map(|l| l.strip_prefix("content-length:").map(|v| v.trim().parse::<usize>().unwrap_or(0)))
                            .unwrap_or(0);
                        want = Some(p + 4 + cl);
                        if req.len() >= p + 4 + cl {
                            break true;
                        }
                    }
                    let mut tmp = [0u8; 512];
                    match sock.read(&mut tmp) {
                        Ok(0) | Err(_) => break false,
                        Ok(n) => req.extend_from_slice(&tmp[..n]),
                    }
                };
                if !got_one {
                    break;
                }
                let _ = sock.write_all(&response);
                req.drain(..want.unwrap());
            }
        }
        accepted
    });
    (port, handle)
}

/// Slice-3 R3, end-to-end through the language: two `cl.get()`s on ONE bound client to the same
/// host:port reuse a single pooled keepalive connection — the server accepts exactly ONE connection
/// for the two requests, and both responses parse correctly.
#[test]
fn client_pool_reuses_connection_across_gets() {
    if !backend_available() {
        return;
    }
    let resp = b"HTTP/1.1 200 OK\r\nContent-Length: 2\r\n\r\nhi".to_vec();
    // max_conns = 1: with reuse working the server accepts one conn, handles both requests, then
    // exits on the client's EOF (fast — no deadline wait). If reuse were broken, the 2nd get would
    // open a 2nd conn the server no longer accepts → the get fails → the status assert below catches it.
    let (port, server) = spawn_keepalive_server(resp, 1);
    let prog = "\
import std.http
import std.cli
pub fn main(args: array<str>) -> Result<(), Error> {
  c := cli.command(\"get\")
  c.flag_str(\"url\", \"\")
  p := c.parse(args)?
  cl := http.client()
  r1 := cl.get(p.get_str(\"url\"))?
  print(r1.status())
  r2 := cl.get(p.get_str(\"url\"))?
  print(r2.status())
  return Ok(())
}
";
    let url = format!("http://127.0.0.1:{port}/");
    let out = build_and_run_args("m11-http-keepalive", prog, &["--url", &url]);
    let accepted = server.join().unwrap();
    assert_eq!(out.status.code(), Some(0), "stderr: {}", String::from_utf8_lossy(&out.stderr));
    assert_eq!(String::from_utf8_lossy(&out.stdout), "200\n200\n");
    assert_eq!(accepted, 1, "two gets on one client reused a single pooled connection (R3)");
}
