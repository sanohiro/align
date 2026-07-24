//! std.http I/O timeouts (align-llm Request 2 — the http half). The in-place setters
//! `cl.timeout(ns)` (client default) and `r.timeout(ns)` (per-request override), each `()`, mirror the
//! `r.header()`/`r.body()` bound-local idiom. One `ns` is applied as the connect + send + receive
//! per-op deadline; an expiry yields `Err(Error.Timeout)` (the shared `AL_TIMEOUT` variant). `ns == 0`
//! = no timeout (byte-identical to the pre-timeout blocking client). A request's own `timeout > 0`
//! overrides the client default; an unset request timeout inherits it. A negative `ns` aborts at the
//! setter. (`docs/impl/std-design/http.md` "I/O timeouts".)

mod common;
use common::*;

/// Bind an in-process loopback listener that accepts ONE connection then stays SILENT (never writes a
/// response), holding the socket open for `hold`, then closes. A client that has sent its request and
/// is awaiting the response blocks until either the armed `SO_RCVTIMEO` fires or the peer goes away —
/// the deadline must fire first.
fn spawn_silent_http_server(hold: std::time::Duration) -> (u16, std::thread::JoinHandle<()>) {
    use std::io::Read;
    let listener = std::net::TcpListener::bind("127.0.0.1:0").expect("bind loopback");
    let port = listener.local_addr().unwrap().port();
    let handle = std::thread::spawn(move || {
        if let Ok((mut sock, _)) = listener.accept() {
            let mut tmp = [0u8; 512];
            let _ = sock.read(&mut tmp); // drain the request head, then never respond
            std::thread::sleep(hold);
            drop(sock);
        }
    });
    (port, handle)
}

