//! M12 std.http — the SSE/chunked **streaming response** (`respond_stream`, the runway A5 remainder;
//! reworked per `docs/impl/std-design/http.md` item 8 for pkg.web stream routes).
//! `ctx.respond_stream(rb)` consumes `rb` ONLY (header-only — a bodied builder aborts) and **borrows**
//! `ctx`: it validates + serializes the head + the transfer framing (`Transfer-Encoding: chunked` for
//! a 1.1 client, or close-delimited raw for a 1.0 client) into the owned `http_stream` — **lazy**, the
//! first `send`/`finish` writes it — lifts the accepted fd, and leaves the ctx with the caller SPENT
//! (views stay valid; a later `respond`/`respond_stream` on it is `Err`). `s.send(chunk)` writes ONE
//! chunk frame in ONE write (`send("")` is a no-op — an empty chunk is the terminator); `s.finish()`
//! is the sole clean terminator (`0\r\n\r\n` + close), consuming `s`; `s.reject(rb)` — legal only
//! before the first send — discards the stored head and answers with a complete NORMAL response
//! (consuming BOTH). Drop is **close-only** (no terminal write — abrupt close is chunked's own
//! truncation signal). The wire framing / lazy-head / reject / poison / version threading are
//! runtime-unit-tested in `align_runtime`; here we drive a real Align streaming server end-to-end (a
//! Rust client that DECODES the chunked framing) and check the Gate-1 rejections + the recorded
//! 1.1/1.0 + truncation + poison + spent-ctx + reject paths.

mod common;
use common::*;

use std::io::{Read, Write};
use std::net::TcpStream;
use std::time::{Duration, Instant};

/// A free loopback port (bind :0, read the port, drop) — `http.serve` rejects port 0.
fn free_loopback_port() -> u16 {
    let probe = std::net::TcpListener::bind("127.0.0.1:0").expect("bind probe");
    let port = probe.local_addr().unwrap().port();
    drop(probe);
    port
}

/// Connect (retrying while the server is still binding), send `req`, read the WHOLE response until the
/// server closes, and return it. The server closes after `finish` / on Drop, so `read_to_end` returns.
fn client_read_all(port: u16, req: &[u8]) -> Vec<u8> {
    let deadline = Instant::now() + Duration::from_secs(30);
    loop {
        match TcpStream::connect(("127.0.0.1", port)) {
            Ok(mut sock) => {
                sock.set_read_timeout(Some(Duration::from_secs(30))).unwrap();
                sock.write_all(req).expect("write request");
                let mut resp = Vec::new();
                let _ = sock.read_to_end(&mut resp);
                return resp;
            }
            Err(_) if Instant::now() < deadline => std::thread::sleep(Duration::from_millis(25)),
            Err(e) => panic!("server never came up: {e}"),
        }
    }
}

/// Split a raw HTTP response at the head/body boundary (`\r\n\r\n`), returning `(head_text, body_bytes)`.
/// The head keeps the terminating CRLF of its last header line (up to but excluding the blank line), so
/// every header line — including the last — ends in `\r\n` for a `contains("Header: v\r\n")` check.
fn split_head_body(resp: &[u8]) -> (String, Vec<u8>) {
    let sep = b"\r\n\r\n";
    let pos = resp.windows(4).position(|w| w == sep).expect("head/body boundary");
    (String::from_utf8_lossy(&resp[..pos + 2]).into_owned(), resp[pos + 4..].to_vec())
}

/// Decode a chunked body into its list of chunk payloads (each a separate `Vec` — so the caller can
/// assert one-frame-per-send). Stops at the `0`-length terminating chunk. Panics on malformed framing.
fn decode_chunks(body: &[u8]) -> Vec<Vec<u8>> {
    let mut out = Vec::new();
    let mut i = 0usize;
    loop {
        // chunk-size line (hex) up to CRLF.
        let nl = body[i..].windows(2).position(|w| w == b"\r\n").expect("chunk-size CRLF") + i;
        let size_hex = std::str::from_utf8(&body[i..nl]).expect("hex utf8");
        let size = usize::from_str_radix(size_hex.trim(), 16).expect("hex chunk-size");
        i = nl + 2;
        if size == 0 {
            break; // terminator
        }
        out.push(body[i..i + size].to_vec());
        i += size;
        assert_eq!(&body[i..i + 2], b"\r\n", "chunk data must be CRLF-terminated");
        i += 2;
    }
    out
}

