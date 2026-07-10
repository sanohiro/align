//! M11 std.http item 6 — `cl.get_many(urls, max_concurrency)` (R5): batched concurrent GET returning
//! an owned `array<response>` (results in input order, all-or-Err). This exercises the whole pipeline
//! end-to-end (sema → MIR → codegen → runtime) plus the new `array<response>` capability's Gate-1
//! rejections (element move-out, whole-array move / use-after-move, `print`/`==` on the array, unbound
//! receiver, import). The claim-loop ordering under latency jitter + the error/leak paths are covered
//! by the `align_runtime` unit tests; here we verify the compiled Align program observes the array.
//! (`docs/impl/std-design/http.md` item 6.)

mod common;
use common::*;

/// A keepalive loopback HTTP/1.1 server that handles **multiple concurrent connections** (one handler
/// thread per accepted conn), each serving every request on its conn until the client closes — so a
/// bounded-concurrency `get_many` (several overlapping conns, then pooled reuse) is served rather than
/// deadlocked on a single-accept server. Returns the ephemeral port; the listener thread runs for the
/// whole test process. `body` is echoed as every response's body (a fixed keepalive 200).
fn spawn_keepalive_server(body: &'static str) -> u16 {
    use std::io::{Read, Write};
    let listener = std::net::TcpListener::bind("127.0.0.1:0").expect("bind loopback");
    let port = listener.local_addr().unwrap().port();
    std::thread::spawn(move || {
        for stream in listener.incoming() {
            let Ok(mut sock) = stream else { continue };
            std::thread::spawn(move || {
                sock.set_read_timeout(Some(std::time::Duration::from_secs(5))).ok();
                let resp = format!("HTTP/1.1 200 OK\r\nContent-Length: {}\r\n\r\n{}", body.len(), body);
                let mut buf: Vec<u8> = Vec::new();
                let mut tmp = [0u8; 512];
                loop {
                    let end = loop {
                        if let Some(p) = buf.windows(4).position(|w| w == b"\r\n\r\n") {
                            break Some(p + 4);
                        }
                        match sock.read(&mut tmp) {
                            Ok(0) | Err(_) => break None,
                            Ok(n) => buf.extend_from_slice(&tmp[..n]),
                        }
                    };
                    let Some(p) = end else { break };
                    if sock.write_all(resp.as_bytes()).is_err() {
                        break;
                    }
                    buf.drain(..p); // GETs carry no body
                }
            });
        }
    });
    port
}

