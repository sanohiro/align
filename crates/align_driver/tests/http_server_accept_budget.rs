//! Plan 90 H3: source authority, eager operands and imported whole/per-unit reconstruction.
mod common;
use common::*;

#[test]
fn server_accept_budget_rejects_wrong_arguments_and_shared_authority() {
    for body in [
        "s.accept_timeout_ns()",
        "s.accept_timeout_ns(1, 2)",
        "s.accept_timeout_ns(true)",
        "s.accept_timeout_ns(1 as u64)",
    ] {
        let source = format!(
            "import std.http\nfn configure() -> Result<(), Error> {{ mut s := http.serve(\"127.0.0.1\", 0)?; {body} }}\nfn main() -> i32 = 0\n"
        );
        let diagnostics = check_diagnostics("server-budget-bad", &source);
        assert!(check_errs("server-budget-bad", &source), "{body}");
        assert!(!diagnostics.contains("unknown type"), "{diagnostics}");
    }
    let source = "import std.http\nfn configure() -> Result<(), Error> { mut s := http.serve(\"127.0.0.1\", 0)?; s.accept_timeout_ns(0) }\nfn main() -> i32 = 0\n";
    assert!(!check_errs("server-budget-inferred", source));
    // HttpServer has no source type spelling. Shared/exclusive borrowed authority is
    // checked by the independently forged HIR owner, without widening nameability.
}

#[test]
fn server_accept_budget_eager_exit_does_not_emit_a_setter_call() {
    if !backend_available() {
        return;
    }
    let source = "import std.http\nfn configure() -> Result<(), Error> {\n  mut s := http.serve(\"127.0.0.1\", 0)?\n  s.accept_timeout_ns({ return Ok(()) })?\n  return Ok(())\n}\nfn main() -> i32 = 0\n";
    assert!(
        !check_errs("server-budget-exit", source),
        "{}",
        check_diagnostics("server-budget-exit", source)
    );
    let mir = whole_mir_multi(
        "server-budget-exit",
        &[("main.align", source)],
        "main.align",
    );
    assert!(
        !mir.contains("http_server_accept_timeout_ns("),
        "diverging eager operand has no native side effect"
    );
}

#[test]
fn server_accept_budget_imported_generic_and_result_exits_agree() {
    let library = "module budget\nimport std.http\npub fn keep_error(error: Error) -> Error = error\npub fn configure<T>(marker: T) -> Result<(), Error> {\n  _ := marker\n  mut s := http.serve(\"127.0.0.1\", 0)?\n  s.accept_timeout_ns(-1) else {}\n  match s.accept_timeout_ns(0) {\n    Ok(_) => {}\n    Err(error) => { return Err(error) }\n  }\n  s.accept_timeout_ns(1000000000).map_err(keep_error)?\n  return Ok(())\n}\n";
    let entry = "import std.http\nimport budget\npub fn run() -> Result<(), Error> {\n  mut stream := http.serve(\"127.0.0.1\", 0)?\n  budget.configure(7)?\n  ctx := stream.accept()?\n  _ := ctx.method()\n  return Ok(())\n}\nfn main() -> i32 = 0\n";
    let files = [("budget.align", library), ("main.align", entry)];
    let verdict = assert_same_verdict("server-budget-import", &files, "main.align");
    assert!(
        !verdict.diags.has_errors(),
        "{}",
        check_multi_diagnostics("server-budget-import", &files, "main.align")
    );
    if !backend_available() {
        return;
    }
    let whole = build_exe_multi("server-budget-import-whole", &files, "main.align");
    assert!(
        std::process::Command::new(&whole.exe)
            .status()
            .unwrap()
            .success()
    );
    let units = build_per_unit_multi("server-budget-import-unit", &files, "main.align");
    assert!(units.link_and_run().status.success());
}

#[test]
fn server_accept_budget_reserves_receiver_until_its_action() {
    for (name, argument) in [
        ("move", "{ taken := server; 0 }"),
        ("replace", "{ server = other; 0 }"),
        ("nested", "0 + { taken := server; 0 }"),
        ("if", "{ if flag { server = other }; 0 }"),
        (
            "match",
            "{ match if flag { 1 } else { 0 } { 1 => { server = other }, _ => {} }; 0 }",
        ),
        ("loop", "loop { server = other; break 0 }"),
    ] {
        let source = format!(
            "import std.http\npub fn configure(flag: bool) -> Result<(), Error> {{\n  mut server := http.serve(\"127.0.0.1\", 0)?\n  other := http.serve(\"127.0.0.1\", 0)?\n  server.accept_timeout_ns({argument})\n}}\nfn main() -> i32 = 0\n"
        );
        let files = [("main.align", source.as_str())];
        let verdict = assert_same_verdict(
            &format!("server-budget-reserve-{name}"),
            &files,
            "main.align",
        );
        assert!(verdict.diags.has_errors(), "{name} admitted");
        let diagnostics = check_multi_diagnostics("server-budget-reserve", &files, "main.align");
        assert!(
            diagnostics.contains("invalidated before"),
            "{name}: {diagnostics}"
        );
    }
}

#[test]
fn server_accept_budget_reservations_are_call_scoped() {
    let source = r#"import std.http
pub fn nested() -> Result<(), Error> {
    server := http.serve("127.0.0.1", 0)?
    other := http.serve("127.0.0.1", 0)?
    server.accept_timeout_ns({ server.accept_timeout_ns(7) else {}; taken := other; 0 })?
    server.accept_timeout_ns(loop { break 1 })?
    taken := server
    Ok(())
}
pub fn terminal(flag: bool) -> Result<(), Error> {
    server := http.serve("127.0.0.1", 0)?
    server.accept_timeout_ns(if flag { taken := server; return Ok(()) } else { 0 })?
    taken := server
    Ok(())
}
pub fn ended() -> Result<(), Error> {
    server := http.serve("127.0.0.1", 0)?
    server.accept_timeout_ns({ taken := server; return Ok(()) })?
    return Ok(())
}
fn main() -> i32 = 0
"#;
    let files = [("main.align", source)];
    let verdict = assert_same_verdict("server-budget-scoped", &files, "main.align");
    assert!(
        !verdict.diags.has_errors(),
        "{}",
        check_multi_diagnostics("server-budget-scoped", &files, "main.align")
    );
    if backend_available() {
        assert!(
            std::process::Command::new(
                build_exe_multi("server-budget-scoped", &files, "main.align").exe
            )
            .status()
            .unwrap()
            .success()
        );
        assert!(
            build_per_unit_multi("server-budget-scoped-unit", &files, "main.align")
                .link_and_run()
                .status
                .success()
        );
    }
}
