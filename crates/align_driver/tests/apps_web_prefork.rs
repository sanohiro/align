//! pkg.web concurrent serve — `web.serve(host, port, routes, workers)` prefork.
//!
//! `workers >= 2` spawns that many request loops in a `task_group`, each binding its OWN
//! `SO_REUSEPORT` listener (`http.serve_shared`, std.http item 9 ①) so the kernel balances inbound
//! connections across them. There is no shared mutable state: no Move handle crosses a `spawn`, and
//! the route table travels as a Copy `slice<Route>` view.
//!
//! What this file pins is the property the sequential loop could not have: **an open stream occupies
//! exactly ONE worker while the others keep serving** — the ordering constraint that gated production
//! streaming (`docs/impl/pkg-design/web.md` → "Concurrent serve (prefork)") — plus many concurrent
//! clients all being answered by a multi-worker server.

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

/// The application: a trivial unary route and an SSE route whose pump holds its worker for a long
/// time (an event, then a slow tick loop) — the "one long stream" the other workers must survive.
const APP: &str = "module main\n\
import std.cli\n\
import std.time\n\
import pkg.web\n\
import pkg.web.types\n\
\n\
fn ping(c: pkg.web.types.Ctx) -> Result<response_builder, Error> {\n\
  return pkg.web.text(\"pong\")\n\
}\n\
\n\
fn hold(c: pkg.web.types.Ctx, s: http_stream) -> Result<(), Error> {\n\
  s.send_event(\"open\")?\n\
  mut i := 0\n\
  loop {\n\
    if i >= 100 {\n\
      break\n\
    }\n\
    time.sleep(100000000)\n\
    s.send_event(\"tick\")?\n\
    i = i + 1\n\
  }\n\
  s.finish()\n\
}\n\
\n\
pub fn main(args: array<str>) -> Result<(), Error> {\n\
  cmd := cli.command(\"srv\")\n\
  cmd.flag_i64(\"port\", 0)\n\
  cmd.flag_i64(\"workers\", 1)\n\
  p := cmd.parse(args)?\n\
  routes := [\n\
    pkg.web.get(\"/ping\", ping),\n\
    pkg.web.sse(\"/hold\", hold),\n\
  ]\n\
  return pkg.web.serve(\"127.0.0.1\", p.get_i64(\"port\"), routes, p.get_i64(\"workers\"))\n\
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

fn start(name: &str, workers: u32) -> Server {
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
        .args(["--port", &port.to_string(), "--workers", &workers.to_string()])
        .spawn()
        .expect("spawn server");
    std::thread::sleep(Duration::from_millis(300));
    if let Ok(Some(st)) = child.try_wait() {
        panic!("server exited at startup: {st:?}");
    }
    Server { child, port, _built: built }
}

/// One request over its own connection (`Connection: close`, so the read ends at EOF), retrying the
/// connect until the server is up.
fn exchange(port: u16, req: &[u8]) -> String {
    let req = one_shot(req);
    let deadline = Instant::now() + Duration::from_secs(30);
    loop {
        match TcpStream::connect(("127.0.0.1", port)) {
            Ok(mut sock) => {
                sock.set_read_timeout(Some(Duration::from_secs(30))).expect("read timeout");
                sock.write_all(&req).expect("write request");
                let mut resp = Vec::new();
                let _ = sock.read_to_end(&mut resp);
                return String::from_utf8_lossy(&resp).into_owned();
            }
            Err(_) if Instant::now() < deadline => std::thread::sleep(Duration::from_millis(25)),
            Err(e) => panic!("server never came up: {e}"),
        }
    }
}

/// Four workers, sixteen concurrent clients: every request is answered. With one shared port and
/// four `SO_REUSEPORT` listeners, the kernel spreads the connections; the assertion is simply that
/// none of them is lost or refused.
#[test]
fn prefork_workers_answer_concurrent_clients() {
    if !backend_available() {
        return;
    }
    let srv = start("web-prefork", 4);
    let port = srv.port;
    let clients: Vec<_> = (0..16)
        .map(|_| std::thread::spawn(move || exchange(port, b"GET /ping HTTP/1.1\r\nHost: h\r\n\r\n")))
        .collect();
    for (i, c) in clients.into_iter().enumerate() {
        let resp = c.join().expect("client thread");
        assert!(resp.starts_with("HTTP/1.1 200 OK\r\n"), "client {i}: {resp:?}");
        assert!(resp.ends_with("pong"), "client {i} body: {resp:?}");
    }
}

/// **The ordering constraint, lifted.** A held-open SSE stream occupies exactly its own worker; the
/// remaining `W - 1` keep answering. The stream client reads its first event (so the pump is
/// provably mid-generation), then plain requests are served while it is still open.
#[test]
fn a_held_open_stream_occupies_one_worker_while_the_others_serve() {
    if !backend_available() {
        return;
    }
    let srv = start("web-prefork-stream", 3);
    let port = srv.port;
    // Open the stream and read its first event — after this the pump is inside its tick loop,
    // holding one worker for ~10s.
    let deadline = Instant::now() + Duration::from_secs(30);
    let mut stream_sock = loop {
        match TcpStream::connect(("127.0.0.1", port)) {
            Ok(s) => break s,
            Err(_) if Instant::now() < deadline => std::thread::sleep(Duration::from_millis(25)),
            Err(e) => panic!("server never came up: {e}"),
        }
    };
    stream_sock.set_read_timeout(Some(Duration::from_secs(30))).expect("read timeout");
    stream_sock.write_all(b"GET /hold HTTP/1.1\r\nHost: h\r\n\r\n").expect("write");
    let mut seen = Vec::new();
    let mut chunk = [0u8; 1024];
    while !seen.windows(6).any(|w| w == b"open\n\n") {
        let n = stream_sock.read(&mut chunk).expect("read stream head + first event");
        assert!(n > 0, "the stream closed before its first event: {:?}", String::from_utf8_lossy(&seen));
        seen.extend_from_slice(&chunk[..n]);
    }
    let head = String::from_utf8_lossy(&seen).into_owned();
    assert!(head.contains("Content-Type: text/event-stream\r\n"), "SSE head: {head:?}");

    // While that stream is still open, ordinary requests keep being answered — on other workers.
    for i in 0..4 {
        let resp = exchange(port, b"GET /ping HTTP/1.1\r\nHost: h\r\n\r\n");
        assert!(resp.ends_with("pong"), "request {i} during the open stream: {resp:?}");
    }
    // The stream is STILL live (the pump is ticking, nothing closed it).
    let n = stream_sock.read(&mut chunk).expect("the held stream is still delivering");
    assert!(n > 0, "the stream must still be open after the other requests");
    drop(stream_sock);
}

/// `workers < 1` is a programmer-config error, aborted at startup like a malformed route table —
/// before anything binds.
#[test]
fn a_worker_count_below_one_aborts_at_startup() {
    if !backend_available() {
        return;
    }
    let built = build_exe_multi(
        "web-prefork-zero",
        &[
            ("pkg/web/internal/router.align", ROUTER),
            ("pkg/web/internal/query.align", QUERY),
            ("pkg/web/types.align", TYPES),
            ("pkg/web.align", WEB_ROOT),
            ("main.align", APP),
        ],
        "main.align",
    );
    let out = std::process::Command::new(&built.exe)
        .args(["--port", &free_loopback_port().to_string(), "--workers", "0"])
        .output()
        .expect("run");
    assert!(!out.status.success(), "a zero worker count must abort: {out:?}");
    let err = String::from_utf8_lossy(&out.stderr);
    assert!(err.contains("workers must be at least 1"), "diagnosis names the problem: {err:?}");
}
