//! pkg.web W6 streaming wiring (streaming enabler 5): stream routes in the SAME route table via
//! the `Handler` sum type (`Respond` / `Stream`), the `web.sse` / `web.stream` constructors,
//! `serve`'s stream arm (framework-built lazy head + `Cache-Control: no-cache`), and the WHATWG
//! event framing `s.send_event(data)` (std.http's — a pkg-level free fn cannot borrow a Move
//! handle, so `pkg.web` deliberately ships no wrapper).
//!
//! What this pins, over a real socket against the REAL framework source (`include_str!`):
//!   - an SSE route dispatches through the shared radix tree, its pump reads the request through
//!     the borrowed-and-spent ctx's views (`param` / `has_query` / `body` MID-PUMP), and each
//!     `send_event` goes out as one chunk frame `data: {data}\n\n`;
//!   - `s.reject(rb)` is the pre-stream 4xx window: the client gets a complete NORMAL response
//!     (CL-framed, none of the discarded stream head) and the serve loop keeps serving;
//!   - stream and unary routes coexist in one table: same dispatch, same 404, and a stream
//!     route's method contributes to the 405 `Allow` set like any other row.
//!
//! NOTE: v1 `serve` is sequential, so an open stream starves other clients — shipping streaming in
//! production is gated on concurrent serve (the recorded follow-up). Everything here completes each
//! stream before the next request, which the sequential loop handles fine.

mod common;
use common::*;

use std::io::{Read, Write};
use std::net::TcpStream;
use std::time::{Duration, Instant};

const ROUTER: &str = include_str!("../../../apps/web/pkg/web/internal/router.align");
const TYPES: &str = include_str!("../../../apps/web/pkg/web/types.align");
const WEB_ROOT: &str = include_str!("../../../apps/web/pkg/web.align");
const QUERY: &str = include_str!("../../../apps/web/pkg/web/internal/query.align");

fn free_loopback_port() -> u16 {
    let probe = std::net::TcpListener::bind("127.0.0.1:0").expect("bind probe");
    let port = probe.local_addr().unwrap().port();
    drop(probe);
    port
}

