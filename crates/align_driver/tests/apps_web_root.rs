//! pkg.web W2 — the real `pkg.web` root: route constructors, `serve`'s accept loop, and the
//! responders.
//!
//! What this pins is the whole designed surface running as one program: an application builds a
//! route table out of `web.get`/`web.post`, hands it to `web.serve`, and writes handlers that
//! respond through `web.text` / `web.json` / `web.status`. `serve` owns the accept loop, so the
//! automatic responses are the framework's, not the app's — a 405 carries the `Allow` set for the
//! matched path, and an unmatched path is a 404. Unlike `apps_web_serve.rs` (one request, then
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

fn free_loopback_port() -> u16 {
    let probe = std::net::TcpListener::bind("127.0.0.1:0").expect("bind probe");
    let port = probe.local_addr().unwrap().port();
    drop(probe);
    port
}

/// One request over its own connection (v1 closes after each response), retrying the connect until
/// the server is up.
fn exchange(port: u16, req: &[u8]) -> String {
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
import std.http\n\
import pkg.web\n\
import pkg.web.types\n\
\n\
fn list_models(c: pkg.web.types.Ctx) -> Result<(), Error> {\n\
  return pkg.web.json(c, \"{\\\"models\\\":[]}\")\n\
}\n\
\n\
fn get_model(c: pkg.web.types.Ctx) -> Result<(), Error> {\n\
  rb := http.response(200)\n\
  rb.header(\"Content-Type\", \"text/plain; charset=utf-8\")\n\
  rb.body(pkg.web.param(c.pattern, c.req.path(), \"id\"))\n\
  return c.req.respond(rb)\n\
}\n\
\n\
fn create_model(c: pkg.web.types.Ctx) -> Result<(), Error> {\n\
  return pkg.web.status(c, 201)\n\
}\n\
\n\
fn boom(c: pkg.web.types.Ctx) -> Result<(), Error> {\n\
  pkg.web.status_text(c, 500, \"boom\")?\n\
  return Err(Error.Invalid)\n\
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
    pkg.web.get(\"/boom\", boom),\n\
  ]\n\
  return pkg.web.serve(\"127.0.0.1\", p.get_i64(\"port\"), routes)\n\
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

    // A handler that responds and THEN fails: the client still gets its response, and the loop
    // keeps serving (the request below proves it).
    let boom = exchange(port, b"GET /boom HTTP/1.1\r\nHost: h\r\n\r\n");
    assert!(boom.starts_with("HTTP/1.1 500 "), "boom: {boom:?}");
    assert!(boom.ends_with("\r\n\r\nboom"), "boom body: {boom:?}");

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

    // A parameterised pattern gets the same treatment — the method set comes from its own row, not
    // from the static sibling's.
    let wrong_param = exchange(port, b"PUT /v1/models/42 HTTP/1.1\r\nHost: h\r\n\r\n");
    assert!(
        wrong_param.starts_with("HTTP/1.1 405 "),
        "405 param: {wrong_param:?}"
    );
    assert!(
        wrong_param.contains("Allow: GET\r\n"),
        "405 param allow set: {wrong_param:?}"
    );
}
