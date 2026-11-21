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
#[path = "helpers/owned_fixture.rs"]
mod owned_fixture;
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

/// `dns.resolve("localhost")` returns a non-empty owned `array<string>`, resolved via `/etc/hosts`
/// even with no external resolver. Move-element indexing is deferred project-wide, so the runtime
/// unit test checks that an element is a loopback address; this driver gate checks the public
/// surface and deep-`Drop` at scope end. If the sandbox has no name resolution at all, the program
/// exits non-zero (the `?` propagates the Err) — skip gracefully.
#[test]
fn dns_resolve_localhost_is_nonempty() {
    if !backend_available() {
        return;
    }
    let prog = "\
import std.net
pub fn main() -> Result<(), Error> {
  ips := dns.resolve(\"localhost\")?
  print(ips.len())
  return Ok(())
}
";
    let out = build_and_run("m11net-localhost", prog);
    if out.status.code() != Some(0) {
        return; // no resolver in this sandbox — skip
    }
    let stdout = String::from_utf8_lossy(&out.stdout);
    let count: i64 = stdout.trim().parse().unwrap_or(0);
    assert!(count > 0, "localhost resolves to at least one usable IP string; stdout: {stdout:?}");
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
  mut b := buffer(64)
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

/// The server publishes its bound port only after the kernel has assigned it. Both echo owners
/// run in a bounded process group so a failed build or stalled exchange retires every child.
fn listening_server(exe: &std::path::Path) -> (std::process::Child, u16) {
    use std::io::{BufRead, BufReader};
    use std::process::{Command, Stdio};
    let mut child = Command::new(exe).stdin(Stdio::null()).stdout(Stdio::piped()).spawn().unwrap();
    let mut line = String::new();
    BufReader::new(child.stdout.take().unwrap()).read_line(&mut line).unwrap();
    let port = line.trim().parse::<u16>().expect("server must report its bound port");
    assert_ne!(port, 0);
    (child, port)
}

#[test]
fn tcp_listen_accept_serves_two_clients() {
    if !backend_available() { return; }
    owned_fixture::run("tcp_listen_accept_serves_two_clients", |_| {
        use std::io::{Read, Write};
        let src = r#"
import std.net
import std.io
pub fn main() -> Result<(), Error> {
    listener := tcp.listen("127.0.0.1", 0)?
    address := listener.local_addr()?
    print(address.port)
    mut i: i64 := 0
    loop {
        conn := listener.accept()?
        local := conn.local_addr()?
        if local.port != address.port || local.host != address.host { return Err(Error.Invalid) }
        r := conn.reader()
        w := conn.writer()
        mut b := buffer(64)
        n := r.read(b)?
        w.write(b.bytes())?
        i = i + 1
        if i == 2 { break }
    }
    return Ok(())
}
"#;
        let built = build_exe("m11net-listen-serve", src);
        let (mut child, port) = listening_server(&built.exe);
        for msg in [b"hello", b"again"] {
            let mut sock = std::net::TcpStream::connect(("127.0.0.1", port)).unwrap();
            sock.write_all(msg).unwrap();
            let mut got = [0; 5];
            sock.read_exact(&mut got).unwrap();
            assert_eq!(&got, msg);
        }
        assert!(child.wait().unwrap().success());
    });
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

#[test]
fn tcp_connected_local_addr() {
    if !backend_available() { return; }
    let out = build_and_run("m11net-tcp-local", r#"
import std.net
pub fn main() -> Result<(), Error> {
    listener := tcp.listen("127.0.0.1", 0)?
    address := listener.local_addr()?
    conn := tcp.connect(address.host, address.port)?
    client := conn.local_addr()?
    if client.port <= 0 || client.port == address.port || client.host != "127.0.0.1" { return Err(Error.Invalid) }
    return Ok(())
}"#);
    assert!(out.status.success(), "{out:?}");
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
  mut buf := buffer(4)
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

/// Adversarial-review F1: a direct constructor's named return summary is `None`, so an unrelated
/// frame-region argument does not taint the returned owned reader. This is the precise counterpart
/// to the static-argument constructor test above: only a callee that explicitly returns a view of an
/// argument contributes that argument's region to the call result.
#[test]
fn reader_through_call_with_frame_arg_direct_constructor_is_precise() {
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
        !check_errs("m11net-reader-call-frame-arg", src),
        "an unrelated frame-region argument must not taint a direct owned-reader constructor"
    );
}

// --- Slice 4: udp_socket (bind + send_to + recv_from) + Gate-1 rejections ----------------------

/// Two independent senders receive replies using only the endpoint carried by each datagram.
#[test]
fn udp_bind_send_to_recv_from_roundtrip() {
    if !backend_available() { return; }
    owned_fixture::run("udp_bind_send_to_recv_from_roundtrip", |_| {
        let built = build_exe("m11net-udp-roundtrip", r#"
import std.net
pub fn main() -> Result<(), Error> {
    server := udp.bind("127.0.0.1", 0)?
    address := server.local_addr()?
    print(address.port)
    mut buf := buffer(4)
    mut i: i64 := 0
    loop {
        packet := server.recv_from(buf)?
        if packet.n != buf.bytes().len() { return Err(Error.Invalid) }
        server.send_to(buf.bytes(), packet.peer.host, packet.peer.port)?
        i = i + 1
        if i == 2 { break }
    }
    return Ok(())
}"#);
        let (mut child, port) = listening_server(&built.exe);
        let a = std::net::UdpSocket::bind("127.0.0.1:0").unwrap();
        let b = std::net::UdpSocket::bind("127.0.0.1:0").unwrap();
        for (peer, data) in [(&a, b"abcdef".as_slice()), (&b, b"".as_slice())] {
            peer.set_read_timeout(Some(std::time::Duration::from_secs(5))).unwrap();
            peer.send_to(data, ("127.0.0.1", port)).unwrap();
            let mut reply = [0; 8];
            let (n, from) = peer.recv_from(&mut reply).unwrap();
            assert_eq!(from.port(), port);
            assert_eq!(&reply[..n], &data[..data.len().min(4)]);
        }
        assert!(child.wait().unwrap().success());
    });
}

#[test]
fn datagram_records_across_cached_module_boundaries() {
    if !backend_available() { return; }
    owned_fixture::run("datagram_records_across_cached_module_boundaries", |_| {
        let helper = fixture("crates/align_driver/tests/fixtures/socket_endpoint_ownership.align");
        // This module uses std.net only in signatures. Replaying its interface must preserve
        // the nested Move result and the source-level builtin import.
        let types = "module types\nimport std.net\npub fn peer(message: net.datagram) -> net.endpoint = message.peer\n";
        let main = r#"import packets
import types
fn main() -> Result<(), Error> {
    match packets.receive(true, 2).map_err(packets.unchanged) {
        Ok(_) => { return Err(Error.Invalid) }, Err(_) => {}
    }
    ignored := packets.receive(false, 3)?
    result := packets.receive(false, 4)?
    peer := types.peer(result)
    if peer.host != "127.0.0.1" || peer.port <= 0 { return Err(Error.Invalid) }
    print(peer.host)
    return Ok(())
}"#;
        let files = [("main.align", main), ("packets.align", helper), ("types.align", types)];
        let whole = build_and_run_multi("socket-observation-whole", &files, "main.align");
        assert!(whole.status.success(), "{whole:?}");
        assert_eq!(whole.stdout, b"127.0.0.1\n");
        let project = Proj::new("socket-observation-units", &files, "main.align");
        let cache = project.cache();
        for warm in [false, true] {
            let built = thin_build(&project, &cache, 1);
            assert!(if warm { built.all_hit() } else { built.all_miss() });
            assert_eq!(built.run(&project), "127.0.0.1\n");
        }
    });
}

#[cfg(target_os = "linux")]
#[test]
fn exactly_once_endpoint_cleanup() {
    use align_driver::{BuildTarget, CDriver, Profile};
    use std::os::unix::fs::PermissionsExt;
    use std::process::Command;
    owned_fixture::run("exactly_once_endpoint_cleanup", |stage| {
        let helper = fixture("crates/align_driver/tests/fixtures/socket_endpoint_ownership.align");
        let main = r#"import packets
extern "C" fn endpoint_balance() -> i32
fn exercise() -> Result<(), Error> {
    match packets.receive(true, 0).map_err(packets.unchanged) { Ok(_) => {return Err(Error.Invalid)}, Err(_) => {} }
    ignored := packets.receive(false, 1)?
    packet := packets.receive(false, 2)?
    endpoint := packet.peer
    name := endpoint.host
    if name != "127.0.0.1" { return Err(Error.Invalid) }
    return Ok(())
}
fn main() -> i32 {
    exercise() else {return 1}
    if unsafe { endpoint_balance() } != 1 { return 2 }
    return 0
}"#;
        std::fs::write(stage.join("packets.align"), helper).unwrap();
        let path = stage.join("main.align");
        std::fs::write(&path, main).unwrap();
        let mut sources = SourceMap::new();
        let checked = align_driver::check(&mut sources, path.to_str().unwrap(), main);
        assert!(!checked.diags.has_errors(), "{}", align_driver::format_diagnostics(&sources, &checked.diags));
        let mir = align_driver::lower_to_mir(&checked.hir);
        let object = stage.join("main.o");
        align_driver::emit_object_file(&mir, &object, BuildTarget::Baseline, Profile::Release, &[], false).unwrap();
        let c = stage.join("observe.c");
        let observer = stage.join("observe.o");
        std::fs::write(&c, fixture("crates/align_driver/tests/fixtures/socket_endpoint_cleanup.c")).unwrap();
        assert!(Command::new("cc").arg("-c").arg(c).arg("-o").arg(&observer).status().unwrap().success());
        let wrapper = stage.join("link-observer");
        std::fs::write(&wrapper, "#!/bin/sh\nexec cc -Wl,--wrap=align_rt_free,--wrap=align_rt_udp_local_addr,--wrap=align_rt_udp_recv_datagram \"$@\"\n").unwrap();
        std::fs::set_permissions(&wrapper, std::fs::Permissions::from_mode(0o700)).unwrap();
        let output = stage.join("run");
        align_driver::link_objects(&CDriver::explicit(wrapper).unwrap(), &[&object, &observer], &output,
            &mir.link_libs, Profile::Release).unwrap();
        let run = Command::new(output).output().unwrap();
        assert!(run.status.success(), "{run:?}");
    });
}

#[test]
fn recv_from_requires_exclusive_buffer_and_local_handle() {
    let used = check_diagnostics("net-type-only", "import std.net\nfn peer(d:net.datagram)->net.endpoint = d.peer\nfn main() {}");
    assert!(!used.contains("unused import") && !used.contains("error:"), "{used}");
    for (id, body) in [
        ("socket-temp", "point:=udp.bind(\"127.0.0.1\",0)?.local_addr()?"),
        ("read-only", "u:=udp.bind(\"127.0.0.1\",0)?; b:=buffer(1); d:=u.recv_from(b)?"),
        ("live-view", "u:=udp.bind(\"127.0.0.1\",0)?; mut b:=buffer(4); view:=b.bytes(); d:=u.recv_from(b)?; print(view.len())"),
        ("wrong-socket", "u:=udp.bind(\"127.0.0.1\",0)?; d:=u.local_addr(1)?"),
    ] {
        assert!(check_errs(id, &format!("import std.net\nfn main()->Result<(),Error> {{ {body}; return Ok(()) }}")));
    }
}

/// A `udp_socket` is an owned handle bound to one local — it cannot be collected into an array (a
/// copied socket would double-`close` its fd), rejected at construction like `tcp_conn`.
#[test]
fn udp_socket_rejected_as_array_element() {
    let src = "\
import std.net
fn f(a: udp_socket, b: udp_socket) -> i64 {
  xs := [a, b]
  return xs.len()
}
pub fn main() -> i32 {
  return 0
}
";
    assert!(check_errs("m11net-udp-array", src), "a udp_socket cannot be an array element");
}

/// `udp.bind` is a syscall — impure. A closure that binds is never `Pure`, so `par_map` rejects it
/// (the `tcp.connect` / `tcp.listen` impurity precedent).
#[test]
fn udp_bind_rejected_by_par_map() {
    let src = "\
import std.net
fn f(x: i64) -> i64 {
  u := udp.bind(\"127.0.0.1\", 8080) else { return x }
  b := buffer(8)
  n := u.recv_from(b) else { return x }
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
    assert!(check_errs("m11net-udp-parmap", src), "a udp.bind-using (impure) closure must be rejected by par_map");
}

/// P6: a `udp_socket` is an owned Move handle — an unbound temporary (`udp.bind(...)?.recv_from(b)`)
/// is not dropped yet, so it cannot be a method receiver in v1. Bind it first. (Mirrors the
/// `tcp_conn` / `tcp_listener` bound-receiver restriction.)
#[test]
fn udp_socket_unbound_temporary_receiver_rejected() {
    let src = "\
import std.net
pub fn main() -> Result<(), Error> {
  b := buffer(8)
  n := udp.bind(\"127.0.0.1\", 8080)?.recv_from(b)?
  return Ok(())
}
";
    assert!(check_errs("m11net-udp-temp-recv", src), "a method on an unbound udp_socket temporary must be rejected (bind first)");
}

/// `udp.bind` requires `import std.net` (the capability-header rule).
#[test]
fn udp_bind_requires_import() {
    let src = "\
pub fn main() -> Result<(), Error> {
  u := udp.bind(\"127.0.0.1\", 8080)?
  return Ok(())
}
";
    assert!(check_errs("m11net-udp-noimport", src), "udp.bind without `import std.net` must error");
}

/// The endpoint owns its host string after the originating UDP socket closes.
#[test]
fn udp_local_address_survives_handle() {
    if !backend_available() { return; }
    let out = build_and_run("m11net-udp-port0", r#"
import std.net
fn bound() -> Result<net.endpoint, Error> {
    sock := udp.bind("127.0.0.1", 0)?
    return sock.local_addr()
}
pub fn main() -> Result<(), Error> {
    local := bound()?
    if local.port <= 0 || local.host != "127.0.0.1" { return Err(Error.Invalid) }
    return Ok(())
}"#);
    assert!(out.status.success(), "{out:?}");
}

/// `send_to` to port `0` is an `Err` (Error.Invalid) at runtime — a datagram destination port must
/// be `1..=65535`. `main` exits non-zero, never an abort.
#[test]
fn udp_send_to_port_zero_is_err() {
    if !backend_available() {
        return;
    }
    let prog = r#"import std.net
pub fn main() -> Result<(), Error> {
  u := udp.bind("127.0.0.1", 0)?
  u.send_to("x", "127.0.0.1", 0)?
  return Ok(())
}"#;
    let out = build_and_run("m11net-udp-sendport0", prog);
    assert_ne!(out.status.code(), Some(0), "send_to to port 0 is Error.Invalid (main exits non-zero), never an abort");
}
