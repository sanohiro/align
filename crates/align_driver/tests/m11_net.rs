//! M11 Slice 1 — std.net `dns.resolve`: resolve a host to its IP-address strings via `getaddrinfo`,
//! returned as an owned `array<string>` (deep-`Drop`, the `fs.read_dir` template). Validates the
//! errno/EAI-status path, the owned-string-array deep drop, the `import std.net` capability gate,
//! and impurity (rejected by `par_map`). (`docs/impl/std-design/net.md` Slice 1; `draft.md` §18.2.)
//!
//! M11 Slice 2 — std.net `tcp_conn`: `tcp.connect(host, port)` opens a TCP connection (an owned Move
//! handle owning the socket fd; `Drop` closes it), and `c.reader()`/`c.writer()` borrow M9
//! reader/writer over the same fd (`owns_fd:false`), region-bound to `c`. Round-trips bytes against
//! an in-process Rust echo listener, and checks the Gate-1 rejections: reader-past-conn escape (P2),
//! conn-as-array-element, conn-in-`par_map`, unbound-temporary receiver (P6), bad/refused ports, and
//! the import gate. (`docs/impl/std-design/net.md` Slice 2; `draft.md` §18.2.)

mod common;
use common::*;

/// Bind an in-process echo server on an ephemeral loopback port; return `(port, join_handle)`. The
/// thread accepts one connection and echoes every chunk until the client closes (EOF) — the m9 io
/// harness pattern. The port is a real OS-assigned `1..=65535` value.
fn spawn_echo_server() -> (u16, std::thread::JoinHandle<()>) {
    use std::io::{Read, Write};
    let listener = std::net::TcpListener::bind("127.0.0.1:0").expect("bind loopback");
    let port = listener.local_addr().unwrap().port();
    let handle = std::thread::spawn(move || {
        if let Ok((mut sock, _)) = listener.accept() {
            let mut buf = [0u8; 256];
            loop {
                match sock.read(&mut buf) {
                    Ok(0) | Err(_) => break,
                    Ok(n) => {
                        if sock.write_all(&buf[..n]).is_err() {
                            break;
                        }
                    }
                }
            }
        }
    });
    (port, handle)
}

