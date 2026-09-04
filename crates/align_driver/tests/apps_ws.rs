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

fn sources_with_ws<'a>(main: &'a str, ws_root: &'a str) -> [(&'a str, &'a str); 6] {
    [
        ("pkg/web/internal/router.align", ROUTER),
        ("pkg/web/internal/query.align", QUERY),
        ("pkg/web/types.align", TYPES),
        ("pkg/web.align", WEB_ROOT),
        ("pkg/ws.align", ws_root),
        ("main.align", main),
    ]
}

fn sources(main: &str) -> [(&str, &str); 6] {
    sources_with_ws(main, WS_ROOT)
}

#[test]
fn shipped_package_chain_checks_whole_and_per_unit() {
    assert!(TYPES.contains("values_valid: bool"));
    assert!(!TYPES.contains("validate: fn(slice<str>)"));
    assert!(!ROUTER.contains("handler.validate("));
    assert!(WS_ROOT.contains("protocols_valid(protocols)"));
    let main = r#"module main
import pkg.ws
import pkg.web
import pkg.web.types

fn header_queries(ctx: http_request_ctx) -> bool =
  ctx.headers().count("Host") == 1
    && ctx.headers().tokens_valid("Connection")
    && ctx.headers().contains_token("Connection", "Upgrade")
    && ctx.headers().contains_token_exact("Sec-WebSocket-Protocol", "chat.v1")
    && ctx.upgrade_ready()

fn pump(c: pkg.web.types.Ctx, connection: http_upgrade, selected: string) -> Result<(), Error> {
  connection.shutdown()?
  return Ok(())
}

fn prepare(c: pkg.web.types.Ctx, values: slice<str>) -> pkg.web.types.UpgradeDecision =
  pkg.web.types.UpgradeDecision.Failed(Error.Invalid)

fn main() -> i32 {
  protocols := ["chat.v1"]
  routes := [
    pkg.ws.route("/chat", protocols, pump),
    pkg.web.upgrade("GET", "/raw", protocols, true, prepare, pump),
  ]
  return routes.len() as i32 - 2
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
  message := pkg.ws.receive(transport, 131072)?
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

const RESOURCE_APP: &str = r#"module main
import std.cli
import pkg.ws
import pkg.web
import pkg.web.types

RESOURCE_MAX: i64 := 1024
RESOURCE_TEXT_PEAK: i64 := 34944
RESOURCE_BINARY_PEAK: i64 := 33920

extern "C" {
  fn align_rt_requested_live_reset()
  fn align_rt_requested_live_peak() -> i64
}

fn pump(c: pkg.web.types.Ctx, connection: http_upgrade, selected: string) -> Result<(), Error> {
  mut transport := connection
  unsafe { align_rt_requested_live_reset() }
  message := pkg.ws.receive(transport, RESOURCE_MAX)?
  peak := unsafe { align_rt_requested_live_peak() }
  mut expected := 0
  match message {
    Text(_) => { expected = RESOURCE_TEXT_PEAK }
    Binary(_) => { expected = RESOURCE_BINARY_PEAK }
    Close(_) => { expected = -1 }
  }
  code := if peak == expected { 1000 } else { 1011 }
  return pkg.ws.close(transport, code, "", 1000000000)
}

pub fn main(args: array<str>) -> Result<(), Error> {
  cmd := cli.command("ws-resource-server")
  cmd.flag_i64("port", 0)
  parsed := cmd.parse(args)?
  protocols := ["chat.v1"]
  routes := [pkg.ws.route("/chat", protocols, pump)]
  return pkg.web.serve("127.0.0.1", parsed.get_i64("port"), routes, 1)
}
"#;

fn source_i64_constant(source: &str, name: &str) -> i64 {
    let prefix = format!("{name}: i64 := ");
    source
        .lines()
        .find_map(|line| line.strip_prefix(&prefix))
        .unwrap_or_else(|| panic!("missing shipped constant {name}"))
        .parse()
        .unwrap_or_else(|error| panic!("invalid shipped constant {name}: {error}"))
}

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
    let mut frame = vec![(if fin { 0x80 } else { 0 }) | opcode];
    if payload.len() <= 125 {
        frame.push(0x80 | payload.len() as u8);
    } else if payload.len() <= usize::from(u16::MAX) {
        frame.push(0x80 | 126);
        frame.extend_from_slice(&(payload.len() as u16).to_be_bytes());
    } else {
        frame.push(0x80 | 127);
        frame.extend_from_slice(&(payload.len() as u64).to_be_bytes());
    }
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

fn request_head(port: u16, request: &[u8]) -> String {
    let mut sock = connect_retry(port);
    sock.set_read_timeout(Some(Duration::from_secs(30))).unwrap();
    sock.set_write_timeout(Some(Duration::from_secs(30))).unwrap();
    sock.write_all(request).unwrap();
    String::from_utf8(read_http_head(&mut sock)).unwrap()
}

fn assert_protocol_close(port: u16, bytes: &[u8], label: &str) {
    let mut sock = open_websocket(port);
    sock.write_all(bytes).unwrap();
    assert_eq!(read_frame(&mut sock), (true, 8, vec![3, 234]), "{label}");
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

fn start_server_with(source: &str, build_name: &str) -> Server {
    let port = free_loopback_port();
    let files = sources(source);
    let built = build_exe_multi(build_name, &files, "main.align");
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

fn start_server_with_ws(source: &str, ws_root: &str, build_name: &str) -> Server {
    let port = free_loopback_port();
    let files = sources_with_ws(source, ws_root);
    let built = build_exe_multi(build_name, &files, "main.align");
    let child = std::process::Command::new(&built.exe)
        .args(["--port", &port.to_string()])
        .stderr(std::process::Stdio::piped())
        .spawn()
        .expect("spawn bounded WebSocket server");
    let mut server = Server { child, port, _built: built };
    std::thread::sleep(Duration::from_millis(300));
    if let Some(status) = server.child.try_wait().expect("poll bounded WebSocket server") {
        let mut stderr = String::new();
        server.child.stderr.take().unwrap().read_to_string(&mut stderr).unwrap();
        panic!("bounded server exited at startup: {status:?}; stderr: {stderr}");
    }
    server
}

fn start_server() -> Server {
    start_server_with(APP, "apps-ws-socket")
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

    for (label, request) in [
        (
            "HTTP version/body readiness",
            b"GET /chat HTTP/1.0\r\nHost: localhost\r\nUpgrade: websocket\r\nConnection: Upgrade\r\nSec-WebSocket-Version: 13\r\nSec-WebSocket-Key: dGhlIHNhbXBsZSBub25jZQ==\r\nSec-WebSocket-Protocol: chat.v1\r\n\r\n".as_slice(),
        ),
        (
            "Upgrade token",
            b"GET /chat HTTP/1.1\r\nHost: localhost\r\nUpgrade: h2c\r\nConnection: Upgrade\r\nSec-WebSocket-Version: 13\r\nSec-WebSocket-Key: dGhlIHNhbXBsZSBub25jZQ==\r\nSec-WebSocket-Protocol: chat.v1\r\n\r\n".as_slice(),
        ),
        (
            "Connection token",
            b"GET /chat HTTP/1.1\r\nHost: localhost\r\nUpgrade: websocket\r\nConnection: keep-alive\r\nSec-WebSocket-Version: 13\r\nSec-WebSocket-Key: dGhlIHNhbXBsZSBub25jZQ==\r\nSec-WebSocket-Protocol: chat.v1\r\n\r\n".as_slice(),
        ),
        (
            "canonical key",
            b"GET /chat HTTP/1.1\r\nHost: localhost\r\nUpgrade: websocket\r\nConnection: Upgrade\r\nSec-WebSocket-Version: 13\r\nSec-WebSocket-Key: dGhlIHNhbXBsZSBub25jZ===\r\nSec-WebSocket-Protocol: chat.v1\r\n\r\n".as_slice(),
        ),
    ] {
        let response = request_head(server.port, request);
        assert!(response.starts_with("HTTP/1.1 400 "), "{label}: {response:?}");
        assert!(!response.contains("Sec-WebSocket-Version:"), "{label}: non-version rejection");
    }

    let earlier_host_wins = request_head(
        server.port,
        b"GET /chat HTTP/1.1\r\nUpgrade: websocket\r\nConnection: Upgrade\r\nSec-WebSocket-Version: 12\r\nSec-WebSocket-Key: bad\r\nSec-WebSocket-Protocol: chat.v1\r\n\r\n",
    );
    assert!(earlier_host_wins.starts_with("HTTP/1.1 400 "), "{earlier_host_wins:?}");
    assert!(!earlier_host_wins.contains("Sec-WebSocket-Version:"), "Host validation precedes version");

    let version_wins = request_head(
        server.port,
        b"GET /chat HTTP/1.1\r\nHost: localhost\r\nUpgrade: websocket\r\nConnection: Upgrade\r\nSec-WebSocket-Version: 12\r\nSec-WebSocket-Key: bad\r\nSec-WebSocket-Protocol: chat.v1\r\n\r\n",
    );
    assert!(version_wins.contains("Sec-WebSocket-Version: 13\r\n"), "version validation precedes key");

    let case_mismatch = request_head(
        server.port,
        b"GET /chat HTTP/1.1\r\nHost: localhost\r\nUpgrade: websocket\r\nConnection: Upgrade\r\nSec-WebSocket-Version: 13\r\nSec-WebSocket-Key: dGhlIHNhbXBsZSBub25jZQ==\r\nSec-WebSocket-Protocol: CHAT.V1\r\n\r\n",
    );
    assert!(case_mismatch.starts_with("HTTP/1.1 400 "), "subprotocol matching is byte-exact");

    let mut sock = connect_retry(server.port);
    sock.set_read_timeout(Some(Duration::from_secs(30))).unwrap();
    sock.set_write_timeout(Some(Duration::from_secs(30))).unwrap();
    sock.write_all(
        b"GET /chat HTTP/1.1\r\nHost: localhost\r\nUpgrade: h2c\r\nUpgrade: websocket\r\nConnection: keep-alive, Upgrade\r\nSec-WebSocket-Version: 13\r\nSec-WebSocket-Key: dGhlIHNhbXBsZSBub25jZQ==\r\nSec-WebSocket-Protocol: other\r\nSec-WebSocket-Protocol: chat.v1\r\n\r\n",
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

    for (length, opcode) in [(126usize, 1u8), (65_536usize, 2u8), (131_072usize, 2u8)] {
        let mut boundary = open_websocket(server.port);
        let payload = vec![if opcode == 1 { b'a' } else { 0xa5 }; length];
        let frame = masked_frame(true, opcode, &payload, [2, 4, 6, 8]);
        if length == 126 {
            for byte in frame {
                boundary.write_all(&[byte]).unwrap();
            }
        } else {
            boundary.write_all(&frame).unwrap();
        }
        assert_eq!(read_frame(&mut boundary), (true, opcode, payload));
        finish_server_close(&mut boundary, 1000);
    }

    let mut unmasked = open_websocket(server.port);
    unmasked.write_all(&[0x81, 0]).unwrap();
    assert_eq!(read_frame(&mut unmasked), (true, 8, vec![3, 234]), "unmasked input closes 1002");

    let mut bad_text = open_websocket(server.port);
    bad_text.write_all(&masked_frame(true, 1, &[0xff], [1, 1, 1, 1])).unwrap();
    assert_eq!(read_frame(&mut bad_text), (true, 8, vec![3, 239]), "invalid UTF-8 closes 1007");

    let mut nonminimal16 = open_websocket(server.port);
    nonminimal16.write_all(&[0x82, 0xfe, 0, 125, 1, 2, 3, 4]).unwrap();
    assert_eq!(read_frame(&mut nonminimal16), (true, 8, vec![3, 234]), "nonminimal 16-bit length closes 1002");

    let mut nonminimal64 = open_websocket(server.port);
    let mut nonminimal64_head = vec![0x82, 0xff];
    nonminimal64_head.extend_from_slice(&65_535u64.to_be_bytes());
    nonminimal64_head.extend_from_slice(&[1, 2, 3, 4]);
    nonminimal64.write_all(&nonminimal64_head).unwrap();
    assert_eq!(read_frame(&mut nonminimal64), (true, 8, vec![3, 234]), "nonminimal 64-bit length closes 1002");

    let mut oversized = open_websocket(server.port);
    let mut oversized_head = vec![0x82, 0xff];
    oversized_head.extend_from_slice(&131_073u64.to_be_bytes());
    oversized_head.extend_from_slice(&[1, 2, 3, 4]);
    oversized.write_all(&oversized_head).unwrap();
    assert_eq!(read_frame(&mut oversized), (true, 8, vec![3, 241]), "rejected-next message byte closes 1009 without reading payload");

    let mut continuation = open_websocket(server.port);
    continuation.write_all(&masked_frame(true, 0, b"x", [2, 3, 4, 5])).unwrap();
    assert_eq!(read_frame(&mut continuation), (true, 8, vec![3, 234]), "orphan continuation closes 1002");

    assert_protocol_close(server.port, &[0xc1, 0x80, 1, 2, 3, 4], "RSV is forbidden");
    assert_protocol_close(server.port, &[0x83, 0x80, 1, 2, 3, 4], "reserved opcode is forbidden");
    assert_protocol_close(server.port, &[0x09, 0x80, 1, 2, 3, 4], "control frames must be final");
    assert_protocol_close(
        server.port,
        &[0x89, 0xfe, 0, 126, 1, 2, 3, 4],
        "control payloads cannot use 126 bytes",
    );
    let mut overlapping_messages = masked_frame(false, 1, b"", [1, 2, 3, 4]);
    overlapping_messages.extend(masked_frame(true, 2, b"", [4, 3, 2, 1]));
    assert_protocol_close(server.port, &overlapping_messages, "a second data opcode cannot interrupt fragments");

    let mut split_utf8 = open_websocket(server.port);
    let mut split_utf8_frames = masked_frame(false, 1, &[0xc3], [1, 4, 1, 4]);
    split_utf8_frames.extend(masked_frame(true, 0, &[0xa9], [2, 7, 1, 8]));
    split_utf8.write_all(&split_utf8_frames).unwrap();
    assert_eq!(read_frame(&mut split_utf8), (true, 1, vec![0xc3, 0xa9]));
    finish_server_close(&mut split_utf8, 1000);

    let mut client_only_close = open_websocket(server.port);
    client_only_close
        .write_all(&masked_frame(true, 8, &[3, 242], [6, 7, 8, 9]))
        .unwrap();
    assert_eq!(read_frame(&mut client_only_close), (true, 8, Vec::new()), "1010 gets an empty acknowledgment");

    let mut empty_close = open_websocket(server.port);
    empty_close.write_all(&masked_frame(true, 8, b"", [6, 6, 6, 6])).unwrap();
    assert_eq!(read_frame(&mut empty_close), (true, 8, Vec::new()), "empty Close is acknowledged empty");

    let mut one_byte_close = open_websocket(server.port);
    one_byte_close.write_all(&masked_frame(true, 8, &[3], [7, 7, 7, 7])).unwrap();
    assert_eq!(read_frame(&mut one_byte_close), (true, 8, vec![3, 234]), "one-byte Close is invalid");

    let mut invalid_reason = open_websocket(server.port);
    invalid_reason
        .write_all(&masked_frame(true, 8, &[3, 232, 0xff], [8, 8, 8, 8]))
        .unwrap();
    assert_eq!(read_frame(&mut invalid_reason), (true, 8, vec![3, 239]), "invalid Close UTF-8 uses 1007");

    let mut reason_123 = vec![3, 232];
    reason_123.extend(std::iter::repeat_n(b'x', 123));
    let mut max_reason = open_websocket(server.port);
    max_reason.write_all(&masked_frame(true, 8, &reason_123, [9, 9, 9, 9])).unwrap();
    assert_eq!(read_frame(&mut max_reason), (true, 8, reason_123), "123-byte Close reason is accepted");

    let mut reason_124 = vec![3, 232];
    reason_124.extend(std::iter::repeat_n(b'x', 124));
    let mut oversized_reason = open_websocket(server.port);
    oversized_reason
        .write_all(&masked_frame(true, 8, &reason_124, [10, 10, 10, 10]))
        .unwrap();
    assert_eq!(read_frame(&mut oversized_reason), (true, 8, vec![3, 234]), "124-byte reason exceeds control framing");

    for code in [
        1000u16, 1001, 1002, 1003, 1007, 1008, 1009, 1011, 1012, 1013, 1014, 3000, 3003,
        3008, 4000, 4999,
    ] {
        let mut close = open_websocket(server.port);
        close.write_all(&masked_frame(true, 8, &code.to_be_bytes(), [1, 3, 5, 7]))
            .unwrap();
        assert_eq!(read_frame(&mut close), (true, 8, code.to_be_bytes().to_vec()), "allowed Close code {code}");
    }
    for code in [999u16, 1004, 1005, 1006, 1015, 1016, 2999, 3001, 3999, 5000] {
        let mut close = open_websocket(server.port);
        close.write_all(&masked_frame(true, 8, &code.to_be_bytes(), [2, 4, 6, 8]))
            .unwrap();
        assert_eq!(read_frame(&mut close), (true, 8, vec![3, 234]), "forbidden Close code {code}");
    }

}

#[test]
fn protocol_ping_and_peer_close_reply_transport_failures_win() {
    if !backend_available() {
        return;
    }
    use std::os::fd::AsRawFd;

    let mut server = start_server_with(APP, "apps-ws-reply-failures");
    for (label, frame) in [
        ("protocol Close", vec![0x81, 0]),
        ("Ping Pong", masked_frame(true, 9, b"ping", [1, 2, 3, 4])),
        ("peer Close echo", masked_frame(true, 8, &[3, 232], [4, 3, 2, 1])),
    ] {
        let mut reset = open_websocket(server.port);
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
            "{label}",
        );
        reset.write_all(&frame).unwrap();
        drop(reset);
        std::thread::sleep(Duration::from_millis(100));
    }
    let _ = server.child.kill();
    let _ = server.child.wait();
    let mut stderr = String::new();
    server.child.stderr.take().unwrap().read_to_string(&mut stderr).unwrap();
    assert_eq!(
        stderr.matches("Code(").count(),
        3,
        "every automatic reply failure must beat the ordinary protocol result:\n{stderr}",
    );
}

fn finish_server_close(sock: &mut TcpStream, expected_code: u16) {
    let (fin, opcode, payload) = read_frame(sock);
    assert!(fin);
    assert_eq!(opcode, 8);
    assert_eq!(payload, expected_code.to_be_bytes());
    sock.write_all(&masked_frame(true, 8, &expected_code.to_be_bytes(), [7, 8, 9, 10]))
        .unwrap();
}

#[test]
fn receive_resource_probe_pins_binary_text_and_maximum_live_byte_peaks() {
    if !backend_available() {
        return;
    }
    let max_message = source_i64_constant(WS_ROOT, "MAX_MESSAGE_BYTES");
    let scratch = source_i64_constant(WS_ROOT, "SCRATCH_BYTES");
    assert_eq!(128 + scratch + 2 * max_message, 1_073_774_720);
    let server = start_server_with(RESOURCE_APP, "apps-ws-resource");

    let mut text = open_websocket(server.port);
    text.write_all(&masked_frame(true, 1, &vec![b'x'; 1024], [1, 2, 3, 4]))
        .unwrap();
    finish_server_close(&mut text, 1000);

    let mut binary = open_websocket(server.port);
    binary
        .write_all(&masked_frame(true, 2, &vec![0x5a; 1024], [4, 3, 2, 1]))
        .unwrap();
    finish_server_close(&mut binary, 1000);

    let growth_app = RESOURCE_APP
        .replace("RESOURCE_MAX: i64 := 1024", "RESOURCE_MAX: i64 := 1025")
        .replace("RESOURCE_BINARY_PEAK: i64 := 33920", "RESOURCE_BINARY_PEAK: i64 := 34948");
    assert_ne!(growth_app, RESOURCE_APP, "the scaled owner must rewrite its exact peak");
    let growth_server = start_server_with(&growth_app, "apps-ws-resource-growth");
    let mut growth = open_websocket(growth_server.port);
    let mut frames = masked_frame(false, 2, &[0x5a; 4], [1, 3, 5, 7]);
    frames.extend(masked_frame(true, 0, &vec![0x5a; 1021], [2, 4, 6, 8]));
    growth.write_all(&frames).unwrap();
    finish_server_close(&mut growth, 1000);
}

#[test]
fn exact_and_rejected_next_source_work_boundaries_are_discriminated() {
    if !backend_available() {
        return;
    }
    let bounded_ws = WS_ROOT.replace(
        "SOURCE_WORK_BYTES: i64 := 1048576",
        "SOURCE_WORK_BYTES: i64 := 24",
    );
    assert_ne!(bounded_ws, WS_ROOT, "the owner must rewrite the one shipped work constant");
    let server = start_server_with_ws(APP, &bounded_ws, "apps-ws-source-work");

    for (opcode, label) in [(0u8, "continuation"), (9, "Ping"), (10, "Pong")] {
        let mut exact = open_websocket(server.port);
        let mut exact_frames = masked_frame(false, 1, b"", [1, 2, 3, 4]);
        for mask in [[2, 3, 4, 5], [3, 4, 5, 6]] {
            exact_frames.extend(masked_frame(opcode != 0, opcode, b"", mask));
        }
        exact_frames.extend(masked_frame(true, 0, b"x", [4, 5, 6, 7]));
        exact.write_all(&exact_frames).unwrap();
        if opcode == 9 {
            for _ in 0..2 {
                assert_eq!(
                    read_frame(&mut exact),
                    (true, 10, Vec::new()),
                    "empty Ping is charged and answered",
                );
            }
        }
        assert_eq!(
            read_frame(&mut exact),
            (true, 1, b"x".to_vec()),
            "exact {label} work exhaustion is allowed",
        );
        finish_server_close(&mut exact, 1000);

        let mut rejected = open_websocket(server.port);
        let mut rejected_frames = masked_frame(false, 1, b"", [1, 1, 1, 1]);
        for mask in [[2, 2, 2, 2], [3, 3, 3, 3], [4, 4, 4, 4]] {
            rejected_frames.extend(masked_frame(opcode != 0, opcode, b"", mask));
        }
        rejected_frames.extend(masked_frame(true, 0, b"x", [5, 5, 5, 5]));
        rejected.write_all(&rejected_frames).unwrap();
        if opcode == 9 {
            for _ in 0..3 {
                assert_eq!(read_frame(&mut rejected), (true, 10, Vec::new()));
            }
        }
        assert_eq!(
            read_frame(&mut rejected),
            (true, 8, vec![3, 241]),
            "the rejected-next header after {label} work closes 1009",
        );
    }

    let control_ws = WS_ROOT.replace(
        "SOURCE_WORK_BYTES: i64 := 1048576",
        "SOURCE_WORK_BYTES: i64 := 28",
    );
    assert_ne!(control_ws, WS_ROOT, "the nonempty-control owner must rewrite the bound");
    let control_server = start_server_with_ws(APP, &control_ws, "apps-ws-control-work");
    for (opcode, label) in [(9u8, "Ping"), (10, "Pong")] {
        let mut exact = open_websocket(control_server.port);
        let mut exact_frames = masked_frame(false, 1, b"", [1, 2, 3, 4]);
        exact_frames.extend(masked_frame(true, opcode, b"ab", [2, 3, 4, 5]));
        exact_frames.extend(masked_frame(true, opcode, b"cd", [3, 4, 5, 6]));
        exact_frames.extend(masked_frame(true, 0, b"x", [4, 5, 6, 7]));
        exact.write_all(&exact_frames).unwrap();
        if opcode == 9 {
            assert_eq!(read_frame(&mut exact), (true, 10, b"ab".to_vec()));
            assert_eq!(read_frame(&mut exact), (true, 10, b"cd".to_vec()));
        }
        assert_eq!(
            read_frame(&mut exact),
            (true, 1, b"x".to_vec()),
            "exact nonempty {label} payload charging is allowed",
        );
        finish_server_close(&mut exact, 1000);

        let mut rejected = open_websocket(control_server.port);
        let mut rejected_frames = masked_frame(false, 1, b"", [1, 1, 1, 1]);
        rejected_frames.extend(masked_frame(true, opcode, b"ab", [2, 2, 2, 2]));
        rejected_frames.extend(masked_frame(true, opcode, b"cd", [3, 3, 3, 3]));
        rejected_frames.extend(masked_frame(true, opcode, b"ef", [4, 4, 4, 4]));
        rejected.write_all(&rejected_frames).unwrap();
        if opcode == 9 {
            assert_eq!(read_frame(&mut rejected), (true, 10, b"ab".to_vec()));
            assert_eq!(read_frame(&mut rejected), (true, 10, b"cd".to_vec()));
        }
        assert_eq!(
            read_frame(&mut rejected),
            (true, 8, vec![3, 241]),
            "the rejected nonempty {label} payload closes 1009 before reading it",
        );
    }
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
