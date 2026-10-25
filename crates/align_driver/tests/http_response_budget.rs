//! Plan241: consuming operand staging, checked imports and bounded ordinary responses.
mod common;
use common::*;
#[path = "helpers/owned_fixture.rs"]
mod owned_fixture;

#[test]
fn response_budget_rejects_wrong_arguments_and_borrowed_consumption() {
    for body in [
        "ctx.respond_timeout_ns()",
        "ctx.respond_timeout_ns(rb)",
        "ctx.respond_timeout_ns(rb, 1, 2)",
        "ctx.respond_timeout_ns(1, 1)",
        "ctx.respond_timeout_ns(rb, true)",
        "ctx.respond_timeout_ns(rb, 1 as u64)",
        "{ ctx.respond_timeout_ns(rb, 1) else {}; ctx.method(); Ok(()) }",
        "{ ctx.respond_timeout_ns(rb, 1) else {}; rb.body(\"bad\"); Ok(()) }",
    ] {
        let source = format!(
            "import std.http\nfn send(ctx: http_request_ctx, rb: response_builder) -> Result<(), Error> = {body}\nfn main() {{}}\n"
        );
        let files = [("main.align", source.as_str())];
        assert!(
            assert_same_verdict("response-budget-bad", &files, "main.align")
                .diags
                .has_errors(),
            "{body}"
        );
    }
    for mode in ["borrow ", "borrow mut "] {
        for (ctx_mode, rb_mode) in [(mode, ""), ("", mode)] {
            let source = format!(
                "import std.http\nfn send({ctx_mode}ctx: http_request_ctx, {rb_mode}rb: response_builder) -> Result<(), Error> = ctx.respond_timeout_ns(rb, 1)\nfn main() {{}}\n"
            );
            assert!(check_errs("response-budget-borrow", &source), "{source}");
        }
    }
}

#[test]
fn response_budget_reserves_consuming_operands_until_action() {
    for argument in [
        "{ taken := ctx; 1 }",
        "{ ctx = other; 1 }",
        "{ taken := rb; 1 }",
        "{ rb = http.response(201); 1 }",
        "{ if flag { taken := rb }; 1 }",
        "loop { taken := ctx; break 1 }",
        "{ ctx.respond(http.response(200)) else {}; 1 }",
    ] {
        let source = format!(
            "import std.http\nfn send(c: http_request_ctx, other: http_request_ctx, b: response_builder, flag: bool) -> Result<(), Error> {{\nmut ctx := c\nmut rb := b\nctx.respond_timeout_ns(rb, {argument})\n}}\nfn main() {{}}\n"
        );
        let files = [("main.align", source.as_str())];
        assert!(
            assert_same_verdict("response-budget-reservation", &files, "main.align")
                .diags
                .has_errors(),
            "{argument}"
        );
    }
}

#[test]
fn response_budget_reserves_each_selected_builder_source() {
    for builder in [
        "rb",
        "{ rb }",
        "if flag { rb } else { http.response(201) }",
        "match choice { 0 => rb, _ => http.response(201) }",
        "unsafe { if flag { rb } else { http.response(201) } }",
        "task_group { if flag { rb } else { http.response(201) } }",
        "arena { if flag { rb } else { http.response(201) } }",
        "optional else rb",
        "optional else if flag { rb } else { http.response(201) }",
        "loop { break rb }",
    ] {
        for (timeout, rejected) in [
            ("1", false),
            ("{ return Ok(()) }", false),
            ("{ taken := rb; 1 }", true),
            ("{ rb = http.response(202); 1 }", true),
            ("{ other.respond(rb) else {}; 1 }", true),
        ] {
            // A loop break completes its own transfer before the outer call; replacing the old
            // source then is safe. Selected branch/fallback sources remain deferred instead.
            let rejected =
                rejected && !(builder.starts_with("loop") && timeout.starts_with("{ rb ="));
            let source = format!(
                "import std.http\nfn send(ctx: http_request_ctx, other: http_request_ctx, b: response_builder, optional: Option<response_builder>, flag: bool, choice: i64) -> Result<(), Error> {{\nmut rb := b\nctx.respond_timeout_ns({builder}, {timeout})\n}}\nfn main() {{}}\n"
            );
            let files = [("main.align", source.as_str())];
            let checked = assert_same_verdict("response-budget-selected", &files, "main.align");
            assert_eq!(
                checked.diags.has_errors(),
                rejected,
                "{builder}/{timeout}: {}",
                check_multi_diagnostics("response-budget-selected", &files, "main.align")
            );
        }
    }
}

#[test]
fn response_budget_reserves_selected_field_sources() {
    for builder in [
        "row.value",
        "if flag { row.value } else { http.response(201) }",
    ] {
        for (timeout, rejected) in [
            ("1", false),
            ("{ return Ok(()) }", false),
            ("{ taken := row.value; 1 }", true),
            ("{ row = Holder { value: http.response(202) }; 1 }", true),
        ] {
            let source = format!(
                "import std.http\nHolder {{ value: response_builder }}\nfn send(ctx: http_request_ctx, b: response_builder, flag: bool) -> Result<(), Error> {{\nmut row := Holder {{ value: b }}\nctx.respond_timeout_ns({builder}, {timeout})\n}}\nfn main() {{}}\n"
            );
            let files = [("main.align", source.as_str())];
            let checked = assert_same_verdict("response-budget-field", &files, "main.align");
            assert_eq!(
                checked.diags.has_errors(),
                rejected,
                "{builder}/{timeout}: {}",
                check_multi_diagnostics("response-budget-field", &files, "main.align")
            );
        }
    }
}

