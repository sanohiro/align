//! pkg.web W2 — the real `pkg.web` root: route constructors, `serve`'s accept loop, and the
//! responders.
//!
//! What this pins is the whole designed surface running as one program: an application builds a
//! route table out of `web.get`/`web.post`, hands it to `web.serve`, and writes handlers that
//! BUILD a response through `web.text` / `web.json` / `web.status` and hand it back. `serve` owns
//! the accept loop AND the request handle, so every automatic response is the framework's — a 405
//! carries the `Allow` set for the matched path, an unmatched path is a 404, and a handler that
//! returns `Err` becomes a 500 (only expressible because the handle never left `serve`'s frame). Unlike `apps_web_serve.rs` (one request, then
//! exit) the server here stays up across requests: the loop surviving request after request,
//! including a handler that returns `Err`, is itself an assertion — every path consumes the
//! request handle exactly once, and a double free would abort the process mid-suite.

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

/// One request over its own connection (v1 closes after each response), retrying the connect until
/// the server is up.
fn exchange(port: u16, req: &[u8]) -> String {
    // One request per connection: keep-alive would leave the socket parked and this read blocked.
    let req = &one_shot(req)[..];
    let deadline = Instant::now() + Duration::from_secs(30);
    loop {
        match TcpStream::connect(("127.0.0.1", port)) {
            Ok(mut sock) => {
                sock.write_all(req).expect("write request");
                let mut resp = Vec::new();
                let _ = sock.read_to_end(&mut resp);
                return String::from_utf8_lossy(&resp).into_owned();
            }
            Err(_) if Instant::now() < deadline => std::thread::sleep(Duration::from_millis(25)),
            Err(e) => panic!("server never came up: {e}"),
        }
    }
}

/// The application: a route table built with the per-method constructors, one handler per route,
/// and `web.serve` owning everything else. `boom` returns `Err` AFTER responding — the case the
/// serve loop must survive.
const APP: &str = "module main\n\
import std.cli\n\
import pkg.web\n\
import pkg.web.types\n\
\n\
fn list_models(c: pkg.web.types.Ctx) -> Result<response_builder, Error> {\n\
  return pkg.web.json(\"{\\\"models\\\":[]}\")\n\
}\n\
\n\
fn get_model(c: pkg.web.types.Ctx) -> Result<response_builder, Error> {\n\
  return pkg.web.text(pkg.web.param(c, \"id\"))\n\
}\n\
\n\
fn search(c: pkg.web.types.Ctx) -> Result<response_builder, Error> {\n\
  return pkg.web.text(pkg.web.query(c, \"q\"))\n\
}\n\
\n\
fn create_model(c: pkg.web.types.Ctx) -> Result<response_builder, Error> {\n\
  return pkg.web.status(201)\n\
}\n\
\n\
fn replace_model(c: pkg.web.types.Ctx) -> Result<response_builder, Error> {\n\
  return pkg.web.status_json(202, \"{\\\"queued\\\":true}\")\n\
}\n\
\n\
fn catch_all(c: pkg.web.types.Ctx) -> Result<response_builder, Error> {\n\
  return pkg.web.text(\"any\")\n\
}\n\
\n\
fn boom(c: pkg.web.types.Ctx) -> Result<response_builder, Error> {\n\
  return Err(Error.Invalid)\n\
}\n\
\n\
fn echo(c: pkg.web.types.Ctx) -> Result<response_builder, Error> {\n\
  b := pkg.web.body(c)\n\
  if b.len() == 0 {\n\
    return pkg.web.status_text(400, \"empty\")\n\
  }\n\
  match pkg.web.body_str(c) {\n\
    Ok(s) => pkg.web.text(s)\n\
    Err(e) => pkg.web.status_text(400, \"not utf-8\")\n\
  }\n\
}\n\
\n\
pub fn main(args: array<str>) -> Result<(), Error> {\n\
  cmd := cli.command(\"srv\")\n\
  cmd.flag_i64(\"port\", 0)\n\
  p := cmd.parse(args)?\n\
  routes := [\n\
    pkg.web.get(\"/v1/models\", list_models),\n\
    pkg.web.post(\"/v1/models\", create_model),\n\
    pkg.web.get(\"/v1/models/:id\", get_model),\n\
    pkg.web.put(\"/v1/models/:id\", replace_model),\n\
    pkg.web.get(\"/search\", search),\n\
    pkg.web.get(\"/boom\", boom),\n\
    pkg.web.any(\"/health\", catch_all),\n\
    pkg.web.post(\"/echo\", echo),\n\
  ]\n\
  return pkg.web.serve(\"127.0.0.1\", p.get_i64(\"port\"), routes, 1)\n\
}\n";

