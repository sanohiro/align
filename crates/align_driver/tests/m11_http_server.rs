//! M11 std.http Slice 4 — the plaintext HTTP/1.1 **server** primitive (NOT a framework).
//! `http.serve(host, port)` binds a listener -> `Result<http_server, Error>` (Move; wraps the net
//! rail's `tcp.listen` — SO_REUSEADDR, backlog 128). `srv.accept()` blocks for one connection, reads +
//! parses the request (a NEW request-head parser with five inbound smuggling guards: strict CRLF, no
//! space before colon, no Transfer-Encoding, origin-form target only, method-token/CR-LF-NUL guards) ->
//! `Result<http_request_ctx, Error>`. `ctx.method()`/`path()`/`header(name)`/`body()` are zero-copy
//! **views** region-bound to `ctx` (#297). `http.response(status)` builds a Move `response_builder`
//! (`rb.header`/`rb.body`; a CR/LF/NUL in a header **aborts**, P6); `ctx.respond(rb)` **consumes both**
//! ctx and rb, serializes (auto Content-Length — a bodiless body-allowed status frames as 0; a caller Content-Length is rejected;
//! `Connection: close`; no Date/Server), one-writes, and closes the fd (v1: one request per conn). All
//! server ops are Impure. The wire-level parse/serialize + the five guards + fd-leak are
//! runtime-unit-tested in `align_runtime`; here we drive a real Align server end-to-end (a Rust client,
//! plus a dogfood run of the shipped Align `cl.get` client) and check the Gate-1 rejections.
//! (`docs/impl/std-design/http.md` Slice 4.)

mod common;
use common::*;

use std::io::{BufRead, Read, Write};
use std::net::TcpStream;
use std::time::{Duration, Instant};

/// A free loopback port via the probe pattern (bind :0, read the port, drop). `http.serve` rejects
/// port 0, so a concrete port is needed; the drop→re-bind window is small (a failure would surface as
/// a clean `EADDRINUSE`, never a hang).
fn free_loopback_port() -> u16 {
    let probe = std::net::TcpListener::bind("127.0.0.1:0").expect("bind probe");
    let port = probe.local_addr().unwrap().port();
    drop(probe);
    port
}

/// Connect to `port` (retrying while the just-spawned server is still binding — a *failed* connect
/// queues nothing, so the first *successful* connect is the one the server's single `accept` gets),
/// send `req`, and return the full response bytes the server wrote (it closes after `respond`).
fn client_exchange(port: u16, req: &[u8]) -> Vec<u8> {
    // One request per connection: keep-alive would leave the socket parked and this read blocked.
    let req = &one_shot(req)[..];
    let deadline = Instant::now() + Duration::from_secs(30);
    loop {
        match TcpStream::connect(("127.0.0.1", port)) {
            Ok(mut sock) => {
                sock.write_all(req).expect("write request");
                let mut resp = Vec::new();
                let _ = sock.read_to_end(&mut resp);
                return resp;
            }
            Err(_) if Instant::now() < deadline => std::thread::sleep(Duration::from_millis(25)),
            Err(e) => panic!("server never came up: {e}"),
        }
    }
}

