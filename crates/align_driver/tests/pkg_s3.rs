//! S3 signing-to-wire, validation, ownership and package identity owners.
mod common;
use common::*;

fn bytes(value: align_runtime::AlignStr) -> Vec<u8> {
    assert_eq!(value.len, 32);
    assert!(!value.ptr.is_null());
    let output = unsafe { std::slice::from_raw_parts(value.ptr, 32) }.to_vec();
    unsafe { align_runtime::align_rt_free(value.ptr.cast_mut()) };
    output
}
fn sha(data: &[u8]) -> Vec<u8> {
    bytes(unsafe {
        align_runtime::align_rt_crypto_sha256(data.as_ptr(), data.len().try_into().unwrap())
    })
}
fn mac(key: &[u8], data: &[u8]) -> Vec<u8> {
    bytes(unsafe {
        align_runtime::align_rt_crypto_hmac_sha256(
            key.as_ptr(),
            key.len().try_into().unwrap(),
            data.as_ptr(),
            data.len().try_into().unwrap(),
        )
    })
}
fn hex(data: &[u8]) -> String {
    data.iter().map(|byte| format!("{byte:02x}")).collect()
}
fn quote(text: &str) -> String {
    let mut result = String::from("\"");
    for ch in text.chars() {
        match ch {
            '\0' => result.push_str("\\0"),
            '\n' => result.push_str("\\n"),
            '\r' => result.push_str("\\r"),
            '\t' => result.push_str("\\t"),
            '\\' => result.push_str("\\\\"),
            '"' => result.push_str("\\\""),
            _ => result.push(ch),
        }
    }
    result.push('"');
    result
}
const SECRET: &[u8] = b"wJalrXUtnFEMI/K7MDENG/bPxRfiCYEXAMPLEKEY";
const DATE: &str = "20130524T000000Z";
const NOW: i64 = 1369353600000000000;

fn accept(listener: &TcpListener) -> TcpStream {
    listener.set_nonblocking(true).unwrap();
    let deadline = Instant::now() + Duration::from_secs(15);
    loop {
        match listener.accept() {
            Ok((stream, _)) => {
                stream
                    .set_read_timeout(Some(Duration::from_secs(5)))
                    .unwrap();
                stream
                    .set_write_timeout(Some(Duration::from_secs(5)))
                    .unwrap();
                return stream;
            }
            Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                assert!(Instant::now() < deadline, "accept deadline");
                std::thread::sleep(Duration::from_millis(2));
            }
            Err(error) => panic!("accept: {error}"),
        }
    }
}

fn read_request(stream: &mut TcpStream) -> (String, Vec<(String, String)>, Vec<u8>) {
    let mut head = Vec::new();
    while !head.ends_with(b"\r\n\r\n") {
        assert!(head.len() < 262144);
        let mut byte = [0];
        stream.read_exact(&mut byte).unwrap();
        head.push(byte[0]);
    }
    let text = String::from_utf8(head).unwrap();
    let mut lines = text.split("\r\n");
    let line = lines.next().unwrap().to_owned();
    let headers: Vec<_> = lines
        .filter(|line| !line.is_empty())
        .map(|line| {
            let (name, value) = line.split_once(": ").unwrap();
            (name.to_ascii_lowercase(), value.to_owned())
        })
        .collect();
    let size: usize = headers
        .iter()
        .find(|(name, _)| name == "content-length")
        .map(|(_, value)| value.parse().unwrap())
        .unwrap_or(0);
    assert!(size <= 65536);
    let mut body = vec![0; size];
    stream.read_exact(&mut body).unwrap();
    (line, headers, body)
}

fn verify_wire(
    stream: &mut TcpStream,
    authority: &str,
    method: &str,
    target: &str,
    body: &[u8],
    extra: &[(&str, &str)],
    token: bool,
    secret: &[u8],
) {
    let (line, actual_headers, actual_body) = read_request(stream);
    assert_eq!(line, format!("{method} {target} HTTP/1.1"));
    assert_eq!(actual_body, body);
    let mut expected = std::collections::BTreeMap::from([
        ("host".to_string(), authority.to_string()),
        ("x-amz-content-sha256".to_string(), hex(&sha(body))),
        ("x-amz-date".to_string(), DATE.to_string()),
    ]);
    for (name, value) in extra {
        expected.insert((*name).to_string(), (*value).to_string());
    }
    if token {
        expected.insert("x-amz-security-token".to_string(), "token/+==".to_string());
    }
    let signed = expected.keys().cloned().collect::<Vec<_>>().join(";");
    let canonical_headers = expected
        .iter()
        .map(|(name, value)| format!("{name}:{value}\n"))
        .collect::<String>();
    let (uri, query) = target.split_once('?').unwrap_or((target, ""));
    let canonical = format!(
        "{method}\n{uri}\n{query}\n{canonical_headers}\n{signed}\n{}",
        hex(&sha(body))
    );
    let auth = authorization(&canonical, &signed, secret);
    // Pin order as well as the complete set. Auto Host is first and Content-Length last.
    let mut wire = vec![("host".to_string(), authority.to_string())];
    wire.extend(expected.into_iter().filter(|(name, _)| name != "host"));
    wire.push(("authorization".to_string(), auth));
    wire.push(("content-length".to_string(), body.len().to_string()));
    assert_eq!(actual_headers, wire);
    // Reconstruct canonical bytes independently from observed wire, not only expected inputs.
    let captured = actual_headers
        .iter()
        .filter(|(name, _)| name != "authorization" && name != "content-length")
        .map(|(name, value)| (name.clone(), value.clone()))
        .collect::<std::collections::BTreeMap<_, _>>();
    let captured_headers = captured
        .iter()
        .map(|(name, value)| format!("{name}:{value}\n"))
        .collect::<String>();
    assert_eq!(captured_headers, canonical_headers);
}

