//! RFC 6455 package owner: compile the shipped pkg.web/pkg.ws producer-consumer chain through
//! whole-program and per-unit checking, then drive the handshake and frame pump over a real socket.

mod common;
use common::*;

use std::io::{Read, Write};
use std::net::TcpStream;
use std::time::{Duration, Instant};

const ROUTER: &str = include_str!("../../../apps/web/pkg/web/internal/router.align");
const TYPES: &str = include_str!("../../../apps/web/pkg/web/types.align");
const WEB_ROOT: &str = include_str!("../../../apps/web/pkg/web.align");
const QUERY: &str = include_str!("../../../apps/web/pkg/web/internal/query.align");
const WS_ROOT: &str = include_str!("../../../apps/ws/pkg/ws.align");

fn sources<'a>(main: &'a str) -> [(&'a str, &'a str); 6] {
    [
        ("pkg/web/internal/router.align", ROUTER),
        ("pkg/web/internal/query.align", QUERY),
        ("pkg/web/types.align", TYPES),
        ("pkg/web.align", WEB_ROOT),
        ("pkg/ws.align", WS_ROOT),
        ("main.align", main),
    ]
}

#[test]
fn shipped_package_chain_checks_whole_and_per_unit() {
    let main = r#"module main
import pkg.ws
import pkg.web.types

fn header_queries(ctx: http_request_ctx) -> bool =
  ctx.headers().count("Host") == 1
    && ctx.headers().tokens_valid("Connection")
    && ctx.headers().contains_token("Connection", "Upgrade")
    && ctx.upgrade_ready()

fn pump(c: pkg.web.types.Ctx, connection: http_upgrade, selected: string) -> Result<(), Error> {
  connection.shutdown()?
  return Ok(())
}

fn main() -> i32 {
  protocols := ["chat.v1"]
  routes := [pkg.ws.route("/chat", protocols, pump)]
  return routes.len() as i32 - 1
}
"#;
    let files = sources(main);
    let checked = diff_check_multi("apps-ws-check", &files, "main.align");
    assert!(!checked.whole_errors, "whole-program diagnostics:\n{}", checked.whole_diags);
    assert!(!checked.per_unit_errors, "per-unit diagnostics:\n{}", checked.per_unit_diags);
}

#[test]
fn http_upgrade_rejects_storage_parameter_out_and_return_edges() {
    let main = r#"module main

fn bad_option(value: Option<http_upgrade>) -> i32 = 0
fn bad_result_parameter(value: Result<http_upgrade, Error>) -> i32 = 0
fn bad_out(out value: http_upgrade) -> i32 = 0
fn bad_return(value: http_upgrade) -> http_upgrade = value

fn bad_local(value: http_upgrade) -> i32 {
  wrapped: Option<http_upgrade> := Some(value)
  return 0
}

fn main() -> i32 = 0
"#;
    let files = sources(main);
    let checked = diff_check_multi("apps-ws-placement", &files, "main.align");
    assert!(checked.whole_errors && checked.per_unit_errors);
    for diagnostics in [&checked.whole_diags, &checked.per_unit_diags] {
        for expected in [
            "http_upgrade may appear only as a bare parameter or one same-frame Result Ok payload",
            "Result<http_upgrade, E> is a same-frame local only and cannot be a parameter",
            "http_upgrade cannot be an `out` parameter",
            "http_upgrade cannot be returned; consume or drop it in the current frame",
            "http_upgrade may be stored only as a bare local or one unnested Result<http_upgrade, E> local",
        ] {
            assert!(diagnostics.contains(expected), "missing {expected:?}:\n{diagnostics}");
        }
    }
}

const APP: &str = r#"module main
import std.cli
import pkg.ws
import pkg.web
import pkg.web.types

