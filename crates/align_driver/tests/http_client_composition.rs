//! HTTP package ownership: named owners, borrowed helpers and recursive carriers.
mod common;
use common::*;

const HELPERS: &str = r#"module helpers
import std.http

pub Owners { client: http_client, request: http_request, response: http_response }
pub Choice { Request(http_request), Client(http_client), Response(http_response) }
pub Boxed<T> { value: T }
pub Views { body: slice<u8>, header: Option<str> }
pub fn views(borrow response: http_response) -> Views = Views { body: body(response), header: header(response) }

pub fn prepare(text: str) -> Result<http_request, Error> {
  req := http.request("POST", "http://example.com/object")
  req.header("x-test", text)
  req.body(text)
  return Ok(req)
}
pub fn client() -> http_client = http.client()
pub fn batch(borrow client: http_client, urls: slice<str>) -> Result<array<http_response>, Error> =
  client.get_many(urls, 2)
pub fn response() -> Result<http_response, Error> =
  http.parse("HTTP/1.1 200 OK\r\nContent-Length: 3\r\nx-test: yes\r\n\r\nabc")
pub fn relay<T>(value: T) -> T = value
pub fn configure(borrow mut req: http_request) { req.timeout(1000000000) }
pub fn status(borrow value: http_response) -> i64 = value.status()
pub fn body(borrow value: http_response) -> slice<u8> = value.body()
pub fn header(borrow value: http_response) -> Option<str> = value.header("x-test")
pub fn stream(borrow client: http_client, req: http_request) -> Result<http_read_stream, Error> =
  client.request_stream(req)
pub fn send(borrow client: http_client, req: http_request) -> Result<http_response, Error> =
  client.request(req)
"#;

use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, Instant};

struct Project(PathBuf);
impl Project {
    fn new(main: &str) -> Self {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let project = loop {
            let dir = std::env::temp_dir().join(format!(
                "align-http-owners-{}-{}",
                std::process::id(),
                NEXT.fetch_add(1, Ordering::Relaxed)
            ));
            match std::fs::create_dir(&dir) {
                Ok(()) => break Self(dir),
                Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => continue,
                Err(error) => panic!("acquire project: {error}"),
            }
        };
        std::fs::write(project.0.join("helpers.align"), HELPERS).unwrap();
        std::fs::write(project.0.join("main.align"), main).unwrap();
        project
    }
    fn entry(&self) -> String {
        self.0.join("main.align").display().to_string()
    }
    fn build(&self, main: &str, unit: bool, profile: Profile) -> PathBuf {
        let mut sm = SourceMap::new();
        let programs = if unit {
            let walk = build_per_unit(&mut sm, &self.entry(), main);
            assert!(
                !walk.diags.has_errors(),
                "{}",
                align_driver::format_diagnostics(&sm, &walk.diags)
            );
            walk.units
                .into_iter()
                .map(|unit| unit.mir)
                .collect::<Vec<_>>()
        } else {
            let checked = check(&mut sm, &self.entry(), main);
            assert!(
                !checked.diags.has_errors(),
                "{}",
                align_driver::format_diagnostics(&sm, &checked.diags)
            );
            vec![lower_to_mir(&checked.hir)]
        };
        let mut objects = Vec::new();
        let mut libraries = Vec::new();
        for (index, program) in programs.iter().enumerate() {
            let object = self.0.join(format!("unit{index}.o"));
            emit_object_file(program, &object, BuildTarget::Baseline, profile, &[], false)
                .expect("codegen");
            objects.push(object);
            for library in &program.link_libs {
                if !libraries.contains(library) {
                    libraries.push(library.clone());
                }
            }
        }
        let refs = objects.iter().map(PathBuf::as_path).collect::<Vec<_>>();
        let exe = self.0.join(format!("run{}", std::env::consts::EXE_SUFFIX));
        link_objects(
            &align_driver::CDriver::default(),
            &refs,
            &exe,
            &libraries,
            profile,
        )
        .expect("link");
        exe
    }
}
impl Drop for Project {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

struct ChildGuard(Option<std::process::Child>);
impl Drop for ChildGuard {
    fn drop(&mut self) {
        if let Some(child) = self.0.as_mut() {
            #[cfg(unix)]
            if let Ok(pid) = i32::try_from(child.id()) {
                // The child is placed in its own process group before exec.
                unsafe {
                    libc::kill(-pid, libc::SIGKILL);
                }
            }
            let _ = child.kill();
            let _ = child.wait();
        }
    }
}
fn run_bounded(exe: &Path) -> String {
    let dir = exe.parent().unwrap();
    let stdout = dir.join("stdout");
    let stderr = dir.join("stderr");
    let mut command = Command::new(exe);
    command.stdout(Stdio::from(std::fs::File::create(&stdout).unwrap()));
    command.stderr(Stdio::from(std::fs::File::create(&stderr).unwrap()));
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        command.process_group(0);
    }
    let mut child = ChildGuard(Some(command.spawn().expect("spawn")));
    let deadline = Instant::now() + Duration::from_secs(20);
    loop {
        assert!(
            Instant::now() < deadline,
            "HTTP owner child timed out: {}",
            std::fs::read_to_string(&stderr).unwrap()
        );
        match child.0.as_mut().unwrap().try_wait() {
            Ok(Some(status)) => {
                child.0.take();
                assert!(
                    status.success(),
                    "child {status}: {}",
                    std::fs::read_to_string(&stderr).unwrap()
                );
                return std::fs::read_to_string(&stdout).unwrap();
            }
            Ok(None) => std::thread::sleep(Duration::from_millis(5)),
            Err(error) if error.kind() == std::io::ErrorKind::Interrupted => {}
            Err(error) => panic!("wait: {error}"),
        }
    }
}