#[test]
fn operation_round_trip() {
    if !backend_available() {
        return;
    }
    for unit in [false, true] {
        for profile in [Profile::Dev, Profile::Release] {
            let listener = TcpListener::bind("127.0.0.1:0").unwrap();
            let authority = listener.local_addr().unwrap().to_string();
            let main = format!(
                r#"import pkg.s3
import std.http
fn main() -> Result<(), Error> {{
  credentials := pkg.s3.Credentials{{access_key: "AKIAIOSFODNN7EXAMPLE", secret_key: "wJalrXUtnFEMI/K7MDENG/bPxRfiCYEXAMPLEKEY".bytes(), session_token: Some("token/+==")}}
  endpoint := pkg.s3.Endpoint{{origin: "http://{authority}", region: "us-east-1"}}
  client := http.client()
  client.timeout(3000000000)
  mut payload := buffer(3)
  payload.append("a\0b")
  request := arena {{
    access := "AKIAIOSFODNN7EXAMPLE".clone()
    secret := "wJalrXUtnFEMI/K7MDENG/bPxRfiCYEXAMPLEKEY".clone()
    token := "token/+==".clone()
    temporary := pkg.s3.Credentials{{access_key: access[0..access.len()], secret_key: secret.bytes(), session_token: Some(token[0..token.len()])}}
    query := [pkg.s3.Field{{name: "z", value: "!"}}, pkg.s3.Field{{name: "a", value: "z"}}, pkg.s3.Field{{name: "a", value: " "}}, pkg.s3.Field{{name: "a-", value: ""}}, pkg.s3.Field{{name: "a", value: " "}}, pkg.s3.Field{{name: "é\0", value: "?%"}}]
    headers := [pkg.s3.Field{{name: "X-Test", value: " \talpha  \t beta\t "}}]
    pkg.s3.request(temporary, endpoint, "PUT", "/bucket/a//../%é\0", query, headers, payload.bytes(), {NOW})?
  }}
  payload.append("new")
  response := client.request(request)?
  print(response.status())
  arena {{
    fields := [pkg.s3.Field{{name: "unused", value: ""}}]
    empty := fields[0..0]
    plain := pkg.s3.Credentials{{access_key: "AKIAIOSFODNN7EXAMPLE", secret_key: "a\0b".bytes(), session_token: None}}
    put := pkg.s3.request(plain, endpoint, "PUT", "/empty", empty, empty, "".bytes(), {NOW})?
    put_response := client.request(put)?
    print(put_response.status())
    get := pkg.s3.request(credentials, endpoint, "GET", "/empty", empty, empty, "".bytes(), {NOW})?
    get_response := client.request(get)?
    print(get_response.status())
    head := pkg.s3.request(credentials, endpoint, "HEAD", "/empty", empty, empty, "".bytes(), {NOW})?
    head_response := client.request(head)?
    print(head_response.status())
    remove := pkg.s3.request(credentials, endpoint, "DELETE", "/empty", empty, empty, "".bytes(), {NOW})?
    remove_response := client.request(remove)?
    print(remove_response.status())
    listing := [pkg.s3.Field{{name: "prefix", value: "é/"}}, pkg.s3.Field{{name: "list-type", value: "2"}}]
    list := pkg.s3.request(credentials, endpoint, "GET", "/bucket/", listing, empty, "".bytes(), {NOW})?
    list_response := client.request(list)?
    print(list_response.status())
    print(list_response.body().as_str()?)
    download := pkg.s3.request(credentials, endpoint, "GET", "/stream", empty, empty, "".bytes(), {NOW})?
    stream := client.request_stream(download)?
    print(stream.status())
    denied := pkg.s3.request(credentials, endpoint, "GET", "/denied", empty, empty, "".bytes(), {NOW})?
    denied_response := client.request(denied)?
    print(denied_response.status())
    print(denied_response.body().as_str()?)
    failed := pkg.s3.request(credentials, endpoint, "GET", "/failed", empty, empty, "".bytes(), {NOW})?
    failed_response := client.request(failed)?
    print(failed_response.status())
    broken := pkg.s3.request(credentials, endpoint, "GET", "/broken", empty, empty, "".bytes(), {NOW})?
    print(match client.request(broken) {{ Ok(_) => false, Err(_) => true }})
  }}
  return Ok(())
}}
"#
            );
            let project = Project::new(&main);
            let exe = project.build(&main, unit, profile);
            std::thread::scope(|scope| {
                let peer = scope.spawn(|| {
                    let mut first = accept(&listener);
                    for (method, target, body, extra) in [
                        (
                            "PUT",
                            "/bucket/a//../%25%C3%A9%00?%C3%A9%00=%3F%25&a=%20&a=%20&a=z&a-=&z=%21",
                            b"a\0b".as_slice(),
                            vec![("x-test", "alpha beta")],
                        ),
                        ("PUT", "/empty", b"", vec![]),
                        ("GET", "/empty", b"", vec![]),
                        ("HEAD", "/empty", b"", vec![]),
                        ("DELETE", "/empty", b"", vec![]),
                        ("GET", "/bucket/?list-type=2&prefix=%C3%A9%2F", b"", vec![]),
                    ] {
                        let plain = method == "PUT" && target == "/empty";
                        verify_wire(
                            &mut first,
                            &authority,
                            method,
                            target,
                            body,
                            &extra,
                            !plain,
                            if plain { b"a\0b" } else { SECRET },
                        );
                        if target.starts_with("/bucket/?") {
                            first
                                .write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 7\r\n\r\n<List/>")
                                .unwrap();
                        } else {
                            first
                                .write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 0\r\n\r\n")
                                .unwrap();
                        }
                    }
                    verify_wire(
                        &mut first,
                        &authority,
                        "GET",
                        "/stream",
                        b"",
                        &[],
                        true,
                        SECRET,
                    );
                    first
                        .write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 100\r\n\r\nprefix")
                        .unwrap();
                    let mut second = accept(&listener);
                    verify_wire(
                        &mut second,
                        &authority,
                        "GET",
                        "/denied",
                        b"",
                        &[],
                        true,
                        SECRET,
                    );
                    second
                        .write_all(b"HTTP/1.1 403 Forbidden\r\nContent-Length: 8\r\n\r\n<Error/>")
                        .unwrap();
                    verify_wire(
                        &mut second,
                        &authority,
                        "GET",
                        "/failed",
                        b"",
                        &[],
                        true,
                        SECRET,
                    );
                    second
                        .write_all(b"HTTP/1.1 500 Failure\r\nContent-Length: 0\r\n\r\n")
                        .unwrap();
                    verify_wire(
                        &mut second,
                        &authority,
                        "GET",
                        "/broken",
                        b"",
                        &[],
                        true,
                        SECRET,
                    );
                    // Some response bytes prevent the transport's one stale-pool retry.
                    second
                        .write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 2\r\n\r\nx")
                        .unwrap();
                    second.shutdown(std::net::Shutdown::Write).unwrap();
                    for stream in [&mut first, &mut second] {
                        let mut byte = [0];
                        assert_eq!(
                            stream.read(&mut byte).unwrap(),
                            0,
                            "owners close both pooled and unfinished connections"
                        );
                    }
                });
                assert_eq!(
                    run_bounded(&exe),
                    "200\n200\n200\n200\n200\n200\n<List/>\n200\n403\n<Error/>\n500\ntrue\n"
                );
                peer.join().unwrap();
            });
        }
    }
}

