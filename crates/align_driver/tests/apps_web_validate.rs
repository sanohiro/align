//! pkg.web W4 — startup route-table validation: a malformed table is a PROGRAMMER error, printed
//! to stderr and aborted (`process.abort`, exit 1) BEFORE the port is bound — the design's error
//! policy ("tree-build abort is a startup abort, not Err"). Per-route: a known uppercase method
//! (or "" = any), a leading-`/` pattern, named `:`/`*` segments, `*` only in the tail, no
//! parameter name twice. Per-pair: no duplicate path claim the later row can never win — same
//! method twice (also the source of a duplicated 405 `Allow` join), or anything after an
//! any-method route on that claim. Names in the message pin WHICH route is wrong.
//!
//! The rejects run the real framework source (`include_str!`) and assert the process EXITS with
//! the diagnosis — no socket is ever bound; the accept case proves a specific-then-any table (the
//! legal shadowing direction) still serves.

mod common;
use common::*;

use std::io::Read;
use std::time::{Duration, Instant};

const ROUTER: &str = include_str!("../../../apps/web/pkg/web/internal/router.align");
const TYPES: &str = include_str!("../../../apps/web/pkg/web/types.align");
const WEB_ROOT: &str = include_str!("../../../apps/web/pkg/web.align");
const QUERY: &str = include_str!("../../../apps/web/pkg/web/internal/query.align");

/// A `main` serving `routes_src` on a port that is never reached for the reject cases.
fn app(routes_src: &str) -> String {
    format!(
        "module main\n\
import pkg.web\n\
import pkg.web.types\n\
\n\
fn h(c: pkg.web.types.Ctx) -> Result<response_builder, Error> {{\n\
  return pkg.web.text(\"ok\")\n\
}}\n\
\n\
pub fn main() -> Result<(), Error> {{\n\
  routes := [\n\
{routes_src}\n\
  ]\n\
  return pkg.web.serve(\"127.0.0.1\", 0, routes)\n\
}}\n"
    )
}

/// Build + spawn the server and wait (bounded) for it to exit; return (status, stderr).
fn run_expect_exit(name: &str, routes_src: &str) -> (std::process::ExitStatus, String) {
    let main_src = app(routes_src);
    let built = build_exe_multi(
        name,
        &[
            ("pkg/web/internal/router.align", ROUTER),
            ("pkg/web/internal/query.align", QUERY),
            ("pkg/web/types.align", TYPES),
            ("pkg/web.align", WEB_ROOT),
            ("main.align", &main_src),
        ],
        "main.align",
    );
    let mut child = std::process::Command::new(&built.exe)
        .stderr(std::process::Stdio::piped())
        .spawn()
        .expect("spawn server");
    let deadline = Instant::now() + Duration::from_secs(10);
    let status = loop {
        match child.try_wait().expect("try_wait") {
            Some(st) => break st,
            None if Instant::now() < deadline => std::thread::sleep(Duration::from_millis(25)),
            None => {
                let _ = child.kill();
                let _ = child.wait();
                panic!("server should have aborted at startup but is still running");
            }
        }
    };
    let mut err = String::new();
    child.stderr.take().expect("stderr piped").read_to_string(&mut err).expect("read stderr");
    (status, err)
}

fn assert_aborts(name: &str, routes_src: &str, msg_needle: &str) {
    let (status, err) = run_expect_exit(name, routes_src);
    assert!(!status.success(), "{name}: a malformed table must abort, got {status:?}");
    assert!(
        err.contains("pkg.web: route") && err.contains(msg_needle),
        "{name}: the diagnosis must name the problem ({msg_needle:?}), got stderr {err:?}"
    );
}

#[test]
fn malformed_tables_abort_at_startup_with_a_diagnosis() {
    if !backend_available() {
        return;
    }
    // A lowercase method would compare byte-exactly against "GET" and never match — the exact
    // recorded gap (`web.route("get", ...)` silently dead).
    assert_aborts(
        "web-val-method",
        "    pkg.web.route(\"get\", \"/x\", h),",
        "unknown method",
    );
    // A pattern without the leading slash never matches a request path.
    assert_aborts("web-val-slash", "    pkg.web.get(\"x\", h),", "must start with \"/\"");
    // A nameless parameter segment cannot be read back by `web.param`.
    assert_aborts("web-val-name", "    pkg.web.get(\"/a/:\", h),", "needs a name");
    // The walker treats a wildcard as tail-absorbing; an interior one is a contradiction.
    assert_aborts(
        "web-val-tail",
        "    pkg.web.get(\"/a/*rest/b\", h),",
        "must be the last segment",
    );
    // The same name twice makes `param(c, \"x\")` ambiguous.
    assert_aborts(
        "web-val-dupname",
        "    pkg.web.get(\"/a/:x/:x\", h),",
        "the same parameter name twice",
    );
    // Same method + same claim: the later row can never win (first-registered wins) — and it is
    // exactly what duplicated the 405 `Allow` join ("GET, GET").
    assert_aborts(
        "web-val-dup",
        "    pkg.web.get(\"/x\", h),\n    pkg.web.get(\"/x\", h),",
        "duplicate route",
    );
    // Parameter NAMES don't change what a pattern claims: `/a/:y` after `/a/:x` is dead weight.
    assert_aborts(
        "web-val-claim",
        "    pkg.web.get(\"/a/:x\", h),\n    pkg.web.get(\"/a/:y\", h),",
        "duplicate route",
    );
    // An any-method route answers everything on its claim first; a later specific row is dead.
    assert_aborts(
        "web-val-shadow",
        "    pkg.web.any(\"/x\", h),\n    pkg.web.get(\"/x\", h),",
        "unreachable route",
    );
}