#[test]
fn response_budget_eager_exit_preserves_fresh_and_bound_cleanup() {
    for builder in [
        "rb",
        "http.response(200)",
        "if flag { rb } else { http.response(201) }",
    ] {
        for timeout in [
            "{ return Ok(()) }",
            "if flag { return ctx.respond(http.response(202)) } else { 1000000000 }",
            "loop { return Ok(()) }",
        ] {
            let source = format!(
                "import std.http\nfn send(ctx: http_request_ctx, rb: response_builder, flag: bool) -> Result<(), Error> = ctx.respond_timeout_ns({builder}, {timeout})\nfn main() {{}}\n"
            );
            let files = [("main.align", source.as_str())];
            let checked = assert_same_verdict("response-budget-exit", &files, "main.align");
            assert!(
                !checked.diags.has_errors(),
                "{builder}/{timeout}: {}",
                check_multi_diagnostics("response-budget-exit", &files, "main.align")
            );
            let mir = whole_mir_multi("response-budget-exit", &files, "main.align");
            if !timeout.starts_with("if") {
                assert!(!mir.contains("http_respond_timeout_ns("), "{mir}");
            }
        }
    }
}

#[test]
fn response_budget_imported_native_whole_and_per_unit() {
    owned_fixture::run("response_budget_imported_native_whole_and_per_unit", |_| {
        use std::io::{Read, Write};
        use std::net::{TcpListener, TcpStream};
        use std::process::Command;
        use std::time::{Duration, Instant};
        if !backend_available() {
            return;
        }
        let library = r#"module reply
import std.http
pub fn keep(error: Error) -> Error = error
pub fn send<T>(ctx: http_request_ctx, marker: T) -> Result<(), Error> {
  _ := marker
  rb := http.response(200)
  rb.body("hello")
  ctx.respond_timeout_ns(rb, 1000000000).map_err(keep)?
  Ok(())
}
pub fn abandoned(ctx: http_request_ctx) -> Result<(), Error> {
  ctx.respond_timeout_ns(http.response(201), { return Ok(()) })
}
"#;
        let entry = r#"import std.http
import std.cli
import reply
pub fn main(args: array<str>) -> Result<(), Error> {
  command := cli.command("owner")
  command.flag_i64("port", 0)
  options := command.parse(args)?
  server := http.serve("127.0.0.1", options.get_i64("port"))?
  server.accept_timeout_ns(2000000000)?
  first := server.accept()?
  reply.abandoned(first)?
  second := server.accept()?
  reply.send(second, 7)?
  third := server.accept()?
  reply.send(third, true)?
  Ok(())
}
"#;
        let files = [("reply.align", library), ("main.align", entry)];
        let checked = assert_same_verdict("response-budget-import", &files, "main.align");
        assert!(
            !checked.diags.has_errors(),
            "{}",
            check_multi_diagnostics("response-budget-import", &files, "main.align")
        );
        for units in [false, true] {
            let whole;
            let unit;
            let executable = if units {
                unit = build_per_unit_multi("response-budget-unit", &files, "main.align");
                let objects = unit.emit_objects(false);
                let refs = objects
                    .iter()
                    .map(|path| path.as_path())
                    .collect::<Vec<_>>();
                let executable = unit
                    .dir
                    .join(format!("server{}", std::env::consts::EXE_SUFFIX));
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
                whole = build_exe_multi("response-budget-whole", &files, "main.align");
                whole.exe.clone()
            };
            let probe = TcpListener::bind("127.0.0.1:0").unwrap();
            let port = probe.local_addr().unwrap().port();
            drop(probe);
            let mut child = Command::new(&executable)
                .args(["--port", &port.to_string()])
                .spawn()
                .unwrap();
            // The containing owned fixture guards this server and all linker children.
            let deadline = Instant::now() + Duration::from_secs(10);
            for (index, method) in ["GET", "HEAD", "GET"].iter().enumerate() {
                let mut socket = loop {
                    assert!(Instant::now() < deadline);
                    match TcpStream::connect(("127.0.0.1", port)) {
                        Ok(socket) => break socket,
                        Err(_) => std::thread::sleep(Duration::from_millis(2)),
                    }
                };
                socket
                    .set_read_timeout(Some(Duration::from_secs(3)))
                    .unwrap();
                socket
                    .set_write_timeout(Some(Duration::from_secs(3)))
                    .unwrap();
                socket
                    .write_all(
                        format!("{method} / HTTP/1.1\r\nHost: h\r\nConnection: close\r\n\r\n")
                            .as_bytes(),
                    )
                    .unwrap();
                let mut output = Vec::new();
                socket.read_to_end(&mut output).unwrap();
                if index == 0 {
                    assert!(output.is_empty());
                } else {
                    let body = if *method == "HEAD" { "" } else { "hello" };
                    assert_eq!(output, format!("HTTP/1.1 200 OK\r\nConnection: close\r\nContent-Length: 5\r\n\r\n{body}").as_bytes());
                }
            }
            assert!(child.wait().unwrap().success());
        }
    });
}
