//! Protocol-neutral `pkg.web` Upgrade dispatch owner.
//!
//! `apps_ws` owns RFC 6455. This suite owns the shared framework branches that exist before and
//! after a protocol package: route shape/grouping, middleware, prepare decisions, transfer
//! fallback, and pump diagnostics.

mod common;
use common::*;

use std::io::{Read, Write};
use std::net::TcpStream;
use std::os::fd::AsRawFd;
use std::time::{Duration, Instant};

const ROUTER: &str = include_str!("../../../apps/web/pkg/web/internal/router.align");
const TYPES: &str = include_str!("../../../apps/web/pkg/web/types.align");
const WEB_ROOT: &str = include_str!("../../../apps/web/pkg/web.align");
const QUERY: &str = include_str!("../../../apps/web/pkg/web/internal/query.align");

const APP: &str = r#"module main
import std.cli
import std.http
import pkg.web
import pkg.web.types

fn decision(c: pkg.web.types.Ctx, values: slice<str>) -> pkg.web.types.UpgradeDecision {
  value := values[0]
  if value == "reject" {
    match pkg.web.status_text(403, "rejected") {
      Ok(response) => { return pkg.web.types.UpgradeDecision.Reject(response) }
      Err(error) => { return pkg.web.types.UpgradeDecision.Failed(error) }
    }
  }
  if value == "failed" { return pkg.web.types.UpgradeDecision.Failed(Error.Denied) }
  status := if value == "invalid" { 200 } else { 101 }
  response := http.response(status)
  response.header("Upgrade", "owner")
  response.header("Connection", "Upgrade")
  response.header("X-Pattern", c.pattern)
  return pkg.web.types.UpgradeDecision.Accept(pkg.web.types.UpgradeAccepted {
    response: response,
    selected: value.clone(),
  })
}

fn pump(c: pkg.web.types.Ctx, connection: http_upgrade, selected: string) -> Result<(), Error> {
  mut transport := connection
  transport.shutdown()?
  if selected == "pump-err" { return Err(Error.Denied) }
  return Ok(())
}

fn guard(c: pkg.web.types.Ctx) -> pkg.web.types.Middleware {
  mode := pkg.web.query(c, "mw")
  if mode == "respond" {
    match pkg.web.status_text(401, "middleware") {
      Ok(response) => { return pkg.web.types.Middleware.Respond(response) }
      Err(error) => { return pkg.web.types.Middleware.Failed(error) }
    }
  }
  if mode == "failed" { return pkg.web.types.Middleware.Failed(Error.NotFound) }
  return pkg.web.types.Middleware.Proceed
}

pub fn main(args: array<str>) -> Result<(), Error> {
  cmd := cli.command("web-upgrade-owner")
  cmd.flag_i64("port", 0)
  parsed := cmd.parse(args)?
  grouped := pkg.web.group("/api", [
    pkg.web.upgrade("GET", "/items/:id", ["group"], true, decision, pump),
  ])
  guarded := pkg.web.group_with("/mw", [guard], [
    pkg.web.upgrade("GET", "/files/*rest", ["guard"], true, decision, pump),
  ])
  routes := [
    pkg.web.upgrade("GET", "/static", ["accept"], true, decision, pump),
    pkg.web.upgrade("GET", "/reject", ["reject"], true, decision, pump),
    pkg.web.upgrade("GET", "/failed", ["failed"], true, decision, pump),
    pkg.web.upgrade("GET", "/invalid", ["invalid"], true, decision, pump),
    pkg.web.upgrade("GET", "/pump-err", ["pump-err"], true, decision, pump),
    pkg.web.upgrade("GET", "/write-fail", ["write-fail"], true, decision, pump),
    grouped[0],
    guarded[0],
  ]
  return pkg.web.serve("127.0.0.1", parsed.get_i64("port"), routes, 1)
}
"#;

struct Server {
    child: Option<std::process::Child>,
    port: u16,
    deadline: Instant,
    _built: BuiltExeMulti,
}