fn check_both(label: &str, main: &str, accepted: bool) {
    let project = Project::new(main);
    let mut whole_map = SourceMap::new();
    let whole = check(&mut whole_map, &project.entry(), main);
    let mut unit_map = SourceMap::new();
    let unit = check_per_unit(&mut unit_map, &project.entry(), main);
    assert_eq!(
        (!whole.diags.has_errors(), !unit.diags.has_errors()),
        (accepted, accepted),
        "{label}: whole:\n{}\nunit:\n{}",
        align_driver::format_diagnostics(&whole_map, &whole.diags),
        align_driver::format_diagnostics(&unit_map, &unit.diags)
    );
}

fn run_both(label: &str, main: &str, expected: &str) {
    check_both(label, main, true);
    if !backend_available() {
        return;
    }
    for unit in [false, true] {
        for profile in [Profile::Dev, Profile::Release] {
            let project = Project::new(main);
            let exe = project.build(main, unit, profile);
            assert_eq!(run_bounded(&exe), expected, "{label}/{unit}/{profile:?}");
        }
    }
}

#[test]
fn ownership_control_flow_whole_and_unit() {
    run_both(
        "http-owner-control",
        r#"module main
import helpers

fn main() -> Result<(), Error> {
  mut request := helpers.prepare("hello")?
  helpers.configure(request)
  owned := helpers.Owners {
    client: helpers.client(), request: request, response: helpers.response()?,
  }
  transferred := helpers.relay(owned)
  choice := helpers.Choice.Request(transferred.request)
  selected := match choice { Request(value) => value, _ => { return Err(Error.Invalid) } }
  unused := helpers.relay(selected)
  print(helpers.status(transferred.response))
  print(helpers.body(transferred.response).len())
  print(helpers.header(transferred.response) else { "missing" })
  return Ok(())
}
"#,
        "200\n3\nyes\n",
    );
}

#[test]
fn formation_and_carrier_matrix() {
    for name in ["http_client", "http_request", "http_response"] {
        for shape in [
            format!("{name}<i64>"),
            format!("slice<{name}>"),
            format!("box<{name}>"),
        ] {
            check_both(
                &format!("http-reject-{shape}"),
                &format!(
                    "module main\nimport helpers\nfn reject(value: {shape}) {{}}\nfn main() {{}}\n"
                ),
                false,
            );
        }
        for declaration in [
            format!("extern \"C\" fn invalid(value: {name})"),
            format!(
                "Holder {{ value: {name} }}\nfn invalid(value: Holder) {{ items := [value].map(fn(item: Holder) -> Holder {{ item }}).to_array() }}"
            ),
            format!("fn invalid(value: {name}) -> fn() -> {name} = fn() -> {name} {{ value }}"),
            format!(
                "fn invalid(a: {name}, b: {name}, flag: bool) -> {name} = if flag {{ a }} else {{ b }}"
            ),
            format!("fn invalid(a: {name}, b: {name}) {{ values := [a, b] }}"),
        ] {
            check_both(
                &format!("http-forbidden-carrier {declaration}"),
                &format!("module main\nimport helpers\n{declaration}\nfn main() {{}}\n"),
                false,
            );
        }
        for other in ["http_client", "http_request", "http_response"] {
            if name != other {
                check_both(
                    "http-nominal-identity",
                    &format!(
                        "module main\nfn invalid(value: {name}) -> {other} = value\nfn main() {{}}\n"
                    ),
                    false,
                );
            }
        }
    }
    for operation in [
        "owner.value = helpers.prepare(\"new\")?",
        "moved := values[0].value",
    ] {
        check_both(
            "http-existing-place-restriction",
            &format!(
                r#"module main
import helpers
fn main() -> Result<(), Error> {{
  mut owner := helpers.Boxed {{ value: helpers.prepare("old")? }}
  values := [helpers.Boxed {{ value: helpers.prepare("array")? }}]
  {operation}
  return Ok(())
}}
"#
            ),
            false,
        );
    }
}