/// One request over its own connection, reading until the server closes (a finished stream or a
/// v1 per-request close), retrying the connect until the server is up.
fn exchange(port: u16, req: &[u8]) -> Vec<u8> {
    // One request per connection: keep-alive would leave the socket parked and this read blocked.
    let req = &one_shot(req)[..];
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

/// Split a raw HTTP response at the head/body boundary. The head keeps its last header line's CRLF
/// so `contains("Header: v\r\n")` works for every header.
fn split_head_body(resp: &[u8]) -> (String, Vec<u8>) {
    let pos = resp.windows(4).position(|w| w == b"\r\n\r\n").expect("head/body boundary");
    (String::from_utf8_lossy(&resp[..pos + 2]).into_owned(), resp[pos + 4..].to_vec())
}

/// Decode a chunked body into its chunk payloads (one `Vec` per frame — so a test can assert
/// one-frame-per-send). Stops at the `0` terminator; panics on malformed framing.
fn decode_chunks(body: &[u8]) -> Vec<Vec<u8>> {
    let mut out = Vec::new();
    let mut i = 0usize;
    loop {
        let nl = body[i..].windows(2).position(|w| w == b"\r\n").expect("chunk-size CRLF") + i;
        let size_hex = std::str::from_utf8(&body[i..nl]).expect("hex utf8");
        let size = usize::from_str_radix(size_hex.trim(), 16).expect("hex chunk-size");
        i = nl + 2;
        if size == 0 {
            break;
        }
        out.push(body[i..i + size].to_vec());
        i += size;
        assert_eq!(&body[i..i + 2], b"\r\n", "chunk data must be CRLF-terminated");
        i += 2;
    }
    out
}

/// The application: one table mixing unary and stream routes. The SSE pump reads the request
/// MID-PUMP through the Copy ctx (`param`, `has_query`) — the spent-ctx views-stay-valid
/// guarantee — and rejects pre-stream on `?bad`; the generic stream route echoes the request body.
const APP: &str = "module main\n\
import std.cli\n\
import pkg.web\n\
import pkg.web.types\n\
\n\
fn plain(c: pkg.web.types.Ctx) -> Result<response_builder, Error> {\n\
  return pkg.web.text(\"plain\")\n\
}\n\
\n\
fn events(c: pkg.web.types.Ctx, s: http_stream) -> Result<(), Error> {\n\
  if pkg.web.has_query(c, \"bad\") {\n\
    rb := pkg.web.status_text(400, \"bad request\")?\n\
    return s.reject(rb)\n\
  }\n\
  s.send_event(pkg.web.param(c, \"channel\"))?\n\
  s.send_event(\"tick\")?\n\
  s.finish()\n\
}\n\
\n\
fn echo_stream(c: pkg.web.types.Ctx, s: http_stream) -> Result<(), Error> {\n\
  s.send(pkg.web.body(c))?\n\
  s.finish()\n\
}\n\
\n\
fn fail_stream(c: pkg.web.types.Ctx, s: http_stream) -> Result<(), Error> {\n\
  return Err(Error.Denied)\n\
}\n\
\n\
pub fn main(args: array<str>) -> Result<(), Error> {\n\
  cmd := cli.command(\"srv\")\n\
  cmd.flag_i64(\"port\", 0)\n\
  p := cmd.parse(args)?\n\
  routes := [\n\
    pkg.web.get(\"/plain\", plain),\n\
    pkg.web.sse(\"/events/:channel\", events),\n\
    pkg.web.stream(\"POST\", \"/ndjson\", \"application/x-ndjson\", echo_stream),\n\
    pkg.web.stream(\"GET\", \"/fail-stream\", \"text/plain\", fail_stream),\n\
  ]\n\
  return pkg.web.serve(\"127.0.0.1\", p.get_i64(\"port\"), routes, 1)\n\
}\n";

/// A server child killed on drop — `serve` never returns, so every test must reap it.
struct Server {
    child: std::process::Child,
    port: u16,
    _built: BuiltExeMulti,
}

impl Drop for Server {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

impl Server {
    fn stop_and_stderr(&mut self) -> String {
        let _ = self.child.kill();
        let _ = self.child.wait();
        let mut stderr = String::new();
        self.child
            .stderr
            .take()
            .expect("stderr piped")
            .read_to_string(&mut stderr)
            .expect("read stderr");
        stderr
    }
}

fn start(name: &str) -> Server {
    let port = free_loopback_port();
    let built = build_exe_multi(
        name,
        &[
            ("pkg/web/internal/router.align", ROUTER),
            ("pkg/web/internal/query.align", QUERY),
            ("pkg/web/types.align", TYPES),
            ("pkg/web.align", WEB_ROOT),
            ("main.align", APP),
        ],
        "main.align",
    );
    let mut child = std::process::Command::new(&built.exe)
        .args(["--port", &port.to_string()])
        .stderr(std::process::Stdio::piped())
        .spawn()
        .expect("spawn server");
    std::thread::sleep(Duration::from_millis(300));
    if let Ok(Some(st)) = child.try_wait() {
        let mut stderr = String::new();
        child.stderr.take().expect("stderr piped").read_to_string(&mut stderr).expect("read stderr");
        panic!("server exited at startup: {st:?}; stderr: {stderr}");
    }
    Server { child, port, _built: built }
}

#[test]
fn a_stream_handler_error_is_logged_and_the_loop_survives() {
    if !backend_available() {
        return;
    }
    let mut srv = start("web-stream-handler-log");

    // The lazy stream head has not committed, but a pump Err is not a normal response builder: the
    // connection closes, the failure is logged, and the next request still runs.
    let failed = exchange(srv.port, b"GET /fail-stream HTTP/1.1\r\nHost: h\r\n\r\n");
    assert!(failed.is_empty(), "a pre-send pump Err writes no partial stream: {failed:?}");
    let plain = exchange(srv.port, b"GET /plain HTTP/1.1\r\nHost: h\r\n\r\n");
    let (head, body) = split_head_body(&plain);
    assert!(head.starts_with("HTTP/1.1 200 OK\r\n"), "loop survived: {head:?}");
    assert_eq!(body, b"plain");
    assert_eq!(srv.stop_and_stderr(), "pkg.web: handler Err (GET /fail-stream): Denied\n");
}

#[test]
fn an_sse_route_streams_whatwg_event_frames() {
    if !backend_available() {
        return;
    }
    let srv = start("web-stream-sse");
    let port = srv.port;

    // The pump reads the `:channel` capture MID-PUMP (the ctx is spent but its views hold) and
    // sends two events; the framework built the head (SSE content type + no-cache, chunked).
    let resp = exchange(port, b"GET /events/news HTTP/1.1\r\nHost: h\r\n\r\n");
    let (head, body) = split_head_body(&resp);
    assert!(head.starts_with("HTTP/1.1 200 OK\r\n"), "sse head: {head:?}");
    assert!(head.contains("Content-Type: text/event-stream\r\n"), "sse content type: {head:?}");
    assert!(head.contains("Cache-Control: no-cache\r\n"), "sse cache-control: {head:?}");
    assert!(head.contains("Transfer-Encoding: chunked\r\n"), "sse framing: {head:?}");
    let frames = decode_chunks(&body);
    assert_eq!(frames.len(), 2, "one chunk frame per send_event: {frames:?}");
    assert_eq!(frames[0], b"data: news\n\n", "the :channel capture, WHATWG-framed");
    assert_eq!(frames[1], b"data: tick\n\n");

    // The loop survived the stream: a unary route still answers.
    let plain = exchange(port, b"GET /plain HTTP/1.1\r\nHost: h\r\n\r\n");
    let (phead, pbody) = split_head_body(&plain);
    assert!(phead.starts_with("HTTP/1.1 200 OK\r\n"), "plain after stream: {phead:?}");
    assert_eq!(pbody, b"plain");
}

#[test]
fn reject_is_the_pre_stream_4xx_window_and_the_loop_survives() {
    if !backend_available() {
        return;
    }
    let srv = start("web-stream-reject");
    let port = srv.port;

    // The pump validates the request (a query flag here) BEFORE any send and rejects: the client
    // gets a complete NORMAL response — CL-framed, no chunked framing, none of the discarded
    // stream head (no text/event-stream).
    let resp = exchange(port, b"GET /events/news?bad=1 HTTP/1.1\r\nHost: h\r\n\r\n");
    let (head, body) = split_head_body(&resp);
    assert!(head.starts_with("HTTP/1.1 400 Bad Request\r\n"), "reject head: {head:?}");
    assert!(head.contains("Content-Length: 11\r\n"), "a NORMAL (CL-framed) response: {head:?}");
    assert!(!head.to_ascii_lowercase().contains("transfer-encoding"), "no stream framing: {head:?}");
    assert!(!head.contains("text/event-stream"), "the discarded stream head leaked: {head:?}");
    assert_eq!(body, b"bad request");

    // The same route still streams for a good request — serve held the loop through the reject.
    let ok = exchange(port, b"GET /events/news HTTP/1.1\r\nHost: h\r\n\r\n");
    let (ohead, obody) = split_head_body(&ok);
    assert!(ohead.starts_with("HTTP/1.1 200 OK\r\n"), "post-reject stream: {ohead:?}");
    assert_eq!(decode_chunks(&obody).len(), 2);
}

#[test]
fn stream_and_unary_routes_share_one_table_dispatch_404_and_405() {
    if !backend_available() {
        return;
    }
    let srv = start("web-stream-table");
    let port = srv.port;

    // A generic (non-SSE) stream route: the framework head carries ITS content type, and the pump
    // reads the request BODY mid-pump (a Ctx view into the spent ctx's parse buffer).
    let echoed = exchange(
        port,
        b"POST /ndjson HTTP/1.1\r\nHost: h\r\nContent-Length: 8\r\n\r\n{\"a\":1}\n",
    );
    let (ehead, ebody) = split_head_body(&echoed);
    assert!(ehead.starts_with("HTTP/1.1 200 OK\r\n"), "ndjson head: {ehead:?}");
    assert!(ehead.contains("Content-Type: application/x-ndjson\r\n"), "ndjson content type: {ehead:?}");
    assert_eq!(decode_chunks(&ebody), vec![b"{\"a\":1}\n".to_vec()], "the request body, echoed as one chunk");

    // A stream route contributes to method resolution like any row: wrong method -> 405 with its
    // method in `Allow`; unmatched path -> 404.
    let wrong = exchange(port, b"DELETE /ndjson HTTP/1.1\r\nHost: h\r\n\r\n");
    let (whead, _) = split_head_body(&wrong);
    assert!(whead.starts_with("HTTP/1.1 405 "), "405: {whead:?}");
    assert!(whead.contains("Allow: POST\r\n"), "405 allow from a stream row: {whead:?}");
    let missing = exchange(port, b"GET /nope HTTP/1.1\r\nHost: h\r\n\r\n");
    let (mhead, _) = split_head_body(&missing);
    assert!(mhead.starts_with("HTTP/1.1 404 "), "404: {mhead:?}");
}