fn terminate_child_by(
    mut child: std::process::Child,
    deadline: Instant,
) -> Option<std::process::Child> {
    let remaining = deadline.saturating_duration_since(Instant::now());
    let (sender, receiver) = std::sync::mpsc::sync_channel(1);
    std::thread::spawn(move || {
        let _ = child.kill();
        loop {
            match child.wait() {
                Ok(_) => break,
                Err(error) if error.kind() == std::io::ErrorKind::Interrupted => {}
                Err(_) => break,
            }
        }
        let _ = sender.send(child);
    });
    receiver.recv_timeout(remaining).ok()
}

impl Drop for Server {
    fn drop(&mut self) {
        if let Some(child) = self.child.take() {
            let _ = terminate_child_by(child, self.deadline);
        }
    }
}

impl Server {
    fn stop_and_stderr(&mut self) -> String {
        let child = self.child.take().expect("Upgrade owner child exists");
        let mut child = terminate_child_by(child, self.deadline)
            .expect("Upgrade owner did not stop before its deadline");
        let _ = child.wait();
        let mut stderr = String::new();
        child.stderr.take().unwrap().read_to_string(&mut stderr).unwrap();
        stderr
    }
}

struct PendingChild {
    child: Option<std::process::Child>,
    deadline: Instant,
}

impl PendingChild {
    fn child(&mut self) -> &mut std::process::Child {
        self.child.as_mut().expect("pending child exists")
    }

    fn take(&mut self) -> std::process::Child {
        self.child.take().expect("pending child exists")
    }
}

impl Drop for PendingChild {
    fn drop(&mut self) {
        if let Some(child) = self.child.take() {
            let _ = terminate_child_by(child, self.deadline);
        }
    }
}

fn free_loopback_port() -> u16 {
    let probe = std::net::TcpListener::bind("127.0.0.1:0").expect("bind probe");
    let port = probe.local_addr().unwrap().port();
    drop(probe);
    port
}

fn start_server() -> Server {
    let built = build_exe_multi(
        "apps-web-upgrade",
        &[
            ("pkg/web/internal/router.align", ROUTER),
            ("pkg/web/internal/query.align", QUERY),
            ("pkg/web/types.align", TYPES),
            ("pkg/web.align", WEB_ROOT),
            ("main.align", APP),
        ],
        "main.align",
    );
    let deadline = Instant::now() + Duration::from_secs(30);
    for attempt in 0..8 {
        let port = free_loopback_port();
        let mut child = PendingChild {
            child: Some(
                std::process::Command::new(&built.exe)
                .args(["--port", &port.to_string()])
                .stderr(std::process::Stdio::piped())
                .spawn()
                .expect("spawn Upgrade owner server"),
            ),
            deadline,
        };
        std::thread::sleep(Duration::from_millis(300));
        assert!(Instant::now() < deadline, "Upgrade owner startup exceeded its deadline");
        if let Some(status) = child.child().try_wait().expect("poll Upgrade owner server") {
            let mut stderr = String::new();
            child.child().stderr.take().unwrap().read_to_string(&mut stderr).unwrap();
            let bind_failed = matches!(status.code(), Some(48 | 98))
                || stderr.to_ascii_lowercase().contains("address already in use");
            if bind_failed && attempt < 7 {
                continue;
            }
            panic!("Upgrade owner exited at startup: {status:?}; stderr: {stderr}");
        }
        return Server { child: Some(child.take()), port, deadline, _built: built };
    }
    unreachable!("startup retry loop returns or panics")
}

fn connect_retry(port: u16, deadline: Instant) -> TcpStream {
    loop {
        match TcpStream::connect(("127.0.0.1", port)) {
            Ok(sock) => return sock,
            Err(_) if Instant::now() < deadline => std::thread::sleep(Duration::from_millis(25)),
            Err(error) => panic!("Upgrade owner never came up: {error}"),
        }
    }
}

fn exchange(server: &Server, request: &[u8]) -> String {
    let mut sock = connect_retry(server.port, server.deadline);
    let remaining = server
        .deadline
        .checked_duration_since(Instant::now())
        .expect("Upgrade owner deadline exhausted before exchange");
    sock.set_read_timeout(Some(remaining)).unwrap();
    sock.set_write_timeout(Some(remaining)).unwrap();
    sock.write_all(&one_shot(request)).unwrap();
    let mut response = Vec::new();
    sock.read_to_end(&mut response).unwrap();
    String::from_utf8_lossy(&response).into_owned()
}

