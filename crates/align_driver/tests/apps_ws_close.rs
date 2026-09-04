//! Public `pkg.ws.close` owner: outbound validation and wire bytes, the Closing-state frame
//! policy, one cumulative deadline, and transport/protocol/EOF failure precedence.

mod common;
use common::*;

use std::io::{Read, Write};
use std::net::{Shutdown, TcpStream};
use std::os::fd::AsRawFd;
use std::path::Path;
use std::time::{Duration, Instant};

fn router() -> &'static str {
    static SOURCE: std::sync::LazyLock<&str> =
        std::sync::LazyLock::new(|| fixture("apps/web/pkg/web/internal/router.align"));
    *SOURCE
}
fn types() -> &'static str {
    static SOURCE: std::sync::LazyLock<&str> =
        std::sync::LazyLock::new(|| fixture("apps/web/pkg/web/types.align"));
    *SOURCE
}
fn web_root() -> &'static str {
    static SOURCE: std::sync::LazyLock<&str> =
        std::sync::LazyLock::new(|| fixture("apps/web/pkg/web.align"));
    *SOURCE
}
fn query() -> &'static str {
    static SOURCE: std::sync::LazyLock<&str> =
        std::sync::LazyLock::new(|| fixture("apps/web/pkg/web/internal/query.align"));
    *SOURCE
}
fn ws_root() -> &'static str {
    static SOURCE: std::sync::LazyLock<&str> =
        std::sync::LazyLock::new(|| fixture("apps/ws/pkg/ws.align"));
    *SOURCE
}

const APP: &str = r#"module main
import std.cli
import pkg.ws
import pkg.web
import pkg.web.types

fn repeated_x(count: i64) -> string {
  mut out := builder()
  mut index := 0
  loop {
    if index >= count { break }
    out.write("x")
    index = index + 1
  }
  return out.to_string()
}

fn pump(c: pkg.web.types.Ctx, connection: http_upgrade, selected: string) -> Result<(), Error> {
  mut transport := connection
  mut code: i64 := 1000
  mut reason := "".clone()
  mut timeout_ns: i64 := 1000000000
  if c.path == "/c1001" { code = 1001 }
  if c.path == "/c1002" { code = 1002 }
  if c.path == "/c1003" { code = 1003 }
  if c.path == "/c1007" { code = 1007 }
  if c.path == "/c1008" { code = 1008 }
  if c.path == "/c1009" { code = 1009 }
  if c.path == "/c1011" { code = 1011 }
  if c.path == "/c1012" { code = 1012 }
  if c.path == "/c1013" { code = 1013 }
  if c.path == "/c1014" { code = 1014 }
  if c.path == "/c3000" { code = 3000 }
  if c.path == "/c3003" { code = 3003 }
  if c.path == "/c3008" { code = 3008 }
  if c.path == "/c4000" { code = 4000 }
  if c.path == "/c4999" { code = 4999 }
  if c.path == "/cnegative" { code = -1 }
  if c.path == "/c999" { code = 999 }
  if c.path == "/c1004" { code = 1004 }
  if c.path == "/c1005" { code = 1005 }
  if c.path == "/c1006" { code = 1006 }
  if c.path == "/c1010" { code = 1010 }
  if c.path == "/c1015" { code = 1015 }
  if c.path == "/c1016" { code = 1016 }
  if c.path == "/c2999" { code = 2999 }
  if c.path == "/c3001" { code = 3001 }
  if c.path == "/c3999" { code = 3999 }
  if c.path == "/c5000" { code = 5000 }
  if c.path == "/c65536" { code = 65536 }
  if c.path == "/reason123" { reason = repeated_x(123) }
  if c.path == "/reason124" { reason = repeated_x(124) }
  if c.path == "/timeout0" { timeout_ns = 0 }
  if c.path == "/timeout-min" { timeout_ns = 1 }
  if c.path == "/timeout-max" { timeout_ns = 86400000000000 }
  if c.path == "/timeout-over" { timeout_ns = 86400000000001 }
  if c.path == "/timeout-short" { timeout_ns = 600000000 }
  if c.path == "/write-fail" {
    mut sync := buffer(1)
    transport.read_exact(sync, 1)?
  }
  return pkg.ws.close(transport, code, reason, timeout_ns)
}