#[test]
fn borrow_authority_and_consumption_matrix() {
    for (name, operation) in [
        ("http_client", "value.timeout(1)"),
        ("http_client", "value.max_response_body_bytes(1)"),
        ("http_request", "value.timeout(1)"),
        ("http_request", "value.max_response_body_bytes(1)"),
        ("http_request", "value.header(\"x\", \"y\")"),
        ("http_request", "value.body(\"body\")"),
    ] {
        for mode in ["borrow", "borrow mut"] {
            let source = format!(
                "module main\nimport helpers\nfn configure({mode} value: {name}) {{ {operation} }}\nfn main() {{}}\n"
            );
            check_both(
                &format!("http-authority-{name}-{mode}-{operation}"),
                &source,
                mode == "borrow mut",
            );
        }
    }
    for method in [
        "client.get(url)",
        "client.post(url, \"body\")",
        "client.request(req)",
        "client.request_stream(req)",
        "client.get_many(urls, 2)",
    ] {
        check_both(
            "http-shared-network",
            &format!(
                "module main\nfn use_client(borrow client: http_client, req: http_request, url: str, urls: slice<str>) {{ result := {method} }}\nfn main() {{}}\n"
            ),
            true,
        );
    }
    for mode in ["borrow", "borrow mut"] {
        for method in ["request", "request_stream"] {
            check_both(
                "http-native-borrow-consume",
                &format!(
                    "module main\nfn invalid(borrow client: http_client, {mode} req: http_request) {{ result := client.{method}(req) }}\nfn main() {{}}\n"
                ),
                false,
            );
        }
        check_both(
            "http-borrow-consume",
            &format!(
                r#"module main
import helpers
fn invalid({mode} value: http_request) -> http_request = value
fn main() {{}}
"#
            ),
            false,
        );
    }
}

#[test]
fn retained_views_and_stream_origins() {
    // A view returned by an imported helper keeps the selected response owner, not the helper frame.
    run_both(
        "http-retained-positive",
        r#"module main
import helpers
fn main() -> Result<(), Error> {
  owner := helpers.response()?
  views := helpers.relay(helpers.views(owner))
  body := views.body
  header := views.header else { "missing" }
  print(body.as_str()?)
  print(header)
  return Ok(())
}
"#,
        "abc\nyes\n",
    );
    for view in [
        "helpers.body(owner)",
        "helpers.header(owner)",
        "helpers.relay(helpers.views(owner))",
    ] {
        check_both(
            "http-retained-replacement",
            &format!(
                r#"module main
import helpers
fn main() -> Result<(), Error> {{
  mut owner := helpers.response()?
  view := {view}
  owner = helpers.response()?
  value := view
  return Ok(())
}}
"#
            ),
            false,
        );
    }
    check_both(
        "http-retained-local-escape",
        r#"module main
import helpers
fn invalid() -> Result<slice<u8>, Error> {
  owner := helpers.response()?
  return Ok(helpers.body(owner))
}
fn main() {}
"#,
        false,
    );
    for action in [
        "moved := client",
        "client = helpers.client()",
        "configure(client)",
    ] {
        check_both(
            "http-stream-client-loan",
            &format!(
                r#"module main
import helpers
fn configure(borrow mut client: http_client) {{ client.timeout(1) }}
fn main() -> Result<(), Error> {{
  mut client := helpers.client()
  req := helpers.prepare("x")?
  stream := helpers.relay(helpers.stream(client, req)?)
  {action}
  print(stream.status())
  return Ok(())
}}
"#
            ),
            false,
        );
    }
}

