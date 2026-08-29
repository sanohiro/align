//! Raw HTTP client receive streaming: framing, dependent ownership, carrier placement, and the
//! zero-capacity precondition. Runtime unit tests own the byte-by-byte decoder fault matrix; this
//! file pins the compiled source surface and native ABI path.

mod common;
use common::*;

use std::io::{Read, Write};

fn spawn_response(response: Vec<u8>) -> (u16, std::thread::JoinHandle<Vec<u8>>) {
    let listener = std::net::TcpListener::bind("127.0.0.1:0").expect("bind stream fixture");
    let port = listener.local_addr().unwrap().port();
    let server = std::thread::spawn(move || {
        let (mut socket, _) = listener.accept().expect("accept Align client");
        socket
            .set_read_timeout(Some(std::time::Duration::from_secs(10)))
            .unwrap();
        let mut request = Vec::new();
        let mut scratch = [0u8; 512];
        while !request.windows(4).any(|window| window == b"\r\n\r\n") {
            let n = socket.read(&mut scratch).expect("read request head");
            if n == 0 {
                break;
            }
            request.extend_from_slice(&scratch[..n]);
        }
        socket.write_all(&response).expect("write response");
        request
    });
    (port, server)
}

fn client_program() -> &'static str {
    "\
import std.http
import std.io
fn drain(stream: http_read_stream) -> Result<(), Error> {
  print(stream.status())
  match stream.header(\"x-test\") {
    Some(value) => io.stdout.write(value)?,
    None => io.stdout.write(\"missing\")?,
  }
  io.stdout.write(\"\\n\")?
  mut out := buffer(4)
  loop {
    n := stream.read(out)?
    if n == 0 { break }
    io.stdout.write(out.bytes())?
  }
  io.stdout.write(\"\\n\")?
  return Ok(())
}
pub fn main(args: array<str>) -> Result<(), Error> {
  client := http.client()
  request := http.request(\"GET\", args[1])
  stream := client.request_stream(request)?
  drain(stream)?
  return Ok(())
}
"
}

#[test]
fn compiled_stream_decodes_fixed_chunked_close_and_interim_responses() {
    if !backend_available() {
        return;
    }
    let client = build_exe("http-read-stream-framing", client_program());
    let cases: [(&str, &[u8], &str); 4] = [
        (
            "fixed",
            b"HTTP/1.1 200 OK\r\nX-Test: fixed\r\nContent-Length: 10\r\n\r\nabcdefghij",
            "200\nfixed\nabcdefghij\n",
        ),
        (
            "chunked",
            b"HTTP/1.1 201 Created\r\nX-Test: chunked\r\nTransfer-Encoding: chunked\r\n\r\n4\r\nWiki\r\n5;ext=1\r\npedia\r\n0\r\nX-Trailer: hidden\r\n\r\n",
            "201\nchunked\nWikipedia\n",
        ),
        (
            "close",
            b"HTTP/1.1 202 Accepted\r\nX-Test: close\r\nConnection: close\r\n\r\nclosed",
            "202\nclose\nclosed\n",
        ),
        (
            "interim",
            b"HTTP/1.1 100 Continue\r\nIgnored: yes\r\n\r\nHTTP/1.1 203 Non-Authoritative Information\r\nX-Test: final\r\nContent-Length: 2\r\n\r\nok",
            "203\nfinal\nok\n",
        ),
    ];

    for (label, response, expected) in cases {
        let (port, server) = spawn_response(response.to_vec());
        let url = format!("http://127.0.0.1:{port}/{label}");
        let output = std::process::Command::new(&client.exe)
            .arg(&url)
            .output()
            .expect("run streaming client");
        let request = server.join().unwrap();
        assert_eq!(
            output.status.code(),
            Some(0),
            "{label} stderr: {}",
            String::from_utf8_lossy(&output.stderr),
        );
        assert_eq!(String::from_utf8_lossy(&output.stdout), expected, "{label}");
        assert!(request.starts_with(format!("GET /{label} HTTP/1.1\r\n").as_bytes()));
    }
}

#[test]
fn zero_capacity_buffer_aborts_before_stream_read_abi() {
    if !backend_available() {
        return;
    }
    let source = "\
import std.http
pub fn main(args: array<str>) -> Result<(), Error> {
  client := http.client()
  request := http.request(\"GET\", args[1])
  stream := client.request_stream(request)?
  mut out := buffer(0)
  stream.read(out)?
  return Ok(())
}
";
    let client = build_exe("http-read-stream-zero-cap", source);
    let (port, server) = spawn_response(
        b"HTTP/1.1 200 OK\r\nConnection: close\r\nContent-Length: 1\r\n\r\nx".to_vec(),
    );
    let url = format!("http://127.0.0.1:{port}/zero");
    let output = std::process::Command::new(&client.exe)
        .arg(url)
        .output()
        .expect("run zero-capacity client");
    let _ = server.join().unwrap();
    assert!(!output.status.success(), "a zero-capacity stream read must abort");
}

#[test]
fn request_is_consumed_and_stream_keeps_client_alive() {
    let request_reuse = "\
import std.http
fn main() -> Result<(), Error> {
  client := http.client()
  request := http.request(\"GET\", \"http://127.0.0.1/\")
  stream := client.request_stream(request)?
  request.header(\"X-Late\", \"no\")
  return Ok(())
}
";
    assert!(check_errs("http-read-stream-request-move", request_reuse));

    let client_escape = "\
import std.http
fn open() -> Result<http_read_stream, Error> {
  client := http.client()
  request := http.request(\"GET\", \"http://127.0.0.1/\")
  return client.request_stream(request)
}
fn main() -> i32 = 0
";
    assert!(check_errs("http-read-stream-client-escape", client_escape));

    let client_move = "\
import std.http
fn main() -> Result<(), Error> {
  client := http.client()
  request := http.request(\"GET\", \"http://127.0.0.1/\")
  stream := client.request_stream(request)?
  moved := client
  print(stream.status())
  return Ok(())
}
";
    assert!(check_errs("http-read-stream-client-move", client_move));
}

#[test]
fn carrier_grammar_accepts_only_builtin_tags_at_function_boundaries() {
    let accepted = "\
fn keep_error(error: Error) -> Error = error
fn direct(value: http_read_stream) -> http_read_stream = value
fn optional(value: Option<http_read_stream>) -> Option<http_read_stream> = value
fn nested(value: Option<Result<http_read_stream, Error>>) -> Option<Result<http_read_stream, Error>> = value
fn err_arm(value: Result<i64, http_read_stream>) -> Result<i64, http_read_stream> = value
fn both_arms(value: Result<http_read_stream, http_read_stream>) -> Result<http_read_stream, http_read_stream> = value
fn remap(value: Result<http_read_stream, Error>) -> Result<http_read_stream, Error> = value.map_err(keep_error)
fn propagate(value: Result<http_read_stream, Error>) -> Result<http_read_stream, Error> {
  stream := value?
  return Ok(stream)
}
fn require(value: Option<http_read_stream>) -> Result<http_read_stream, Error> {
  selected := value else { return Err(Error.Invalid) }
  return Ok(selected)
}
fn choose(value: Option<http_read_stream>) -> Result<http_read_stream, Error> {
  selected := match value {
    Some(stream) => stream,
    None => { return Err(Error.Invalid) },
  }
  return Ok(selected)
}
fn replace(current: http_read_stream, replacement: http_read_stream) -> http_read_stream {
  mut active := current
  active = replacement
  return active
}
fn observe(borrow value: http_read_stream) -> i64 = value.status()
fn observe_header(borrow value: http_read_stream) -> Option<str> = value.header(\"x-test\")
fn advance(borrow mut value: http_read_stream, borrow mut out: buffer) -> Result<i64, Error> = value.read(out)
fn advance_owned(value: http_read_stream, borrow mut out: buffer) -> Result<i64, Error> = value.read(out)
fn generic<T>(value: T) -> T = value
fn through_generic(value: http_read_stream) -> http_read_stream = generic(value)
fn indirect(value: http_read_stream) -> http_read_stream {
  function := direct
  return function(value)
}
fn higher(function: fn(http_read_stream) -> http_read_stream, value: http_read_stream) -> http_read_stream = function(value)
fn main() -> i32 = 0
";
    let mut source_map = SourceMap::new();
    let checked = check(&mut source_map, "http-read-stream-carrier-ok", accepted);
    assert!(
        !checked.diags.has_errors(),
        "unexpected carrier errors:\n{}",
        align_driver::format_diagnostics(&source_map, &checked.diags),
    );
    let _ = lower_to_mir(&checked.hir);

    let rejected = [
        "Holder { value: http_read_stream }\nfn main() -> i32 = 0\n",
        "Holder { value: Option<Result<http_read_stream, Error>> }\nfn main() -> i32 = 0\n",
        "Choice { Stream(http_read_stream), Empty }\nfn main() -> i32 = 0\n",
        "Holder<T> { value: T }\nfn bad(value: Holder<http_read_stream>) -> i32 = 0\nfn main() -> i32 = 0\n",
        "Choice<T> { Value(T), Empty }\nfn bad(value: Choice<http_read_stream>) -> i32 = 0\nfn main() -> i32 = 0\n",
        "fn bad(value: array<http_read_stream>) -> i32 = 0\nfn main() -> i32 = 0\n",
        "fn bad(value: slice<http_read_stream>) -> i32 = 0\nfn main() -> i32 = 0\n",
        "fn bad(value: box<http_read_stream>) -> i32 = 0\nfn main() -> i32 = 0\n",
        "fn bad(value: (http_read_stream, i64)) -> i32 = 0\nfn main() -> i32 = 0\n",
        "fn bad(out value: http_read_stream) -> i32 = 0\nfn main() -> i32 = 0\n",
        "fn bad(stream: http_read_stream) -> i32 {\n  closure := fn { stream.status() }\n  return 0\n}\nfn main() -> i32 = 0\n",
        "extern \"C\" fn bad(value: http_read_stream) -> i32\nfn main() -> i32 = 0\n",
        "fn bad(value: http_read_stream) -> i32 {\n  print(value)\n  return 0\n}\nfn main() -> i32 = 0\n",
        "fn bad(value: http_read_stream) -> i32 {\n  copy := value.clone()\n  return 0\n}\nfn main() -> i32 = 0\n",
        "fn bad(left: http_read_stream, right: http_read_stream) -> bool = left == right\nfn main() -> i32 = 0\n",
        "fn bad(borrow value: http_read_stream, borrow mut out: buffer) -> Result<i64, Error> = value.read(out)\nfn main() -> i32 = 0\n",
    ];
    for (index, source) in rejected.into_iter().enumerate() {
        assert!(
            check_errs(&format!("http-read-stream-carrier-bad-{index}"), source),
            "forbidden carrier fixture {index} compiled",
        );
    }
}

#[test]
fn request_stream_effect_is_rejected_from_parallel_callbacks() {
    let direct = "\
import std.http
fn network(value: i64) -> i64 {
  client := http.client()
  request := http.request(\"GET\", \"http://127.0.0.1/\")
  result := client.request_stream(request)
  return value
}
fn main() -> i32 {
  values := [1, 2].par_map(network)
  return values.len() as i32
}
";
    assert!(check_errs("http-read-stream-parallel-effect", direct));

    let generic = "\
import std.http
fn generic<T>(value: T) -> T {
  client := http.client()
  request := http.request(\"GET\", \"http://127.0.0.1/\")
  result := client.request_stream(request)
  return value
}
fn network(value: i64) -> i64 = generic(value)
fn main() -> i32 {
  values := [1, 2].par_map(network)
  return values.len() as i32
}
";
    assert!(check_errs(
        "http-read-stream-generic-parallel-effect",
        generic,
    ));

    let files = &[
        (
            "streams.align",
            "\
module streams
import std.http
pub fn network(value: i64) -> i64 {
  client := http.client()
  request := http.request(\"GET\", \"http://127.0.0.1/\")
  result := client.request_stream(request)
  return value
}
",
        ),
        (
            "main.align",
            "\
module main
import streams
fn main() -> i32 {
  values := [1, 2].par_map(streams.network)
  return values.len() as i32
}
",
        ),
    ];
    let imported = diff_check_multi(
        "http-read-stream-imported-parallel-effect",
        files,
        "main.align",
    );
    assert!(
        imported.whole_errors && imported.per_unit_errors,
        "whole diagnostics:\n{}\nper-unit diagnostics:\n{}",
        imported.whole_diags,
        imported.per_unit_diags,
    );
}

#[test]
fn conditional_stream_provenance_keeps_every_possible_client_live() {
    let accepted = "\
import std.http
fn main() -> Result<(), Error> {
  first := http.client()
  second := http.client()
  selected := if true {
    request := http.request(\"GET\", \"http://127.0.0.1/first\")
    first.request_stream(request)?
  } else {
    request := http.request(\"GET\", \"http://127.0.0.1/second\")
    second.request_stream(request)?
  }
  print(selected.status())
  return Ok(())
}
";
    let mut source_map = SourceMap::new();
    let checked = check(&mut source_map, "http-read-stream-client-join", accepted);
    assert!(
        !checked.diags.has_errors(),
        "unexpected provenance-join errors:\n{}",
        align_driver::format_diagnostics(&source_map, &checked.diags),
    );
    let _ = lower_to_mir(&checked.hir);

    for (index, client) in ["first", "second"].into_iter().enumerate() {
        let rejected = accepted.replace(
            "  print(selected.status())",
            &format!("  moved := {client}\n  print(selected.status())"),
        );
        assert!(
            check_errs(&format!("http-read-stream-client-join-move-{index}"), &rejected),
            "moving possible client root {client} compiled",
        );
    }
}

#[test]
fn imported_carriers_and_drop_only_units_preserve_interface_and_tls_capability() {
    let files = &[
        (
            "streams.align",
            "\
module streams
pub fn forward(value: http_read_stream) -> http_read_stream = value
pub fn nested(value: Option<Result<http_read_stream, Error>>) -> Option<Result<http_read_stream, Error>> = value
",
        ),
        (
            "main.align",
            "\
module main
import streams
fn main() -> i32 {
  function := streams.forward
  return 0
}
",
        ),
    ];
    let differential = diff_check_multi("http-read-stream-interface", files, "main.align");
    assert!(
        !differential.whole_errors && !differential.per_unit_errors,
        "whole diagnostics:\n{}\nper-unit diagnostics:\n{}",
        differential.whole_diags,
        differential.per_unit_diags,
    );
    if !backend_available() {
        return;
    }
    let per_unit = build_per_unit_multi("http-read-stream-per-unit", files, "main.align");
    assert_eq!(per_unit.link_and_run().status.code(), Some(0));
    assert!(
        per_unit.unit("streams").mir.link_libs.iter().any(|library| library == "ssl"),
        "a unit that only moves/drops http_read_stream still needs its TLS-aware free ABI",
    );
}

#[test]
fn stream_header_view_cannot_outlive_stream() {
    let source = "\
import std.http
fn header(url: str) -> Result<str, Error> {
  client := http.client()
  request := http.request(\"GET\", url)
  stream := client.request_stream(request)?
  return Ok(stream.header(\"x-test\") else \"\")
}
fn main() -> i32 = 0
";
    assert!(check_errs("http-read-stream-header-escape", source));
}