pub fn main(args: array<str>) -> Result<(), Error> {
  cmd := cli.command("ws-close-owner")
  cmd.flag_i64("port", 0)
  parsed := cmd.parse(args)?
  routes := [pkg.ws.route("/:mode", [], pump)]
  return pkg.web.serve("127.0.0.1", parsed.get_i64("port"), routes, 1)
}
"#;

struct Server {
    child: Option<std::process::Child>,
    port: u16,
    deadline: Instant,
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
                Err(_) => std::thread::sleep(Duration::from_millis(10)),
            }
        }
        let _ = sender.send(child);
    });
    receiver.recv_timeout(remaining).ok()
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

impl Drop for Server {
    fn drop(&mut self) {
        if let Some(child) = self.child.take() {
            let _ = terminate_child_by(child, self.deadline);
        }
    }
}

impl Server {
    fn stop_and_stderr(&mut self) -> String {
        let child = self.child.take().expect("close owner child exists");
        let mut child = terminate_child_by(child, self.deadline)
            .expect("close owner did not stop before its deadline");
        let _ = child.wait();
        let mut stderr = String::new();
        child.stderr.take().unwrap().read_to_string(&mut stderr).unwrap();
        stderr
    }

    fn wait_for_abort(&mut self) -> (std::process::ExitStatus, String) {
        let status = loop {
            match self
                .child
                .as_mut()
                .expect("close owner child exists")
                .try_wait()
            {
                Ok(Some(status)) => break status,
                Ok(None) if Instant::now() < self.deadline => {
                    std::thread::sleep(Duration::from_millis(10));
                }
                Ok(None) => panic!("invalid close input did not abort"),
                Err(error) if error.kind() == std::io::ErrorKind::Interrupted => {}
                Err(error) => panic!("poll invalid close server: {error}"),
            }
        };
        let mut stderr = String::new();
        self.child
            .as_mut()
            .expect("close owner child exists")
            .stderr
            .take()
            .unwrap()
            .read_to_string(&mut stderr)
            .unwrap();
        (status, stderr)
    }
}

fn build_app_with_ws(name: &str, ws_root: &str) -> BuiltExeMulti {
    build_exe_multi(
        name,
        &[
            ("pkg/web/internal/router.align", router()),
            ("pkg/web/internal/query.align", query()),
            ("pkg/web/types.align", types()),
            ("pkg/web.align", web_root()),
            ("pkg/ws.align", ws_root),
            ("main.align", APP),
        ],
        "main.align",
    )
}

fn build_app() -> BuiltExeMulti {
    build_app_with_ws("apps-ws-close", ws_root())
}

fn free_loopback_port() -> u16 {
    let probe = std::net::TcpListener::bind("127.0.0.1:0").expect("bind probe");
    let port = probe.local_addr().unwrap().port();
    drop(probe);
    port
}

fn start_server(exe: &Path) -> Server {
    for attempt in 0..8 {
        let port = free_loopback_port();
        let deadline = Instant::now() + Duration::from_secs(10);
        let mut child = PendingChild {
            child: Some(
                std::process::Command::new(exe)
                .args(["--port", &port.to_string()])
                .stderr(std::process::Stdio::piped())
                .spawn()
                .expect("spawn close owner"),
            ),
            deadline,
        };
        std::thread::sleep(Duration::from_millis(300));
        if let Some(status) = child.child().try_wait().expect("poll close owner") {
            let mut stderr = String::new();
            child.child().stderr.take().unwrap().read_to_string(&mut stderr).unwrap();
            let bind_failed = matches!(status.code(), Some(48 | 98))
                || stderr.to_ascii_lowercase().contains("address already in use");
            if bind_failed && attempt < 7 {
                continue;
            }
            panic!("close owner exited at startup: {status:?}; stderr: {stderr}");
        }
        return Server { child: Some(child.take()), port, deadline };
    }
    unreachable!("startup retry loop returns or panics")
}