#[test]
fn input_validation() {
    validation_cases(false);
}

#[test]
fn presign_validation() {
    validation_cases(true);
}

fn validation_cases(presign: bool) {
    if !backend_available() {
        return;
    }
    let mut main = String::from(
        r#"import pkg.s3
fn observed(credentials: pkg.s3.Credentials, endpoint: pkg.s3.Endpoint, method: str, path: str,
  query: slice<pkg.s3.Field>, headers: slice<pkg.s3.Field>, body: slice<u8>, now_ns: i64,
) -> bool = match pkg.s3.request(credentials, endpoint, method, path, query, headers, body, now_ns) {
  Ok(_) => true,
  Err(error) => match error { Invalid => false, _ => { print("wrong error"); false } },
}
fn repeated(count: i64, text: str) -> string {
  mut output := builder()
  mut i := 0
  loop { if i >= count { break }; output.write(text); i = i + 1 }
  return output.to_string()
}
fn test_credentials(access: str, secret: slice<u8>) -> pkg.s3.Credentials =
  pkg.s3.Credentials{access_key: access, secret_key: secret, session_token: None}
fn token_credentials(token: str) -> pkg.s3.Credentials =
  pkg.s3.Credentials{access_key: "key", secret_key: "s".bytes(), session_token: Some(token)}
fn test_endpoint(origin: str, region: str) -> pkg.s3.Endpoint =
  pkg.s3.Endpoint{origin: origin, region: region}
fn long_origin(count: i64) -> string {
  mut output := builder()
  output.write("http://")
  output.write(repeated(count, "a"))
  return output.to_string()
}
fn main() {
  credentials := pkg.s3.Credentials{access_key: "key", secret_key: "a\0b".bytes(), session_token: None}
  endpoint := pkg.s3.Endpoint{origin: "https://example.com", region: "us-east-1"}
  arena {
    fields := [pkg.s3.Field{name: "x", value: ""}]
    empty := fields[0..0]
"#,
    );
    let mut expected = String::new();
    let defaults = [
        "credentials",
        "endpoint",
        "\"GET\"",
        "\"/\"",
        "empty",
        "empty",
        "\"\".bytes()",
        "0",
    ]
    .map(str::to_string);
    let mut case = |arg: usize, expression: String, good: bool| {
        let mut args = defaults.clone();
        args[arg] = expression;
        main.push_str(&format!("    print(observed({}))\n", args.join(", ")));
        expected.push_str(if good { "true\n" } else { "false\n" });
    };
    for (origin, good) in [
        ("http://LOCAL_host:00080", true),
        ("https://example.com:443", true),
        ("http://[::1]:8080", true),
        ("http://[::::]", true),
        ("http://", false),
        ("HTTP://example.com", false),
        ("ftp://host", false),
        ("http://user@host", false),
        ("http://host/", false),
        ("http://host?x", false),
        ("http://host#x", false),
        ("http://host%00", false),
        ("http://host\0", false),
        ("http://host\t", false),
        ("http://é", false),
        ("http://host:0", false),
        ("http://host:65536", false),
        ("http://host:000080", false),
        ("http://host:", false),
        ("http://host:8:9", false),
        ("http://[]", false),
        ("http://[1]", false),
        ("http://[::1", false),
        ("http://[::1]suffix", false),
        ("http://[fe80::1%eth0]", false),
    ] {
        case(
            1,
            format!(
                "pkg.s3.Endpoint{{origin: {}, region: \"us-east-1\"}}",
                quote(origin)
            ),
            good,
        );
    }
    for (region, good) in [
        ("", false),
        ("US-east-1", false),
        ("a/b", false),
        ("auto", true),
        ("a-1", true),
    ] {
        case(
            1,
            format!(
                "pkg.s3.Endpoint{{origin: \"http://host\", region: {}}}",
                quote(region)
            ),
            good,
        );
    }
    for (method, good) in [
        ("", false),
        ("get", false),
        ("GET ", false),
        ("GÉT", false),
        ("GET\r\n", false),
        ("POST", true),
    ] {
        case(2, quote(method), good);
    }
    for (path, good) in [("", false), ("relative", false), ("/a//../%?é\0", true)] {
        case(3, quote(path), good);
    }
    for (access, good) in [
        ("", false),
        ("bad/key", false),
        ("bad, key", false),
        ("é", false),
        ("key_1-A", true),
    ] {
        case(
            0,
            format!(
                "pkg.s3.Credentials{{access_key: {}, secret_key: \"secret\".bytes(), session_token: None}}",
                quote(access)
            ),
            good,
        );
    }
    case(
        0,
        "pkg.s3.Credentials{access_key: \"key\", secret_key: \"\".bytes(), session_token: None}"
            .into(),
        false,
    );
    for (token, good) in [
        ("", false),
        ("a b", false),
        ("\t", false),
        ("\0", false),
        ("é", false),
        ("token/+==", true),
    ] {
        case(
            0,
            format!(
                "pkg.s3.Credentials{{access_key: \"key\", secret_key: \"secret\".bytes(), session_token: Some({})}}",
                quote(token)
            ),
            good,
        );
    }
    for now in [
        "-1",
        "-9223372036854775807",
        "9223372036854775807",
        "999999999",
    ] {
        case(7, now.into(), !now.starts_with('-'));
    }
    // Length boundary values are formed at runtime rather than bloating compiler input.
    for (count, good) in [(256, true), (257, false)] {
        case(
            0,
            format!("test_credentials(repeated({count}, \"a\"), \"s\".bytes())"),
            good,
        );
    }
    for (count, good) in [(4096, true), (4097, false)] {
        case(
            0,
            format!(
                "pkg.s3.Credentials{{access_key: \"key\", secret_key: repeated({count}, \"s\").bytes(), session_token: None}}"
            ),
            good,
        );
    }
    for (count, good) in [(16384, true), (16385, false)] {
        case(
            0,
            format!("token_credentials(repeated({count}, \"t\"))"),
            good,
        );
    }
    for (count, good) in [(1017, true), (1018, false)] {
        case(
            1,
            format!("test_endpoint(long_origin({count}), \"r\")"),
            good,
        );
    }
    for (count, good) in [(64, true), (65, false)] {
        case(
            1,
            format!("test_endpoint(\"http://host\", repeated({count}, \"a\"))"),
            good,
        );
    }
    for (count, good) in [(32, true), (33, false)] {
        case(2, format!("repeated({count}, \"A\")"), good);
    }
    for (count, good) in [(8192, true), (8193, false)] {
        case(3, format!("repeated({count}, \"/\")"), good);
    }
    drop(case);
    let mut index = 0;
    for (name, value, good) in [
        ("Host", "x", false),
        ("Content-Length", "0", false),
        ("AUTHORIZATION", "x", false),
        ("transfer-encoding", "chunked", false),
        ("connection", "close", false),
        ("trailer", "x", false),
        ("te", "trailers", false),
        ("upgrade", "websocket", false),
        ("proxy-authorization", "x", false),
        ("proxy-authenticate", "x", false),
        ("expect", "100-continue", false),
        ("x-amz-date", "x", false),
        ("x-amz-content-sha256", "x", false),
        ("x-amz-security-token", "x", false),
        ("", "", false),
        ("x:y", "", false),
        ("x y", "", false),
        ("é", "", false),
        ("x", "\r", false),
        ("x", "\n", false),
        ("x", "\0", false),
        ("x", "é", false),
        ("range", " bytes=0-9 ", true),
        ("content-md5", "hash", true),
        ("date", "now", true),
        ("x-amz-meta-test", " \t ", true),
    ] {
        main.push_str(&format!("    h{index} := [pkg.s3.Field{{name: {}, value: {}}}]\n    print(observed(credentials, endpoint, \"GET\", \"/\", empty, h{index}, \"\".bytes(), 0))\n",quote(name),quote(value)));
        expected.push_str(if good { "true\n" } else { "false\n" });
        index += 1;
    }
    for (name, value, good) in [
        ("", "", false),
        ("X-AmZ-Date", "x", false),
        ("é\0", "?%", true),
        ("a", "", true),
    ] {
        main.push_str(&format!("    q{index} := [pkg.s3.Field{{name: {}, value: {}}}]\n    print(observed(credentials, endpoint, \"GET\", \"/\", q{index}, empty, \"\".bytes(), 0))\n",quote(name),quote(value)));
        expected.push_str(if good { "true\n" } else { "false\n" });
        index += 1;
    }
    main.push_str("    duplicate := [pkg.s3.Field{name: \"x\", value: \"one\"}, pkg.s3.Field{name: \"X\", value: \"two\"}]\n    print(observed(credentials, endpoint, \"GET\", \"/\", empty, duplicate, \"\".bytes(), 0))\n");
    expected.push_str("false\n");
    for (role, name_len, value_len, good) in [
        ("q", 1024, 8192, true),
        ("q", 1025, 0, false),
        ("q", 1, 8193, false),
        ("h", 256, 16384, true),
        ("h", 257, 0, false),
        ("h", 1, 16385, false),
    ] {
        let binding = format!("bounded{index}");
        index += 1;
        main.push_str(&format!("    name{index} := repeated({name_len}, \"x\")\n    value{index} := repeated({value_len}, \"a\")\n    {binding} := [pkg.s3.Field{{name: name{index}[0..name{index}.len()], value: value{index}[0..value{index}.len()]}}]\n"));
        let (query, headers) = if role == "q" {
            (binding.as_str(), "empty")
        } else {
            ("empty", binding.as_str())
        };
        main.push_str(&format!("    print(observed(credentials, endpoint, \"GET\", \"/\", {query}, {headers}, \"\".bytes(), 0))\n"));
        expected.push_str(if good { "true\n" } else { "false\n" });
    }
    main.push_str("  }\n}\n");
    if presign {
        let original = main.clone();
        main = main.replace(
            "pkg.s3.request(credentials, endpoint, method, path, query, headers, body, now_ns)",
            "pkg.s3.presign(credentials, endpoint, method, path, query, headers, now_ns, 86400)",
        );
        assert_ne!(main, original, "presign validation routing");
    }
    let project = Project::new(&main);
    let exe = project.build(&main, true, Profile::Dev);
    assert_eq!(run_bounded(&exe), expected);
}

#[test]
fn count_and_body_length_boundaries() {
    if !backend_available() {
        return;
    }
    let main = r#"import pkg.s3
fn main() {
  print(pkg.s3.size_probe(128, 120, 1073741824))
  print(pkg.s3.size_probe(129, 120, 1073741824))
  print(pkg.s3.size_probe(128, 121, 1073741824))
  print(pkg.s3.size_probe(128, 120, 1073741825))
  print(pkg.s3.size_probe(0, 0, 0))
}

"#;
    let project = Project::new(main);
    // Reach the actual admission predicate without allocating/copying a one-GiB success body.
    let source = format!(
        "{}\npub fn size_probe(q: i64, h: i64, b: i64) -> bool = input_sizes_valid(q, h, b)\n",
        fixture("apps/s3/pkg/s3.align")
    );
    std::fs::write(project.0.join("pkg/s3.align"), source).unwrap();
    let exe = project.build(main, true, Profile::Dev);
    assert_eq!(run_bounded(&exe), "true\nfalse\nfalse\nfalse\ntrue\n");
}

#[test]
fn explicit_body_wire() {
    if !backend_available() {
        return;
    }
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let authority = listener.local_addr().unwrap().to_string();
    let main = format!(
        r#"import std.http
fn main() -> Result<(), Error> {{
  client := http.client()
  client.timeout(3000000000)
  get := client.get("http://{authority}/get")?
  post := client.post("http://{authority}/post", "")?
  absent := http.request("PUT", "http://{authority}/absent")
  absent_response := client.request(absent)?
  empty := http.request("PUT", "http://{authority}/empty")
  empty.body("")
  empty_response := client.request(empty)?
  replaced := http.request("PUT", "http://{authority}/replaced")
  replaced.body("old")
  replaced.body("")
  replaced_response := client.request(replaced)?
  arena {{
    urls := ["http://{authority}/batch"]
    batch := client.get_many(urls, 1)?
  }}
  print(1)
  return Ok(())
}}
"#
    );
    let project = Project::new(&main);
    let exe = project.build(&main, true, Profile::Dev);
    std::thread::scope(|scope| {
        let peer = scope.spawn(|| {
            let mut stream = accept(&listener);
            for (method, path, present) in [
                ("GET", "get", false),
                ("POST", "post", true),
                ("PUT", "absent", false),
                ("PUT", "empty", true),
                ("PUT", "replaced", true),
                ("GET", "batch", false),
            ] {
                let (line, headers, body) = read_request(&mut stream);
                assert_eq!(line, format!("{method} /{path} HTTP/1.1"));
                let mut expected = vec![("host".to_string(), authority.clone())];
                if present {
                    expected.push(("content-length".to_string(), "0".to_string()));
                }
                assert_eq!(headers, expected);
                assert_eq!(body.len(), 0);
                stream
                    .write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 0\r\n\r\n")
                    .unwrap();
            }
            let mut byte = [0];
            assert_eq!(stream.read(&mut byte).unwrap(), 0);
        });
        assert_eq!(run_bounded(&exe), "1\n");
        peer.join().unwrap();
    });
}

#[test]
fn imports_effects_cache() {
    cache_cases(false);
}

#[test]
fn presign_imports_effects_cache() {
    cache_cases(true);
}

fn cache_cases(presign: bool) {
    use align_driver::{CacheContext, UnitReuse, build_package};
    let main = r#"import pkg.s3
fn main() -> Result<(), Error> {
  credentials := pkg.s3.Credentials{access_key: "key", secret_key: "secret".bytes(), session_token: None}
  endpoint := pkg.s3.Endpoint{origin: "https://example.com", region: "us-east-1"}
  arena {
    fields := [pkg.s3.Field{name: "unused", value: ""}]
    empty := fields[0..0]
    request := pkg.s3.request(credentials, endpoint, "GET", "/", empty, empty, "".bytes(), 0)?
  }
  return Ok(())
}
"#;
    let main = if presign {
        main.replace(
            "pkg.s3.request(credentials, endpoint, \"GET\", \"/\", empty, empty, \"\".bytes(), 0)",
            "pkg.s3.presign(credentials, endpoint, \"GET\", \"/\", empty, empty, 0, 86400)",
        )
    } else {
        main.to_owned()
    };
    assert_eq!(main.contains("pkg.s3.presign("), presign);
    let main = main.as_str();
    let project = Project::new(main);
    let context = CacheContext::at(project.0.join("cache"));
    let build = || {
        let mut sm = SourceMap::new();
        let mut result = build_package(
            &mut sm,
            &project.entry(),
            main,
            &context,
            UnitReuse::Allowed,
        );
        assert!(
            !result.diags.has_errors(),
            "{}",
            align_driver::format_diagnostics(&sm, &result.diags)
        );
        let hit = result
            .units
            .iter()
            .find(|u| u.unit == "pkg.s3")
            .unwrap()
            .frontend
            .as_ref()
            .unwrap()
            .hit;
        let mir = (0..result.units.len())
            .map(|i| align_mir::print::program_to_string(result.materialize(i).unwrap()))
            .collect::<Vec<_>>();
        (hit, mir)
    };
    let (hit, cold) = build();
    assert!(!hit);
    assert_eq!(build(), (true, cold.clone()));
    let path = project.0.join("pkg/s3.align");
    let original = fixture("apps/s3/pkg/s3.align");
    let changed = original.replace(
        "signing.write(\"AWS4-HMAC-SHA256\\n\")",
        "signing.write(\"AWS4-HMAC-SHA256-changed\\n\")",
    );
    assert_ne!(changed, original);
    std::fs::write(&path, changed).unwrap();
    let (hit, edited) = build();
    assert!(!hit);
    assert_ne!(edited, cold);
    std::fs::write(&path, original).unwrap();
    assert_eq!(build(), (true, cold));

    let negative = main.replace(
        "fn main() -> Result<(), Error>",
        "fn impure(value: i64) -> Result<(), Error>",
    ) + "\nfn worker(value: i64) -> i64 { impure(value) else { return 0 }; return value }\nfn main() { arena { print([1, 2].par_map(worker).sum()) } }\n";
    std::fs::write(project.0.join("main.align"), &negative).unwrap();
    for unit in [false, true] {
        let mut sm = SourceMap::new();
        let text = if unit {
            let checked = check_per_unit(&mut sm, &project.entry(), &negative);
            align_driver::format_diagnostics(&sm, &checked.diags)
        } else {
            let checked = check(&mut sm, &project.entry(), &negative);
            align_driver::format_diagnostics(&sm, &checked.diags)
        };
        assert!(text.to_ascii_lowercase().contains("pure"), "{text}");
    }
}

fn authorization(canonical: &str, signed: &str, secret: &[u8]) -> String {
    let scope = "20130524/us-east-1/s3/aws4_request";
    let input = format!(
        "AWS4-HMAC-SHA256\n{DATE}\n{scope}\n{}",
        hex(&sha(canonical.as_bytes()))
    );
    let mut key = b"AWS4".to_vec();
    key.extend_from_slice(secret);
    for part in ["20130524", "us-east-1", "s3", "aws4_request"] {
        key = mac(&key, part.as_bytes());
    }
    format!(
        "AWS4-HMAC-SHA256 Credential=AKIAIOSFODNN7EXAMPLE/{scope},SignedHeaders={signed},Signature={}",
        hex(&mac(&key, input.as_bytes()))
    )
}

#[test]
fn signature_wire_vectors() {
    if !backend_available() {
        return;
    }
    let empty = hex(&sha(b""));
    let body_hash = hex(&sha(b"Welcome to Amazon S3."));
    let rows = [
        (
            "GET",
            "/test.txt",
            "",
            format!(
                "host:examplebucket.s3.amazonaws.com\nrange:bytes=0-9\nx-amz-content-sha256:{empty}\nx-amz-date:{DATE}\n"
            ),
            "host;range;x-amz-content-sha256;x-amz-date",
            empty.clone(),
            "7344ae5b7ee6c3e7e6b0fe0640412a37625d1fbfff95c48bbb2dc43964946972",
            "f0e8bdb87c964420e857bd35b5d6ed310bd44f0170aba48dd91039c6036bdb41",
        ),
        (
            "PUT",
            "/test%24file.text",
            "",
            format!(
                "date:Fri, 24 May 2013 00:00:00 GMT\nhost:examplebucket.s3.amazonaws.com\nx-amz-content-sha256:{body_hash}\nx-amz-date:{DATE}\nx-amz-storage-class:REDUCED_REDUNDANCY\n"
            ),
            "date;host;x-amz-content-sha256;x-amz-date;x-amz-storage-class",
            body_hash,
            "9e0e90d9c76de8fa5b200d8c849cd5b8dc7a3be3951ddb7f6a76b4158342019d",
            "98ad721746da40c64f1a55b78f14c238d841ea1380cd77a1b5971af0ece108bd",
        ),
        (
            "GET",
            "/",
            "lifecycle=",
            format!(
                "host:examplebucket.s3.amazonaws.com\nx-amz-content-sha256:{empty}\nx-amz-date:{DATE}\n"
            ),
            "host;x-amz-content-sha256;x-amz-date",
            empty.clone(),
            "9766c798316ff2757b517bc739a67f6213b4ab36dd5da2f94eaebf79c77395ca",
            "fea454ca298b7da1c68078a5d1bdbfbbe0d65c699e0f91ac7a200a0136783543",
        ),
        (
            "GET",
            "/",
            "max-keys=2&prefix=J",
            format!(
                "host:examplebucket.s3.amazonaws.com\nx-amz-content-sha256:{empty}\nx-amz-date:{DATE}\n"
            ),
            "host;x-amz-content-sha256;x-amz-date",
            empty,
            "df57d21db20da04d7fa30298dd4488ba3a2b47ca3a489c74750e0f1e7df1b9b7",
            "34b48302e7b5fa45bde8084f4b7868a86f0a534bc59db6670ed5711ef69dc6f7",
        ),
    ];
    // A test-only export reaches the actual private signing function. The package's request
    // construction and canonicalization are independently exercised over the wire below.
    let adapter = r#"
pub fn vector_auth(canonical: str, signed: str) -> string {
  credentials := Credentials{access_key: "AKIAIOSFODNN7EXAMPLE", secret_key: "wJalrXUtnFEMI/K7MDENG/bPxRfiCYEXAMPLEKEY".bytes(), session_token: None}
  digest := crypto.sha256(canonical)
  encoded := encoding.hex_encode(digest[0..digest.len()])
  return authorization(credentials, "us-east-1", "20130524T000000Z", encoded, signed)
}
"#;
    let mut main = String::from("import pkg.s3\nfn main() {\n");
    let mut expected = String::new();
    for (method, uri, query, headers, signed, payload, hash, signature) in rows {
        let canonical = format!("{method}\n{uri}\n{query}\n{headers}\n{signed}\n{payload}");
        assert_eq!(hex(&sha(canonical.as_bytes())), hash);
        let auth = authorization(&canonical, signed, SECRET);
        assert_eq!(auth.rsplit('=').next(), Some(signature));
        main.push_str(&format!(
            "print(pkg.s3.vector_auth({}, {}))\n",
            quote(&canonical),
            quote(signed)
        ));
        expected.push_str(&auth);
        expected.push('\n');
    }
    main.push_str("}\n");
    for unit in [false, true] {
        let project = Project::new(&main);
        std::fs::write(
            project.0.join("pkg/s3.align"),
            format!("{}{adapter}", fixture("apps/s3/pkg/s3.align")),
        )
        .unwrap();
        let exe = project.build(&main, unit, Profile::Dev);
        assert_eq!(run_bounded(&exe), expected);
    }
}

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
                "align-s3-{}-{}",
                std::process::id(),
                NEXT.fetch_add(1, Ordering::Relaxed)
            ));
            match std::fs::create_dir(&dir) {
                Ok(()) => break Self(dir),
                Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => continue,
                Err(error) => panic!("acquire project: {error}"),
            }
        };
        std::fs::create_dir(project.0.join("pkg")).unwrap();
        std::fs::write(
            project.0.join("pkg/s3.align"),
            fixture("apps/s3/pkg/s3.align"),
        )
        .unwrap();
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

