//! Plan250: ordinary Copy request views preserve owner authority and provenance.
mod common;
use common::*;
#[path = "helpers/owned_fixture.rs"]
mod owned_fixture;

const HELPERS: &str = r#"module requests
import std.http
pub Request { view: http_request_view }
pub fn relay<T>(value: T) -> T = value
pub fn view(borrow ctx: http_request_ctx) -> http_request_view = ctx.view()
pub fn path(value: http_request_view) -> str = value.path()
pub fn body(value: http_request_view) -> slice<u8> = value.body()
pub fn headers(value: http_request_view) -> http_headers = value.headers()
pub fn checkpoint(callback: fn() -> bool) -> bool = callback()
pub fn observe(value: Request) -> bool = value.view.peer_eof() else false
pub fn stream(borrow ctx: http_request_ctx) -> Result<http_stream, Error> = ctx.respond_stream(http.response(200))
pub fn upgrade(borrow ctx: http_request_ctx) -> Result<(), Error> { stream := ctx.respond_upgrade(http.response(101))?; Ok(()) }
pub fn keep(error: Error) -> Error = error
"#;

fn check(source: &str, accepted: bool) -> String {
    let files = [("requests.align", HELPERS), ("main.align", source)];
    let verdict = diff_check_multi("request-view", &files, "main.align");
    assert_eq!(verdict.whole_errors, !accepted, "whole: {}\n{source}", verdict.whole_diags);
    assert_eq!(verdict.per_unit_errors, !accepted, "unit: {}\n{source}", verdict.per_unit_diags);
    verdict.whole_diags
}

