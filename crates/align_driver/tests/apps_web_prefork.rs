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

/// Serializes every test in this file. Two are needed:
/// - `SO_REUSEPORT` makes a port collision SILENT. `free_loopback_port` can hand the same port to
///   two concurrent tests; before `serve_shared` the loser failed loudly with `EADDRINUSE`, but now
///   both bind it, the kernel splits connections between two unrelated servers, and the listener
///   count sees both.
/// - `listening_sockets` scans the machine-wide `/proc/net/tcp`, so a sibling test's listeners on a
///   colliding port would inflate it.
static PREFORK_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

/// How many workers this machine can actually run: `task_group` dispatches onto a pool sized by the
/// available parallelism plus the calling thread, and `web.serve` aborts above that — so a test that
/// asks for more than the runner has would (correctly) fail to start.
fn max_workers() -> u32 {
    std::thread::available_parallelism().map_or(1, |n| n.get() as u32) + 1
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
    let _serial = PREFORK_LOCK.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
    let workers = 4.min(max_workers());
    let srv = start("web-prefork", workers);
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
    let _serial = PREFORK_LOCK.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
    // Needs at least two workers for the property to mean anything: one holds the stream, another
    // answers. A single-core runner cannot demonstrate it — skip rather than assert nothing.
    if max_workers() < 2 {
        return;
    }
    let srv = start("web-prefork-stream", 3.min(max_workers()));
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
    // Deliberately NOT timed: `SO_REUSEPORT` picks a listener by 4-tuple hash, so a given connection
    // MAY land on the busy worker's accept queue and wait for the stream to end (prefork's honest
    // semantics, shared with nginx/fasthttp under a blocking handler). Asserting a latency bound
    // here would be asserting the kernel's hash. What must hold — and does — is that the server as a
    // whole keeps answering while a stream is mid-generation, which the sequential loop could not.
    for i in 0..4 {
        let resp = exchange(port, b"GET /ping HTTP/1.1\r\nHost: h\r\n\r\n");
        assert!(resp.ends_with("pong"), "request {i} during the open stream: {resp:?}");
    }
    // The stream is STILL live (the pump is ticking, nothing closed it).
    let n = stream_sock.read(&mut chunk).expect("the held stream is still delivering");
    assert!(n > 0, "the stream must still be open after the other requests");
    drop(stream_sock);
}

/// How many sockets are listening on `port` right now — one per prefork worker, since each binds
/// its own `SO_REUSEPORT` listener. Read from `/proc/net/tcp{,6}` (Linux only; the caller skips
/// elsewhere): a LISTEN row (state `0A`) whose local port matches.
#[cfg(target_os = "linux")]
fn listening_sockets(port: u16) -> usize {
    let want = format!(":{port:04X}");
    ["/proc/net/tcp", "/proc/net/tcp6"]
        .iter()
        .filter_map(|p| std::fs::read_to_string(p).ok())
        .flat_map(|s| s.lines().map(str::to_string).collect::<Vec<_>>())
        .filter(|line| {
            let mut f = line.split_whitespace();
            let (_, local, _, state) = (f.next(), f.next(), f.next(), f.next());
            local.is_some_and(|l| l.to_ascii_uppercase().ends_with(&want)) && state == Some("0A")
        })
        .count()
}

/// The worker count is REAL: `serve(..., W)` binds exactly `W` listeners on the port. Without this,
/// every other test in this file would pass on a single-worker server — `task_group` dispatches onto
/// a bounded pool, so "spawned `W` tasks" does not by itself mean "`W` request loops are running".
#[test]
#[cfg(target_os = "linux")]
fn each_worker_binds_its_own_listener() {
    if !backend_available() {
        return;
    }
    let _serial = PREFORK_LOCK.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
    for workers in [1u32, 2, 4].into_iter().filter(|w| *w <= max_workers()) {
        let srv = start(&format!("web-prefork-count-{workers}"), workers);
        // The server is up (start() already waited); confirm it answers, then count the listeners.
        assert!(exchange(srv.port, b"GET /ping HTTP/1.1\r\nHost: h\r\n\r\n").ends_with("pong"));
        assert_eq!(
            listening_sockets(srv.port),
            workers as usize,
            "serve(..., {workers}) must bind {workers} listeners"
        );
    }
}

/// `workers < 1` is a programmer-config error, aborted at startup like a malformed route table —
/// before anything binds. So is a count above the available parallelism: those workers would never
/// start (the pool is sized by `process.cpu_count()`), and silently serving with fewer request loops
/// than the call site says would break the promise the parameter makes.
#[test]
fn a_worker_count_outside_the_runnable_range_aborts_at_startup() {
    if !backend_available() {
        return;
    }
    let _serial = PREFORK_LOCK.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
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

    // Above the available parallelism: `std::thread::available_parallelism` is what both
    // `process.cpu_count()` and the task pool read, and no machine reports 100k cores.
    let over = std::process::Command::new(&built.exe)
        .args(["--port", &free_loopback_port().to_string(), "--workers", "100000"])
        .output()
        .expect("run");
    assert!(!over.status.success(), "more workers than cores must abort: {over:?}");
    let over_err = String::from_utf8_lossy(&over.stderr);
    assert!(
        over_err.contains("exceeds what this machine can run"),
        "diagnosis names the cap: {over_err:?}"
    );
}
