//! Whole-body HTTP/1.1 chunked response adoption. Runtime units own the exhaustive wire/fault and
//! resource matrices; these tests pin the compiled Align `http.parse` and `client.get` surfaces with
//! the two-SSE-chunk provider shape that motivated align-llm Request 4.

mod common;
use common::*;

use std::io::{Read, Write};

#[test]
fn parse_chunked_response_exposes_only_compacted_payload() {
    if !backend_available() {
        return;
    }
    let prog = "\
import std.http
import std.io
pub fn main() -> Result<(), Error> {
  resp := http.parse(\"HTTP/1.1 200 OK\\r\\nContent-Type: text/event-stream\\r\\nTransfer-Encoding: chunked\\r\\n\\r\\nB;kind=sse\\r\\ndata: one\\n\\n\\r\\nB\\r\\ndata: two\\n\\n\\r\\n0\\r\\nX-Trailer: hidden\\r\\n\\r\\n\")?
  print(resp.status())
  match resp.header(\"content-type\") {
    Some(v) => io.stdout.write(v)?,
    None => io.stdout.write(\"missing\")?,
  }
  io.stdout.write(\"\\n\")?
  io.stdout.write(resp.body())?
  match resp.header(\"X-Trailer\") {
    Some(v) => io.stdout.write(v)?,
    None => io.stdout.write(\"trailers-hidden\\n\")?,
  }
  return Ok(())
}
";
    let out = build_and_run("http-chunked-parse", prog);
    assert_eq!(out.status.code(), Some(0), "stderr: {}", String::from_utf8_lossy(&out.stderr));
    assert_eq!(
        String::from_utf8_lossy(&out.stdout),
        "200\ntext/event-stream\ndata: one\n\ndata: two\n\ntrailers-hidden\n"
    );
}

#[test]
fn client_get_decodes_provider_sse_chunks_end_to_end() {
    if !backend_available() {
        return;
    }
    let prog = "\
import std.http
import std.io
import std.cli
pub fn main(args: array<str>) -> Result<(), Error> {
  c := cli.command(\"chunked\")
  c.flag_str(\"url\", \"\")
  p := c.parse(args)?
  cl := http.client()
  resp := cl.get(p.get_str(\"url\"))?
  print(resp.status())
  io.stdout.write(resp.body())?
  return Ok(())
}
";
    // Compile before the fixture begins blocking in `accept`, so a compiler regression cannot
    // strand the server thread and turn the test failure into a process-wide hang.
    let client = build_exe("http-chunked-provider", prog);
    let listener = std::net::TcpListener::bind("127.0.0.1:0").expect("bind provider fixture");
    let port = listener.local_addr().unwrap().port();
    let server = std::thread::spawn(move || {
        let (mut socket, _) = listener.accept().expect("accept Align client");
        socket.set_read_timeout(Some(std::time::Duration::from_secs(10))).unwrap();
        let mut request = Vec::new();
        let mut scratch = [0u8; 512];
        while !request.windows(4).any(|w| w == b"\r\n\r\n") {
            let n = socket.read(&mut scratch).expect("read request");
            if n == 0 {
                break;
            }
            request.extend_from_slice(&scratch[..n]);
        }
        socket
            .write_all(
                b"HTTP/1.1 200 OK\r\nConnection: close\r\nContent-Type: text/event-stream\r\nTransfer-Encoding: chunked\r\n\r\nB\r\ndata: one\n\n\r\nB\r\ndata: two\n\n\r\n0\r\nProvider-Debug: discarded\r\n\r\n",
            )
            .expect("write chunked response");
        request
    });
    let url = format!("http://127.0.0.1:{port}/v1/chat/completions");
    let out = std::process::Command::new(&client.exe)
        .args(["--url", &url])
        .output()
        .expect("run chunked client");
    let request = server.join().unwrap();
    assert_eq!(out.status.code(), Some(0), "stderr: {}", String::from_utf8_lossy(&out.stderr));
    assert_eq!(String::from_utf8_lossy(&out.stdout), "200\ndata: one\n\ndata: two\n\n");
    assert!(request.starts_with(b"GET /v1/chat/completions HTTP/1.1\r\n"));
}
