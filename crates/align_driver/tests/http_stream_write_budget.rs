//! Plan 90 H2: source authority, eager operands and imported whole/per-unit reconstruction.
mod common;
use common::*;

#[test]
fn stream_write_budget_rejects_wrong_arguments_and_shared_authority() {
    for body in [
        "s.write_timeout_ns()",
        "s.write_timeout_ns(1, 2)",
        "s.write_timeout_ns(true)",
        "s.write_timeout_ns(1 as u64)",
    ] {
        let source = format!(
            "import std.http\nfn configure(s: http_stream) -> Result<(), Error> = {body}\nfn main() -> i32 = 0\n"
        );
        assert!(check_errs("stream-budget-bad", &source), "{body}");
    }
    for mode in ["", "borrow mut ", "borrow "] {
        let source = format!(
            "import std.http\nfn configure({mode}s: http_stream) -> Result<(), Error> = s.write_timeout_ns(0)\nfn main() -> i32 = 0\n"
        );
        assert_eq!(
            check_errs("stream-budget-authority", &source),
            mode == "borrow "
        );
    }
}

#[test]
fn stream_write_budget_eager_exit_does_not_emit_a_setter_call() {
    if !backend_available() {
        return;
    }
    let source = "import std.http\nfn configure(s: http_stream) -> Result<(), Error> {\n  s.write_timeout_ns({ return Ok(()) })?\n  return Ok(())\n}\nfn main() -> i32 = 0\n";
    assert!(
        !check_errs("stream-budget-exit", source),
        "{}",
        check_diagnostics("stream-budget-exit", source)
    );
    let mir = whole_mir_multi(
        "stream-budget-exit",
        &[("main.align", source)],
        "main.align",
    );
    assert!(
        !mir.contains("http_stream_write_timeout_ns("),
        "diverging eager operand has no native side effect"
    );
}

#[test]
fn stream_write_budget_imported_generic_and_result_exits_agree() {
    let library = "module budget\nimport std.http\npub fn keep_error(error: Error) -> Error = error\npub fn configure<T>(borrow mut s: http_stream, marker: T) -> Result<(), Error> {\n  _ := marker\n  s.write_timeout_ns(-1) else {}\n  match s.write_timeout_ns(0) {\n    Ok(_) => {}\n    Err(error) => { return Err(error) }\n  }\n  s.write_timeout_ns(1000000000).map_err(keep_error)?\n  return Ok(())\n}\n";
    let entry = "import std.http\nimport budget\npub fn run(s: http_stream) -> Result<(), Error> {\n  mut stream := s\n  budget.configure(stream, 7)?\n  stream.send_event(\"ok\")?\n  return stream.finish()\n}\nfn main() -> i32 = 0\n";
    let files = [("budget.align", library), ("main.align", entry)];
    let verdict = assert_same_verdict("stream-budget-import", &files, "main.align");
    assert!(
        !verdict.diags.has_errors(),
        "{}",
        check_multi_diagnostics("stream-budget-import", &files, "main.align")
    );
    if !backend_available() {
        return;
    }
    let whole = build_exe_multi("stream-budget-import-whole", &files, "main.align");
    assert!(
        std::process::Command::new(&whole.exe)
            .status()
            .unwrap()
            .success()
    );
    let units = build_per_unit_multi("stream-budget-import-unit", &files, "main.align");
    assert!(units.link_and_run().status.success());
}

#[test]
fn stream_write_budget_reserves_receiver_until_its_action() {
    for (name, argument) in [
        ("finish", "{ stream.finish() else {}; 0 }"),
        ("move", "{ taken := stream; 0 }"),
        ("replace", "{ stream = other; 0 }"),
        ("nested", "0 + { stream.finish() else {}; 0 }"),
        ("if", "{ if flag { stream = other }; 0 }"),
        (
            "match",
            "{ match if flag { 1 } else { 0 } { 1 => { stream = other }, _ => {} }; 0 }",
        ),
        ("loop", "loop { stream = other; break 0 }"),
    ] {
        let source = format!(
            "import std.http\npub fn configure(s: http_stream, t: http_stream, flag: bool) -> Result<(), Error> {{\n  mut stream := s\n  other := t\n  stream.write_timeout_ns({argument})\n}}\nfn main() -> i32 = 0\n"
        );
        let files = [("main.align", source.as_str())];
        let verdict = assert_same_verdict(
            &format!("stream-budget-reserve-{name}"),
            &files,
            "main.align",
        );
        assert!(verdict.diags.has_errors(), "{name} admitted");
        let diagnostics = check_multi_diagnostics("stream-budget-reserve", &files, "main.align");
        assert!(
            diagnostics.contains("invalidated before"),
            "{name}: {diagnostics}"
        );
    }
}

#[test]
fn stream_write_budget_reservations_are_call_scoped() {
    let source = r#"import std.http
pub fn nested(s: http_stream, t: http_stream) -> Result<(), Error> {
    s.write_timeout_ns({ s.write_timeout_ns(7) else {}; t.finish() else {}; 0 })?
    s.write_timeout_ns(loop { break 1 })?
    s.finish()
}
pub fn terminal(s: http_stream, flag: bool) -> Result<(), Error> {
    s.write_timeout_ns(if flag { return s.finish() } else { 0 })?
    s.finish()
}
pub fn ended(s: http_stream) -> Result<(), Error> {
    s.write_timeout_ns({ return s.finish() })?
    return Ok(())
}
fn main() -> i32 = 0
"#;
    let files = [("main.align", source)];
    let verdict = assert_same_verdict("stream-budget-scoped", &files, "main.align");
    assert!(
        !verdict.diags.has_errors(),
        "{}",
        check_multi_diagnostics("stream-budget-scoped", &files, "main.align")
    );
    if backend_available() {
        assert!(
            std::process::Command::new(
                build_exe_multi("stream-budget-scoped", &files, "main.align").exe
            )
            .status()
            .unwrap()
            .success()
        );
        assert!(
            build_per_unit_multi("stream-budget-scoped-unit", &files, "main.align")
                .link_and_run()
                .status
                .success()
        );
    }
}