/// A server child that is killed on drop — `serve` never returns, so every test must reap it.
struct Server {
    child: std::process::Child,
    port: u16,
    /// The built executable, held so its temp project (and the exe file) outlives the child.
    _built: BuiltExeMulti,
}

impl Drop for Server {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
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
        .spawn()
        .expect("spawn server");
    // A server that dies at startup (bind failure, arg parse) would otherwise show up only as a
    // 30-second connect timeout, so surface it as itself.
    std::thread::sleep(Duration::from_millis(300));
    if let Ok(Some(st)) = child.try_wait() {
        panic!("server exited at startup: {st:?}");
    }
    Server { child, port, _built: built }
}

/// Read exactly ONE `Content-Length`-framed response off a kept-alive socket, leaving later bytes in
/// `buf`. A keep-alive client cannot read to EOF — the connection stays open by design.
fn read_framed(sock: &mut TcpStream, buf: &mut Vec<u8>) -> String {
    let mut chunk = [0u8; 4096];
    loop {
        if let Some(hp) = buf.windows(4).position(|w| w == b"\r\n\r\n") {
            let head_end = hp + 4;
            let head = String::from_utf8_lossy(&buf[..head_end]).to_string();
            let cl = head
                .lines()
                .find_map(|l| l.to_ascii_lowercase().strip_prefix("content-length:").map(|v| v.trim().parse::<usize>().unwrap_or(0)))
                .unwrap_or(0);
            if buf.len() >= head_end + cl {
                let resp = String::from_utf8_lossy(&buf[..head_end + cl]).to_string();
                buf.drain(..head_end + cl);
                return resp;
            }
        }
        let n = sock.read(&mut chunk).expect("read response");
        assert!(n > 0, "server closed mid-response: {:?}", String::from_utf8_lossy(buf));
        buf.extend_from_slice(&chunk[..n]);
    }
}

/// **Keep-alive × the pkg.web serve loop** (std.http item 9 ②): the framework's loop is byte-identical
/// before and after keep-alive — `srv.accept()` simply yields the next request off the SAME connection.
/// Three requests over one socket, each routed and answered on its own, and none of the responses
/// carries a `Connection` header (absence = persistent, the 1.1 default).
#[test]
fn keep_alive_serves_many_requests_over_one_connection() {
    if !backend_available() {
        return;
    }
    let srv = start("web-root-keepalive");
    let deadline = Instant::now() + Duration::from_secs(30);
    let mut sock = loop {
        match TcpStream::connect(("127.0.0.1", srv.port)) {
            Ok(s) => break s,
            Err(_) if Instant::now() < deadline => std::thread::sleep(Duration::from_millis(25)),
            Err(e) => panic!("server never came up: {e}"),
        }
    };
    sock.set_read_timeout(Some(Duration::from_secs(30))).expect("read timeout");
    let mut buf = Vec::new();
    let mut resps = Vec::new();
    for req in [
        &b"GET /v1/models HTTP/1.1\r\nHost: h\r\n\r\n"[..],
        &b"GET /v1/models/42 HTTP/1.1\r\nHost: h\r\n\r\n"[..],
        &b"GET /nope HTTP/1.1\r\nHost: h\r\n\r\n"[..],
    ] {
        sock.write_all(req).expect("write request");
        resps.push(read_framed(&mut sock, &mut buf));
    }
    assert!(resps[0].ends_with("{\"models\":[]}"), "request 1 on the connection: {:?}", resps[0]);
    assert!(resps[1].ends_with("\r\n\r\n42"), "request 2 — its OWN param capture: {:?}", resps[1]);
    assert!(resps[2].starts_with("HTTP/1.1 404 "), "request 3 — the framework 404: {:?}", resps[2]);
    for r in &resps {
        assert!(!r.to_ascii_lowercase().contains("connection:"), "a persistent response emits no Connection header: {r:?}");
    }
    // The connection is still open (parked) — a close would have shown up as an EOF read above.
    sock.set_read_timeout(Some(Duration::from_millis(400))).expect("probe timeout");
    let mut probe = [0u8; 1];
    assert!(!matches!(sock.read(&mut probe), Ok(0)), "the connection is parked, not closed");

    // A SECOND client does not cost the first its connection: the server parks a set, not one slot.
    // (With a single slot this second connect would have closed `sock`, and the request below would
    // have hit a dead socket — the failure mode a keep-alive client cannot safely retry.)
    let mut sock2 = TcpStream::connect(("127.0.0.1", srv.port)).expect("second client");
    sock2.set_read_timeout(Some(Duration::from_secs(30))).expect("read timeout");
    let mut buf2 = Vec::new();
    sock2.write_all(b"GET /health HTTP/1.1\r\nHost: h\r\n\r\n").expect("write");
    assert!(read_framed(&mut sock2, &mut buf2).ends_with("any"), "the new client is served");
    sock.set_read_timeout(Some(Duration::from_secs(30))).expect("read timeout");
    sock.write_all(b"GET /v1/models HTTP/1.1\r\nHost: h\r\n\r\n")
        .expect("the first connection must still be alive");
    assert!(read_framed(&mut sock, &mut buf).ends_with("{\"models\":[]}"), "client 1 survived client 2");
}