fn pump(c: pkg.web.types.Ctx, connection: http_upgrade, selected: string) -> Result<(), Error> {
  mut transport := connection
  if selected != "chat.v1" {
    transport.shutdown()?
    return Err(Error.Invalid)
  }
  message := pkg.ws.receive(transport, 1024)?
  match message {
    Text(text) => { pkg.ws.send_text(transport, text)? }
    Binary(data) => { pkg.ws.send_binary(transport, data[0..data.len()])? }
    Close(_) => { return Ok(()) }
  }
  return pkg.ws.close(transport, 1000, "", 1000000000)
}

pub fn main(args: array<str>) -> Result<(), Error> {
  cmd := cli.command("ws-server")
  cmd.flag_i64("port", 0)
  parsed := cmd.parse(args)?
  protocols := ["chat.v1"]
  routes := [pkg.ws.route("/chat", protocols, pump)]
  return pkg.web.serve("127.0.0.1", parsed.get_i64("port"), routes, 1)
}
"#;

fn free_loopback_port() -> u16 {
    let probe = std::net::TcpListener::bind("127.0.0.1:0").expect("bind probe");
    let port = probe.local_addr().unwrap().port();
    drop(probe);
    port
}

fn connect_retry(port: u16) -> TcpStream {
    let deadline = Instant::now() + Duration::from_secs(30);
    loop {
        match TcpStream::connect(("127.0.0.1", port)) {
            Ok(sock) => return sock,
            Err(_) if Instant::now() < deadline => std::thread::sleep(Duration::from_millis(25)),
            Err(error) => panic!("server never came up: {error}"),
        }
    }
}

fn read_http_head(sock: &mut TcpStream) -> Vec<u8> {
    let mut head = Vec::new();
    loop {
        let mut byte = [0u8; 1];
        sock.read_exact(&mut byte).expect("read response head");
        head.push(byte[0]);
        if head.ends_with(b"\r\n\r\n") {
            return head;
        }
        assert!(head.len() < 16 * 1024, "bounded response head");
    }
}

fn masked_frame(fin: bool, opcode: u8, payload: &[u8], mask: [u8; 4]) -> Vec<u8> {
    assert!(payload.len() <= 125, "test helper uses only short frames");
    let mut frame = vec![(if fin { 0x80 } else { 0 }) | opcode, 0x80 | payload.len() as u8];
    frame.extend_from_slice(&mask);
    frame.extend(payload.iter().enumerate().map(|(i, byte)| byte ^ mask[i % 4]));
    frame
}

fn read_frame(sock: &mut TcpStream) -> (bool, u8, Vec<u8>) {
    let mut first = [0u8; 2];
    sock.read_exact(&mut first).expect("read frame head");
    assert_eq!(first[1] & 0x80, 0, "server frames are never masked");
    let mut length = usize::from(first[1] & 0x7f);
    if length == 126 {
        let mut extended = [0u8; 2];
        sock.read_exact(&mut extended).expect("read 16-bit frame length");
        length = usize::from(u16::from_be_bytes(extended));
    } else if length == 127 {
        let mut extended = [0u8; 8];
        sock.read_exact(&mut extended).expect("read 64-bit frame length");
        length = usize::try_from(u64::from_be_bytes(extended)).expect("host frame length");
    }
    let mut payload = vec![0u8; length];
    sock.read_exact(&mut payload).expect("read frame payload");
    ((first[0] & 0x80) != 0, first[0] & 0x0f, payload)
}

fn open_websocket(port: u16) -> TcpStream {
    let mut sock = connect_retry(port);
    sock.set_read_timeout(Some(Duration::from_secs(30))).unwrap();
    sock.set_write_timeout(Some(Duration::from_secs(30))).unwrap();
    sock.write_all(
        b"GET /chat HTTP/1.1\r\nHost: localhost\r\nUpgrade: websocket\r\nConnection: Upgrade\r\nSec-WebSocket-Version: 13\r\nSec-WebSocket-Key: dGhlIHNhbXBsZSBub25jZQ==\r\nSec-WebSocket-Protocol: chat.v1\r\n\r\n",
    )
    .unwrap();
    let head = String::from_utf8(read_http_head(&mut sock)).unwrap();
    assert!(head.starts_with("HTTP/1.1 101 Switching Protocols\r\n"), "{head:?}");
    sock
}

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