fn percent(text: &str, path: bool) -> String {
    let mut encoded = String::new();
    for byte in text.bytes() {
        if byte.is_ascii_alphanumeric() || b"-_.~".contains(&byte) || (path && byte == b'/') {
            encoded.push(char::from(byte));
        } else {
            encoded.push_str(&format!("%{byte:02X}"));
        }
    }
    encoded
}

fn presigned_oracle(
    origin: &str,
    method: &str,
    path: &str,
    query: &[(&str, &str)],
    headers: &[(&str, &str)],
    token: Option<&str>,
    secret: &[u8],
) -> (String, String) {
    let authority = origin.split_once("://").unwrap().1;
    let mut rows = std::collections::BTreeMap::from([("host".to_owned(), authority.to_owned())]);
    for (name, value) in headers {
        rows.insert(
            name.to_ascii_lowercase(),
            value.split_ascii_whitespace().collect::<Vec<_>>().join(" "),
        );
    }
    let names = rows.keys().cloned().collect::<Vec<_>>().join(";");
    let mut fields = query
        .iter()
        .map(|(k, v)| (percent(k, false), percent(v, false)))
        .collect::<Vec<_>>();
    for (name, value) in [
        ("X-Amz-Algorithm", "AWS4-HMAC-SHA256"),
        (
            "X-Amz-Credential",
            "AKIAIOSFODNN7EXAMPLE/20130524/us-east-1/s3/aws4_request",
        ),
        ("X-Amz-Date", DATE),
        ("X-Amz-Expires", "86400"),
        ("X-Amz-SignedHeaders", &names),
    ] {
        fields.push((percent(name, false), percent(value, false)));
    }
    if let Some(token) = token {
        fields.push(("X-Amz-Security-Token".into(), percent(token, false)));
    }
    fields.sort();
    let query = fields
        .iter()
        .map(|(k, v)| format!("{k}={v}"))
        .collect::<Vec<_>>()
        .join("&");
    let uri = percent(path, true);
    let canonical_headers = rows
        .iter()
        .map(|(k, v)| format!("{k}:{v}\n"))
        .collect::<String>();
    let canonical =
        format!("{method}\n{uri}\n{query}\n{canonical_headers}\n{names}\nUNSIGNED-PAYLOAD");
    let auth = authorization(&canonical, &names, secret);
    let signature = auth.rsplit('=').next().unwrap();
    (
        format!("{origin}{uri}?{query}&X-Amz-Signature={signature}"),
        canonical,
    )
}