#[test]
fn the_serve_loop_answers_request_after_request() {
    if !backend_available() {
        return;
    }
    let srv = start("web-root");
    let port = srv.port;

    // A static GET through `web.json`.
    let list = exchange(port, b"GET /v1/models HTTP/1.1\r\nHost: h\r\n\r\n");
    assert!(list.starts_with("HTTP/1.1 200 OK\r\n"), "list: {list:?}");
    assert!(
        list.contains("Content-Type: application/json\r\n"),
        "list content type: {list:?}"
    );
    assert!(list.ends_with("{\"models\":[]}"), "list body: {list:?}");

    // The same PATH with a different METHOD reaches the other handler — method-aware dispatch.
    let created = exchange(port, b"POST /v1/models HTTP/1.1\r\nHost: h\r\n\r\n");
    assert!(
        created.starts_with("HTTP/1.1 201 "),
        "created: {created:?}"
    );

    // A `:id` capture read back out of the pattern + path, zero-copy into the response body.
    let one = exchange(port, b"GET /v1/models/42 HTTP/1.1\r\nHost: h\r\n\r\n");
    assert!(one.starts_with("HTTP/1.1 200 OK\r\n"), "one: {one:?}");
    assert!(
        one.contains("Content-Type: text/plain; charset=utf-8\r\n"),
        "one content type: {one:?}"
    );
    assert!(one.ends_with("\r\n\r\n42"), "one body: {one:?}");

    // A handler that just FAILS, without ever building a response. Under the old shape this hung:
    // the handler owned the request handle, so a failure left nothing to answer through. Now the
    // framework holds it and answers 500 itself, and the loop keeps serving (the request below
    // proves it).
    let boom = exchange(port, b"GET /boom HTTP/1.1\r\nHost: h\r\n\r\n");
    assert!(boom.starts_with("HTTP/1.1 500 "), "boom: {boom:?}");

    // Still alive after all of the above.
    let again = exchange(port, b"GET /v1/models HTTP/1.1\r\nHost: h\r\n\r\n");
    assert!(again.starts_with("HTTP/1.1 200 OK\r\n"), "again: {again:?}");
}

#[test]
fn an_unmatched_path_is_a_404_and_a_wrong_method_is_a_405_with_allow() {
    if !backend_available() {
        return;
    }
    let srv = start("web-root-auto");
    let port = srv.port;

    // No pattern matches at all -> 404, no `Allow` owed.
    let missing = exchange(port, b"GET /nope HTTP/1.1\r\nHost: h\r\n\r\n");
    assert!(missing.starts_with("HTTP/1.1 404 "), "404: {missing:?}");

    // The path matches but DELETE is not registered on it -> 405 carrying the real method set for
    // that pattern, in table order.
    let wrong = exchange(port, b"DELETE /v1/models HTTP/1.1\r\nHost: h\r\n\r\n");
    assert!(wrong.starts_with("HTTP/1.1 405 "), "405: {wrong:?}");
    assert!(
        wrong.contains("Allow: GET, POST\r\n"),
        "405 allow set: {wrong:?}"
    );

    // A parameterised pattern gets the same treatment — the method set comes from its own row
    // (GET + PUT), not from the static sibling's (GET + POST).
    let wrong_param = exchange(port, b"DELETE /v1/models/42 HTTP/1.1\r\nHost: h\r\n\r\n");
    assert!(
        wrong_param.starts_with("HTTP/1.1 405 "),
        "405 param: {wrong_param:?}"
    );
    assert!(
        wrong_param.contains("Allow: GET, PUT\r\n"),
        "405 param allow set: {wrong_param:?}"
    );
}