fn connect_retry(port: u16, deadline: Instant) -> TcpStream {
    loop {
        match TcpStream::connect(("127.0.0.1", port)) {
            Ok(sock) => return sock,
            Err(_) if Instant::now() < deadline => std::thread::sleep(Duration::from_millis(10)),
            Err(error) => panic!("close owner never came up: {error}"),
        }
    }
}

fn read_http_head(sock: &mut TcpStream) -> Vec<u8> {
    let mut bytes = Vec::new();
    loop {
        let mut byte = [0u8; 1];
        sock.read_exact(&mut byte).expect("read HTTP Upgrade response");
        bytes.push(byte[0]);
        if bytes.ends_with(b"\r\n\r\n") {
            return bytes;
        }
    }
}

fn masked_frame(fin: bool, opcode: u8, payload: &[u8], mask: [u8; 4]) -> Vec<u8> {
    assert!(payload.len() <= 125);
    let mut frame = vec![(if fin { 0x80 } else { 0 }) | opcode, 0x80 | payload.len() as u8];
    frame.extend_from_slice(&mask);
    frame.extend(payload.iter().enumerate().map(|(index, byte)| byte ^ mask[index % 4]));
    frame
}

fn read_frame(sock: &mut TcpStream) -> (bool, u8, Vec<u8>) {
    let mut head = [0u8; 2];
    sock.read_exact(&mut head).expect("read frame head");
    assert_eq!(head[1] & 0x80, 0, "server frames are unmasked");
    let length = (head[1] & 0x7f) as usize;
    assert!(length <= 125, "close owner emits control frames only");
    let mut payload = vec![0; length];
    sock.read_exact(&mut payload).expect("read frame payload");
    (head[0] & 0x80 != 0, head[0] & 0x0f, payload)
}

fn open_websocket(server: &Server, path: &str) -> TcpStream {
    let mut sock = connect_retry(server.port, server.deadline);
    let remaining = server
        .deadline
        .checked_duration_since(Instant::now())
        .expect("close owner deadline exhausted before exchange");
    sock.set_read_timeout(Some(remaining)).unwrap();
    sock.set_write_timeout(Some(remaining)).unwrap();
    let request = format!(
        "GET {path} HTTP/1.1\r\nHost: localhost\r\nUpgrade: websocket\r\nConnection: Upgrade\r\nSec-WebSocket-Version: 13\r\nSec-WebSocket-Key: dGhlIHNhbXBsZSBub25jZQ==\r\n\r\n"
    );
    sock.write_all(request.as_bytes()).unwrap();
    let head = String::from_utf8(read_http_head(&mut sock)).unwrap();
    assert!(head.starts_with("HTTP/1.1 101 "), "{path}: {head:?}");
    sock
}

fn reply_close(sock: &mut TcpStream, payload: &[u8]) {
    sock.write_all(&masked_frame(true, 8, payload, [1, 3, 5, 7])).unwrap();
}

fn expect_connection_end(sock: &mut TcpStream) {
    let mut byte = [0u8; 1];
    match sock.read(&mut byte) {
        Ok(0) => {}
        Err(error)
            if matches!(
                error.kind(),
                std::io::ErrorKind::ConnectionReset
                    | std::io::ErrorKind::ConnectionAborted
                    | std::io::ErrorKind::BrokenPipe
            ) => {}
        outcome => panic!("expected closed/reset connection, got {outcome:?}"),
    }
}