/// The legal shadowing direction still serves: a specific route first, `any` as the fallback on
/// the same pattern — the fallback catches the OTHER methods, so both rows are reachable.
#[test]
fn specific_then_any_on_one_pattern_is_legal_and_serves() {
    if !backend_available() {
        return;
    }
    use std::io::Write;
    let main_src = "module main\n\
import std.cli\n\
import pkg.web\n\
import pkg.web.types\n\
\n\
fn getter(c: pkg.web.types.Ctx) -> Result<response_builder, Error> {\n\
  return pkg.web.text(\"get\")\n\
}\n\
\n\
fn fallback(c: pkg.web.types.Ctx) -> Result<response_builder, Error> {\n\
  return pkg.web.text(\"any\")\n\
}\n\
\n\
pub fn main(args: array<str>) -> Result<(), Error> {\n\
  cmd := cli.command(\"srv\")\n\
  cmd.flag_i64(\"port\", 0)\n\
  p := cmd.parse(args)?\n\
  routes := [\n\
    pkg.web.get(\"/x\", getter),\n\
    pkg.web.any(\"/x\", fallback),\n\
  ]\n\
  return pkg.web.serve(\"127.0.0.1\", p.get_i64(\"port\"), routes)\n\
}\n";
    let built = build_exe_multi(
        "web-val-ok",
        &[
            ("pkg/web/internal/router.align", ROUTER),
            ("pkg/web/internal/query.align", QUERY),
            ("pkg/web/types.align", TYPES),
            ("pkg/web.align", WEB_ROOT),
            ("main.align", main_src),
        ],
        "main.align",
    );
    let probe = std::net::TcpListener::bind("127.0.0.1:0").expect("bind probe");
    let port = probe.local_addr().unwrap().port();
    drop(probe);
    let mut child = std::process::Command::new(&built.exe)
        .args(["--port", &port.to_string()])
        .spawn()
        .expect("spawn server");
    let deadline = Instant::now() + Duration::from_secs(30);
    let resp = loop {
        match std::net::TcpStream::connect(("127.0.0.1", port)) {
            Ok(mut sock) => {
                sock.write_all(b"DELETE /x HTTP/1.1\r\nHost: h\r\n\r\n").expect("write");
                let mut out = Vec::new();
                let _ = sock.read_to_end(&mut out);
                break String::from_utf8_lossy(&out).into_owned();
            }
            Err(_) if Instant::now() < deadline => std::thread::sleep(Duration::from_millis(25)),
            Err(e) => {
                let _ = child.kill();
                let _ = child.wait();
                panic!("server never came up: {e}");
            }
        }
    };
    let _ = child.kill();
    let _ = child.wait();
    // DELETE bypasses the GET row and lands on the any fallback — both rows are live.
    assert!(resp.starts_with("HTTP/1.1 200 OK\r\n"), "fallback served: {resp:?}");
    assert!(resp.ends_with("\r\n\r\nany"), "the any row answered: {resp:?}");
}

/// A stream route must carry a content type: `serve` builds the stream head from `stream_type`
/// (an empty one would emit a blank `Content-Type:`), and `stream_type == ""` is the invariant
/// the HEAD→GET fallback reads as "a Respond row" — so an empty-typed stream row is a startup
/// abort, keeping the invariant total.
#[test]
fn a_stream_route_with_an_empty_content_type_aborts() {
    if !backend_available() {
        return;
    }
    let main_src = "module main\n\
import pkg.web\n\
import pkg.web.types\n\
\n\
fn pump(c: pkg.web.types.Ctx, s: http_stream) -> Result<(), Error> {\n\
  s.finish()\n\
}\n\
\n\
pub fn main() -> Result<(), Error> {\n\
  routes := [\n\
    pkg.web.stream(\"POST\", \"/x\", \"\", pump),\n\
  ]\n\
  return pkg.web.serve(\"127.0.0.1\", 0, routes)\n\
}\n";
    let built = build_exe_multi(
        "web-val-streamct",
        &[
            ("pkg/web/internal/router.align", ROUTER),
            ("pkg/web/internal/query.align", QUERY),
            ("pkg/web/types.align", TYPES),
            ("pkg/web.align", WEB_ROOT),
            ("main.align", main_src),
        ],
        "main.align",
    );
    let mut child = std::process::Command::new(&built.exe)
        .stderr(std::process::Stdio::piped())
        .spawn()
        .expect("spawn server");
    let deadline = Instant::now() + Duration::from_secs(10);
    let status = loop {
        match child.try_wait().expect("try_wait") {
            Some(st) => break st,
            None if Instant::now() < deadline => std::thread::sleep(Duration::from_millis(25)),
            None => {
                let _ = child.kill();
                let _ = child.wait();
                panic!("an empty-typed stream route must abort at startup");
            }
        }
    };
    let mut err = String::new();
    child.stderr.take().expect("stderr piped").read_to_string(&mut err).expect("read stderr");
    assert!(!status.success(), "must abort, got {status:?}");
    assert!(err.contains("stream route with an empty content type"), "diagnosis: {err:?}");
}