/// `dns.resolve("localhost")` returns an owned `array<string>` of IP strings containing at least one
/// loopback form (`127.0.0.1` or `::1`), resolved via `/etc/hosts` even with no external resolver.
/// The array is deep-`Drop`-freed at scope end. If the sandbox has no name resolution at all, the
/// program exits non-zero (the `?` propagates the Err) — skip gracefully.
#[test]
fn dns_resolve_localhost_contains_loopback() {
    if !backend_available() {
        return;
    }
    let prog = "\
import std.net
pub fn main() -> Result<(), Error> {
  ips := dns.resolve(\"localhost\")?
  print(ips.len())
  hit := ips.any(fn ip { ip.contains(\"127.0.0.1\") || ip.contains(\"::1\") })
  if hit {
    print(\"loopback\")
  }
  return Ok(())
}
";
    let out = build_and_run("m11net-localhost", prog);
    if out.status.code() != Some(0) {
        return; // no resolver in this sandbox — skip
    }
    let stdout = String::from_utf8_lossy(&out.stdout);
    let mut lines = stdout.lines();
    let count: i64 = lines.next().unwrap_or("0").parse().unwrap_or(0);
    assert!(count > 0, "localhost resolves to at least one usable IP string; stdout: {stdout:?}");
    assert!(stdout.contains("loopback"), "localhost includes a loopback address (127.0.0.1 or ::1); stdout: {stdout:?}");
}

/// A definitively invalid name (`.invalid` is RFC 6761 reserved and never resolves) is an `Err`,
/// not a value and never an abort — `main` exits non-zero. This holds with or without a resolver
/// (a resolver returns NXDOMAIN → `EAI_NONAME`; no resolver returns `EAI_AGAIN`), so no skip.
#[test]
fn dns_resolve_invalid_host_is_err() {
    if !backend_available() {
        return;
    }
    let prog = "\
import std.net
pub fn main() -> Result<(), Error> {
  ips := dns.resolve(\"no-such-host.invalid\")?
  print(ips.len())
  return Ok(())
}
";
    let out = build_and_run("m11net-invalid", prog);
    assert_ne!(out.status.code(), Some(0), "an unresolvable name is an Err (main exits non-zero), never a value");
    assert!(out.stdout.is_empty(), "the Err path prints nothing (the `?` short-circuits before print)");
}

/// The owned `array<string>` from `dns.resolve` is bound, used, then deep-`Drop`-freed at scope end
/// (each IP string buffer, then the header) — the `DynArray(String)` drop path, no leak. `main`
/// exits 0 (the resolve of `localhost` succeeds via `/etc/hosts`); skip if there is no resolver.
#[test]
fn dns_resolve_array_deep_drops() {
    if !backend_available() {
        return;
    }
    let prog = "\
import std.net
pub fn main() -> Result<(), Error> {
  ips := dns.resolve(\"localhost\")?
  n := ips.len()
  print(n)
  return Ok(())
}
";
    let out = build_and_run("m11net-deepdrop", prog);
    if out.status.code() != Some(0) {
        return; // no resolver — skip
    }
    let n: i64 = String::from_utf8_lossy(&out.stdout).trim().parse().unwrap_or(0);
    assert!(n > 0, "each element is a usable string (len reported > 0)");
}

// --- capability header (import required) -------------------------------------------------------

/// Every `dns.*` use requires `import std.net` (the capability-header rule), like the other `std`
/// namespaces.
#[test]
fn dns_resolve_requires_import() {
    let src = "\
pub fn main() -> Result<(), Error> {
  ips := dns.resolve(\"localhost\")?
  print(ips.len())
  return Ok(())
}
";
    assert!(check_errs("m11net-noimport", src), "dns.resolve without `import std.net` must error");
}

// --- impurity (rejected by par_map) ------------------------------------------------------------

/// `dns.resolve` is a syscall — impure. A closure that calls it is never `Pure`, so `par_map`
/// (which requires a Pure closure) rejects it (the `fs`/`io`/`rand` impurity precedent).
#[test]
fn dns_resolve_rejected_by_par_map() {
    let src = "\
import std.net
fn f(x: i64) -> i64 {
  ips := dns.resolve(\"localhost\") else { return x }
  return ips.len()
}
pub fn main() -> i32 {
  arena {
    ys := [1, 2, 3, 4][0..4].par_map(f).to_array()
    print(ys.len())
  }
  return 0
}
";
    assert!(check_errs("m11net-parmap", src), "a dns.resolve-using (impure) closure must be rejected by par_map");
}

// --- Slice 2: tcp_conn round-trip + Gate-1 rejections ------------------------------------------

/// `tcp.connect` to an in-process echo listener, then write through `c.writer()` and read the echo
/// back through `c.reader()` — the reader/writer-reuse proof. The port is passed as a `--port` flag
/// (an OS-assigned ephemeral port). Both streams borrow the conn's fd (`owns_fd:false`); only the
/// conn's `Drop` closes it, so the whole program exits 0 with no double-close.
#[test]
fn tcp_connect_round_trip_bytes() {
    if !backend_available() {
        return;
    }
    let (port, server) = spawn_echo_server();
    let prog = "\
import std.net
import std.io
import std.cli
pub fn main(args: array<str>) -> Result<(), Error> {
  c := cli.command(\"rt\")
  c.flag_i64(\"port\", 0)
  p := c.parse(args)?
  conn := tcp.connect(\"127.0.0.1\", p.get_i64(\"port\"))?
  w := conn.writer()
  w.write(\"ping\\n\")?
  r := conn.reader()
  b := buffer(64)
  n := r.read(b)?
  print(n)
  io.stdout.write(b.bytes())?
  return Ok(())
}
";
    let out = build_and_run_args("m11net-roundtrip", prog, &["--port", &port.to_string()]);
    let _ = server.join();
    assert_eq!(out.status.code(), Some(0), "stderr: {}", String::from_utf8_lossy(&out.stderr));
    assert_eq!(String::from_utf8_lossy(&out.stdout), "5\nping\n", "the 5 bytes round-trip through the borrowed reader/writer");
}

/// P2 (#297): `c.reader()` borrows the conn's fd, so its region is bound to `c`. Returning it out of
/// the function (past `c`'s `Drop`, which closes the fd) is a use-after-close — a compile error.
#[test]
fn tcp_conn_reader_cannot_escape_conn() {
    let src = "\
import std.net
fn steal() -> Result<reader, Error> {
  conn := tcp.connect(\"127.0.0.1\", 80)?
  return Ok(conn.reader())
}
pub fn main() -> Result<(), Error> {
  r := steal()?
  return Ok(())
}
";
    assert!(check_errs("m11net-reader-escape", src), "a reader borrowing a local conn must not escape the conn's scope");
}

/// A `tcp_conn` is an owned handle bound to one local — it cannot be collected into an array (a
/// copied conn would double-`close` its fd), rejected at construction like `reader`/`writer`.
#[test]
fn tcp_conn_rejected_as_array_element() {
    let src = "\
import std.net
fn f(a: tcp_conn, b: tcp_conn) -> i64 {
  xs := [a, b]
  return xs.len()
}
pub fn main() -> i32 {
  return 0
}
";
    assert!(check_errs("m11net-conn-array", src), "a tcp_conn cannot be an array element");
}

/// `tcp.connect` is a syscall — impure. A closure that connects is never `Pure`, so `par_map`
/// rejects it (the `dns.resolve` / `fs` / `io` impurity precedent).
#[test]
fn tcp_connect_rejected_by_par_map() {
    let src = "\
import std.net
fn f(x: i64) -> i64 {
  conn := tcp.connect(\"127.0.0.1\", 80) else { return x }
  r := conn.reader()
  b := buffer(8)
  n := r.read(b) else { return x }
  return n
}
pub fn main() -> i32 {
  arena {
    ys := [1, 2, 3, 4][0..4].par_map(f).to_array()
    print(ys.len())
  }
  return 0
}
";
    assert!(check_errs("m11net-conn-parmap", src), "a tcp.connect-using (impure) closure must be rejected by par_map");
}

/// P6: a `tcp_conn` is an owned Move handle — an unbound temporary (`tcp.connect(...)?.reader()`) is
/// not dropped yet, so it cannot be a method receiver in v1. Bind it first. (Mirrors the M9
/// bound-receiver restriction on reader/writer.)
#[test]
fn tcp_conn_unbound_temporary_receiver_rejected() {
    let src = "\
import std.net
pub fn main() -> Result<(), Error> {
  r := tcp.connect(\"127.0.0.1\", 80)?.reader()
  return Ok(())
}
";
    assert!(check_errs("m11net-conn-temp-recv", src), "a stream borrow on an unbound conn temporary must be rejected (bind first)");
}

/// Connecting to a closed port is an `Err` (a refused connection), never an abort — `main` exits
/// non-zero. Bind then drop a listener so its port is closed.
#[test]
fn tcp_connect_refused_is_err() {
    if !backend_available() {
        return;
    }
    // Bind then immediately drop a listener: its port is (almost certainly) now closed, so a
    // connect is refused.
    let closed = std::net::TcpListener::bind("127.0.0.1:0").expect("bind");
    let closed_port = closed.local_addr().unwrap().port();
    drop(closed);
    let prog = "\
import std.net
import std.cli
pub fn main(args: array<str>) -> Result<(), Error> {
  c := cli.command(\"rf\")
  c.flag_i64(\"port\", 0)
  p := c.parse(args)?
  conn := tcp.connect(\"127.0.0.1\", p.get_i64(\"port\"))?
  return Ok(())
}
";
    let out = build_and_run_args("m11net-refused", prog, &["--port", &closed_port.to_string()]);
    assert_ne!(out.status.code(), Some(0), "connecting to a closed port is an Err (main exits non-zero), never an abort");
}

/// An out-of-range port (0 or 70000) is an `Err` (Error.Invalid) at runtime — never an abort, never
/// a wrap into a valid port. `main` exits non-zero.
#[test]
fn tcp_connect_invalid_port_is_err() {
    if !backend_available() {
        return;
    }
    // The port arrives through `--port` (parsed to an i64), so one program serves both cases.
    let prog = "\
import std.net
import std.cli
pub fn main(args: array<str>) -> Result<(), Error> {
  c := cli.command(\"bp\")
  c.flag_i64(\"port\", -1)
  p := c.parse(args)?
  conn := tcp.connect(\"127.0.0.1\", p.get_i64(\"port\"))?
  return Ok(())
}
";
    for bad in ["0", "70000"] {
        let out = build_and_run_args("m11net-badport", prog, &["--port", bad]);
        assert_ne!(out.status.code(), Some(0), "port {bad} is Error.Invalid (main exits non-zero), never an abort");
    }
}

/// `tcp.connect` requires `import std.net` (the capability-header rule).
#[test]
fn tcp_connect_requires_import() {
    let src = "\
pub fn main() -> Result<(), Error> {
  conn := tcp.connect(\"127.0.0.1\", 80)?
  return Ok(())
}
";
    assert!(check_errs("m11net-conn-noimport", src), "tcp.connect without `import std.net` must error");
}

// --- Slice 3: tcp_listener (Align listens, Rust connects) + Gate-1 rejections ------------------

/// Full Align-to-Align-shaped round trip in the server direction: an Align program `tcp.listen`s on
/// `127.0.0.1`, then `accept`s **two sequential clients**, echoing each one's message back through
/// the accepted conn's `reader()`/`writer()`. The Rust test drives both clients. The port is probed
/// (bind `:0`, read the port, drop) then passed via `--port`; there is a small TOCTOU window between
/// the probe drop and the Align `bind` (documented tradeoff — a collision would surface as a clean
/// `EADDRINUSE` Err, never a hang). The client retries connecting until the server is listening.
#[test]
fn tcp_listen_accept_serves_two_clients() {
    if !backend_available() {
        return;
    }
    use std::io::{Read, Write};
    use std::net::TcpStream;
    use std::process::{Command, Stdio};

    // Probe a free loopback port (the Align listener can't use port 0 — it's rejected in v1).
    let probe = std::net::TcpListener::bind("127.0.0.1:0").expect("probe bind");
    let port = probe.local_addr().unwrap().port();
    drop(probe);

    // The server: listen, then serve two sequential clients (accept → read one message → echo it).
    // Each accepted conn borrows a reader + writer over its fd (`owns_fd:false`); only the conn's
    // Drop closes the fd. Exits 0 after both clients.
    let prog = "\
import std.net
import std.io
import std.cli
pub fn main(args: array<str>) -> Result<(), Error> {
  c := cli.command(\"srv\")
  c.flag_i64(\"port\", 0)
  p := c.parse(args)?
  l := tcp.listen(\"127.0.0.1\", p.get_i64(\"port\"))?
  conn1 := l.accept()?
  r1 := conn1.reader()
  w1 := conn1.writer()
  b1 := buffer(64)
  n1 := r1.read(b1)?
  w1.write(b1.bytes())?
  conn2 := l.accept()?
  r2 := conn2.reader()
  w2 := conn2.writer()
  b2 := buffer(64)
  n2 := r2.read(b2)?
  w2.write(b2.bytes())?
  return Ok(())
}
";
    let built = build_exe("m11net-listen-serve", prog);
    let mut child = Command::new(&built.exe)
        .args(["--port", &port.to_string()])
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("spawn the Align server");

    // Two sequential clients; each connects (with retry until the server is listening), sends its
    // message, and reads the echo back.
    for i in 0..2 {
        let msg = format!("client{i}\n");
        let mut sock = None;
        for _ in 0..100 {
            match TcpStream::connect(("127.0.0.1", port)) {
                Ok(s) => {
                    sock = Some(s);
                    break;
                }
                Err(_) => std::thread::sleep(std::time::Duration::from_millis(20)),
            }
        }
        let mut sock = match sock {
            Some(s) => s,
            None => {
                // The server never came up (e.g. the probe-port TOCTOU race lost) — surface stderr.
                let _ = child.kill();
                let out = child.wait_with_output().expect("wait server");
                panic!("client {i} could not connect; server stderr: {}", String::from_utf8_lossy(&out.stderr));
            }
        };
        sock.write_all(msg.as_bytes()).expect("client write");
        let mut buf = [0u8; 64];
        let n = sock.read(&mut buf).expect("client read");
        assert_eq!(&buf[..n], msg.as_bytes(), "client {i} receives its echoed message");
    }

    let out = child.wait_with_output().expect("wait server");
    assert_eq!(
        out.status.code(),
        Some(0),
        "the server accepts 2 clients and exits 0; stderr: {}",
        String::from_utf8_lossy(&out.stderr)
    );
}

/// A `tcp_listener` is an owned handle bound to one local — it cannot be collected into an array (a
/// copied listener would double-`close` its fd), rejected at construction like `tcp_conn`.
#[test]
fn tcp_listener_rejected_as_array_element() {
    let src = "\
import std.net
fn f(a: tcp_listener, b: tcp_listener) -> i64 {
  xs := [a, b]
  return xs.len()
}
pub fn main() -> i32 {
  return 0
}
";
    assert!(check_errs("m11net-listener-array", src), "a tcp_listener cannot be an array element");
}

/// `tcp.listen` is a syscall — impure. A closure that listens is never `Pure`, so `par_map` rejects
/// it (the `tcp.connect` / `dns.resolve` impurity precedent).
#[test]
fn tcp_listen_rejected_by_par_map() {
    let src = "\
import std.net
fn f(x: i64) -> i64 {
  l := tcp.listen(\"127.0.0.1\", 8080) else { return x }
  conn := l.accept() else { return x }
  return x
}
pub fn main() -> i32 {
  arena {
    ys := [1, 2, 3, 4][0..4].par_map(f).to_array()
    print(ys.len())
  }
  return 0
}
";
    assert!(check_errs("m11net-listen-parmap", src), "a tcp.listen-using (impure) closure must be rejected by par_map");
}

/// P6: a `tcp_listener` is an owned Move handle — an unbound temporary (`tcp.listen(...)?.accept()`)
/// is not dropped yet, so it cannot be a method receiver in v1. Bind it first. (Mirrors the
/// `tcp_conn` bound-receiver restriction.)
#[test]
fn tcp_listener_unbound_temporary_receiver_rejected() {
    let src = "\
import std.net
pub fn main() -> Result<(), Error> {
  conn := tcp.listen(\"127.0.0.1\", 8080)?.accept()?
  return Ok(())
}
";
    assert!(check_errs("m11net-listener-temp-recv", src), "accept on an unbound listener temporary must be rejected (bind first)");
}

/// `tcp.listen` requires `import std.net` (the capability-header rule).
#[test]
fn tcp_listen_requires_import() {
    let src = "\
pub fn main() -> Result<(), Error> {
  l := tcp.listen(\"127.0.0.1\", 8080)?
  return Ok(())
}
";
    assert!(check_errs("m11net-listen-noimport", src), "tcp.listen without `import std.net` must error");
}

/// Port `0` (kernel-assigned) is rejected in v1 — there is no way to read the bound port back yet, so
/// `tcp.listen("127.0.0.1", 0)` is an `Err` (Error.Invalid) at runtime, never an abort. `main` exits
/// non-zero.
#[test]
fn tcp_listen_port_zero_is_err() {
    if !backend_available() {
        return;
    }
    let prog = "\
import std.net
pub fn main() -> Result<(), Error> {
  l := tcp.listen(\"127.0.0.1\", 0)?
  return Ok(())
}
";
    let out = build_and_run("m11net-listen-port0", prog);
    assert_ne!(out.status.code(), Some(0), "port 0 (kernel-assigned) is rejected in v1 (main exits non-zero), never an abort");
}

/// Adversarial-review F1: an owned reader constructed by a **direct** builtin (`fs.open`) inside a
/// user function, called with only `Static`-region arguments (`args[1]` — an index into a
/// parameter, never `Let`-bound to a shorter region), stays `Static` end to end. `region_of(Call)`
/// folds over zero/`Static` args and produces `Static`, so the caller can bind the returned reader
/// and use it normally (read from it) — contrast with the conservative-rejection case below, which
/// differs only in that the argument itself is `Frame`-region.
#[test]
fn owned_reader_direct_constructor_returnable() {
    let src = "\
import std.fs
import std.io
fn make(p: str) -> Result<reader, Error> {
  return fs.open(p)
}
pub fn main(args: array<str>) -> Result<(), Error> {
  r := make(args[1])?
  buf := buffer(4)
  n := r.read(buf)?
  print(n)
  return Ok(())
}
";
    let mut sm = SourceMap::new();
    let checked = check(&mut sm, "m11net-reader-direct-ctor", src);
    assert!(
        !checked.diags.has_errors(),
        "a reader from a direct builtin constructor, called with only Static args, must be usable by the caller:\n{}",
        align_driver::format_diagnostics(&sm, &checked.diags)
    );
}

/// Adversarial-review F1: the conservative counterpart to the test above. `open_it`'s own reader is
/// an unrelated fixed-path `fs.open` — it borrows nothing from `tag` — but `region_of(Call)` has no
/// per-fn "does this borrow arg i" fact, so it folds in *every* argument's region regardless. Passing
/// a `Frame`-region argument (a `string` local auto-borrowed to `str`, MMv2 slice 7b) taints the call
/// result, so returning it out of `steal` (past the frame) is conservatively rejected — sound (never
/// miscompiles), just imprecise. (Documented on `tracks_region`'s `Reader | Writer` arm.)
#[test]
fn reader_through_call_with_frame_arg_conservatively_rejected() {
    let src = "\
import std.fs
fn open_it(tag: str) -> Result<reader, Error> {
  return fs.open(\"/tmp/align-m11-conservative-test-nonexistent\")
}
fn steal() -> Result<reader, Error> {
  b := builder()
  b.write(\"x\")
  tag := b.to_string()
  return open_it(tag)
}
pub fn main() -> Result<(), Error> {
  r := steal()?
  return Ok(())
}
";
    assert!(
        check_errs("m11net-reader-call-frame-arg", src),
        "a reader returned through a user call with a Frame-region argument must be conservatively rejected"
    );
}