fn capture_requests(mut socket: TcpStream) -> Vec<Vec<u8>> {
    socket
        .set_read_timeout(Some(Duration::from_secs(2)))
        .unwrap();
    socket
        .set_write_timeout(Some(Duration::from_secs(2)))
        .unwrap();
    let mut captured = Vec::new();
    let deadline = Instant::now() + Duration::from_secs(15);
    loop {
        let mut request = Vec::new();
        loop {
            assert!(Instant::now() < deadline, "peer deadline");
            let mut byte = [0];
            match socket.read(&mut byte) {
                Ok(0) => {
                    assert!(request.is_empty(), "partial request at EOF");
                    return captured;
                }
                Ok(_) => request.push(byte[0]),
                Err(error) if error.kind() == std::io::ErrorKind::Interrupted => continue,
                Err(error) => panic!("peer read: {error}"),
            }
            assert!(request.len() <= 8192, "request bound");
            if request.ends_with(b"\r\n\r\n") {
                break;
            }
        }
        let text = std::str::from_utf8(&request).unwrap();
        let body_len = text
            .lines()
            .find_map(|line| line.strip_prefix("Content-Length: "))
            .map_or(0, |n| n.parse::<usize>().unwrap());
        assert!(body_len <= 32, "body bound");
        let offset = request.len();
        request.resize(offset + body_len, 0);
        socket.read_exact(&mut request[offset..]).unwrap();
        captured.push(request);
        socket
            .write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 3\r\nx-test: yes\r\n\r\nabc")
            .unwrap();
    }
}

#[test]
fn package_request_and_pool_wire_round_trip() {
    if !backend_available() {
        return;
    }
    for unit in [false, true] {
        for profile in [Profile::Dev, Profile::Release] {
            let listener = TcpListener::bind("127.0.0.1:0").unwrap();
            listener.set_nonblocking(true).unwrap();
            let authority = format!("127.0.0.1:{}", listener.local_addr().unwrap().port());
            let main = format!(
                r#"module main
import helpers
import std.http

fn prepare(url: str, text: str) -> Result<http_request, Error> {{
  request := http.request("PUT", url)
  request.header("x-test", text)
  request.body("a\0b")
  return Ok(request)
}}
fn main() -> Result<(), Error> {{
  client := helpers.client()
  client.timeout(1000000000)
  req := prepare("http://{authority}/first", "one")?
  stream := helpers.relay(helpers.stream(client, req)?)
  second := prepare("http://{authority}/second", "two")?
  response := helpers.send(client, second)?
  print(helpers.status(response))
  mut out := buffer(8)
  print(stream.read(out)?)
  print(out.bytes().as_str()?)
  print(stream.read(out)?)
  third := prepare("http://{authority}/third", "three")?
  final_response := helpers.send(client, third)?
  print(helpers.body(final_response).as_str()?)
  return Ok(())
}}
"#
            );
            let project = Project::new(&main);
            let exe = project.build(&main, unit, profile);
            std::thread::scope(|scope| {
                let peer = scope.spawn(move || {
                    let deadline = Instant::now() + Duration::from_secs(15);
                    let mut peers = Vec::new();
                    while peers.len() < 2 {
                        assert!(Instant::now() < deadline, "accept deadline");
                        match listener.accept() {
                            Ok((socket, _)) => {
                                peers.push(scope.spawn(move || capture_requests(socket)))
                            }
                            Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                                std::thread::sleep(Duration::from_millis(5))
                            }
                            Err(error) if error.kind() == std::io::ErrorKind::Interrupted => {}
                            Err(error) => panic!("accept: {error}"),
                        }
                    }
                    let mut requests = Vec::new();
                    for peer in peers {
                        requests.extend(peer.join().unwrap());
                    }
                    requests.sort();
                    requests
                });
                assert_eq!(run_bounded(&exe), "200\n3\nabc\n0\nabc\n");
                let mut expected = [("first", "one"), ("second", "two"), ("third", "three")].map(|(path, text)| format!("PUT /{path} HTTP/1.1\r\nHost: {authority}\r\nx-test: {text}\r\nContent-Length: 3\r\n\r\na\0b").into_bytes()).to_vec();
                expected.sort();
                assert_eq!(peer.join().unwrap(), expected, "{unit}/{profile:?}");
            });
        }
    }
}

