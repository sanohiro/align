//! Plan243: shared observation, effects, imports and actual non-consuming HTTP reuse.
mod common;
use common::*;
#[path = "helpers/owned_fixture.rs"]
mod owned_fixture;

#[test]
fn peer_eof_types_effects_and_ownership() {
    for body in [
        "ctx.peer_eof(1)",
        "{ ctx.respond(http.response(200))?; ctx.peer_eof() }",
        "{ temporary().peer_eof() }",
    ] {
        let source = format!("import std.http\nfn temporary() -> http_request_ctx = loop {{}}\nfn observe(ctx: http_request_ctx) -> Result<bool, Error> = {body}\nfn main() {{}}");
        assert!(assert_same_verdict("peer-eof-negative", &[("main.align", &source)], "main.align").diags.has_errors(), "{body}");
    }
    let positive = "import std.http\nHolder { ctx: http_request_ctx }\nfn observe(borrow ctx: http_request_ctx) -> Result<bool, Error> { body := ctx.body(); observed := ctx.peer_eof().map_err(fn error: Error { error })?; print(body.len()); Ok(observed) }\nfn field(borrow holder: Holder) -> Result<bool, Error> = holder.ctx.peer_eof()\nfn branch(borrow ctx: http_request_ctx) -> bool { result := ctx.peer_eof(); value := match result { Ok(eof) => eof, Err(_) => false }; after := ctx.peer_eof() else false; value || after }\nfn main() {}";
    let files = [("main.align", positive)];
    assert!(!assert_same_verdict("peer-eof-positive", &files, "main.align").diags.has_errors(), "{}", check_multi_diagnostics("peer-eof-positive", &files, "main.align"));
    let impure = "import std.http\nfn observe(borrow ctx: http_request_ctx) { values := [1, 2]; result := values.par_map(fn value: i64 { observed := ctx.peer_eof() else false; if observed { value } else { 0 } }); print(result.len()) }\nfn main() {}";
    assert!(assert_same_verdict("peer-eof-impure", &[("main.align", impure)], "main.align").diags.has_errors());
}

#[test]
fn peer_eof_imported_native_and_keepalive() {
    owned_fixture::run("peer_eof_imported_native_and_keepalive", |_| {
        use std::io::{Read, Write};
        use std::net::{Shutdown, TcpListener, TcpStream};
        use std::process::Command;
        use std::time::{Duration, Instant};
        assert!(backend_available(), "native owner requires LLVM");
        let library = r#"module observe
import std.http
pub fn peer<T>(borrow ctx: http_request_ctx, marker: T) -> Result<bool, Error> {
  _ := marker
  path := ctx.path()
  value := ctx.peer_eof()?
  if path != "/first" && path != "/second" { return Err(Error.Invalid) }
  Ok(value)
}
"#;
        let entry = r#"import std.http
import std.cli
import std.time
import observe
pub fn main(args: array<str>) -> Result<(), Error> {
  command := cli.command("owner")
  command.flag_i64("port", 0)
  options := command.parse(args)?
  server := http.serve("127.0.0.1", options.get_i64("port"))?
  server.accept_timeout_ns(2000000000)?
  first := server.accept()?
  a := observe.peer(first, 1)?
  if a { return Err(Error.Invalid) }
  rb := http.response(200)
  rb.body("open")
  first.respond_timeout_ns(rb, 1000000000)?
  second := server.accept()?
  mut probes := 0
  loop {
    if observe.peer(second, true)? { break }
    probes = probes + 1
    if probes == 1000 { return Err(Error.Invalid) }
    time.sleep(1000000)
  }
  reply := http.response(200)
  reply.body("eof")
  second.respond_timeout_ns(reply, 1000000000)?
  Ok(())
}
"#;
        let files = [("observe.align", library), ("main.align", entry)];
        assert!(!assert_same_verdict("peer-eof-import", &files, "main.align").diags.has_errors(), "{}", check_multi_diagnostics("peer-eof-import", &files, "main.align"));
        for per_unit in [false, true] {
            let whole;
            let unit;
            let executable = if per_unit {
                unit = build_per_unit_multi("peer-eof-unit", &files, "main.align");
                let objects = unit.emit_objects(false);
                let refs = objects.iter().map(|path| path.as_path()).collect::<Vec<_>>();
                let executable = unit.dir.join(format!("server{}", std::env::consts::EXE_SUFFIX));
                link_objects(&align_driver::CDriver::default(), &refs, &executable, &unit.link_libs_union(), Profile::Release).unwrap();
                executable
            } else {
                whole = build_exe_multi("peer-eof-whole", &files, "main.align");
                whole.exe.clone()
            };
            let probe = TcpListener::bind("127.0.0.1:0").unwrap();
            let port = probe.local_addr().unwrap().port();
            drop(probe);
            let mut child = Command::new(&executable).args(["--port", &port.to_string()]).spawn().unwrap();
            // The owned fixture guards this server's process group and all artifacts on unwind.
            let deadline = Instant::now() + Duration::from_secs(10);
            let mut socket = loop {
                assert!(Instant::now() < deadline);
                match TcpStream::connect(("127.0.0.1", port)) {
                    Ok(socket) => break socket,
                    Err(_) => std::thread::sleep(Duration::from_millis(2)),
                }
            };
            socket.set_read_timeout(Some(Duration::from_secs(3))).unwrap();
            socket.set_write_timeout(Some(Duration::from_secs(3))).unwrap();
            socket.write_all(b"GET /first HTTP/1.1\r\nHost: h\r\n\r\n").unwrap();
            let expected = b"HTTP/1.1 200 OK\r\nContent-Length: 4\r\n\r\nopen";
            let mut first = vec![0; expected.len()];
            socket.read_exact(&mut first).unwrap();
            assert_eq!(first, expected);
            socket.write_all(b"GET /second HTTP/1.1\r\nHost: h\r\nConnection: close\r\n\r\n").unwrap();
            socket.shutdown(Shutdown::Write).unwrap();
            let mut second = Vec::new();
            socket.read_to_end(&mut second).unwrap();
            assert_eq!(second, b"HTTP/1.1 200 OK\r\nConnection: close\r\nContent-Length: 3\r\n\r\neof");
            while child.try_wait().unwrap().is_none() {
                assert!(Instant::now() < deadline);
                std::thread::sleep(Duration::from_millis(2));
            }
            assert!(child.wait().unwrap().success());
        }
    });
}