#[test]
fn formation_and_authority() {
    owned_fixture::run("formation_and_authority", |_| {
        check(r#"import requests
import std.http
Holder<T> { value: T }
fn annotated(value: fn() -> http_request_view, headers: fn() -> http_headers) {}
fn forwarded(borrow ctx: http_request_ctx) -> http_request_view = requests.relay(ctx.view())
fn read(borrow ctx: http_request_ctx) -> Result<(), Error> {
  value := requests.Request { view: requests.view(ctx) }
  callback := fn { requests.observe(value) }
  _ := requests.checkpoint(callback)
  _ := requests.path(value.view)
  _ := requests.headers(value.view).get("host")
  _ := requests.body(requests.relay(value.view))
  _ := value.view.method()
  _ := value.view.upgrade_ready()
  _ := Holder { value: value.view }
  Ok(())
}
fn main() {}
"#, true);
        for (declaration, expected) in [
            ("fn bad(value: http_request_view<i64>) {}", "takes no type arguments"),
            ("fn bad(value: Option<http_request_view>) {}", "http_request_view"),
            ("fn bad(value: Result<http_request_view, Error>) {}", "http_request_view"),
            ("fn bad(value: array<http_request_view>) {}", "http_request_view"),
            ("fn bad(value: slice<http_request_view>) {}", "http_request_view"),
            ("fn bad(value: box<http_request_view>) {}", "http_request_view"),
            ("fn bad(value: fn(http_request_view) -> bool) {}", "http_request_view"),
            ("fn bad() { f := requests.path }", "function value"),
            ("fn bad(borrow ctx:http_request_ctx) { v:=ctx.view(); f := fn { v }; _:=f }", "return"),
            ("fn bad(borrow ctx:http_request_ctx) { v:=ctx.headers(); f := fn { v }; _:=f }", "return"),
            ("extern \"C\" fn bad(value: http_request_view)", "extern"),
            ("fn bad(value: http_request_view) { value.respond(http.response(200)) }", "no method"),
            ("fn bad(value:http_request_view) { mut bytes:=value.body(); bytes.fill(0) }", "read-only"),
            ("fn bad(value: http_request_view) { value.respond_stream(http.response(200)) }", "no method"),
            ("fn bad(value: http_request_view) { value.respond_upgrade(http.response(101)) }", "no method"),
            ("fn bad(borrow ctx: http_request_ctx) { _ := ctx.view(1) }", "no arguments"),
            ("fn bad(borrow ctx: http_request_ctx) { f := fn { ctx.peer_eof() }; _ := f }", "capture"),
            ("fn temporary() -> http_request_ctx = loop {}\nfn bad() { _ := temporary().view() }", "local"),
        ] {
            let diagnostics = check(&format!("import requests\nimport std.http\n{declaration}\nfn main() {{}}\n"), false);
            assert!(diagnostics.contains(expected), "{declaration}: {diagnostics}");
        }
    });
}

#[test]
fn lifetime_and_control_flow() {
    owned_fixture::run("lifetime_and_control_flow", |_| {
        let prefix = "import requests\nimport std.http\n";
        for declaration in [
            "fn bad(ctx: http_request_ctx) -> http_request_view = ctx.view()",
            "fn bad(ctx: http_request_ctx) -> slice<u8> = requests.body(ctx.view())",
            "fn bad(ctx: http_request_ctx) -> requests.Request = requests.Request { view: ctx.view() }",
            "fn bad(ctx: http_request_ctx) -> fn() -> bool { v := ctx.view(); return fn { v.peer_eof() else false } }",
            "fn bad(ctx: http_request_ctx) -> Result<(), Error> { v := ctx.view(); ctx.respond(http.response(200))?; print(v.path()); Ok(()) }",
            "fn bad(ctx: http_request_ctx) -> Result<(), Error> { v := ctx.view(); f := fn { v.peer_eof() else false }; ctx.respond(http.response(200))?; print(requests.checkpoint(f)); Ok(()) }",
            "fn bad(mut ctx: http_request_ctx, next: http_request_ctx) { v := ctx.view(); ctx = next; print(v.path()) }",
            "fn bad(ctx: http_request_ctx, flag: bool) -> Result<(), Error> { v := ctx.view(); if flag { ctx.respond(http.response(200))? }; print(v.path()); Ok(()) }",
            "fn bad(ctx: http_request_ctx) -> http_request_view = loop { break ctx.view() }",
            "fn bad(mut ctx: http_request_ctx, next: http_request_ctx) { v := ctx.view(); loop { ctx = next; break }; print(v.path()) }",
        ] {
            check(&format!("{prefix}{declaration}\nfn main() {{}}"), false);
        }
        check(&format!(r#"{prefix}
fn choose(borrow ctx: http_request_ctx, flag: bool) -> http_request_view {{
  v := requests.relay(ctx.view())
  joined := if flag {{ v }} else {{ requests.view(ctx) }}
  return loop {{ break match (if flag {{ 1 }} else {{ 0 }}) {{ 1 => joined, _ => v }} }}
}}
fn read(ctx: http_request_ctx, other: http_request_ctx) -> Result<(), Error> {{
  v := choose(ctx, true)
  other.respond(http.response(200)).map_err(requests.keep)?
  _ := requests.headers(v).get("host") else ""
  f := fn {{ v.peer_eof().map_err(requests.keep) else false }}
  g := fn {{ requests.checkpoint(f) }}
  _ := requests.checkpoint(g)
  ctx.respond(http.response(200))
}}
fn main() {{}}
"#), true);
    });
}

#[test]
fn callback_and_transition_overlap() {
    owned_fixture::run("callback_and_transition_overlap", |_| {
        for transition in ["ctx.respond_stream(http.response(200))", "requests.stream(ctx)", "indirect(ctx)",
                           "ctx.respond_upgrade(http.response(101))", "requests.upgrade(ctx)"] {
            for nested in [false, true] {
                let action = if nested { format!("task_group {{ _ := {transition}; wait() }}") }
                    else { format!("_ := {transition}") };
                let source = format!(r#"import requests
import std.http
fn inspect(borrow ctx: http_request_ctx) {{
  view := ctx.view()
  indirect := requests.stream
  task_group {{ spawn(fn {{ view.peer_eof() else false }}); {action}; wait() }}
}}
fn main() {{}}
"#);
                let diagnostics = check(&source, false);
                assert!(diagnostics.contains("task") || diagnostics.contains("capture"), "{diagnostics}");
            }
        }
        check(r#"import requests
import std.http
fn inspect(ctx: http_request_ctx, other: http_request_ctx) -> Result<(), Error> {
  view := ctx.view()
  task_group { spawn(fn { view.peer_eof() else false }); wait(); _ := requests.stream(ctx) }
  task_group { spawn(fn { view.peer_eof() }); _ := wait(); _ := requests.upgrade(ctx) }
  task_group { spawn(fn { view.peer_eof() else false }); _ := requests.stream(other); wait() }
  _ := view.method()
  _ := view.path()
  _ := view.body()
  _ := view.headers().get("host")
  _ := view.peer_eof()
  Ok(())
}
fn eager(ctx: http_request_ctx) -> Result<(), Error> {
  view := ctx.view()
  task_group { spawn(fn { view.peer_eof() else false }); _ := ctx.respond_stream({ return Ok(()); http.response(200) }); wait() }
  return Ok(())
}
fn pure(borrow ctx: http_request_ctx) { v := ctx.view(); _ := [1,2].par_map(fn n:i64 { n + v.path().len() }) }
fn main() {}
"#, true);
        check("import requests\nfn inspect(borrow ctx:http_request_ctx, borrow other:http_request_ctx) { view:=ctx.view(); task_group { spawn(fn { view.peer_eof() else false }); _:=requests.stream(other); wait() } }\nfn main(){}", false);
        check("import requests\nimport std.http\nfn inspect(ctx:http_request_ctx)->Result<(),Error> { view:=ctx.view(); task_group { spawn(fn { view.peer_eof() else false }); ctx.respond(http.response(200))?; wait() }; Ok(()) }\nfn main(){}", false);
        check("import requests\nfn inspect(borrow ctx:http_request_ctx) { view:=ctx.view(); task_group { spawn(fn { requests.Request { view:view } }); wait() } }\nfn main(){}", false);
        let bad = "import requests\nfn bad(borrow ctx:http_request_ctx) { v:=ctx.view(); _ := [1,2].par_map(fn n:i64 { if v.peer_eof() else false { n } else { 0 } }) }\nfn main() {}";
        assert!(check(bad, false).contains("Pure"));
    });
}

#[test]
fn http_checkpoint_and_allocation_parity() {
    owned_fixture::run("http_checkpoint_and_allocation_parity", |_| {
        use std::io::{Read, Write};
        use std::net::{Shutdown, TcpListener, TcpStream};
        use std::process::Command;
        use std::time::{Duration, Instant};
        assert!(backend_available());
        let source = r#"import std.http
import std.cli
import std.time
import requests
extern "C" fn align_rt_alloc_count() -> i64
fn count() -> i64 = unsafe { align_rt_alloc_count() }
fn repeat(borrow ctx: http_request_ctx) -> Result<(), Error> {
  before := count()
  mut i := 0
  loop {
    if i == 128 { break }
    v := requests.relay(ctx.view())
    callback := fn { v.peer_eof() else false }
    if requests.checkpoint(callback) { return Err(Error.Invalid) }
    if requests.path(v) != "/first" || v.method() != "GET" { return Err(Error.Invalid) }
    if (requests.headers(v).get("host") else "") != "h" || requests.body(v).len() != 0 { return Err(Error.Invalid) }
    i = i + 1
  }
  after := count()
  headers := ctx.headers()
  mut j := 0
  loop { if j == 128 { break }; callback := fn { (headers.get("host") else "") == "h" }; _ := requests.checkpoint(callback); j = j + 1 }
  if after != before || count() != after { return Err(Error.Invalid) }
  Ok(())
}
fn main(args: array<str>) -> Result<(), Error> {
  command := cli.command("view-owner")
  command.flag_i64("port", 0)
  options := command.parse(args)?
  port := options.get_i64("port")
  cycle(port)?
  reopened := http.serve("127.0.0.1", port)?
  Ok(())
}
fn cycle(port: i64) -> Result<(), Error> {
  server := http.serve("127.0.0.1", port)?
  server.accept_timeout_ns(2000000000)?
  first := server.accept()?
  repeat(first)?
  view := first.view()
  task_group { observed := spawn(fn { view.peer_eof() }); wait()?; if observed.get() { return Err(Error.Invalid) } }
  response := http.response(200)
  response.body("open")
  first.respond_timeout_ns(response, 1000000000)?
  second := server.accept()?
  final_view := requests.view(second)
  mut probes := 0
  loop {
    if requests.checkpoint(fn { final_view.peer_eof() else false }) { break }
    if probes == 1000 { return Err(Error.Invalid) }
    probes = probes + 1
    time.sleep(1000000)
  }
  stream := requests.stream(second)?
  if final_view.path() != "/second" || final_view.method() != "GET" || final_view.body().len() != 0 { return Err(Error.Invalid) }
  if (final_view.headers().get("host") else "") != "h" { return Err(Error.Invalid) }
  match final_view.peer_eof() { Err(error) => match error { Invalid => {}, _ => { return Err(Error.Invalid) } }, _ => { return Err(Error.Invalid) } }
  stream.send("eof")?
  stream.finish()?
  third := server.accept()?
  upgrade_view := third.view()
  if !upgrade_view.upgrade_ready() { return Err(Error.Invalid) }
  response_upgrade := http.response(101)
  response_upgrade.header("Connection", "Upgrade")
  response_upgrade.header("Upgrade", "test")
  transport := third.respond_upgrade(response_upgrade)?
  if upgrade_view.path() != "/upgrade" || upgrade_view.method() != "GET" || upgrade_view.body().len() != 0 { return Err(Error.Invalid) }
  if (upgrade_view.headers().get("upgrade") else "") != "test" { return Err(Error.Invalid) }
  match upgrade_view.peer_eof() { Err(error) => match error { Invalid => {}, _ => { return Err(Error.Invalid) } }, _ => { return Err(Error.Invalid) } }
  Ok(())
}
"#;
        check(source, true);
        let files = [("requests.align", HELPERS), ("main.align", source)];
        for per_unit in [false, true] {
            let whole;
            let unit;
            let executable = if per_unit {
                unit = build_per_unit_multi("request-view-unit", &files, "main.align");
                let objects = unit.emit_objects(false);
                let refs = objects.iter().map(|path| path.as_path()).collect::<Vec<_>>();
                let executable = unit.dir.join(format!("server{}", std::env::consts::EXE_SUFFIX));
                link_objects(&align_driver::CDriver::default(), &refs, &executable, &unit.link_libs_union(), Profile::Release).unwrap();
                executable
            } else {
                whole = build_exe_multi("request-view-whole", &files, "main.align");
                whole.exe.clone()
            };
            let probe = TcpListener::bind("127.0.0.1:0").unwrap();
            let port = probe.local_addr().unwrap().port();
            drop(probe);
            let mut child = Command::new(&executable).args(["--port", &port.to_string()]).spawn().unwrap();
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
            assert!(second.ends_with(b"3\r\neof\r\n0\r\n\r\n"), "{}", String::from_utf8_lossy(&second));
            let mut upgrade = TcpStream::connect(("127.0.0.1", port)).unwrap();
            upgrade.set_read_timeout(Some(Duration::from_secs(3))).unwrap();
            upgrade.set_write_timeout(Some(Duration::from_secs(3))).unwrap();
            upgrade.write_all(b"GET /upgrade HTTP/1.1\r\nHost: h\r\nConnection: Upgrade\r\nUpgrade: test\r\n\r\n").unwrap();
            let mut upgraded = Vec::new();
            upgrade.read_to_end(&mut upgraded).unwrap();
            assert!(upgraded.starts_with(b"HTTP/1.1 101 "), "{}", String::from_utf8_lossy(&upgraded));
            assert!(upgraded.windows(13).any(|bytes| bytes == b"Upgrade: test"));
            drop(upgrade);
            while child.try_wait().unwrap().is_none() {
                assert!(Instant::now() < deadline);
                std::thread::sleep(Duration::from_millis(2));
            }
            assert!(child.wait().unwrap().success());
            // The executable reopened its listener after retiring all three contexts.
            drop(socket);
        }
    });
}

#[test]
fn interfaces_and_cache() {
    owned_fixture::run("interfaces_and_cache", |directory| {
        use align_driver::{CacheContext, UnitReuse, build_package};
        let source = "import requests\nfn inspect(borrow ctx: http_request_ctx) -> bool { view := requests.relay(ctx.view()); return requests.checkpoint(fn { view.peer_eof() else false }) }\nfn main() {}\n";
        let entry = directory.join("main.align");
        std::fs::write(&entry, source).unwrap();
        let library = directory.join("requests.align");
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