#[test]
fn presign_vectors() {
    if !backend_available() {
        return;
    }
    let mut main = String::from("import pkg.s3\nfn main() -> Result<(), Error> { arena {\n");
    let mut expected = String::new();
    for (i, (origin, method, path, query, headers, token, secret)) in [
        (
            "https://examplebucket.s3.amazonaws.com",
            "GET",
            "/test.txt",
            vec![],
            vec![],
            None,
            SECRET,
        ),
        (
            "http://example.com:9000",
            "PUT",
            "/bucket/a//../%é\0",
            vec![
                ("a-", ""),
                ("a", "z"),
                ("a", " "),
                ("a", " "),
                ("é\0", "?%"),
            ],
            vec![
                ("X-Test", " alpha\t beta "),
                ("Content-Type", " application/octet-stream "),
            ],
            Some("token/+=="),
            b"a\0b".as_slice(),
        ),
    ]
    .into_iter()
    .enumerate()
    {
        let (url, canonical) =
            presigned_oracle(origin, method, path, &query, &headers, token, secret);
        if i == 0 {
            assert_eq!(
                hex(&sha(canonical.as_bytes())),
                "3bfa292879f6447bbcda7001decf97f4a54dc650c8942174ae0a9121cf58ad04"
            );
            assert!(
                url.ends_with("aeeed9bbccd4d02ee5c0109b86d86835f995330da4c265957d157751f604d404")
            );
        }
        let fields = |rows: &[(&str, &str)]| {
            rows.iter()
                .map(|(k, v)| format!("pkg.s3.Field{{name: {}, value: {}}}", quote(k), quote(v)))
                .collect::<Vec<_>>()
                .join(", ")
        };
        main.push_str(&format!("c{i} := pkg.s3.Credentials{{access_key: \"AKIAIOSFODNN7EXAMPLE\", secret_key: {}.bytes(), session_token: {}}}\ne{i} := pkg.s3.Endpoint{{origin: {}, region: \"us-east-1\"}}\nf{i} := [pkg.s3.Field{{name: \"unused\", value: \"\"}}]\n", quote(std::str::from_utf8(secret).unwrap()), token.map(|t| format!("Some({})", quote(t))).unwrap_or("None".into()), quote(origin)));
        let mut input = Vec::new();
        for (role, rows) in [("q", &query), ("h", &headers)] {
            if rows.is_empty() {
                input.push(format!("f{i}[0..0]"));
            } else {
                main.push_str(&format!("{role}{i} := [{}]\n", fields(rows)));
                input.push(format!("{role}{i}"));
            }
        }
        main.push_str(&format!("r{i} := pkg.s3.presign(c{i}, e{i}, {}, {}, {}, {}, {NOW}, 86400)?\nprint(r{i}.method)\nprint(r{i}.url)\nprint(r{i}.headers.len())\n",quote(method),quote(path),input[0],input[1]));
        expected.push_str(&format!("{method}\n{url}\n{}\n", headers.len()));
        if !headers.is_empty() {
            main.push_str(&format!("print(r{i}.headers[0].name)\nprint(r{i}.headers[0].value)\nprint(r{i}.headers[1].name)\nprint(r{i}.headers[1].value)\n"));
            expected.push_str("content-type\napplication/octet-stream\nx-test\nalpha beta\n");
        }
    }
    main.push_str("}\nreturn Ok(())\n}\n");
    for unit in [false, true] {
        for profile in [Profile::Dev, Profile::Release] {
            let project = Project::new(&main);
            let exe = project.build(&main, unit, profile);
            assert_eq!(run_bounded(&exe), expected);
        }
    }
}