fn get(path: &str) -> Vec<u8> {
    format!("GET {path} HTTP/1.1\r\nHost: h\r\n\r\n").into_bytes()
}

#[test]
fn upgrade_dispatch_group_middleware_prepare_transfer_and_pump_matrix() {
    if !backend_available() {
        return;
    }
    assert!(!TYPES.contains("validate: fn(slice<str>)"));
    assert!(!ROUTER.contains("handler.validate("));
    let mut server = start_server();

    for (path, pattern) in [
        ("/static", "/static"),
        ("/api/items/42", "/items/:id"),
        ("/mw/files/a/b", "/files/*rest"),
    ] {
        let response = exchange(&server, &get(path));
        assert!(response.starts_with("HTTP/1.1 101 "), "{path}: {response:?}");
        assert!(response.contains(&format!("X-Pattern: {pattern}\r\n")), "{path}: {response:?}");
    }

    let head = exchange(&server, b"HEAD /static HTTP/1.1\r\nHost: h\r\n\r\n");
    assert!(head.starts_with("HTTP/1.1 405 "), "Upgrade has no implicit HEAD: {head:?}");
    assert!(head.contains("Allow: GET\r\n"), "Upgrade contributes Allow: {head:?}");
    let post = exchange(&server, b"POST /api/items/42 HTTP/1.1\r\nHost: h\r\n\r\n");
    assert!(post.starts_with("HTTP/1.1 405 "), "grouped param method: {post:?}");

    let responded = exchange(&server, &get("/mw/files/a?mw=respond"));
    assert!(responded.starts_with("HTTP/1.1 401 "), "middleware Respond: {responded:?}");
    assert!(responded.ends_with("middleware"), "middleware body: {responded:?}");
    let middleware_failed = exchange(&server, &get("/mw/files/a?mw=failed"));
    assert!(middleware_failed.starts_with("HTTP/1.1 500 "), "middleware Failed: {middleware_failed:?}");

    let rejected = exchange(&server, &get("/reject"));
    assert!(rejected.starts_with("HTTP/1.1 403 "), "prepare Reject: {rejected:?}");
    assert!(rejected.ends_with("rejected"), "prepare Reject body: {rejected:?}");
    let failed = exchange(&server, &get("/failed"));
    assert!(failed.starts_with("HTTP/1.1 500 "), "prepare Failed: {failed:?}");
    let invalid = exchange(&server, &get("/invalid"));
    assert!(invalid.starts_with("HTTP/1.1 500 "), "pre-write validation fallback: {invalid:?}");
    let pump_error = exchange(&server, &get("/pump-err"));
    assert!(pump_error.starts_with("HTTP/1.1 101 "), "pump starts only after 101: {pump_error:?}");
    assert_eq!(pump_error.matches("HTTP/1.1 ").count(), 1, "pump Err cannot send a fallback");

    let mut reset = connect_retry(server.port, server.deadline);
    let linger = libc::linger { l_onoff: 1, l_linger: 0 };
    assert_eq!(
        unsafe {
            libc::setsockopt(
                reset.as_raw_fd(),
                libc::SOL_SOCKET,
                libc::SO_LINGER,
                (&raw const linger).cast(),
                std::mem::size_of::<libc::linger>() as libc::socklen_t,
            )
        },
        0,
    );
    let remaining = server
        .deadline
        .checked_duration_since(Instant::now())
        .expect("Upgrade owner deadline exhausted before reset request");
    reset.set_write_timeout(Some(remaining)).unwrap();
    reset.write_all(&get("/write-fail")).unwrap();
    drop(reset);
    std::thread::sleep(Duration::from_millis(100));

    let stderr = server.stop_and_stderr();
    for expected in [
        "pkg.web: middleware Failed (GET /mw/files/a): NotFound",
        "pkg.web: handler Err (GET /failed): Denied",
        "pkg.web: handler Err (GET /invalid): Invalid",
        "pkg.web: handler Err (GET /pump-err): Denied",
        "pkg.web: handler Err (GET /write-fail): Code(",
    ] {
        assert!(stderr.contains(expected), "missing {expected:?}:\n{stderr}");
    }
}