#[test]
fn an_any_route_answers_every_method_and_status_json_carries_its_code() {
    if !backend_available() {
        return;
    }
    let srv = start("web-root-any");
    let port = srv.port;

    // `web.any` registers an EMPTY method, which `dispatch_routes` treats as matching anything —
    // the catch-all form. Both a GET and a method nothing else in the table uses reach it.
    for verb in [&b"GET"[..], &b"DELETE"[..]] {
        let mut req = verb.to_vec();
        req.extend_from_slice(b" /health HTTP/1.1\r\nHost: h\r\n\r\n");
        let resp = exchange(port, &req);
        assert!(
            resp.starts_with("HTTP/1.1 200 OK\r\n"),
            "any {:?}: {resp:?}",
            String::from_utf8_lossy(verb)
        );
        assert!(resp.ends_with("\r\n\r\nany"), "any body: {resp:?}");
    }

    // `web.put` reaches its own handler on a pattern it SHARES with a GET route, and
    // `status_json` sends a non-200 code with the JSON content type.
    let queued = exchange(port, b"PUT /v1/models/7 HTTP/1.1\r\nHost: h\r\n\r\n");
    assert!(queued.starts_with("HTTP/1.1 202 "), "queued: {queued:?}");
    assert!(
        queued.contains("Content-Type: application/json\r\n"),
        "queued content type: {queued:?}"
    );
    assert!(queued.ends_with("{\"queued\":true}"), "queued body: {queued:?}");
}

#[test]
fn a_query_string_does_not_break_routing() {
    if !backend_available() {
        return;
    }
    // REGRESSION. `ctx.path()` returns the raw request-TARGET, query string and all, and the first
    // version of `serve` matched routes against it directly — so `/v1/models?limit=5` did not match
    // the pattern `/v1/models` and every route answered 404 the moment a client sent a query. This
    // is the shape essentially every real REST client uses.
    let srv = start("web-root-query");
    let port = srv.port;

    let listed = exchange(port, b"GET /v1/models?limit=5 HTTP/1.1\r\nHost: h\r\n\r\n");
    assert!(
        listed.starts_with("HTTP/1.1 200 OK\r\n"),
        "a query string must not change which route matches: {listed:?}"
    );

    // A `:param` capture still reads the path half only — the query must not leak into it.
    let one = exchange(port, b"GET /v1/models/42?verbose=1 HTTP/1.1\r\nHost: h\r\n\r\n");
    assert!(one.starts_with("HTTP/1.1 200 OK\r\n"), "param+query: {one:?}");
    assert!(one.ends_with("\r\n\r\n42"), "the capture must be `42`, not `42?verbose=1`: {one:?}");

    // And the query half is what `web.query` reads.
    let found = exchange(port, b"GET /search?q=hello&n=2 HTTP/1.1\r\nHost: h\r\n\r\n");
    assert!(found.starts_with("HTTP/1.1 200 OK\r\n"), "search: {found:?}");
    assert!(found.ends_with("\r\n\r\nhello"), "query value: {found:?}");

    // No query at all: the whole target is the path, and `web.query` finds nothing.
    let empty = exchange(port, b"GET /search HTTP/1.1\r\nHost: h\r\n\r\n");
    assert!(empty.starts_with("HTTP/1.1 200 OK\r\n"), "no query: {empty:?}");
    assert!(empty.ends_with("\r\n\r\n"), "an absent query reads as empty: {empty:?}");
}