#[test]
fn outbound_close_validation_wire_state_deadline_and_failures() {
    if !backend_available() {
        return;
    }
    let built = build_app();
    let mut server = start_server(&built.exe);

    for code in [
        1000u16, 1001, 1002, 1003, 1007, 1008, 1009, 1011, 1012, 1013, 1014, 3000, 3003,
        3008, 4000, 4999,
    ] {
        let mut sock = open_websocket(&server, &format!("/c{code}"));
        let expected = code.to_be_bytes().to_vec();
        assert_eq!(read_frame(&mut sock), (true, 8, expected.clone()), "outbound code {code}");
        reply_close(&mut sock, &expected);
        expect_connection_end(&mut sock);
    }

    let mut reason = open_websocket(&server, "/reason123");
    let mut expected_reason = 1000u16.to_be_bytes().to_vec();
    expected_reason.extend(std::iter::repeat_n(b'x', 123));
    assert_eq!(read_frame(&mut reason), (true, 8, expected_reason.clone()));
    reply_close(&mut reason, &expected_reason);
    expect_connection_end(&mut reason);

    let mut maximum_timeout = open_websocket(&server, "/timeout-max");
    assert_eq!(read_frame(&mut maximum_timeout), (true, 8, 1000u16.to_be_bytes().to_vec()));
    reply_close(&mut maximum_timeout, &1000u16.to_be_bytes());
    expect_connection_end(&mut maximum_timeout);

    // Closing discards data and Pong, answers Ping, and completes only on a valid peer Close.
    let mut state = open_websocket(&server, "/state");
    assert_eq!(read_frame(&mut state), (true, 8, 1000u16.to_be_bytes().to_vec()));
    state.write_all(&masked_frame(true, 1, b"ignored", [2, 4, 6, 8])).unwrap();
    state.write_all(&masked_frame(true, 10, b"ignored", [3, 5, 7, 9])).unwrap();
    state.write_all(&masked_frame(true, 9, b"ping", [4, 6, 8, 10])).unwrap();
    assert_eq!(read_frame(&mut state), (true, 10, b"ping".to_vec()));
    reply_close(&mut state, &1001u16.to_be_bytes());
    expect_connection_end(&mut state);

    // A single budget governs the initial Close and every later Ping/read; activity cannot reset it.
    let started = Instant::now();
    let mut bounded = open_websocket(&server, "/timeout-short");
    assert_eq!(read_frame(&mut bounded), (true, 8, 1000u16.to_be_bytes().to_vec()));
    for mask in [[1, 1, 1, 1], [2, 2, 2, 2]] {
        std::thread::sleep(Duration::from_millis(120));
        bounded.write_all(&masked_frame(true, 9, b"p", mask)).unwrap();
        assert_eq!(read_frame(&mut bounded), (true, 10, b"p".to_vec()));
    }
    // The real cumulative budget has about 360ms left here. A buggy per-frame reset would leave
    // the full 600ms, so this client-side bound makes that implementation time out the test.
    bounded.set_read_timeout(Some(Duration::from_millis(480))).unwrap();
    expect_connection_end(&mut bounded);
    assert!(started.elapsed() < Duration::from_millis(750), "deadline must not reset per frame");

    let mut protocol = open_websocket(&server, "/protocol");
    assert_eq!(read_frame(&mut protocol).1, 8);
    protocol.write_all(&[0x81, 0]).unwrap();
    expect_connection_end(&mut protocol);

    let mut invalid_peer_close = open_websocket(&server, "/invalid-peer-close");
    assert_eq!(read_frame(&mut invalid_peer_close).1, 8);
    invalid_peer_close.write_all(&masked_frame(true, 8, &[3], [7, 7, 7, 7])).unwrap();
    expect_connection_end(&mut invalid_peer_close);

    let mut partial_eof = open_websocket(&server, "/partial-eof");
    assert_eq!(read_frame(&mut partial_eof).1, 8);
    partial_eof.write_all(&[0x81]).unwrap();
    partial_eof.shutdown(Shutdown::Write).unwrap();
    expect_connection_end(&mut partial_eof);

    // A 1ns timeout is semantically valid even when it expires before the first write; the worker
    // reports Timeout and remains able to accept the next request instead of aborting validation.
    let mut minimum_timeout = open_websocket(&server, "/timeout-min");
    expect_connection_end(&mut minimum_timeout);
    let mut after_minimum = open_websocket(&server, "/c1000");
    assert_eq!(read_frame(&mut after_minimum), (true, 8, 1000u16.to_be_bytes().to_vec()));
    reply_close(&mut after_minimum, &1000u16.to_be_bytes());
    expect_connection_end(&mut after_minimum);

    // Synchronize inside the pump before resetting the peer, so this failure belongs to close's
    // outbound write rather than the preceding HTTP 101 write.
    let mut reset = open_websocket(&server, "/write-fail");
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
    reset.write_all(&[0]).unwrap();
    drop(reset);
    std::thread::sleep(Duration::from_millis(100));

    let stderr = server.stop_and_stderr();
    assert!(stderr.matches("Invalid").count() >= 2, "protocol and invalid Close errors:\n{stderr}");
    assert!(stderr.contains("NotFound"), "partial EOF must surface NotFound:\n{stderr}");
    assert!(stderr.contains("Timeout"), "deadline exhaustion must surface Timeout:\n{stderr}");
    assert!(stderr.contains("Code("), "outbound reset must surface transport Code:\n{stderr}");

    // Fault-inject between the code and reason writes: the 4-byte frame head/code prefix proves
    // those writes succeeded, then the reset makes the reason-payload write own the error.
    let partial_write_ws = ws_root()
        .replace("import std.http\n", "import std.http\nimport std.time\n")
        .replace(
            "  connection.write(code_bytes[0..2])?\n  return connection.write(reason.bytes())",
            "  connection.write(code_bytes[0..2])?\n  mut sync := buffer(1)\n  match connection.read_exact(sync, 1) {\n    Ok(_) => {}\n    Err(_) => { return Err(Error.NotFound) }\n  }\n  time.sleep(200000000)\n  match connection.write(reason.bytes()) {\n    Ok(_) => { return Err(Error.Denied) }\n    Err(error) => { return Err(error) }\n  }",
        );
    assert_ne!(partial_write_ws, ws_root(), "close-frame write seam must remain recognizable");
    let partial_built = build_app_with_ws("apps-ws-close-partial-write", &partial_write_ws);
    let mut partial_server = start_server(&partial_built.exe);
    let mut partial = open_websocket(&partial_server, "/reason123");
    let mut prefix = [0u8; 4];
    partial.read_exact(&mut prefix).unwrap();
    assert_eq!(prefix, [0x88, 125, 3, 232], "frame head and code were written first");
    let linger = libc::linger { l_onoff: 1, l_linger: 0 };
    assert_eq!(
        unsafe {
            libc::setsockopt(
                partial.as_raw_fd(),
                libc::SOL_SOCKET,
                libc::SO_LINGER,
                (&raw const linger).cast(),
                std::mem::size_of::<libc::linger>() as libc::socklen_t,
            )
        },
        0,
    );
    partial.write_all(&[0]).unwrap();
    drop(partial);
    std::thread::sleep(Duration::from_millis(500));
    let partial_stderr = partial_server.stop_and_stderr();
    assert!(
        partial_stderr.contains("Code("),
        "reason-payload reset must surface transport Code:\n{partial_stderr}"
    );
    assert!(
        !partial_stderr.contains("Denied"),
        "a successful reason write must take the distinct sentinel path:\n{partial_stderr}"
    );
    assert!(
        !partial_stderr.contains("NotFound"),
        "a failed synchronization read must take the distinct sentinel path:\n{partial_stderr}"
    );

    for path in [
        "/cnegative",
        "/c999",
        "/c1004",
        "/c1005",
        "/c1006",
        "/c1010",
        "/c1015",
        "/c1016",
        "/c2999",
        "/c3001",
        "/c3999",
        "/c5000",
        "/c65536",
        "/reason124",
        "/timeout0",
        "/timeout-over",
    ] {
        let mut invalid_server = start_server(&built.exe);
        let mut sock = open_websocket(&invalid_server, path);
        let mut post_upgrade = Vec::new();
        sock.read_to_end(&mut post_upgrade).unwrap_or_else(|error| {
            if error.kind() == std::io::ErrorKind::ConnectionReset {
                0
            } else {
                panic!("{path}: read after invalid close input: {error}")
            }
        });
        assert!(post_upgrade.is_empty(), "{path}: invalid input wrote a WebSocket frame");
        let (status, _) = invalid_server.wait_for_abort();
        assert!(!status.success(), "{path}: invalid close input must abort");
    }
}