/// serve → accept → read the request via the getters → respond: a full round-trip against a real Align
/// server driven by a Rust client. The server echoes the method/path into response headers and the
/// request body into the response body; the client sees the exact serialized response.
#[test]
fn serve_accept_respond_round_trip() {
    if !backend_available() {
        return;
    }
    let prog = "\
import std.http
import std.cli
pub fn main(args: array<str>) -> Result<(), Error> {
  c := cli.command(\"srv\")
  c.flag_i64(\"port\", 0)
  p := c.parse(args)?
  srv := http.serve(\"127.0.0.1\", p.get_i64(\"port\"))?
  ctx := srv.accept()?
  rb := http.response(200)
  rb.header(\"X-Method\", ctx.method())
  rb.header(\"X-Path\", ctx.path())
  rb.body(ctx.body())
  ctx.respond(rb)?
  return Ok(())
}
";
    let port = free_loopback_port();
    let server = build_exe("m11-http-srv-rt", prog);
    let mut child = std::process::Command::new(&server.exe)
        .args(["--port", &port.to_string()])
        .spawn()
        .expect("spawn server");
    let resp = client_exchange(port, b"POST /hi HTTP/1.1\r\nHost: h\r\nContent-Length: 5\r\n\r\nhello");
    let status = child.wait().expect("server exits");
    assert!(status.success(), "server exited with {status:?}");
    let text = String::from_utf8_lossy(&resp);
    assert!(text.starts_with("HTTP/1.1 200 OK\r\n"), "status line: {text:?}");
    assert!(text.contains("X-Method: POST\r\n"), "echoed method: {text:?}");
    assert!(text.contains("X-Path: /hi\r\n"), "echoed path: {text:?}");
    assert!(text.contains("Content-Length: 5\r\n"), "auto Content-Length: {text:?}");
    assert!(text.contains("Connection: close\r\n"), "Connection: close: {text:?}");
    assert!(text.ends_with("\r\n\r\nhello"), "echoed body: {text:?}");
}

/// Dogfood: the shipped Align `cl.get` client against the Align server. The server binds, prints a
/// `ready` marker (flushed), accepts one request, and responds `200 pong`; the client GETs it and
/// prints the status + body. Both are real compiled Align programs talking over a loopback socket.
#[test]
fn dogfood_align_client_against_align_server() {
    if !backend_available() {
        return;
    }
    let server_prog = "\
import std.http
import std.cli
pub fn main(args: array<str>) -> Result<(), Error> {
  c := cli.command(\"srv\")
  c.flag_i64(\"port\", 0)
  p := c.parse(args)?
  srv := http.serve(\"127.0.0.1\", p.get_i64(\"port\"))?
  print(\"ready\")
  ctx := srv.accept()?
  rb := http.response(200)
  rb.body(\"pong\")
  ctx.respond(rb)?
  return Ok(())
}
";
    let client_prog = "\
import std.http
import std.io
import std.cli
pub fn main(args: array<str>) -> Result<(), Error> {
  c := cli.command(\"get\")
  c.flag_str(\"url\", \"\")
  p := c.parse(args)?
  cl := http.client()
  resp := cl.get(p.get_str(\"url\"))?
  print(resp.status())
  io.stdout.write(resp.body())?
  return Ok(())
}
";
    let port = free_loopback_port();
    let server = build_exe("m11-http-dogfood-srv", server_prog);
    let mut child = std::process::Command::new(&server.exe)
        .args(["--port", &port.to_string()])
        .stdout(std::process::Stdio::piped())
        .spawn()
        .expect("spawn server");
    // Wait for the server's flushed `ready` line before running the client (so its single `cl.get`
    // connect lands on a bound listener — the Align client does not retry connect).
    {
        let stdout = child.stdout.as_mut().expect("piped stdout");
        let mut reader = std::io::BufReader::new(stdout);
        let mut line = String::new();
        reader.read_line(&mut line).expect("read ready marker");
        assert_eq!(line.trim(), "ready", "server readiness marker");
    }
    let url = format!("http://127.0.0.1:{port}/");
    let out = build_and_run_args("m11-http-dogfood-cli", client_prog, &["--url", &url]);
    let _ = child.wait();
    assert_eq!(out.status.code(), Some(0), "client stderr: {}", String::from_utf8_lossy(&out.stderr));
    assert_eq!(String::from_utf8_lossy(&out.stdout), "200\npong", "Align client sees the Align server's response");
}

/// A caller-supplied `Content-Length` on the response is rejected at `respond` (a response-smuggling
/// guard, mirror of the client serialize) — `respond` returns `Err`, which the program observes.
#[test]
fn respond_rejects_caller_content_length() {
    if !backend_available() {
        return;
    }
    let prog = "\
import std.http
import std.cli
pub fn main(args: array<str>) -> Result<(), Error> {
  c := cli.command(\"srv\")
  c.flag_i64(\"port\", 0)
  p := c.parse(args)?
  srv := http.serve(\"127.0.0.1\", p.get_i64(\"port\"))?
  ctx := srv.accept()?
  rb := http.response(200)
  rb.header(\"Content-Length\", \"10\")
  rb.body(\"hi\")
  match ctx.respond(rb) {
    Ok(_) => print(0)
    Err(_) => print(1)
  }
  return Ok(())
}
";
    let port = free_loopback_port();
    let server = build_exe("m11-http-srv-cl-reject", prog);
    let child = std::process::Command::new(&server.exe)
        .args(["--port", &port.to_string()])
        .stdout(std::process::Stdio::piped())
        .spawn()
        .expect("spawn server");
    // The client just needs to open a connection so `accept` returns; the response never comes.
    let _ = client_exchange(port, b"GET / HTTP/1.1\r\nHost: h\r\n\r\n");
    let out = child.wait_with_output().expect("server exits");
    assert_eq!(String::from_utf8_lossy(&out.stdout), "1\n", "respond rejects a caller Content-Length");
}

/// P6: a CR/LF in a response header name or value **aborts** (header injection → response smuggling).
#[test]
fn response_header_crlf_injection_aborts() {
    if !backend_available() {
        return;
    }
    let inject = "\
import std.http
pub fn main() -> Result<(), Error> {
  rb := http.response(200)
  rb.header(\"X-Evil\", \"a\\r\\nInjected: 1\")
  print(1)
  return Ok(())
}
";
    let out = build_and_run("m11-http-resp-inject", inject);
    assert!(!out.status.success(), "a CR/LF in a response header value must abort (response smuggling)");
}

// --- http.serve_shared (item 9 ①, the prefork listener) ----------------------------------------

/// `http.serve_shared` is the SIBLING of `http.serve`: same bind, plus `SO_REUSEPORT` so N prefork
/// workers may each own their own listener on ONE port. Two shared binds on one port succeed (the
/// plain `serve` cannot — pinned in the runtime unit test), and a shared listener serves an ordinary
/// request exactly like a strict one.
#[test]
fn serve_shared_binds_twice_and_serves() {
    if !backend_available() {
        return;
    }
    // ① Two live shared listeners on one port. `serve` would fail the second bind with EADDRINUSE;
    //    the program only reaches `bound2` if the kernel accepted both.
    let twice = "\
import std.http
import std.cli
pub fn main(args: array<str>) -> Result<(), Error> {
  c := cli.command(\"srv\")
  c.flag_i64(\"port\", 0)
  p := c.parse(args)?
  a := http.serve_shared(\"127.0.0.1\", p.get_i64(\"port\"))?
  b := http.serve_shared(\"127.0.0.1\", p.get_i64(\"port\"))?
  print(\"bound2\")
  return Ok(())
}
";
    let port = free_loopback_port();
    let exe = build_exe("m11-http-serve-shared-twice", twice);
    let out = std::process::Command::new(&exe.exe).args(["--port", &port.to_string()]).output().expect("run");
    assert!(out.status.success(), "two shared binds on one port succeed: {out:?}");
    assert!(String::from_utf8_lossy(&out.stdout).contains("bound2"), "stdout: {:?}", String::from_utf8_lossy(&out.stdout));

    // ② The same listener serves a normal request — SO_REUSEPORT changes nothing else.
    let serve = "\
import std.http
import std.cli
pub fn main(args: array<str>) -> Result<(), Error> {
  c := cli.command(\"srv\")
  c.flag_i64(\"port\", 0)
  p := c.parse(args)?
  srv := http.serve_shared(\"127.0.0.1\", p.get_i64(\"port\"))?
  ctx := srv.accept()?
  rb := http.response(200)
  rb.header(\"X-Path\", ctx.path())
  rb.body(\"shared\")
  ctx.respond(rb)?
  return Ok(())
}
";
    let port = free_loopback_port();
    let server = build_exe("m11-http-serve-shared-rt", serve);
    let mut child = std::process::Command::new(&server.exe).args(["--port", &port.to_string()]).spawn().expect("spawn server");
    let resp = client_exchange(port, b"GET /hi HTTP/1.1\r\nHost: h\r\n\r\n");
    let status = child.wait().expect("server exits");
    assert!(status.success(), "server exited with {status:?}");
    let text = String::from_utf8_lossy(&resp);
    assert!(text.starts_with("HTTP/1.1 200 OK\r\n"), "status line: {text:?}");
    assert!(text.contains("X-Path: /hi\r\n"), "echoed path: {text:?}");
    assert!(text.ends_with("\r\n\r\nshared"), "body: {text:?}");
}

/// The sibling shares every compile-time rule with `http.serve`: the import gate and the bound-receiver
/// gate (it yields the same Move `http_server`).
#[test]
fn serve_shared_shares_the_serve_gates() {
    let noimport = "\
pub fn main() -> Result<(), Error> {
  srv := http.serve_shared(\"127.0.0.1\", 8080)?
  return Ok(())
}
";
    assert!(check_errs("m11-http-shared-noimport", noimport), "http.serve_shared without `import std.http` must be rejected");
    let unbound = "\
import std.http
pub fn main() -> Result<(), Error> {
  ctx := http.serve_shared(\"127.0.0.1\", 8080)?.accept()?
  return Ok(())
}
";
    assert!(check_errs("m11-http-shared-unbound", unbound), "accept on an unbound shared http_server must be rejected");
    let arity = "\
import std.http
pub fn main() -> Result<(), Error> {
  srv := http.serve_shared(\"127.0.0.1\")?
  return Ok(())
}
";
    assert!(check_errs("m11-http-shared-arity", arity), "http.serve_shared needs both a host and a port");
}

// --- compile-time Gate-1 rejections ------------------------------------------------------------

/// `http.serve` / the server methods require `import std.http`.
#[test]
fn server_requires_import() {
    let src = "\
pub fn main() -> Result<(), Error> {
  srv := http.serve(\"127.0.0.1\", 8080)?
  return Ok(())
}
";
    assert!(check_errs("m11-http-srv-noimport", src), "http.serve without `import std.http` must be rejected");
}

/// v1 bound-receiver gate: a method on an unbound owned server / ctx / builder temporary is rejected.
#[test]
fn server_unbound_receivers_rejected() {
    // `accept` on an unbound `http_server` (`serve(...)?` temporary).
    let srv = "\
import std.http
pub fn main() -> Result<(), Error> {
  ctx := http.serve(\"127.0.0.1\", 8080)?.accept()?
  return Ok(())
}
";
    assert!(check_errs("m11-http-srv-unbound", srv), "accept on an unbound http_server must be rejected");
    // `header` on an unbound `response_builder`.
    let rb = "\
import std.http
pub fn main() -> Result<(), Error> {
  http.response(200).header(\"a\", \"b\")
  return Ok(())
}
";
    assert!(check_errs("m11-http-rb-unbound", rb), "a method on an unbound response_builder must be rejected");
}

/// `respond` consumes BOTH `ctx` and `rb`: using either after the call is a moved-value compile error.
#[test]
fn respond_consumes_ctx_and_rb() {
    let use_ctx = "\
import std.http
import std.cli
pub fn main(args: array<str>) -> Result<(), Error> {
  c := cli.command(\"srv\")
  c.flag_i64(\"port\", 0)
  p := c.parse(args)?
  srv := http.serve(\"127.0.0.1\", p.get_i64(\"port\"))?
  ctx := srv.accept()?
  rb := http.response(200)
  ctx.respond(rb)?
  print(ctx.path())
  return Ok(())
}
";
    assert!(check_errs("m11-http-respond-uses-ctx", use_ctx), "using ctx after respond must be rejected (consumed)");
    let use_rb = "\
import std.http
import std.cli
pub fn main(args: array<str>) -> Result<(), Error> {
  c := cli.command(\"srv\")
  c.flag_i64(\"port\", 0)
  p := c.parse(args)?
  srv := http.serve(\"127.0.0.1\", p.get_i64(\"port\"))?
  ctx := srv.accept()?
  rb := http.response(200)
  ctx.respond(rb)?
  rb.body(\"x\")
  return Ok(())
}
";
    assert!(check_errs("m11-http-respond-uses-rb", use_rb), "using rb after respond must be rejected (consumed)");
}

/// A Move server / builder handle cannot be collected into an array element (a copied handle would
/// double-close its fd / double-free its buffers).
#[test]
fn server_handles_not_array_elements() {
    let src = "\
import std.http
pub fn main() -> Result<(), Error> {
  xs := [http.response(200), http.response(201)]
  return Ok(())
}
";
    assert!(check_errs("m11-http-rb-array", src), "a response_builder must not be an array element");
}