/// W3 body accessors: `web.body(c)` (raw byte view) and `web.body_str(c)` (UTF-8-validated view),
/// driven end-to-end — the body is a zero-copy view carried in the Copy `Ctx`, so a handler reads
/// it while `serve` still owns the request handle.
#[test]
fn body_and_body_str_read_the_request_body() {
    if !backend_available() {
        return;
    }
    let srv = start("web-root-body");
    let port = srv.port;

    // A UTF-8 body echoes back through `body_str` -> `web.text`.
    let ok = exchange(port, b"POST /echo HTTP/1.1\r\nHost: h\r\nContent-Length: 10\r\n\r\nhello body");
    assert!(ok.starts_with("HTTP/1.1 200 OK\r\n"), "echo: {ok:?}");
    assert!(ok.ends_with("\r\n\r\nhello body"), "the body must round-trip: {ok:?}");

    // A bodyless POST reads as an EMPTY view (`web.body(c).len() == 0`), answered 400 by the handler.
    let empty = exchange(port, b"POST /echo HTTP/1.1\r\nHost: h\r\nContent-Length: 0\r\n\r\n");
    assert!(empty.starts_with("HTTP/1.1 400 Bad Request\r\n"), "empty body: {empty:?}");
    assert!(empty.ends_with("\r\n\r\nempty"), "the handler's empty-body answer: {empty:?}");

    // Invalid UTF-8 bytes: `web.body(c)` sees them (len > 0), `web.body_str(c)` returns Err, and
    // the handler answers 400 — the validating view is the boundary, not an abort.
    let bad = exchange(port, b"POST /echo HTTP/1.1\r\nHost: h\r\nContent-Length: 2\r\n\r\n\xff\xfe");
    assert!(bad.starts_with("HTTP/1.1 400 Bad Request\r\n"), "invalid utf-8: {bad:?}");
    assert!(bad.ends_with("\r\n\r\nnot utf-8"), "the handler's utf-8 answer: {bad:?}");

    // The server survived all three (the loop is still up) — a GET after them still answers.
    let alive = exchange(port, b"GET /health HTTP/1.1\r\nHost: h\r\n\r\n");
    assert!(alive.starts_with("HTTP/1.1 200 OK\r\n"), "server alive after body requests: {alive:?}");
}

/// W4: HEAD is GET without the response body (RFC 9110 §9.3.2) — an explicit HEAD route wins,
/// otherwise the path's GET handler answers with the body suppressed by std.http's `respond`
/// (Content-Length kept); and the automatic 404/405/500 responses carry the error policy's fixed
/// minimal JSON bodies.
#[test]
fn head_is_get_without_the_body_and_auto_responses_carry_json() {
    if !backend_available() {
        return;
    }
    let srv = start("web-root-head");
    let port = srv.port;

    // HEAD on a GET route: the GET handler runs, the head is byte-identical to the GET response's
    // head — Content-Type and the body's Content-Length included — and NO body bytes follow.
    let get = exchange(port, b"GET /v1/models HTTP/1.1\r\nHost: h\r\n\r\n");
    let head = exchange(port, b"HEAD /v1/models HTTP/1.1\r\nHost: h\r\n\r\n");
    assert!(head.starts_with("HTTP/1.1 200 OK\r\n"), "head: {head:?}");
    assert!(head.contains("Content-Length: 13\r\n"), "HEAD keeps the body's CL: {head:?}");
    assert!(head.ends_with("\r\n\r\n"), "HEAD sends no body bytes: {head:?}");
    assert!(get.strip_suffix("{\"models\":[]}") == Some(head.as_str()), "HEAD = GET minus the body: {get:?} vs {head:?}");

    // A path whose pattern has no GET row keeps HEAD at 405 (no fallback to invent one).
    let no_get = exchange(port, b"HEAD /echo HTTP/1.1\r\nHost: h\r\n\r\n");
    assert!(no_get.starts_with("HTTP/1.1 405 "), "HEAD on a POST-only path: {no_get:?}");
    assert!(no_get.contains("Allow: POST\r\n"), "405 allow: {no_get:?}");

    // The automatic responses carry fixed minimal JSON bodies (the design's error policy).
    let missing = exchange(port, b"GET /nope HTTP/1.1\r\nHost: h\r\n\r\n");
    assert!(missing.contains("Content-Type: application/json\r\n"), "404 CT: {missing:?}");
    assert!(missing.ends_with("{\"error\":\"not found\"}"), "404 body: {missing:?}");
    let wrong = exchange(port, b"DELETE /v1/models HTTP/1.1\r\nHost: h\r\n\r\n");
    assert!(wrong.contains("Allow: GET, POST\r\n"), "405 keeps Allow: {wrong:?}");
    assert!(wrong.ends_with("{\"error\":\"method not allowed\"}"), "405 body: {wrong:?}");
    let boom = exchange(port, b"GET /boom HTTP/1.1\r\nHost: h\r\n\r\n");
    assert!(boom.ends_with("{\"error\":\"internal error\"}"), "500 body: {boom:?}");

    // A HEAD to a missing path: the 404's JSON body is suppressed too, its CL kept.
    let head_missing = exchange(port, b"HEAD /nope HTTP/1.1\r\nHost: h\r\n\r\n");
    assert!(head_missing.starts_with("HTTP/1.1 404 "), "HEAD 404: {head_missing:?}");
    assert!(head_missing.contains("Content-Length: 21\r\n"), "HEAD 404 CL: {head_missing:?}");
    assert!(head_missing.ends_with("\r\n\r\n"), "HEAD 404 has no body: {head_missing:?}");
}
