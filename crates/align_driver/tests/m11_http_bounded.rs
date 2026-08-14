//! M11 std.http bounded response bodies (align-llm Request 5). The public client/request setters
//! select a receive-allocation cap before socket reads; a syntactically valid response exceeding it
//! is `Error.Code(-1)`, while HTTP status remains ordinary response data.

mod common;
use common::*;

fn spawn_responses(responses: Vec<Vec<u8>>) -> (u16, std::thread::JoinHandle<usize>) {
    use std::io::{Read, Write};

    let listener = std::net::TcpListener::bind("127.0.0.1:0").expect("bind loopback");
    let port = listener.local_addr().unwrap().port();
    listener.set_nonblocking(true).unwrap();
    let handle = std::thread::spawn(move || {
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(30);
        let mut served = 0usize;
        while served < responses.len() && std::time::Instant::now() < deadline {
            match listener.accept() {
                Ok((mut socket, _)) => {
                    socket
                        .set_read_timeout(Some(std::time::Duration::from_secs(5)))
                        .unwrap();
                    let mut request = Vec::new();
                    let mut chunk = [0u8; 512];
                    while !request.windows(4).any(|window| window == b"\r\n\r\n") {
                        match socket.read(&mut chunk) {
                            Ok(0) | Err(_) => break,
                            Ok(n) => request.extend_from_slice(&chunk[..n]),
                        }
                    }
                    socket
                        .write_all(&responses[served])
                        .expect("write response");
                    served += 1;
                }
                Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                    std::thread::sleep(std::time::Duration::from_millis(1));
                }
                Err(_) => break,
            }
        }
        served
    });
    (port, handle)
}