#[test]
fn presign_round_trip() {
    if !backend_available() {
        return;
    }
    for unit in [false, true] {
        for profile in [Profile::Dev, Profile::Release] {
            let listener = TcpListener::bind("127.0.0.1:0").unwrap();
            let authority = listener.local_addr().unwrap().to_string();
            let main = format!(
                r#"import pkg.s3
import std.http
fn make(method: str, token: Option<str>) -> Result<pkg.s3.PresignedRequest, Error> {{
  return arena {{
    access := "AKIAIOSFODNN7EXAMPLE".clone()
    mut secret := buffer(3)
    secret.append("a\0b")
    origin := "http://{authority}".clone()
    path := "/bucket/a//../%é\0".clone()
    value := " alpha\t beta ".clone()
    credentials := pkg.s3.Credentials{{access_key: access[0..access.len()], secret_key: secret.bytes(), session_token: token}}
    endpoint := pkg.s3.Endpoint{{origin: origin[0..origin.len()], region: "us-east-1"}}
    fields := [pkg.s3.Field{{name: "a", value: " "}}, pkg.s3.Field{{name: "a-", value: ""}}]
    headers := [pkg.s3.Field{{name: "X-Test", value: value[0..value.len()]}}, pkg.s3.Field{{name: "Content-Type", value: " application/octet-stream "}}]
    result := pkg.s3.presign(credentials, endpoint, method, path, fields, headers, {NOW}, 86400)?
    secret.append("changed")
    Ok(result)
  }}
}}
fn carry(value: pkg.s3.PresignedRequest) -> Option<pkg.s3.PresignedRequest> = Some(value)
fn send(borrow client: http_client, borrow signed: pkg.s3.PresignedRequest, body: slice<u8>) -> Result<(), Error> {{
  outgoing := http.request(signed.method, signed.url)
  mut i := 0
  loop {{
    if i >= signed.headers.len() {{ break }}
    outgoing.header(signed.headers[i].name, signed.headers[i].value)
    i = i + 1
  }}
  outgoing.body(body)
  response := client.request(outgoing)?
  print(response.status())
  print(response.body().as_str()?)
  return Ok(())
}}
fn main() -> Result<(), Error> {{
  client := http.client()
  client.timeout(3000000000)
  mut signed := carry(make("GET", None)?) else {{ return Err(Error.Invalid) }}
  send(client, signed, "".bytes())?
  signed = make("PUT", Some("token/+=="))?
  send(client, signed, "a\0b".bytes())?
  send(client, signed, "".bytes())?
  (owned_url, owned_method) := (signed.url, signed.method)
  print(owned_url.len() > 0)
  print(signed.headers[0].name)
  return Ok(())
}}
"#
            );
            let (helpers, entry) = main.split_once("fn main()").unwrap();
            let consumer = format!(
                "module pkg.consumer\n{}",
                helpers
                    .replace("fn make(", "pub fn make(")
                    .replace("fn carry(", "pub fn carry(")
                    .replace("fn send(", "pub fn send(")
            );
            let main = format!(
                "import pkg.consumer\nimport std.http\nfn main(){}",
                entry
                    .replace("carry(make(", "pkg.consumer.carry(pkg.consumer.make(")
                    .replace("= make(", "= pkg.consumer.make(")
                    .replace("  send(", "  pkg.consumer.send(")
            );
            let project = Project::new(&main);
            std::fs::write(project.0.join("pkg/consumer.align"), consumer).unwrap();
            let exe = project.build(&main, unit, profile);
            std::thread::scope(|scope| {
                let peer = scope.spawn(|| {
                let mut stream = accept(&listener);
                for (index, (method, token, body)) in [("GET", None, b"".as_slice()), ("PUT", Some("token/+=="), b"a\0b"), ("PUT", Some("token/+=="), b"")].into_iter().enumerate() {
                    let (url, _) = presigned_oracle(&format!("http://{authority}"), method, "/bucket/a//../%é\0", &[("a", " "), ("a-", "")], &[("content-type", "application/octet-stream"), ("x-test", "alpha beta")], token, b"a\0b");
                    let target = url.strip_prefix(&format!("http://{authority}")).unwrap();
                    let (line, headers, actual_body) = read_request(&mut stream);
                    assert_eq!(line, format!("{method} {target} HTTP/1.1"));
                    assert_eq!(actual_body, body);
                    assert_eq!(headers, vec![("host".into(), authority.clone()), ("content-type".into(), "application/octet-stream".into()), ("x-test".into(), "alpha beta".into()), ("content-length".into(), body.len().to_string())]);
                    // Reconstruct the signature input using only the captured target and headers.
                    let captured_target = line.split_whitespace().nth(1).unwrap();
                    let (unsigned, signature) = captured_target.rsplit_once("&X-Amz-Signature=").unwrap();
                    let (uri, query) = unsigned.split_once('?').unwrap();
                    let rows = headers.iter().filter(|(name,_)| name != "content-length").cloned().collect::<std::collections::BTreeMap<_,_>>();
                    let names = rows.keys().cloned().collect::<Vec<_>>().join(";");
                    let canonical_headers = rows.iter().map(|(k,v)| format!("{k}:{v}\n")).collect::<String>();
                    let canonical = format!("{method}\n{uri}\n{query}\n{canonical_headers}\n{names}\nUNSIGNED-PAYLOAD");
                    assert_eq!(authorization(&canonical, &names, b"a\0b").rsplit('=').next().unwrap(), signature);
                    let response = if index == 2 { b"HTTP/1.1 403 Forbidden\r\nContent-Length: 8\r\n\r\n<Error/>".as_slice() } else { b"HTTP/1.1 200 OK\r\nContent-Length: 0\r\n\r\n" };
                    stream.write_all(response).unwrap();
                }
            });
                assert_eq!(
                    run_bounded(&exe),
                    "200\n\n200\n\n403\n<Error/>\ntrue\ncontent-type\n"
                );
                peer.join().unwrap();
            });
        }
    }
}