/// A `cl.get()` against a server that accepts then never responds, with a ~200 ms client timeout
/// armed, returns `Err(Error.Timeout)` — matched DISTINCTLY (exit code 7). The Request-2 acceptance
/// gate: without a timeout this would block on the silent peer for the full 1.5 s hold; the deadline
/// fires first.
#[test]
fn client_read_timeout_yields_timeout_err() {
    if !backend_available() {
        return;
    }
    let (port, server) = spawn_silent_http_server(std::time::Duration::from_millis(1500));
    let prog = "\
import std.http
import std.cli
pub fn main(args: array<str>) -> Result<(), Error> {
  c := cli.command(\"get\")
  c.flag_str(\"url\", \"\")
  p := c.parse(args)?
  cl := http.client()
  cl.timeout(200000000)
  code := match cl.get(p.get_str(\"url\")) {
    Ok(resp) => 0,
    Err(e) => match e {
      Timeout  => 7,
      NotFound => 1,
      Invalid  => 2,
      Denied   => 3,
      Code(cc) => 20 + cc,
    },
  }
  print(code)
  return Ok(())
}
";
    let url = format!("http://127.0.0.1:{port}/hang");
    let out = build_and_run_args("http-timeout-read", prog, &["--url", &url]);
    let _ = server.join();
    assert_eq!(out.status.code(), Some(0), "stderr: {}", String::from_utf8_lossy(&out.stderr));
    assert_eq!(
        String::from_utf8_lossy(&out.stdout),
        "7\n",
        "a response read past the armed client deadline is Err(Error.Timeout)"
    );
}

/// A per-request `r.timeout(ns)` overrides the client default (here left unset = 0 = no timeout): the
/// built request against a silent server times out on its own 200 ms deadline. Proves the override
/// path (`cl.request(req)` reads the request's timeout, not just the client's).
#[test]
fn request_timeout_overrides_client_default() {
    if !backend_available() {
        return;
    }
    let (port, server) = spawn_silent_http_server(std::time::Duration::from_millis(1500));
    let prog = "\
import std.http
import std.cli
pub fn main(args: array<str>) -> Result<(), Error> {
  c := cli.command(\"get\")
  c.flag_str(\"url\", \"\")
  p := c.parse(args)?
  cl := http.client()
  r := http.request(\"GET\", p.get_str(\"url\"))
  r.timeout(200000000)
  code := match cl.request(r) {
    Ok(resp) => 0,
    Err(e) => match e {
      Timeout  => 7,
      NotFound => 1,
      Invalid  => 2,
      Denied   => 3,
      Code(cc) => 20 + cc,
    },
  }
  print(code)
  return Ok(())
}
";
    let url = format!("http://127.0.0.1:{port}/hang");
    let out = build_and_run_args("http-timeout-req-override", prog, &["--url", &url]);
    let _ = server.join();
    assert_eq!(out.status.code(), Some(0), "stderr: {}", String::from_utf8_lossy(&out.stderr));
    assert_eq!(
        String::from_utf8_lossy(&out.stdout),
        "7\n",
        "a per-request r.timeout overrides the (unset) client default"
    );
}

/// The timeout is inert when the response arrives in time: with a generous 5 s client timeout armed,
/// a normal 200 round-trip still succeeds unchanged (the setter arms a socket option, not a new I/O
/// path). This is the byte-identical-when-inert half of the invariant, observed through the surface.
#[test]
fn client_timeout_inert_when_within_deadline() {
    if !backend_available() {
        return;
    }
    use std::io::{Read, Write};
    let listener = std::net::TcpListener::bind("127.0.0.1:0").expect("bind loopback");
    let port = listener.local_addr().unwrap().port();
    let server = std::thread::spawn(move || {
        if let Ok((mut sock, _)) = listener.accept() {
            let mut tmp = [0u8; 512];
            let _ = sock.read(&mut tmp);
            let _ = sock.write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 5\r\n\r\nhello");
        }
    });
    let prog = "\
import std.http
import std.io
import std.cli
pub fn main(args: array<str>) -> Result<(), Error> {
  c := cli.command(\"get\")
  c.flag_str(\"url\", \"\")
  p := c.parse(args)?
  cl := http.client()
  cl.timeout(5000000000)
  resp := cl.get(p.get_str(\"url\"))?
  print(resp.status())
  io.stdout.write(resp.body())?
  return Ok(())
}
";
    let url = format!("http://127.0.0.1:{port}/fast");
    let out = build_and_run_args("http-timeout-inert", prog, &["--url", &url]);
    let _ = server.join();
    assert_eq!(out.status.code(), Some(0), "stderr: {}", String::from_utf8_lossy(&out.stderr));
    assert_eq!(
        String::from_utf8_lossy(&out.stdout),
        "200\nhello",
        "a within-deadline response round-trips unchanged (the timeout is inert)"
    );
}

/// The v1 bound-receiver gate: `cl.timeout(ns)` on an UNBOUND client temporary
/// (`http.client().timeout(...)`) is rejected — the temporary is not dropped yet. Bind it first.
/// (Mirrors `cl.get(...)`'s gate and net's `read_timeout_ns`.)
#[test]
fn client_timeout_unbound_receiver_rejected() {
    let src = "\
import std.http
pub fn main() -> Result<(), Error> {
  http.client().timeout(100)
  return Ok(())
}
";
    assert!(
        check_errs("http-timeout-unbound-client", src),
        "cl.timeout on an unbound client temporary must be rejected (bind first)"
    );
}

/// Likewise `r.timeout(ns)` on an unbound request temporary is rejected.
#[test]
fn request_timeout_unbound_receiver_rejected() {
    let src = "\
import std.http
pub fn main() -> Result<(), Error> {
  http.request(\"GET\", \"http://a/\").timeout(100)
  return Ok(())
}
";
    assert!(
        check_errs("http-timeout-unbound-request", src),
        "r.timeout on an unbound request temporary must be rejected (bind first)"
    );
}

/// `timeout(ns)` requires an `i64` nanosecond argument: a non-integer (a `str`) is a type error,
/// mirroring `c.timeout_ns(ns)` on a command and net's `read_timeout_ns`.
#[test]
fn timeout_non_i64_arg_rejected() {
    let src = "\
import std.http
pub fn main() -> Result<(), Error> {
  cl := http.client()
  cl.timeout(\"soon\")
  return Ok(())
}
";
    assert!(check_errs("http-timeout-bad-arg", src), "cl.timeout must reject a non-i64 argument");
}
