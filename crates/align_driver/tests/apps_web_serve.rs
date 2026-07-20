//! pkg.web W2 — the request context against a real server.
//!
//! What this pins, end to end over a real socket: the request context is a struct that **owns** the
//! `http_request_ctx` handle (F1②) alongside the matched route pattern; the radix router picks the
//! route; `param_value` reads the `:id` capture as a zero-copy view out of the handle's path; and the
//! responder **consumes the handle out of that struct field** — the partial move, with the field
//! nulled so the struct's drop does not double-free it. A clean server exit is itself the assertion:
//! a double-freed request handle would abort the process.
//!
//! This is the DESIGNED contract, end to end: `Route.handler` is `fn(Ctx) -> Result<(), Error>`, the
//! route table holds it in a fn-value field, and the matched handler is invoked through that field
//! (`r.handler(c)`) — nothing here is a stand-in for a missing compiler feature.

mod common;
use common::*;

use std::io::{Read, Write};
use std::net::TcpStream;
use std::time::{Duration, Instant};

const ROUTER: &str = include_str!("../../../apps/web/pkg/web/internal/router.align");

/// A minimal `pkg.web` root wiring the internal router to the public shapes this test drives.
/// The
/// full surface (`get`/`post`/`serve`/`json`/…) is the rest of W2; what is pinned here is that the
/// *shapes* compose — `Ctx` owning the handle, `Route` carrying a fn value, dispatch, param reads,
/// and a consuming respond.
const WEB_ROOT: &str = "module pkg.web\n\
import pkg.web.internal.router\n\
pub Ctx { req: http_request_ctx, pattern: str }\n\
pub Route { pattern: str, handler: fn(Ctx) -> Result<(), Error> }\n\
pub fn dispatch(patterns: slice<str>, path: str) -> i64 = pkg.web.internal.router.tree_dispatch(patterns, path)\n\
pub fn param(pattern: str, path: str, name: str) -> str = pkg.web.internal.router.param_value(pattern, path, name)\n";

fn free_loopback_port() -> u16 {
    let probe = std::net::TcpListener::bind("127.0.0.1:0").expect("bind probe");
    let port = probe.local_addr().unwrap().port();
    drop(probe);
    port
}

fn client_exchange(port: u16, req: &[u8]) -> Vec<u8> {
    let deadline = Instant::now() + Duration::from_secs(30);
    loop {
        match TcpStream::connect(("127.0.0.1", port)) {
            Ok(mut sock) => {
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

/// The server: build a route table of `Route` values (fn-value fields), accept one request, dispatch
/// its path through the radix tree, wrap the accepted handle in a `Ctx` with the matched pattern,
/// and call the winning handler — which reads the `:id` param and responds by consuming the handle.
const SERVER: &str = "module main\n\
import std.http\n\
import std.cli\n\
import pkg.web\n\
\n\
fn get_model(c: pkg.web.Ctx) -> Result<(), Error> {\n\
  id := pkg.web.param(c.pattern, c.req.path(), \"id\")\n\
  rb := http.response(200)\n\
  rb.header(\"X-Route\", \"model\")\n\
  rb.body(id)\n\
  return c.req.respond(rb)\n\
}\n\
\n\
fn list_models(c: pkg.web.Ctx) -> Result<(), Error> {\n\
  rb := http.response(200)\n\
  rb.header(\"X-Route\", \"list\")\n\
  rb.body(\"all\")\n\
  return c.req.respond(rb)\n\
}\n\
\n\
fn not_found(c: pkg.web.Ctx) -> Result<(), Error> {\n\
  rb := http.response(404)\n\
  rb.body(\"nope\")\n\
  return c.req.respond(rb)\n\
}\n\
\n\
pub fn main(args: array<str>) -> Result<(), Error> {\n\
  cmd := cli.command(\"srv\")\n\
  cmd.flag_i64(\"port\", 0)\n\
  p := cmd.parse(args)?\n\
  routes := [\n\
    pkg.web.Route { pattern: \"/v1/models\", handler: list_models },\n\
    pkg.web.Route { pattern: \"/v1/models/:id\", handler: get_model },\n\
  ]\n\
  patterns := [\"/v1/models\", \"/v1/models/:id\"]\n\
  srv := http.serve(\"127.0.0.1\", p.get_i64(\"port\"))?\n\
  ctx := srv.accept()?\n\
  idx := pkg.web.dispatch(patterns, ctx.path())\n\
  if idx < 0 {\n\
    c := pkg.web.Ctx { req: ctx, pattern: \"\" }\n\
    return not_found(c)\n\
  }\n\
  r := routes[idx]\n\
  c := pkg.web.Ctx { req: ctx, pattern: r.pattern }\n\
  return r.handler(c)\n\
}\n";

fn run_server(name: &str, request: &[u8]) -> String {
    let port = free_loopback_port();
    let server = build_exe_multi(
        name,
        &[
            ("pkg/web/internal/router.align", ROUTER),
            ("pkg/web.align", WEB_ROOT),
            ("main.align", SERVER),
        ],
        "main.align",
    );
    let mut child = std::process::Command::new(&server.exe)
        .args(["--port", &port.to_string()])
        .spawn()
        .expect("spawn server");
    let resp = client_exchange(port, request);
    let status = child.wait().expect("server exits");
    // A double-freed request handle (the consuming respond on a struct field) would abort here.
    assert!(status.success(), "server exited with {status:?}");
    String::from_utf8_lossy(&resp).into_owned()
}

#[test]
fn param_route_dispatches_and_responds() {
    if !backend_available() {
        return;
    }
    // `/v1/models/42` matches the `:id` route; the handler reads the capture through the Ctx's
    // pattern + the handle's path and echoes it as the body.
    let text = run_server("w2-param", b"GET /v1/models/42 HTTP/1.1\r\nHost: h\r\n\r\n");
    assert!(text.starts_with("HTTP/1.1 200 OK\r\n"), "status: {text:?}");
    assert!(text.contains("X-Route: model\r\n"), "route: {text:?}");
    assert!(text.ends_with("\r\n\r\n42"), "captured id as body: {text:?}");
}

#[test]
fn static_route_beats_param_and_responds() {
    if !backend_available() {
        return;
    }
    // The static `/v1/models` wins over the `:id` route for the exact path.
    let text = run_server("w2-static", b"GET /v1/models HTTP/1.1\r\nHost: h\r\n\r\n");
    assert!(text.starts_with("HTTP/1.1 200 OK\r\n"), "status: {text:?}");
    assert!(text.contains("X-Route: list\r\n"), "route: {text:?}");
    assert!(text.ends_with("\r\n\r\nall"), "body: {text:?}");
}

#[test]
fn unmatched_path_takes_the_fallback() {
    if !backend_available() {
        return;
    }
    // No route matches, so dispatch returns -1 and the fallback responds 404 — still consuming the
    // handle exactly once.
    let text = run_server("w2-404", b"GET /nope HTTP/1.1\r\nHost: h\r\n\r\n");
    assert!(text.starts_with("HTTP/1.1 404 "), "status: {text:?}");
    assert!(text.ends_with("\r\n\r\nnope"), "body: {text:?}");
}