fn start_server() -> Server {
    let port = free_loopback_port();
    let files = sources(APP);
    let built = build_exe_multi("apps-ws-socket", &files, "main.align");
    let child = std::process::Command::new(&built.exe)
        .args(["--port", &port.to_string()])
        .stderr(std::process::Stdio::piped())
        .spawn()
        .expect("spawn WebSocket server");
    // Install the kill-and-reap guard before any later setup can fail.
    let mut server = Server { child, port, _built: built };
    std::thread::sleep(Duration::from_millis(300));
    if let Some(status) = server.child.try_wait().expect("poll WebSocket server") {
        let mut stderr = String::new();
        server
            .child
            .stderr
            .take()
            .unwrap()
            .read_to_string(&mut stderr)
            .unwrap();
        panic!("server exited at startup: {status:?}; stderr: {stderr}");
    }
    server
}

#[test]
fn handshake_fragment_ping_echo_and_close_run_end_to_end() {
    if !backend_available() {
        return;
    }
    let server = start_server();

    // Upgrade GET rows do not participate in pkg.web's unary HEAD-to-GET fallback.
    let mut head_only = connect_retry(server.port);
    head_only.set_read_timeout(Some(Duration::from_secs(30))).unwrap();
    head_only
        .write_all(b"HEAD /chat HTTP/1.1\r\nHost: localhost\r\nConnection: close\r\n\r\n")
        .unwrap();
    let head_only_response = String::from_utf8(read_http_head(&mut head_only)).unwrap();
    assert!(head_only_response.starts_with("HTTP/1.1 405 "), "{head_only_response:?}");
    assert!(head_only_response.contains("Allow: GET\r\n"), "{head_only_response:?}");

    // A version mismatch is a normal HTTP rejection and advertises the one accepted version.
    let mut rejected = connect_retry(server.port);
    rejected.set_read_timeout(Some(Duration::from_secs(30))).unwrap();
    rejected
        .write_all(
            b"GET /chat HTTP/1.1\r\nHost: localhost\r\nUpgrade: websocket\r\nConnection: Upgrade\r\nSec-WebSocket-Version: 12\r\nSec-WebSocket-Key: dGhlIHNhbXBsZSBub25jZQ==\r\nConnection: close\r\n\r\n",
        )
        .unwrap();
    let rejected_head = String::from_utf8(read_http_head(&mut rejected)).unwrap();
    assert!(rejected_head.starts_with("HTTP/1.1 400 "), "{rejected_head:?}");
    assert!(rejected_head.contains("Sec-WebSocket-Version: 13\r\n"), "{rejected_head:?}");

    let mut sock = connect_retry(server.port);
    sock.set_read_timeout(Some(Duration::from_secs(30))).unwrap();
    sock.set_write_timeout(Some(Duration::from_secs(30))).unwrap();
    sock.write_all(
        b"GET /chat HTTP/1.1\r\nHost: localhost\r\nUpgrade: h2c\r\nUpgrade: websocket\r\nConnection: keep-alive, Upgrade\r\nSec-WebSocket-Version: 13\r\nSec-WebSocket-Key: dGhlIHNhbXBsZSBub25jZQ==\r\nSec-WebSocket-Protocol: other, chat.v1\r\n\r\n",
    )
    .unwrap();
    let head = String::from_utf8(read_http_head(&mut sock)).unwrap();
    assert!(head.starts_with("HTTP/1.1 101 Switching Protocols\r\n"), "{head:?}");
    assert!(head.contains("Upgrade: websocket\r\n"), "{head:?}");
    assert!(head.contains("Connection: Upgrade\r\n"), "{head:?}");
    assert!(head.contains("Sec-WebSocket-Accept: s3pPLMBiTxaQ9kYGzzhZRbK+xOo=\r\n"), "{head:?}");
    assert!(head.contains("Sec-WebSocket-Protocol: chat.v1\r\n"), "{head:?}");
    assert!(!head.contains("Sec-WebSocket-Extensions:"), "{head:?}");

    // One fragmented text message with an interleaved ping: pong precedes the completed echo.
    sock.write_all(&masked_frame(false, 1, b"h", [1, 2, 3, 4])).unwrap();
    sock.write_all(&masked_frame(true, 9, b"p", [5, 6, 7, 8])).unwrap();
    sock.write_all(&masked_frame(true, 0, b"i", [9, 10, 11, 12])).unwrap();
    assert_eq!(read_frame(&mut sock), (true, 10, b"p".to_vec()));
    assert_eq!(read_frame(&mut sock), (true, 1, b"hi".to_vec()));
    assert_eq!(read_frame(&mut sock), (true, 8, vec![3, 232]));

    sock.write_all(&masked_frame(true, 8, &[3, 232], [13, 14, 15, 16])).unwrap();

    let mut unmasked = open_websocket(server.port);
    unmasked.write_all(&[0x81, 0]).unwrap();
    assert_eq!(read_frame(&mut unmasked), (true, 8, vec![3, 234]), "unmasked input closes 1002");

    let mut bad_text = open_websocket(server.port);
    bad_text.write_all(&masked_frame(true, 1, &[0xff], [1, 1, 1, 1])).unwrap();
    assert_eq!(read_frame(&mut bad_text), (true, 8, vec![3, 239]), "invalid UTF-8 closes 1007");

    let mut oversized = open_websocket(server.port);
    oversized.write_all(&[0x82, 0xfe, 0x04, 0x01, 1, 2, 3, 4]).unwrap();
    assert_eq!(read_frame(&mut oversized), (true, 8, vec![3, 241]), "declared byte 1025 closes 1009 without its payload");

    let mut continuation = open_websocket(server.port);
    continuation.write_all(&masked_frame(true, 0, b"x", [2, 3, 4, 5])).unwrap();
    assert_eq!(read_frame(&mut continuation), (true, 8, vec![3, 234]), "orphan continuation closes 1002");

    let mut client_only_close = open_websocket(server.port);
    client_only_close
        .write_all(&masked_frame(true, 8, &[3, 242], [6, 7, 8, 9]))
        .unwrap();
    assert_eq!(read_frame(&mut client_only_close), (true, 8, Vec::new()), "1010 gets an empty acknowledgment");
}