#[test]
fn presign_expiry_counts() {
    if !backend_available() {
        return;
    }
    let mut main = String::from(
        r#"import pkg.s3
fn admitted(query: slice<pkg.s3.Field>, headers: slice<pkg.s3.Field>, now: i64, expiry: i64) -> bool {
  credentials := pkg.s3.Credentials{access_key: "key", secret_key: "secret".bytes(), session_token: None}
  endpoint := pkg.s3.Endpoint{origin: "http://127.0.0.1:1", region: "us-east-1"}
  return match pkg.s3.presign(credentials, endpoint, "GET", "/", query, headers, now, expiry) {
    Ok(_) => true, Err(error) => match error { Invalid => false, _ => { print("wrong error"); false } },
  }
}
fn main() { arena {
  fields := [pkg.s3.Field{name: "x", value: ""}]
  empty := fields[0..0]
"#,
    );
    let mut expected = String::new();
    for expiry in [i64::MIN, -1, 0, 1, 604800, 604801, i64::MAX] {
        let expression = if expiry == i64::MIN {
            "(-9223372036854775807 - 1)".into()
        } else {
            expiry.to_string()
        };
        main.push_str(&format!(
            "print(admitted(empty, empty, 9223372036854775807, {expression}))\n"
        ));
        expected.push_str(if (1..=604800).contains(&expiry) {
            "true\n"
        } else {
            "false\n"
        });
    }
    for (index, (q, h, good)) in [
        (0, 0, true),
        (128, 120, true),
        (129, 120, false),
        (128, 121, false),
        (129, 121, false),
    ]
    .into_iter()
    .enumerate()
    {
        let rows = |count: usize| {
            (0..count.max(1))
                .map(|i| format!("pkg.s3.Field{{name: \"x{i}\", value: \"\"}}"))
                .collect::<Vec<_>>()
                .join(",")
        };
        main.push_str(&format!("q{index} := [{}]\nh{index} := [{}]\nprint(admitted(q{index}[0..{q}], h{index}[0..{h}], 0, 1))\n",rows(q),rows(h)));
        expected.push_str(if good { "true\n" } else { "false\n" });
    }
    main.push_str("print(admitted(empty, empty, -1, 0))\n");
    expected.push_str("false\n");
    // Check serialized seconds and truncation, including a duration whose absolute ns deadline overflows.
    main.push_str(r#"credentials := pkg.s3.Credentials{access_key: "key", secret_key: "secret".bytes(), session_token: None}
endpoint := pkg.s3.Endpoint{origin: "http://127.0.0.1:1", region: "us-east-1"}
"#);
    for (index, (now, expiry, date)) in [
        (0, 1, "19700101T000000Z"),
        (999999999, 604800, "19700101T000000Z"),
        (i64::MAX, 604800, "22620411T234716Z"),
    ]
    .into_iter()
    .enumerate()
    {
        main.push_str(&format!("r{index} := pkg.s3.presign(credentials, endpoint, \"GET\", \"/\", empty, empty, {now}, {expiry}) else {{ print(\"unexpected error\"); return }}\nprint(r{index}.url.contains(\"X-Amz-Date={date}&X-Amz-Expires={expiry}&\"))\n"));
        expected.push_str("true\n");
    }
    main.push_str("}\n}\n");
    let project = Project::new(&main);
    let exe = project.build(&main, true, Profile::Dev);
    assert_eq!(run_bounded(&exe), expected);
}
