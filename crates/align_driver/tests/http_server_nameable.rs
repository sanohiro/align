//! Plan249: the existing HTTP listener can cross explicit ownership boundaries.
mod common;
use common::*;
#[path = "helpers/owned_fixture.rs"]
mod owned_fixture;

const HELPERS: &str = r#"module listeners
import std.http
pub Holder { server: http_server, label: string }
pub fn open(port: i64) -> Result<http_server, Error> = http.serve("127.0.0.1", port)
pub fn relay<T>(value: T) -> T = value
pub fn receive(borrow server: http_server) -> Result<http_request_ctx, Error> = server.accept()
pub fn receive_generic<T>(borrow server: http_server, marker: T) -> Result<http_request_ctx, Error> {
  _ := marker
  callback := receive
  return callback(server)
}
pub fn configure(borrow mut server: http_server) -> Result<(), Error> {
  server.max_request_body_bytes(1024)
  return server.accept_timeout_ns(1000000000)
}
pub fn replace(borrow mut server: http_server, next: http_server) { server = next }
pub fn consume(server: http_server) {}
pub fn keep_error(error: Error) -> Error = error
"#;

fn check_source(source: &str, accepted: bool) -> String {
    let files = [("listeners.align", HELPERS), ("main.align", source)];
    let verdict = diff_check_multi("http-server-nameable", &files, "main.align");
    assert_eq!(
        verdict.whole_errors, !accepted,
        "whole: {}\n{source}",
        verdict.whole_diags
    );
    assert_eq!(
        verdict.per_unit_errors, !accepted,
        "unit: {}\n{source}",
        verdict.per_unit_diags
    );
    verdict.whole_diags
}

#[test]
fn formation_and_authority() {
    owned_fixture::run("formation_and_authority", |_| {
        let positive = r#"import listeners
Holder<T> { value: T }
fn forward(value: http_server) -> http_server = value
fn optional(value: http_server) -> Option<http_server> = Some(value)
fn nested(value: http_server) -> Result<Option<http_server>, Error> = Ok(optional(value))
fn call(f: fn(borrow http_server) -> Result<http_request_ctx, Error>, borrow server: http_server) -> Result<http_request_ctx, Error> = f(server)
fn record(borrow owner: listeners.Holder) -> Result<http_request_ctx, Error> = call(listeners.receive, owner.server)
fn wrap(value: http_server) -> Holder<http_server> = Holder { value: value }
fn main() {}
"#;
        check_source(positive, true);
        for (declaration, expected) in [
            (
                "fn bad(value: http_server<i64>) {}",
                "takes no type arguments",
            ),
            ("fn bad(value: array<http_server>) {}", "owned I/O handle"),
            ("fn bad(value: slice<http_server>) {}", "owned I/O handle"),
            ("fn bad(value: box<http_server>) {}", "owned I/O handle"),
            ("extern \"C\" fn bad(value: http_server)", "extern"),
            (
                "fn bad(borrow value: http_server) { listeners.consume(value) }",
                "borrow",
            ),
            (
                "fn bad(borrow value: http_server) -> http_server = value",
                "borrow",
            ),
            (
                "fn bad(borrow value: http_server) { value.max_request_body_bytes(1) }",
                "borrow",
            ),
            (
                "fn bad(borrow value: http_server) -> Result<(), Error> = value.accept_timeout_ns(1)",
                "borrow",
            ),
            (
                "fn bad(value: http_server) { listeners.consume(value); listeners.consume(value) }",
                "moved",
            ),
            (
                "fn bad(value: http_server, flag: bool) { if flag { listeners.consume(value) }; listeners.consume(value) }",
                "moved",
            ),
            (
                "fn bad(value: http_server) { f := fn { value.accept() }; _ := f }",
                "capture",
            ),
            (
                "fn bad(value: listeners.Holder) { server := value.server; taken := listeners.relay(value) }",
                "moved",
            ),
            (
                "fn bad(borrow owner: listeners.Holder) -> Result<http_request_ctx, Error> = owner.server.accept()",
                "local",
            ),
            (
                "fn bad() -> Result<http_request_ctx, Error> = listeners.open(12345)?.accept()",
                "local",
            ),
        ] {
            let source = format!("import listeners\n{declaration}\nfn main() {{}}\n");
            let diagnostics = check_source(&source, false);
            assert!(
                diagnostics.contains(expected),
                "expected {expected}: {diagnostics}"
            );
            assert!(!diagnostics.contains("unknown type"), "{diagnostics}");
        }
    });
}