#[test]
fn invalid_server_protocol_aborts_before_bind_with_exact_route_diagnosis() {
    if !backend_available() {
        return;
    }
    let invalid = APP.replace("protocols := [\"chat.v1\"]", "protocols := [\"bad protocol\"]");
    let files = sources(&invalid);
    let built = build_exe_multi("apps-ws-invalid-protocol", &files, "main.align");
    let port = free_loopback_port();
    let child = std::process::Command::new(&built.exe)
        .args(["--port", &port.to_string()])
        .stderr(std::process::Stdio::piped())
        .spawn()
        .expect("run invalid WebSocket server");
    // The executable must fail synchronously during route validation. Keep an RAII owner around
    // it so a regression that reaches the accept loop cannot leak a server process from the test.
    let mut child = Server { child, port, _built: built };
    let deadline = Instant::now() + Duration::from_secs(30);
    let status = loop {
        if let Some(status) = child.child.try_wait().expect("poll invalid WebSocket server") {
            break status;
        }
        assert!(Instant::now() < deadline, "invalid startup did not terminate before the deadline");
        std::thread::sleep(Duration::from_millis(10));
    };
    let mut stderr = String::new();
    child
        .child
        .stderr
        .take()
        .unwrap()
        .read_to_string(&mut stderr)
        .expect("read invalid WebSocket stderr");
    assert!(!status.success(), "invalid startup must abort");
    assert_eq!(
        stderr,
        "pkg.web: route 0 (GET /chat) has invalid upgrade values\n",
    );
    let probe = std::net::TcpListener::bind(("127.0.0.1", port));
    assert!(probe.is_ok(), "route validation must happen before bind");
}