/// 1.1 end-to-end: `respond_stream` + N `send`s (one with an empty chunk) + `finish` against a Rust
/// client that decodes the chunked framing. Each non-empty send must arrive as exactly one frame, the
/// empty send must produce NO frame, and the body must end with the `0\r\n\r\n` terminator. The ctx is
/// **borrowed** (item 8 ①): its `path()` view is read AFTER `respond_stream` and streamed as a chunk —
/// the LLM-pump shape (read the request while streaming the response).
#[test]
fn stream_http11_chunked_frames_end_to_end() {
    if !backend_available() {
        return;
    }
    let prog = "\
import std.http
import std.cli
pub fn main(args: array<str>) -> Result<(), Error> {
  c := cli.command(\"srv\")
  c.flag_i64(\"port\", 0)
  p := c.parse(args)?
  srv := http.serve(\"127.0.0.1\", p.get_i64(\"port\"))?
  ctx := srv.accept()?
  rb := http.response(200)
  rb.header(\"X-Kind\", \"stream\")
  s := ctx.respond_stream(rb)?
  s.send(\"data: one\\n\\n\")?
  s.send(\"\")?
  s.send(ctx.path())?
  s.finish()?
  return Ok(())
}
";
    let port = free_loopback_port();
    let server = build_exe("m12-stream-11", prog);
    let mut child = std::process::Command::new(&server.exe)
        .args(["--port", &port.to_string()])
        .spawn()
        .expect("spawn server");
    let resp = client_read_all(port, b"GET /events HTTP/1.1\r\nHost: h\r\n\r\n");
    let status = child.wait().expect("server exits");
    assert!(status.success(), "server exited with {status:?}");
    let (head, body) = split_head_body(&resp);
    assert!(head.starts_with("HTTP/1.1 200 OK\r\n"), "status line: {head:?}");
    assert!(head.contains("X-Kind: stream\r\n"), "caller header: {head:?}");
    assert!(head.contains("Transfer-Encoding: chunked\r\n"), "chunked framing for a 1.1 client: {head:?}");
    assert!(head.contains("Connection: close\r\n"), "auto Connection: close: {head:?}");
    assert!(!head.to_ascii_lowercase().contains("content-length"), "a streamed response has no Content-Length: {head:?}");
    // The terminator must be present (a clean finish).
    assert!(resp.ends_with(b"0\r\n\r\n"), "clean finish writes the 0-chunk terminator");
    // Exactly two frames (the empty send produced NO frame); the second is the ctx's `path()` view,
    // read AFTER `respond_stream` — the ctx borrow keeps the request views valid while streaming.
    let frames = decode_chunks(&body);
    assert_eq!(frames.len(), 2, "one frame per non-empty send; send(\"\") frames nothing");
    assert_eq!(frames[0], b"data: one\n\n");
    assert_eq!(frames[1], b"/events", "ctx.path() stays valid after respond_stream (borrowed ctx)");
}

/// A 1.0 request cannot be chunked: the stream is close-delimited **raw** — no `Transfer-Encoding`
/// header, the payload bytes are written unframed, and close is the terminator (`read_to_end` returns).
#[test]
fn stream_http10_raw_close_delimited() {
    if !backend_available() {
        return;
    }
    let prog = "\
import std.http
import std.cli
pub fn main(args: array<str>) -> Result<(), Error> {
  c := cli.command(\"srv\")
  c.flag_i64(\"port\", 0)
  p := c.parse(args)?
  srv := http.serve(\"127.0.0.1\", p.get_i64(\"port\"))?
  ctx := srv.accept()?
  rb := http.response(200)
  s := ctx.respond_stream(rb)?
  s.send(\"data: one\\n\\n\")?
  s.send(\"\")?
  s.send(\"data: two\\n\\n\")?
  s.finish()?
  return Ok(())
}
";
    let port = free_loopback_port();
    let server = build_exe("m12-stream-10", prog);
    let mut child = std::process::Command::new(&server.exe)
        .args(["--port", &port.to_string()])
        .spawn()
        .expect("spawn server");
    let resp = client_read_all(port, b"GET / HTTP/1.0\r\nHost: h\r\n\r\n");
    let status = child.wait().expect("server exits");
    assert!(status.success(), "server exited with {status:?}");
    let (head, body) = split_head_body(&resp);
    assert!(head.starts_with("HTTP/1.1 200 OK\r\n"), "status line: {head:?}");
    assert!(!head.to_ascii_lowercase().contains("transfer-encoding"), "a 1.0 stream has NO Transfer-Encoding: {head:?}");
    assert!(head.contains("Connection: close\r\n"), "Connection: close: {head:?}");
    // Raw, unframed payload — the concatenation of the non-empty sends (the empty send wrote nothing).
    assert_eq!(body, b"data: one\n\ndata: two\n\n", "raw close-delimited body");
    assert!(!resp.ends_with(b"0\r\n\r\n"), "raw mode has no chunk terminator");
}

/// Drop-without-finish → the client sees the chunk frames but NO terminal `0\r\n\r\n` (truncation is
/// chunked's own signal), and the fd is closed on Drop (else `read_to_end` would hang) — driven across
/// several cycles so a leaked fd would surface. The stream is created + sent to, then dropped at the
/// loop-body scope end WITHOUT `finish`.
#[test]
fn stream_drop_without_finish_truncates_and_closes() {
    if !backend_available() {
        return;
    }
    let prog = "\
import std.http
import std.cli
pub fn main(args: array<str>) -> Result<(), Error> {
  c := cli.command(\"srv\")
  c.flag_i64(\"port\", 0)
  p := c.parse(args)?
  srv := http.serve(\"127.0.0.1\", p.get_i64(\"port\"))?
  mut i: i64 := 0
  loop {
    if i == 5 { break }
    ctx := srv.accept()?
    rb := http.response(200)
    s := ctx.respond_stream(rb)?
    s.send(\"partial\\n\")?
    i = i + 1
  }
  return Ok(())
}
";
    let port = free_loopback_port();
    let server = build_exe("m12-stream-drop", prog);
    let mut child = std::process::Command::new(&server.exe)
        .args(["--port", &port.to_string()])
        .spawn()
        .expect("spawn server");
    for cycle in 0..5 {
        let resp = client_read_all(port, b"GET /x HTTP/1.1\r\nHost: h\r\n\r\n");
        let (head, body) = split_head_body(&resp);
        assert!(head.contains("Transfer-Encoding: chunked\r\n"), "cycle {cycle} head: {head:?}");
        // Exactly one frame was sent; the response is TRUNCATED — no `0\r\n\r\n` terminator.
        assert!(!resp.ends_with(b"0\r\n\r\n"), "cycle {cycle}: a dropped (un-finished) stream must NOT write a terminator");
        assert!(body.starts_with(b"8\r\npartial\n\r\n"), "cycle {cycle}: the one sent frame is present: {body:?}");
        // `read_to_end` returned at all ⇒ the fd was closed on Drop (a leaked fd would hang the read).
    }
    let status = child.wait().expect("server exits");
    assert!(status.success(), "server served all cycles without an fd leak / crash: {status:?}");
}

/// Poison path: the client reads the head then disconnects; the server streams big chunks until a
/// `send` errors (EPIPE), which poisons the stream, and `finish` then returns `Err` (skipping the
/// terminator) WITHOUT hanging. The server observes the Err and exits cleanly.
#[test]
fn stream_poisoned_finish_returns_err_without_hanging() {
    if !backend_available() {
        return;
    }
    // A 64 KiB chunk so the socket buffer fills within a few sends once the peer is gone.
    let big = "z".repeat(65536);
    let prog = format!(
        "\
import std.http
import std.cli
pub fn main(args: array<str>) -> Result<(), Error> {{
  c := cli.command(\"srv\")
  c.flag_i64(\"port\", 0)
  p := c.parse(args)?
  srv := http.serve(\"127.0.0.1\", p.get_i64(\"port\"))?
  ctx := srv.accept()?
  rb := http.response(200)
  s := ctx.respond_stream(rb)?
  mut n: i64 := 0
  loop {{
    if n == 100000 {{ break }}
    match s.send(\"{big}\") {{
      Ok(_) => {{ n = n + 1 }}
      Err(_) => {{ break }}
    }}
  }}
  match s.finish() {{
    Ok(_) => print(0)
    Err(_) => print(1)
  }}
  return Ok(())
}}
"
    );
    let port = free_loopback_port();
    let server = build_exe("m12-stream-poison", &prog);
    let child = std::process::Command::new(&server.exe)
        .args(["--port", &port.to_string()])
        .stdout(std::process::Stdio::piped())
        .spawn()
        .expect("spawn server");
    // Connect, send the request, read a little of the head, then hard-close (drop) the socket.
    let deadline = Instant::now() + Duration::from_secs(30);
    loop {
        match TcpStream::connect(("127.0.0.1", port)) {
            Ok(mut sock) => {
                sock.write_all(b"GET /events HTTP/1.1\r\nHost: h\r\n\r\n").unwrap();
                let mut buf = [0u8; 16];
                let _ = sock.read(&mut buf); // read a bit of the head, then drop → RST
                drop(sock);
                break;
            }
            Err(_) if Instant::now() < deadline => std::thread::sleep(Duration::from_millis(25)),
            Err(e) => panic!("server never came up: {e}"),
        }
    }
    // The server must terminate (not hang) and report the poisoned `finish` as Err (marker `1`).
    let out = child.wait_with_output().expect("server exits without hanging");
    assert!(out.status.success(), "server exited cleanly: {:?}", out.status);
    assert_eq!(String::from_utf8_lossy(&out.stdout), "1\n", "poisoned finish returns Err");
}

/// Dogfood asymmetry (recorded): align's OWN client rejects a chunked response as `Error.Invalid`
/// (client parse stays Content-Length-only). The align streaming server responds chunked; the align
/// `cl.get` client sees `Err`.
#[test]
fn align_client_rejects_chunked_stream_as_invalid() {
    if !backend_available() {
        return;
    }
    let server_prog = "\
import std.http
import std.cli
pub fn main(args: array<str>) -> Result<(), Error> {
  c := cli.command(\"srv\")
  c.flag_i64(\"port\", 0)
  p := c.parse(args)?
  srv := http.serve(\"127.0.0.1\", p.get_i64(\"port\"))?
  print(\"ready\")
  ctx := srv.accept()?
  rb := http.response(200)
  s := ctx.respond_stream(rb)?
  s.send(\"hello\")?
  s.finish()?
  return Ok(())
}
";
    let client_prog = "\
import std.http
import std.cli
pub fn main(args: array<str>) -> Result<(), Error> {
  c := cli.command(\"get\")
  c.flag_str(\"url\", \"\")
  p := c.parse(args)?
  cl := http.client()
  match cl.get(p.get_str(\"url\")) {
    Ok(_) => print(0)
    Err(_) => print(1)
  }
  return Ok(())
}
";
    let port = free_loopback_port();
    let server = build_exe("m12-stream-dogfood-srv", server_prog);
    let mut child = std::process::Command::new(&server.exe)
        .args(["--port", &port.to_string()])
        .stdout(std::process::Stdio::piped())
        .spawn()
        .expect("spawn server");
    {
        use std::io::BufRead;
        let stdout = child.stdout.as_mut().expect("piped stdout");
        let mut reader = std::io::BufReader::new(stdout);
        let mut line = String::new();
        reader.read_line(&mut line).expect("read ready marker");
        assert_eq!(line.trim(), "ready", "server readiness marker");
    }
    let url = format!("http://127.0.0.1:{port}/");
    let out = build_and_run_args("m12-stream-dogfood-cli", client_prog, &["--url", &url]);
    let _ = child.wait();
    assert_eq!(out.status.code(), Some(0), "client stderr: {}", String::from_utf8_lossy(&out.stderr));
    assert_eq!(String::from_utf8_lossy(&out.stdout), "1\n", "align client rejects a chunked response as Invalid");
}

/// A bodied `response_builder` passed to `respond_stream` is a programmer contract bug → **abort**
/// (the streamed body is written with `s.send`, not `rb.body`). The server process aborts (non-zero).
#[test]
fn respond_stream_bodied_builder_aborts() {
    if !backend_available() {
        return;
    }
    let prog = "\
import std.http
import std.cli
pub fn main(args: array<str>) -> Result<(), Error> {
  c := cli.command(\"srv\")
  c.flag_i64(\"port\", 0)
  p := c.parse(args)?
  srv := http.serve(\"127.0.0.1\", p.get_i64(\"port\"))?
  ctx := srv.accept()?
  rb := http.response(200)
  rb.body(\"nope\")
  s := ctx.respond_stream(rb)?
  s.finish()?
  return Ok(())
}
";
    let port = free_loopback_port();
    let server = build_exe("m12-stream-bodied", prog);
    let mut child = std::process::Command::new(&server.exe)
        .args(["--port", &port.to_string()])
        .spawn()
        .expect("spawn server");
    // Unblock `accept`; the server aborts inside `respond_stream` before writing anything.
    let _ = client_read_all(port, b"GET / HTTP/1.1\r\nHost: h\r\n\r\n");
    let status = child.wait().expect("server exits");
    assert!(!status.success(), "a bodied respond_stream must abort");
}

/// The SPENT ctx (item 8 ①): after `respond_stream` the ctx stays with the caller — its views are
/// legal — but a later `respond` on it is a runtime `Err` (the stream owns the connection), pinned
/// end-to-end: the handler observes the Err (marker) and the client still gets a clean stream.
#[test]
fn respond_on_spent_ctx_is_err_end_to_end() {
    if !backend_available() {
        return;
    }
    let prog = "\
import std.http
import std.cli
pub fn main(args: array<str>) -> Result<(), Error> {
  c := cli.command(\"srv\")
  c.flag_i64(\"port\", 0)
  p := c.parse(args)?
  srv := http.serve(\"127.0.0.1\", p.get_i64(\"port\"))?
  ctx := srv.accept()?
  rb := http.response(200)
  s := ctx.respond_stream(rb)?
  rb2 := http.response(500)
  match ctx.respond(rb2) {
    Ok(_) => print(0)
    Err(_) => print(1)
  }
  s.send(\"ok\")?
  s.finish()?
  return Ok(())
}
";
    let port = free_loopback_port();
    let server = build_exe("m12-stream-spent-ctx", prog);
    let child = std::process::Command::new(&server.exe)
        .args(["--port", &port.to_string()])
        .stdout(std::process::Stdio::piped())
        .spawn()
        .expect("spawn server");
    let resp = client_read_all(port, b"GET / HTTP/1.1\r\nHost: h\r\n\r\n");
    let out = child.wait_with_output().expect("server exits");
    assert!(out.status.success(), "server exited cleanly: {:?}", out.status);
    assert_eq!(String::from_utf8_lossy(&out.stdout), "1\n", "respond on a spent ctx returns Err");
    let (head, body) = split_head_body(&resp);
    assert!(head.starts_with("HTTP/1.1 200 OK\r\n"), "the STREAM head went out, not the 500: {head:?}");
    assert_eq!(decode_chunks(&body), vec![b"ok".to_vec()], "the stream itself is unaffected");
}

/// `s.reject(rb)` (item 8 ③) end-to-end: the pre-stream 4xx window. The pump rejects before any
/// send — the client receives a complete NORMAL response (CL + body, no Transfer-Encoding, nothing
/// of the discarded stream head).
#[test]
fn stream_reject_answers_with_a_normal_response() {
    if !backend_available() {
        return;
    }
    let prog = "\
import std.http
import std.cli
pub fn main(args: array<str>) -> Result<(), Error> {
  c := cli.command(\"srv\")
  c.flag_i64(\"port\", 0)
  p := c.parse(args)?
  srv := http.serve(\"127.0.0.1\", p.get_i64(\"port\"))?
  ctx := srv.accept()?
  rb := http.response(200)
  rb.header(\"X-Stream-Head\", \"discarded\")
  s := ctx.respond_stream(rb)?
  rb2 := http.response(400)
  rb2.body(\"bad request\")
  s.reject(rb2)?
  return Ok(())
}
";
    let port = free_loopback_port();
    let server = build_exe("m12-stream-reject", prog);
    let mut child = std::process::Command::new(&server.exe)
        .args(["--port", &port.to_string()])
        .spawn()
        .expect("spawn server");
    let resp = client_read_all(port, b"GET / HTTP/1.1\r\nHost: h\r\n\r\n");
    let status = child.wait().expect("server exits");
    assert!(status.success(), "server exited with {status:?}");
    let (head, body) = split_head_body(&resp);
    assert!(head.starts_with("HTTP/1.1 400 Bad Request\r\n"), "the reject response, not the stream head: {head:?}");
    assert!(head.contains("Content-Length: 11\r\n"), "a NORMAL (CL-framed) response: {head:?}");
    assert!(!head.to_ascii_lowercase().contains("transfer-encoding"), "no stream framing: {head:?}");
    assert!(!head.contains("X-Stream-Head"), "the stored stream head was discarded: {head:?}");
    assert_eq!(body, b"bad request");
}

/// `reject` after the first send is a runtime `Err` (the head is committed — a normal response can
/// no longer be written); the connection just closes (truncation), and the handler observes the Err.
#[test]
fn stream_reject_after_send_is_err() {
    if !backend_available() {
        return;
    }
    let prog = "\
import std.http
import std.cli
pub fn main(args: array<str>) -> Result<(), Error> {
  c := cli.command(\"srv\")
  c.flag_i64(\"port\", 0)
  p := c.parse(args)?
  srv := http.serve(\"127.0.0.1\", p.get_i64(\"port\"))?
  ctx := srv.accept()?
  rb := http.response(200)
  s := ctx.respond_stream(rb)?
  s.send(\"first\")?
  rb2 := http.response(400)
  match s.reject(rb2) {
    Ok(_) => print(0)
    Err(_) => print(1)
  }
  return Ok(())
}
";
    let port = free_loopback_port();
    let server = build_exe("m12-stream-reject-late", prog);
    let child = std::process::Command::new(&server.exe)
        .args(["--port", &port.to_string()])
        .stdout(std::process::Stdio::piped())
        .spawn()
        .expect("spawn server");
    let resp = client_read_all(port, b"GET / HTTP/1.1\r\nHost: h\r\n\r\n");
    let out = child.wait_with_output().expect("server exits");
    assert!(out.status.success(), "server exited cleanly: {:?}", out.status);
    assert_eq!(String::from_utf8_lossy(&out.stdout), "1\n", "reject after a send returns Err");
    let (head, _body) = split_head_body(&resp);
    assert!(head.starts_with("HTTP/1.1 200 OK\r\n"), "the committed stream head stands: {head:?}");
    assert!(!resp.ends_with(b"0\r\n\r\n"), "no terminator — the late reject closed the stream abruptly");
}

// --- compile-time Gate-1 rejections ------------------------------------------------------------

/// `respond_stream` consumes `rb` ONLY: using `rb` after the call is a moved-value error, while the
/// ctx is borrowed — reading its views after the call compiles clean (pinned at check level here;
/// exercised end-to-end above).
#[test]
fn respond_stream_consumes_rb_but_borrows_ctx() {
    let use_ctx = "\
import std.http
import std.cli
pub fn main(args: array<str>) -> Result<(), Error> {
  c := cli.command(\"srv\")
  c.flag_i64(\"port\", 0)
  p := c.parse(args)?
  srv := http.serve(\"127.0.0.1\", p.get_i64(\"port\"))?
  ctx := srv.accept()?
  rb := http.response(200)
  s := ctx.respond_stream(rb)?
  print(ctx.path())
  s.finish()?
  return Ok(())
}
";
    assert!(!check_errs("m12-stream-uses-ctx", use_ctx), "using ctx views after respond_stream is legal (borrowed)");
    let use_rb = "\
import std.http
import std.cli
pub fn main(args: array<str>) -> Result<(), Error> {
  c := cli.command(\"srv\")
  c.flag_i64(\"port\", 0)
  p := c.parse(args)?
  srv := http.serve(\"127.0.0.1\", p.get_i64(\"port\"))?
  ctx := srv.accept()?
  rb := http.response(200)
  s := ctx.respond_stream(rb)?
  rb.body(\"x\")
  return Ok(())
}
";
    assert!(check_errs("m12-stream-uses-rb", use_rb), "using rb after respond_stream must be rejected (consumed)");
}

/// `reject` consumes BOTH the stream and its builder: a `send` after `reject`, and a use of the
/// answering `rb` after `reject`, are moved-value errors.
#[test]
fn stream_reject_consumes_stream_and_rb() {
    let send_after = "\
import std.http
import std.cli
pub fn main(args: array<str>) -> Result<(), Error> {
  c := cli.command(\"srv\")
  c.flag_i64(\"port\", 0)
  p := c.parse(args)?
  srv := http.serve(\"127.0.0.1\", p.get_i64(\"port\"))?
  ctx := srv.accept()?
  rb := http.response(200)
  s := ctx.respond_stream(rb)?
  rb2 := http.response(400)
  s.reject(rb2)?
  s.send(\"x\")?
  return Ok(())
}
";
    assert!(check_errs("m12-stream-send-after-reject", send_after), "send after reject must be rejected (stream consumed)");
    let rb_after = "\
import std.http
import std.cli
pub fn main(args: array<str>) -> Result<(), Error> {
  c := cli.command(\"srv\")
  c.flag_i64(\"port\", 0)
  p := c.parse(args)?
  srv := http.serve(\"127.0.0.1\", p.get_i64(\"port\"))?
  ctx := srv.accept()?
  rb := http.response(200)
  s := ctx.respond_stream(rb)?
  rb2 := http.response(400)
  s.reject(rb2)?
  rb2.body(\"x\")
  return Ok(())
}
";
    assert!(check_errs("m12-stream-rb-after-reject", rb_after), "using rb after reject must be rejected (consumed)");
}

/// `finish` consumes the stream: a `send` after `finish` is a moved-value error.
#[test]
fn stream_send_after_finish_rejected() {
    let src = "\
import std.http
import std.cli
pub fn main(args: array<str>) -> Result<(), Error> {
  c := cli.command(\"srv\")
  c.flag_i64(\"port\", 0)
  p := c.parse(args)?
  srv := http.serve(\"127.0.0.1\", p.get_i64(\"port\"))?
  ctx := srv.accept()?
  rb := http.response(200)
  s := ctx.respond_stream(rb)?
  s.finish()?
  s.send(\"x\")?
  return Ok(())
}
";
    assert!(check_errs("m12-stream-send-after-finish", src), "send after finish must be rejected (stream consumed)");
}

/// v1 bound-receiver gate: a method on an unbound `http_stream` temporary is rejected; and the stream
/// methods require `import std.http` (dispatched on the type, so an unimported program never types).
#[test]
fn stream_unbound_receiver_and_import_gate() {
    // Unbound: `ctx.respond_stream(rb)?.send(...)` — the stream temporary is not bound to a local.
    let unbound = "\
import std.http
import std.cli
pub fn main(args: array<str>) -> Result<(), Error> {
  c := cli.command(\"srv\")
  c.flag_i64(\"port\", 0)
  p := c.parse(args)?
  srv := http.serve(\"127.0.0.1\", p.get_i64(\"port\"))?
  ctx := srv.accept()?
  rb := http.response(200)
  ctx.respond_stream(rb)?.send(\"x\")?
  return Ok(())
}
";
    assert!(check_errs("m12-stream-unbound", unbound), "a method on an unbound http_stream must be rejected");
    // Import gate: `respond_stream` needs `import std.http`.
    let noimport = "\
pub fn main() -> Result<(), Error> {
  srv := http.serve(\"127.0.0.1\", 8080)?
  ctx := srv.accept()?
  rb := http.response(200)
  s := ctx.respond_stream(rb)?
  return Ok(())
}
";
    assert!(check_errs("m12-stream-noimport", noimport), "respond_stream without `import std.http` must be rejected");
}