#[test]
fn bounded_client_and_request_caps_map_only_payload_overflow_to_code_minus_one() {
    if !backend_available() {
        return;
    }
    let (port, server) = spawn_responses(vec![
        b"HTTP/1.1 200 OK\r\nContent-Length: 6\r\n\r\nexact!".to_vec(),
        b"HTTP/1.1 200 OK\r\nContent-Length: 6\r\n\r\nlarge!".to_vec(),
        b"HTTP/1.1 200 OK\r\nContent-Length: 5\r\n\r\nlarge".to_vec(),
        b"HTTP/1.1 413 Payload Too Large\r\nContent-Length: 0\r\n\r\n".to_vec(),
    ]);
    let source = format!(
        r#"import std.http
pub fn main() -> Result<(), Error> {{
  client := http.client()
  client.max_response_body_bytes(6)
  match client.get("http://127.0.0.1:{port}/exact") {{
    Ok(response) => print(response.status()),
    Err(error) => match error {{
      Code(code) => print(code),
      _ => print(99),
    }},
  }}

  client.max_response_body_bytes(5)
  match client.get("http://127.0.0.1:{port}/client-limit") {{
    Ok(response) => print(response.status()),
    Err(error) => match error {{
      Code(code) => print(code),
      _ => print(99),
    }},
  }}

  client.max_response_body_bytes(6)
  request := http.request("GET", "http://127.0.0.1:{port}/request-limit")
  request.max_response_body_bytes(4)
  match client.request(request) {{
    Ok(response) => print(response.status()),
    Err(error) => match error {{
      Code(code) => print(code),
      _ => print(99),
    }},
  }}

  client.max_response_body_bytes(1)
  match client.get("http://127.0.0.1:{port}/status-is-data") {{
    Ok(response) => print(response.status()),
    Err(error) => match error {{
      Code(code) => print(code),
      _ => print(99),
    }},
  }}
  return Ok(())
}}
"#
    );
    let output = build_and_run("m11-http-bounded-runtime", &source);
    assert_eq!(
        server.join().unwrap(),
        4,
        "the program completed all four exchanges"
    );
    assert_eq!(
        output.status.code(),
        Some(0),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(
        String::from_utf8_lossy(&output.stdout),
        "200\n-1\n-1\n413\n"
    );
}

#[test]
fn bounded_setters_match_whole_and_per_unit_codegen() {
    if !backend_available() {
        return;
    }
    let source = r#"import std.http
pub fn main() -> i32 {
  client := http.client()
  client.max_response_body_bytes(1024)
  client.max_response_body_bytes(0)
  request := http.request("GET", "http://127.0.0.1/")
  request.max_response_body_bytes(512)
  request.max_response_body_bytes(0)
  return match client.get("not-a-url") {
    Ok(response) => response.status() as i32,
    Err(error) => 0,
  }
}
"#;
    let whole = build_and_run("m11-http-bounded-whole", source);
    let per_unit = build_per_unit_multi(
        "m11-http-bounded-per-unit",
        &[("main.align", source)],
        "main.align",
    )
    .link_and_run();
    assert_eq!(whole.status.code(), Some(0));
    assert_eq!(per_unit.status.code(), Some(0));
}

#[test]
fn bounded_setters_reject_wrong_arity_type_and_temporary_receivers() {
    let cases = [
        (
            "client-arity",
            "client := http.client()\n  client.max_response_body_bytes()",
        ),
        (
            "client-type",
            "client := http.client()\n  client.max_response_body_bytes(true)",
        ),
        (
            "client-temporary",
            "http.client().max_response_body_bytes(1)",
        ),
        (
            "request-arity",
            "request := http.request(\"GET\", \"http://a/\")\n  request.max_response_body_bytes(1, 2)",
        ),
        (
            "request-type",
            "request := http.request(\"GET\", \"http://a/\")\n  request.max_response_body_bytes(\"1\")",
        ),
        (
            "request-temporary",
            "http.request(\"GET\", \"http://a/\").max_response_body_bytes(1)",
        ),
    ];
    for (name, body) in cases {
        let source =
            format!("import std.http\npub fn main() -> i32 {{\n  {body}\n  return 0\n}}\n");
        assert!(
            check_errs(&format!("m11-http-bounded-{name}"), &source),
            "{name} must be rejected"
        );
    }
}

#[test]
fn bounded_setters_abort_on_dynamic_values_outside_the_public_range() {
    if !backend_available() {
        return;
    }
    for (name, setup) in [
        (
            "client-negative",
            "client := http.client()\n  client.max_response_body_bytes(-1)",
        ),
        (
            "request-too-large",
            "request := http.request(\"GET\", \"http://a/\")\n  request.max_response_body_bytes(1073741825)",
        ),
    ] {
        let source = format!(
            "import std.http\npub fn main() -> i32 {{\n  probe := http.client()\n  {setup}\n  return match probe.get(\"not-a-url\") {{ Ok(response) => response.status() as i32, Err(error) => 0 }}\n}}\n"
        );
        let output = build_and_run(&format!("m11-http-bounded-abort-{name}"), &source);
        assert!(
            !output.status.success(),
            "{name} must abort before storing the limit"
        );
    }
}

#[test]
fn bounded_setter_edit_and_revert_has_exact_cache_identity() {
    if !backend_available() {
        return;
    }
    let dir = std::env::temp_dir().join(format!(
        "align-http-bounded-cache-{}-{}",
        std::process::id(),
        thin_nonce()
    ));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    struct Cleanup(std::path::PathBuf);
    impl Drop for Cleanup {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }
    let _cleanup = Cleanup(dir.clone());
    let source_path = dir.join("main.align");
    let cache = dir.join("cache");
    let client_source = "import std.http\n\
pub fn main() -> i32 {\n\
  client := http.client()\n\
  client.max_response_body_bytes(1024)\n\
  return match client.get(\"not-a-url\") { Ok(response) => response.status() as i32, Err(_) => 0 }\n\
}\n";
    let request_source = "import std.http\n\
pub fn main() -> i32 {\n\
  client := http.client()\n\
  request := http.request(\"GET\", \"http://127.0.0.1/\")\n\
  request.max_response_body_bytes(1024)\n\
  return match client.get(\"not-a-url\") { Ok(response) => response.status() as i32, Err(_) => 0 }\n\
}\n";
    let build = || {
        std::process::Command::new(env!("CARGO_BIN_EXE_alignc"))
            .args(["build", "main.align", "--cache-stats"])
            .current_dir(&dir)
            .env("ALIGNC_CACHE", &cache)
            .output()
            .expect("run alignc")
    };

    std::fs::write(&source_path, client_source).unwrap();
    let cold = build();
    assert!(
        cold.status.success(),
        "cold cache build failed: {}",
        String::from_utf8_lossy(&cold.stderr)
    );
    assert!(String::from_utf8_lossy(&cold.stderr).contains("0 hit, 1 miss"));

    std::fs::write(&source_path, request_source).unwrap();
    let edited = build();
    assert!(
        edited.status.success(),
        "edited cache build failed: {}",
        String::from_utf8_lossy(&edited.stderr)
    );
    assert!(String::from_utf8_lossy(&edited.stderr).contains("0 hit, 1 miss"));

    std::fs::write(&source_path, client_source).unwrap();
    let reverted = build();
    assert!(
        reverted.status.success(),
        "reverted cache build failed: {}",
        String::from_utf8_lossy(&reverted.stderr)
    );
    assert!(
        String::from_utf8_lossy(&reverted.stderr).contains("1 hit, 0 miss"),
        "an exact source revert must select the original cached object: {}",
        String::from_utf8_lossy(&reverted.stderr)
    );
}