fn unused_port() -> u16 {
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    listener.local_addr().unwrap().port()
}

#[test]
fn ownership_and_cleanup() {
    owned_fixture::run("ownership_and_cleanup", |_| {
        assert!(backend_available());
        let port = unused_port();
        let second = loop {
            let value = unused_port();
            if value != port {
                break value;
            }
        };
        let source = format!(
            r#"import std.http
import listeners
fn cycle(port: i64, select: bool) -> Result<(), Error> {{
  owner := listeners.Holder {{ server: listeners.open(port).map_err(listeners.keep_error)?, label: "live".clone() }}
  transferred := listeners.relay(owner.server)
  if owner.label.len() != 4 {{ return Err(Error.Invalid) }}
  chosen := if select {{ transferred }} else {{ return Err(Error.Invalid) }}
  wrapped: Option<http_server> := Some(chosen)
  mut active := match wrapped {{ Some(value) => value, None => {{ return Err(Error.Invalid) }} }}
  listeners.configure(active)?
  Ok(())
}}
fn replacement() -> Result<(), Error> {{
  mut first := listeners.open({port})?
  next := listeners.open({second})?
  listeners.replace(first, next)
  reopened := listeners.open({port})?
  listeners.consume(reopened)
  Ok(())
}}
fn main() -> Result<(), Error> {{
  mut index := 0
  loop {{
    if index == 6 {{ break }}
    cycle({port}, index % 2 == 0) else {{}}
    reopened := listeners.open({port})?
    listeners.consume(reopened)
    index = index + 1
  }}
  replacement()?
  a := listeners.open({port})?
  b := listeners.open({second})?
  // An actual bind failure leaves the current owners valid.
  match listeners.open({port}) {{ Ok(unexpected) => {{ return Err(Error.Invalid) }}, Err(_) => {{}} }}
  print("ok")
  Ok(())
}}
"#
        );
        check_source(&source, true);
        let files = [
            ("listeners.align", HELPERS),
            ("main.align", source.as_str()),
        ];
        let whole = build_and_run_multi("server-owner-whole", &files, "main.align");
        assert!(
            whole.status.success(),
            "{}",
            String::from_utf8_lossy(&whole.stderr)
        );
        assert_eq!(whole.stdout, b"ok\n");
        let unit = build_per_unit_multi("server-owner-unit", &files, "main.align").link_and_run();
        assert!(
            unit.status.success(),
            "{}",
            String::from_utf8_lossy(&unit.stderr)
        );
        assert_eq!(unit.stdout, b"ok\n");
    });
}

#[test]
fn interfaces_and_cache() {
    owned_fixture::run("interfaces_and_cache", |directory| {
        use align_driver::{CacheContext, UnitReuse, build_package};
        let source = "import listeners\nfn open() -> Result<listeners.Holder, Error> = Ok(listeners.Holder { server: listeners.relay(listeners.open(12345)?), label: \"cache\".clone() })\nfn main() {}\n";
        let entry = directory.join("main.align");
        std::fs::write(&entry, source).unwrap();
        let library = directory.join("listeners.align");
        std::fs::write(&library, HELPERS).unwrap();
        let context = CacheContext::at(directory.join("cache"));
        let snapshot = |expected_hit| {
            let mut sm = SourceMap::new();
            let mut result = build_package(
                &mut sm,
                entry.to_str().unwrap(),
                source,
                &context,
                UnitReuse::Allowed,
            );
            assert!(
                !result.diags.has_errors(),
                "{}",
                align_driver::format_diagnostics(&sm, &result.diags)
            );
            for unit in &result.units {
                assert_eq!(
                    unit.frontend.as_ref().unwrap().hit,
                    expected_hit,
                    "{}",
                    unit.unit
                );
            }
            (0..result.units.len())
                .map(|index| {
                    align_mir::print::program_to_string(result.materialize(index).unwrap())
                })
                .collect::<Vec<_>>()
        };
        let cold = snapshot(false);
        assert_eq!(snapshot(true), cold);
        std::fs::write(
            &library,
            HELPERS.replace(
                "pub fn relay<T>(value: T) -> T = value",
                "pub fn relay<T>(value: T) -> T { print(7); return value }",
            ),
        )
        .unwrap();
        assert_ne!(snapshot(false), cold);
        std::fs::write(&library, HELPERS).unwrap();
        assert_eq!(snapshot(true), cold);
    });
}

