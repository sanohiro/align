//! Compiled `std.http` SSE receive surface. Runtime unit tests own the decoder fault matrix; this
//! target pins HIR/MIR/LLVM construction, caller-buffer views, and stream ownership modes.

mod common;
use common::*;

use std::io::{Read, Write};

fn spawn_response(response: Vec<u8>) -> (u16, std::thread::JoinHandle<Vec<u8>>) {
    let listener = std::net::TcpListener::bind("127.0.0.1:0").expect("bind SSE fixture");
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

#[test]
fn compiled_sse_stream_materializes_events_and_committed_state() {
    if !backend_available() {
        return;
    }
    let source = "\
import std.http
import std.io
pub fn main(args: array<str>) -> Result<(), Error> {
  client := http.client()
  request := http.request(\"GET\", args[1])
  raw := client.request_stream(request)?
  events := raw.sse()
  print(events.status())
  io.stdout.write(events.header(\"content-type\") else \"missing\")?
  io.stdout.write(\"\\n\")?
  mut out := buffer(128)
  loop {
    next := events.next(out)?
    match next {
      Some(event) => {
        io.stdout.write(event.event)?
        io.stdout.write(\"|\")?
        io.stdout.write(event.data)?
        io.stdout.write(\"|\")?
        io.stdout.write(event.last_event_id)?
        io.stdout.write(\"|\")?
        match event.retry_ms {
          Some(value) => print(value),
          None => print(-1),
        }
      },
      None => { break },
    }
  }
  io.stdout.write(events.last_event_id())?
  io.stdout.write(\"\\n\")?
  match events.retry_ms() {
    Some(value) => print(value),
    None => print(-1),
  }
  return Ok(())
}
";
    let client = build_exe("http-sse-stream", source);
    let body = b"\xef\xbb\xbfid: base\nretry: 1500\n\nevent: update\ndata: one\ndata: two\nid: delivered\n\ndata:\n\n";
    let mut response =
        b"HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nContent-Length: ".to_vec();
    response.extend_from_slice(body.len().to_string().as_bytes());
    response.extend_from_slice(b"\r\n\r\n");
    response.extend_from_slice(body);
    let (port, server) = spawn_response(response);
    let url = format!("http://127.0.0.1:{port}/events");
    let output = std::process::Command::new(&client.exe)
        .arg(url)
        .output()
        .expect("run SSE client");
    let request = server.join().unwrap();
    assert_eq!(
        output.status.code(),
        Some(0),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr),
    );
    assert_eq!(
        String::from_utf8_lossy(&output.stdout),
        "200\ntext/event-stream\nupdate|one\ntwo|delivered|1500\nmessage||delivered|1500\ndelivered\n1500\n",
    );
    assert!(request.starts_with(b"GET /events HTTP/1.1\r\n"));
}

#[test]
fn sse_transition_and_cursor_modes_are_checked() {
    let accepted = "\
fn observe(borrow events: http_sse_stream) -> i64 = events.status()
fn observe_header(borrow events: http_sse_stream) -> Option<str> = events.header(\"x\")
fn observe_id(borrow events: http_sse_stream) -> str = events.last_event_id()
fn observe_retry(borrow events: http_sse_stream) -> Option<i64> = events.retry_ms()
fn advance(borrow mut events: http_sse_stream, borrow mut out: buffer) -> Result<Option<http_sse_event>, Error> = events.next(out)
fn advance_owned(events: http_sse_stream, borrow mut out: buffer) -> Result<Option<http_sse_event>, Error> = events.next(out)
fn convert(raw: http_read_stream) -> http_sse_stream = raw.sse()
fn main() -> i32 = 0
";
    let mut source_map = SourceMap::new();
    let checked = check(&mut source_map, "http-sse-stream-modes-ok", accepted);
    assert!(
        !checked.diags.has_errors(),
        "unexpected SSE mode errors:\n{}",
        align_driver::format_diagnostics(&source_map, &checked.diags),
    );
    let _ = lower_to_mir(&checked.hir);

    for (index, source) in [
        "fn bad(borrow events: http_sse_stream, borrow mut out: buffer) -> Result<Option<http_sse_event>, Error> = events.next(out)\nfn main() -> i32 = 0\n",
        "fn bad(borrow raw: http_read_stream) -> http_sse_stream = raw.sse()\nfn main() -> i32 = 0\n",
        "fn bad(raw: http_read_stream) -> i64 {\n  events := raw.sse()\n  return raw.status()\n}\nfn main() -> i32 = 0\n",
    ]
    .into_iter()
    .enumerate()
    {
        assert!(
            check_errs(&format!("http-sse-stream-modes-bad-{index}"), source),
            "forbidden SSE mode fixture {index} compiled",
        );
    }
}

#[test]
fn sse_output_views_cannot_survive_buffer_reuse_or_stream_state_change() {
    let next_reuse = "\
fn bad(events: http_sse_stream, borrow mut out: buffer) -> Result<str, Error> {
  first := events.next(out)? else { return Err(Error.Invalid) }
  second := events.next(out)?
  return Ok(first.data)
}
fn main() -> i32 = 0
";
    assert!(check_errs("http-sse-buffer-generation", next_reuse));

    let id_reuse = "\
fn bad(events: http_sse_stream, borrow mut out: buffer) -> Result<str, Error> {
  id := events.last_event_id()
  next := events.next(out)?
  return Ok(id)
}
fn main() -> i32 = 0
";
    assert!(check_errs("http-sse-id-generation", id_reuse));
}

#[test]
fn zero_capacity_sse_buffer_aborts_before_native_read() {
    if !backend_available() {
        return;
    }
    let source = "\
import std.http
pub fn main(args: array<str>) -> Result<(), Error> {
  client := http.client()
  request := http.request(\"GET\", args[1])
  raw := client.request_stream(request)?
  events := raw.sse()
  mut out := buffer(0)
  events.next(out)?
  return Ok(())
}
";
    let client = build_exe("http-sse-stream-zero-cap", source);
    let (port, server) =
        spawn_response(b"HTTP/1.1 200 OK\r\nContent-Length: 8\r\n\r\ndata:x\n\n".to_vec());
    let output = std::process::Command::new(&client.exe)
        .arg(format!("http://127.0.0.1:{port}/events"))
        .output()
        .expect("run zero-capacity SSE client");
    let _ = server.join().unwrap();
    assert!(
        !output.status.success(),
        "a zero-capacity SSE read must abort"
    );
}

#[test]
fn sse_carriers_use_the_shared_builtin_tag_grammar() {
    let accepted = "\
fn keep_error(error: Error) -> Error = error
fn direct(value: http_sse_stream) -> http_sse_stream = value
fn optional(value: Option<http_sse_stream>) -> Option<http_sse_stream> = value
fn nested(value: Option<Result<http_sse_stream, Error>>) -> Option<Result<http_sse_stream, Error>> = value
fn err_arm(value: Result<i64, http_sse_stream>) -> Result<i64, http_sse_stream> = value
fn both_arms(value: Result<http_sse_stream, http_sse_stream>) -> Result<http_sse_stream, http_sse_stream> = value
fn remap(value: Result<http_sse_stream, Error>) -> Result<http_sse_stream, Error> = value.map_err(keep_error)
fn propagate(value: Result<http_sse_stream, Error>) -> Result<http_sse_stream, Error> {
  events := value?
  return Ok(events)
}
fn require(value: Option<http_sse_stream>) -> Result<http_sse_stream, Error> {
  selected := value else { return Err(Error.Invalid) }
  return Ok(selected)
}
fn choose(value: Option<http_sse_stream>) -> Result<http_sse_stream, Error> {
  selected := match value {
    Some(events) => events,
    None => { return Err(Error.Invalid) },
  }
  return Ok(selected)
}
fn replace(current: http_sse_stream, replacement: http_sse_stream) -> http_sse_stream {
  mut active := current
  active = replacement
  return active
}
fn generic<T>(value: T) -> T = value
fn through_generic(value: http_sse_stream) -> http_sse_stream = generic(value)
fn indirect(value: http_sse_stream) -> http_sse_stream {
  function := direct
  return function(value)
}
fn higher(function: fn(http_sse_stream) -> http_sse_stream, value: http_sse_stream) -> http_sse_stream = function(value)
fn main() -> i32 = 0
";
    let mut source_map = SourceMap::new();
    let checked = check(&mut source_map, "http-sse-stream-carrier-ok", accepted);
    assert!(
        !checked.diags.has_errors(),
        "unexpected SSE carrier errors:\n{}",
        align_driver::format_diagnostics(&source_map, &checked.diags),
    );
    let _ = lower_to_mir(&checked.hir);

    let rejected = [
        "Holder { value: http_sse_stream }\nfn main() -> i32 = 0\n",
        "Holder { value: Option<Result<http_sse_stream, Error>> }\nfn main() -> i32 = 0\n",
        "Choice { Stream(http_sse_stream), Empty }\nfn main() -> i32 = 0\n",
        "Holder<T> { value: T }\nfn bad(value: Holder<http_sse_stream>) -> i32 = 0\nfn main() -> i32 = 0\n",
        "Choice<T> { Value(T), Empty }\nfn bad(value: Choice<http_sse_stream>) -> i32 = 0\nfn main() -> i32 = 0\n",
        "fn bad(value: array<http_sse_stream>) -> i32 = 0\nfn main() -> i32 = 0\n",
        "fn bad(value: slice<http_sse_stream>) -> i32 = 0\nfn main() -> i32 = 0\n",
        "fn bad(value: box<http_sse_stream>) -> i32 = 0\nfn main() -> i32 = 0\n",
        "fn bad(value: (http_sse_stream, i64)) -> i32 = 0\nfn main() -> i32 = 0\n",
        "fn bad(out value: http_sse_stream) -> i32 = 0\nfn main() -> i32 = 0\n",
        "fn bad(events: http_sse_stream) -> i32 {\n  closure := fn { events.status() }\n  return 0\n}\nfn main() -> i32 = 0\n",
        "extern \"C\" fn bad(value: http_sse_stream) -> i32\nfn main() -> i32 = 0\n",
        "fn bad(value: http_sse_stream) -> i32 {\n  print(value)\n  return 0\n}\nfn main() -> i32 = 0\n",
        "fn bad(value: http_sse_stream) -> i32 {\n  copy := value.clone()\n  return 0\n}\nfn main() -> i32 = 0\n",
        "fn bad(left: http_sse_stream, right: http_sse_stream) -> bool = left == right\nfn main() -> i32 = 0\n",
    ];
    for (index, source) in rejected.into_iter().enumerate() {
        assert!(
            check_errs(&format!("http-sse-stream-carrier-bad-{index}"), source),
            "forbidden SSE carrier fixture {index} compiled",
        );
    }
}

#[test]
fn sse_transition_preserves_every_possible_client_dependency() {
    let accepted = "\
import std.http
fn main() -> Result<(), Error> {
  first := http.client()
  second := http.client()
  selected := if true {
    request := http.request(\"GET\", \"http://127.0.0.1/first\")
    raw := first.request_stream(request)?
    raw.sse()
  } else {
    request := http.request(\"GET\", \"http://127.0.0.1/second\")
    raw := second.request_stream(request)?
    raw.sse()
  }
  other := http.request(\"GET\", \"http://127.0.0.1/other\")
  pending := first.request(other)
  print(selected.status())
  return Ok(())
}
";
    let mut source_map = SourceMap::new();
    let checked = check(&mut source_map, "http-sse-stream-client-join", accepted);
    assert!(
        !checked.diags.has_errors(),
        "unexpected SSE provenance errors:\n{}",
        align_driver::format_diagnostics(&source_map, &checked.diags),
    );
    let _ = lower_to_mir(&checked.hir);

    for (index, client) in ["first", "second"].into_iter().enumerate() {
        let rejected = accepted.replace(
            "  print(selected.status())",
            &format!("  moved := {client}\n  print(selected.status())"),
        );
        assert!(
            check_errs(
                &format!("http-sse-stream-client-join-move-{index}"),
                &rejected
            ),
            "moving possible SSE client root {client} compiled",
        );
    }

    let escape = "\
import std.http
fn open() -> Result<http_sse_stream, Error> {
  client := http.client()
  request := http.request(\"GET\", \"http://127.0.0.1/\")
  raw := client.request_stream(request)?
  return Ok(raw.sse())
}
fn main() -> i32 = 0
";
    assert!(check_errs("http-sse-stream-client-escape", escape));
}

#[test]
fn imported_sse_carriers_preserve_interface_and_tls_drop_capability() {
    let files = &[
        (
            "streams.align",
            "\
module streams
pub fn forward(value: http_sse_stream) -> http_sse_stream = value
pub fn nested(value: Option<Result<http_sse_stream, Error>>) -> Option<Result<http_sse_stream, Error>> = value
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
    let differential = diff_check_multi("http-sse-stream-interface", files, "main.align");
    assert!(
        !differential.whole_errors && !differential.per_unit_errors,
        "whole diagnostics:\n{}\nper-unit diagnostics:\n{}",
        differential.whole_diags,
        differential.per_unit_diags,
    );
    if !backend_available() {
        return;
    }
    let per_unit = build_per_unit_multi("http-sse-stream-per-unit", files, "main.align");
    assert_eq!(per_unit.link_and_run().status.code(), Some(0));
    assert!(
        per_unit
            .unit("streams")
            .mir
            .link_libs
            .iter()
            .any(|library| library == "ssl"),
        "a unit that only moves/drops http_sse_stream still needs its TLS-aware free ABI",
    );
}