/// End-to-end: `cl.get_many([u,u,u], 2)` over a keepalive server yields an `array<response>` of length
/// 3; `rs.len()`, `rs[i].status()` (borrow read), and `rs[i].body()` (region-bound view) all work, and
/// the owned array drops cleanly (no leak / double-free) at scope exit.
#[test]
fn get_many_round_trip() {
    if !backend_available() {
        return;
    }
    let port = spawn_keepalive_server("hi");
    let prog = "\
import std.http
import std.io
import std.cli
pub fn main(args: array<str>) -> Result<(), Error> {
  c := cli.command(\"getmany\")
  c.flag_str(\"url\", \"\")
  p := c.parse(args)?
  u := p.get_str(\"url\")
  urls := [u, u, u]
  cl := http.client()
  rs := cl.get_many(urls, 2)?
  print(rs.len())
  print(rs[0].status())
  io.stdout.write(rs[1].body())?
  print(rs[2].status())
  return Ok(())
}
";
    let url = format!("http://127.0.0.1:{port}/path");
    let out = build_and_run_args("m11-http-get-many", prog, &["--url", &url]);
    assert_eq!(out.status.code(), Some(0), "stderr: {}", String::from_utf8_lossy(&out.stderr));
    // len=3, rs[0].status()=200, rs[1].body()="hi" (no newline), rs[2].status()=200.
    assert_eq!(String::from_utf8_lossy(&out.stdout), "3\n200\nhi200\n");
}

/// Empty `urls` → `Ok` empty array; `rs.len()` is 0, and the empty owned array drops cleanly.
#[test]
fn get_many_empty_urls() {
    if !backend_available() {
        return;
    }
    let prog = "\
import std.http
pub fn main() -> Result<(), Error> {
  urls := [\"http://127.0.0.1:1/\"]
  empty := urls[0..0]
  cl := http.client()
  rs := cl.get_many(empty, 4)?
  print(rs.len())
  return Ok(())
}
";
    let out = build_and_run("m11-http-get-many-empty", prog);
    assert_eq!(out.status.code(), Some(0), "stderr: {}", String::from_utf8_lossy(&out.stderr));
    assert_eq!(String::from_utf8_lossy(&out.stdout), "0\n");
}

// --- Gate-1 rejections for the `array<response>` capability ------------------------------------

/// Moving a response OUT of the array (`r := rs[i]`) is rejected in v1 (it would copy the handle and
/// double-free). Methods must be called directly on the element.
#[test]
fn get_many_element_move_out_rejected() {
    let src = "\
import std.http
pub fn main() -> Result<(), Error> {
  urls := [\"http://x/\", \"http://y/\"]
  cl := http.client()
  rs := cl.get_many(urls, 2)?
  r := rs[0]
  return Ok(())
}
";
    assert!(check_errs("m11-getmany-moveout", src), "moving a response out of the array must be rejected");
}

/// A whole-array move nulls the source; using it afterward (`rs.len()` after `rs2 := rs`) is a
/// use-after-move — rejected.
#[test]
fn get_many_array_use_after_move_rejected() {
    let src = "\
import std.http
pub fn main() -> Result<(), Error> {
  urls := [\"http://x/\", \"http://y/\"]
  cl := http.client()
  rs := cl.get_many(urls, 2)?
  rs2 := rs
  print(rs.len())
  return Ok(())
}
";
    assert!(check_errs("m11-getmany-uaf", src), "use of an array<response> after a whole-array move must be rejected");
}

/// `print(rs)` is rejected — an `array<response>` is not printable (only scalars/strings are).
#[test]
fn get_many_print_rejected() {
    let src = "\
import std.http
pub fn main() -> Result<(), Error> {
  urls := [\"http://x/\"]
  cl := http.client()
  rs := cl.get_many(urls, 1)?
  print(rs)
  return Ok(())
}
";
    assert!(check_errs("m11-getmany-print", src), "printing an array<response> must be rejected");
}

/// `rs == rs` is rejected — `==` is scalars + strings only.
#[test]
fn get_many_eq_rejected() {
    let src = "\
import std.http
pub fn main() -> Result<(), Error> {
  urls := [\"http://x/\"]
  cl := http.client()
  rs := cl.get_many(urls, 1)?
  if rs == rs {
    return Ok(())
  }
  return Ok(())
}
";
    assert!(check_errs("m11-getmany-eq", src), "== on an array<response> must be rejected");
}

/// The receiver must be a bound client local — an unbound temporary (`http.client().get_many(...)`) is
/// rejected (the v1 Move-temporary gate, like `get`/`post`).
#[test]
fn get_many_unbound_receiver_rejected() {
    let src = "\
import std.http
pub fn main() -> Result<(), Error> {
  urls := [\"http://x/\"]
  rs := http.client().get_many(urls, 1)?
  return Ok(())
}
";
    assert!(check_errs("m11-getmany-unbound", src), "get_many on an unbound client temporary must be rejected");
}

/// P3 (#297): `rs[i].body()` is a `slice<u8>` view region-bound to the array — returning it past the
/// array's `Drop` is a compile error (a use-after-free). The element borrow inherits the array's
/// region (`region_of` of an `Index`), so the same escape check as `resp.body()` fires.
#[test]
fn get_many_element_body_view_cannot_escape() {
    let src = "\
import std.http
fn steal(urls: slice<str>, fallback: slice<u8>) -> slice<u8> {
  cl := http.client()
  match cl.get_many(urls, 2) {
    Ok(rs) => rs[0].body(),
    Err(e) => fallback,
  }
}
pub fn main() -> i32 {
  return 0
}
";
    let d = check_diagnostics("m11-getmany-body-escape", src);
    assert!(
        d.contains("cannot return a slice that views a local") || d.contains("view"),
        "rs[i].body() must not escape the array:\n{d}"
    );
}

/// `cl.get_many` requires `import std.http` (the client itself does, so this is implied — kept for
/// parity with the other client-method import gates).
#[test]
fn get_many_requires_import() {
    let src = "\
pub fn main() -> Result<(), Error> {
  urls := [\"http://x/\"]
  cl := http.client()
  rs := cl.get_many(urls, 1)?
  return Ok(())
}
";
    assert!(check_errs("m11-getmany-noimport", src), "get_many without import std.http must be rejected");
}
