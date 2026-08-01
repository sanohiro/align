//! std.net I/O timeouts (align-llm Request 2, net rail) — the in-place `tcp_conn` deadline setters
//! `c.read_timeout_ns(ns)` / `c.write_timeout_ns(ns)` (`setsockopt(SO_RCVTIMEO/SO_SNDTIMEO)`), each
//! `()`. A read/write past the armed deadline yields `Err(Error.Timeout)` (the shared `AL_TIMEOUT`
//! variant). The connect-timeout substrate lives in `align_rt_tcp_connect` (runtime unit tests); the
//! raw `tcp.connect(host, port)` surface stays timeout-less (Align has no optional args) and lowers a
//! literal `0` (the unchanged blocking connect). (`docs/impl/std-design/net.md` "I/O timeouts".)

mod common;
use common::*;

/// Bind an in-process loopback listener that accepts ONE connection then stays SILENT (never writes),
/// holding the socket open for `hold`, then closes. A client `read` on it blocks until either the
/// armed `SO_RCVTIMEO` fires or the peer goes away — the deadline must fire first.
fn spawn_silent_server(hold: std::time::Duration) -> (u16, std::thread::JoinHandle<()>) {
    let listener = std::net::TcpListener::bind("127.0.0.1:0").expect("bind loopback");
    let port = listener.local_addr().unwrap().port();
    let handle = std::thread::spawn(move || {
        if let Ok((sock, _)) = listener.accept() {
            std::thread::sleep(hold); // send nothing; just hold the connection open
            drop(sock);
        }
    });
    (port, handle)
}

/// A `read` on a silent peer, with a ~200 ms `read_timeout_ns` armed, returns `Err(Error.Timeout)` —
/// matched DISTINCTLY from `Ok`/other error categories (exit code 7). The silent server holds the
/// connection open for 1.5 s, well past the deadline, so a non-firing timeout would instead block
/// until the peer hang-up (a different exit code) — proving the deadline is what fired.
#[test]
fn tcp_read_timeout_yields_timeout_err() {
    if !backend_available() {
        return;
    }
    let (port, server) = spawn_silent_server(std::time::Duration::from_millis(1500));
    let prog = "\
import std.net
import std.io
import std.cli
pub fn main(args: array<str>) -> Result<(), Error> {
  c := cli.command(\"rt\")
  c.flag_i64(\"port\", 0)
  p := c.parse(args)?
  conn := tcp.connect(\"127.0.0.1\", p.get_i64(\"port\"))?
  conn.read_timeout_ns(200000000)
  r := conn.reader()
  mut b := buffer(64)
  code := match r.read(b) {
    Ok(n) => 0,
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
    let out = build_and_run_args("net-timeout-read", prog, &["--port", &port.to_string()]);
    let _ = server.join();
    assert_eq!(out.status.code(), Some(0), "stderr: {}", String::from_utf8_lossy(&out.stderr));
    assert_eq!(
        String::from_utf8_lossy(&out.stdout),
        "7\n",
        "a read past the armed receive deadline is Err(Error.Timeout)"
    );
}

/// `read_timeout_ns` is inert when data arrives in time: with a generous 5 s deadline armed, an echo
/// round-trip still succeeds unchanged (the setter is a plain socket option, not a new I/O path). Also
/// exercises `write_timeout_ns` (armed, never exceeded) — both setters accepted on a bound conn.
#[test]
fn tcp_timeouts_inert_when_within_deadline() {
    if !backend_available() {
        return;
    }
    use std::io::{Read, Write};
    let listener = std::net::TcpListener::bind("127.0.0.1:0").expect("bind loopback");
    let port = listener.local_addr().unwrap().port();
    let server = std::thread::spawn(move || {
        if let Ok((mut sock, _)) = listener.accept() {
            let mut buf = [0u8; 64];
            if let Ok(n) = sock.read(&mut buf) {
                let _ = sock.write_all(&buf[..n]);
            }
        }
    });
    let prog = "\
import std.net
import std.io
import std.cli
pub fn main(args: array<str>) -> Result<(), Error> {
  c := cli.command(\"rt\")
  c.flag_i64(\"port\", 0)
  p := c.parse(args)?
  conn := tcp.connect(\"127.0.0.1\", p.get_i64(\"port\"))?
  conn.read_timeout_ns(5000000000)
  conn.write_timeout_ns(5000000000)
  w := conn.writer()
  w.write(\"ping\\n\")?
  r := conn.reader()
  mut b := buffer(64)
  n := r.read(b)?
  print(n)
  io.stdout.write(b.bytes())?
  return Ok(())
}
";
    let out = build_and_run_args("net-timeout-inert", prog, &["--port", &port.to_string()]);
    let _ = server.join();
    assert_eq!(out.status.code(), Some(0), "stderr: {}", String::from_utf8_lossy(&out.stderr));
    assert_eq!(
        String::from_utf8_lossy(&out.stdout),
        "5\nping\n",
        "a within-deadline read/write round-trips unchanged (the timeout is inert)"
    );
}

/// The v1 bound-receiver gate: `read_timeout_ns` / `write_timeout_ns` on an UNBOUND conn temporary
/// (`tcp.connect(...)?.read_timeout_ns(...)`) is rejected — the temporary is not dropped yet, so
/// setting an option on its about-to-leak fd is meaningless. Bind the conn first. (Mirrors the
/// `c.reader()` / `c.writer()` gate.)
#[test]
fn tcp_read_timeout_unbound_temporary_receiver_rejected() {
    let src = "\
import std.net
pub fn main() -> Result<(), Error> {
  tcp.connect(\"127.0.0.1\", 80)?.read_timeout_ns(100)
  return Ok(())
}
";
    assert!(
        check_errs("net-timeout-unbound-recv", src),
        "a timeout setter on an unbound conn temporary must be rejected (bind first)"
    );
}

/// `read_timeout_ns` requires an `i64` nanosecond argument: a non-integer (a `str`) is a type error,
/// mirroring `c.timeout_ns(ns)` on a command.
#[test]
fn tcp_read_timeout_non_i64_arg_rejected() {
    let src = "\
import std.net
pub fn main() -> Result<(), Error> {
  conn := tcp.connect(\"127.0.0.1\", 80)?
  conn.read_timeout_ns(\"soon\")
  return Ok(())
}
";
    assert!(
        check_errs("net-timeout-bad-arg", src),
        "read_timeout_ns must reject a non-i64 argument"
    );
}