#[test]
fn imported_accept_and_context_lifetime() {
    owned_fixture::run("imported_accept_and_context_lifetime", |_| {
        use std::io::{Read, Write};
        use std::net::TcpStream;
        use std::process::Command;
        use std::time::{Duration, Instant};
        assert!(backend_available());
        let port = unused_port();
        let source = format!(
            r#"import std.http
import listeners
fn accept_then_retire() -> Result<http_request_ctx, Error> {{
  mut server := listeners.open({port})?
  listeners.configure(server)?
  owner := listeners.Holder {{ server: server, label: "live".clone() }}
  first := listeners.receive_generic(owner.server, true)?
  initial := http.response(200)
  initial.body("first")
  first.respond_timeout_ns(initial, 1000000000)?
  ctx := listeners.receive_generic(owner.server, 7)?
  listeners.consume(owner.server)
  reopened := listeners.open({port})?
  listeners.consume(reopened)
  return Ok(ctx)
}}
fn main() -> Result<(), Error> {{
  ctx := accept_then_retire()?
  if ctx.path() != "/alive" {{ return Err(Error.Invalid) }}
  response := http.response(200)
  response.body("survived")
  ctx.respond_timeout_ns(response, 1000000000)
}}
"#
        );
        let files = [
            ("listeners.align", HELPERS),
            ("main.align", source.as_str()),
        ];
        check_source(&source, true);
        for per_unit in [false, true] {
            let whole;
            let unit;
            let executable = if per_unit {
                unit = build_per_unit_multi("listener-unit", &files, "main.align");
                let objects = unit.emit_objects(false);
                let refs = objects
                    .iter()
                    .map(|path| path.as_path())
                    .collect::<Vec<_>>();
                let executable = unit.dir.join("listener");
                link_objects(
                    &align_driver::CDriver::default(),
                    &refs,
                    &executable,
                    &unit.link_libs_union(),
                    Profile::Release,
                )
                .unwrap();
                executable
            } else {
                whole = build_exe_multi("listener-whole", &files, "main.align");
                whole.exe.clone()
            };
            let mut child = Command::new(executable).spawn().unwrap();
            // owned_fixture owns the process group, including native descendants on panic.
            let deadline = Instant::now() + Duration::from_secs(10);
            let connect = || loop {
                assert!(Instant::now() < deadline, "listener startup timed out");
                if let Ok(socket) = TcpStream::connect(("127.0.0.1", port)) {
                    socket
                        .set_read_timeout(Some(Duration::from_secs(3)))
                        .unwrap();
                    socket
                        .set_write_timeout(Some(Duration::from_secs(3)))
                        .unwrap();
                    break socket;
                }
                std::thread::sleep(Duration::from_millis(2));
            };
            let mut malformed = connect();
            malformed
                .write_all(b"GET /bad HTTP/1.1\r\nHost: h\r\nBad Header\r\n\r\n")
                .unwrap();
            let mut rejected = Vec::new();
            malformed.read_to_end(&mut rejected).unwrap();
            drop(malformed);
            let mut first = connect();
            first
                .write_all(b"GET /first HTTP/1.1\r\nHost: h\r\nConnection: close\r\n\r\n")
                .unwrap();
            let mut initial = Vec::new();
            first.read_to_end(&mut initial).unwrap();
            assert_eq!(
                initial,
                b"HTTP/1.1 200 OK\r\nConnection: close\r\nContent-Length: 5\r\n\r\nfirst"
            );
            drop(first);
            let mut socket = connect();
            socket
                .write_all(b"GET /alive HTTP/1.1\r\nHost: h\r\nConnection: close\r\n\r\n")
                .unwrap();
            let mut response = Vec::new();
            socket.read_to_end(&mut response).unwrap();
            assert_eq!(
                response,
                b"HTTP/1.1 200 OK\r\nConnection: close\r\nContent-Length: 8\r\n\r\nsurvived"
            );
            while child.try_wait().unwrap().is_none() {
                assert!(Instant::now() < deadline);
                std::thread::sleep(Duration::from_millis(2));
            }
            assert!(child.wait().unwrap().success());
        }
    });
}