#[test]
fn recursive_carriers_replacement_and_early_exit() {
    run_both(
        "http-recursive-lifecycle",
        r#"module main
import helpers
import std.http

fn owners() -> Result<helpers.Owners, Error> = Ok(helpers.Owners {
  client: helpers.client(), request: helpers.prepare("owned")?, response: helpers.response()?,
})
fn alternate(flag: bool) -> Result<http_request, Error> {
  if flag { return helpers.prepare("early") }
  return Ok(if flag { http.request("GET", "bad") } else { http.request("PUT", "bad") })
}
fn early() -> Result<(), Error> {
  abandoned := owners()?
  failed := http.parse("invalid").map_err(fn error: Error { error })?
  return Ok(())
}
fn main() -> Result<(), Error> {
  mut original := owners()?
  moved := original.request
  extra := alternate(true)?
  replacement := owners()?
  original = replacement
  nested: Option<Result<helpers.Owners, Error>> := Some(Ok(original))
  result := nested else { return Err(Error.Invalid) }
  unpacked := result?
  tuple := (unpacked.client, unpacked.request, unpacked.response)
  (client, request, response) := tuple
  tagged := helpers.Choice.Response(response)
  viewable := match tagged { Response(value) => value, _ => { return Err(Error.Invalid) } }
  print(helpers.status(viewable))
  selected := loop { break helpers.Choice.Client(client) }
  recovered := match selected { Client(value) => value, _ => { return Err(Error.Invalid) } }
  invalid := http.request("GET", "not-a-url")
  failure := helpers.send(recovered, invalid)
  print(match failure { Err(_) => true, Ok(_) => false })
  // Owning record elements have recursive cleanup; direct arrays of handles stay forbidden.
  array := [helpers.Boxed { value: request }, helpers.Boxed { value: moved }]
  print(match early() { Err(_) => true, Ok(_) => false })
  return Ok(())
}
"#,
        "200\ntrue\ntrue\n",
    );
}

#[test]
fn batch_owner_crosses_imported_helper() {
    run_both(
        "http-batch-owner",
        r#"module main
import helpers
fn main() -> Result<(), Error> {
  client := helpers.client()
  batch := helpers.batch(client, [])?
  transferred := helpers.relay(batch)
  print(transferred.len())
  return Ok(())
}
"#,
        "0\n",
    );
}

#[test]
fn interfaces_and_cache_restore() {
    use align_driver::{CacheContext, UnitReuse, build_package};
    let source = r#"module main
import helpers
fn main() -> Result<(), Error> {
  value := helpers.Owners {
    client: helpers.client(), request: helpers.prepare("cache")?, response: helpers.response()?,
  }
  moved := helpers.relay(value)
  print(helpers.status(moved.response))
  return Ok(())
}
"#;
    let project = Project::new(source);
    let context = CacheContext::at(project.0.join("cache"));
    let build = || {
        let mut sm = SourceMap::new();
        let result = build_package(
            &mut sm,
            &project.entry(),
            source,
            &context,
            UnitReuse::Allowed,
        );
        assert!(
            !result.diags.has_errors(),
            "{}",
            align_driver::format_diagnostics(&sm, &result.diags)
        );
        result
    };
    let snapshot = |mut result: align_driver::PackageBuild, expected_hit| {
        for name in ["helpers", "main"] {
            let unit = result.units.iter().find(|unit| unit.unit == name).unwrap();
            assert_eq!(unit.frontend.as_ref().unwrap().hit, expected_hit, "{name}");
        }
        (0..result.units.len())
            .map(|index| {
                align_mir::print::program_to_string(
                    result.materialize(index).expect("HTTP owner MIR"),
                )
            })
            .collect::<Vec<_>>()
    };
    let cold = snapshot(build(), false);
    assert_eq!(snapshot(build(), true), cold);
    let changed = HELPERS.replace(
        "pub fn relay<T>(value: T) -> T = value",
        "pub fn relay<T>(value: T) -> T { print(7); return value }",
    );
    std::fs::write(project.0.join("helpers.align"), changed).unwrap();
    assert_ne!(snapshot(build(), false), cold);
    std::fs::write(project.0.join("helpers.align"), HELPERS).unwrap();
    assert_eq!(snapshot(build(), true), cold);
}
