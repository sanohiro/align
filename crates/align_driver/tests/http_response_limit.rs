//! End-to-end coverage for bounded whole-response HTTP receives.

mod common;
use common::*;

fn spawn_http_server(response: &'static [u8]) -> (u16, std::thread::JoinHandle<()>) {
    use std::io::{Read, Write};
    let listener = std::net::TcpListener::bind("127.0.0.1:0").expect("bind loopback");
    let port = listener.local_addr().unwrap().port();
    let handle = std::thread::spawn(move || {
        if let Ok((mut socket, _)) = listener.accept() {
            let mut request = [0u8; 512];
            let _ = socket.read(&mut request);
            let _ = socket.write_all(response);
        }
    });
    (port, handle)
}

#[test]
fn client_body_limit_maps_private_status_to_code_minus_one() {
    if !backend_available() {
        return;
    }
    let source = "\
import std.http
import std.cli
pub fn main(args: array<str>) -> Result<(), Error> {
  cmd := cli.command(\"get\")
  cmd.flag_str(\"url\", \"\")
  parsed := cmd.parse(args)?
  cl := http.client()
  cl.max_response_body_bytes(4)
  code := match cl.get(parsed.get_str(\"url\")) {
    Ok(resp) => 99,
    Err(e) => match e {
      NotFound => 1,
      Invalid => 2,
      Denied => 3,
      Timeout => 4,
      Code(value) => value,
    },
  }
  print(code)
  return Ok(())
}
";
    // Compile before the fixture begins blocking in `accept`, so a compiler regression cannot
    // strand the server thread and turn the test failure into a process-wide hang.
    let client = build_exe("http-response-limit", source);
    let (port, server) = spawn_http_server(
        b"HTTP/1.1 200 OK\r\nContent-Length: 5\r\nConnection: close\r\n\r\ndata!",
    );
    let url = format!("http://127.0.0.1:{port}/bounded");
    let output = std::process::Command::new(&client.exe)
        .args(["--url", &url])
        .output()
        .expect("run bounded response client");
    server.join().unwrap();
    assert_eq!(
        output.status.code(),
        Some(0),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(String::from_utf8_lossy(&output.stdout), "-1\n");
}

#[test]
fn response_limit_setters_require_bound_receivers_and_i64() {
    for (name, source) in [
        (
            "http-response-limit-unbound-client",
            "import std.http\npub fn main() { http.client().max_response_body_bytes(4) }\n",
        ),
        (
            "http-response-limit-unbound-request",
            "import std.http\npub fn main() { http.request(\"GET\", \"http://a/\").max_response_body_bytes(4) }\n",
        ),
        (
            "http-response-limit-type",
            "import std.http\npub fn main() { cl := http.client()\ncl.max_response_body_bytes(\"four\") }\n",
        ),
    ] {
        assert!(check_errs(name, source), "{name} must be rejected");
    }
}
