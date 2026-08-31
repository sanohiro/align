//! `std.log` owner tests: exact output, gating/latching surface, ownership, and the one enum-tag ABI
//! extraction boundary.

mod common;
use common::*;

#[test]
fn logger_emits_exact_text_and_builder_records() {
    if !backend_available() {
        return;
    }
    let source = r#"import std.io
import std.log
pub fn main() -> Result<(), Error> {
  logger := log.new(io.stdout.buffered(), log.level.Info)
  print(logger.enabled(log.level.Debug))
  print(logger.enabled(log.level.Info))
  logger.line(log.level.Debug, "suppressed")
  logger.line(log.level.Info, "a\\b\nc\rdé")
  mut message := builder()
  message.write("built")
  message.write_char('\n')
  message.write("line")
  logger.line(log.level.Warn, message)
  logger.flush()?
  return Ok(())
}
"#;
    let output = build_and_run("std-log-exact", source);
    assert_eq!(
        output.status.code(),
        Some(0),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr),
    );
    assert_eq!(
        String::from_utf8_lossy(&output.stdout),
        "false\ntrue\n[INFO] a\\\\b\\nc\\rdé\n[WARN] built\\nline\n",
    );
}

#[test]
fn logger_is_a_region_tracked_move_handle_that_can_cross_function_boundaries() {
    if !backend_available() {
        return;
    }
    let source = r#"import std.io
import std.log
fn make(output: writer) -> Result<log.logger, Error> {
  logger := log.new(output, {
    output.flush()?
    log.level.Error
  })
  return Ok(logger)
}
fn emit(logger: log.logger) -> log.logger {
  logger.line(log.level.Warn, "suppressed")
  logger.line(log.level.Error, "kept")
  return logger
}
pub fn main() -> Result<(), Error> {
  output := io.stdout.buffered()
  logger := make(output)?
  logger2 := emit(logger)
  logger2.flush()?
  return Ok(())
}
"#;
    let output = build_and_run("std-log-move", source);
    assert_eq!(output.status.code(), Some(0));
    assert_eq!(String::from_utf8_lossy(&output.stdout), "[ERROR] kept\n");
}

#[test]
fn logger_surface_rejects_missing_import_invalid_carriers_and_unbound_methods() {
    assert!(check_errs(
        "std-log-import",
        "import std.io\nfn main() { logger := log.new(io.stdout, log.level.Info) }\n",
    ));
    assert!(check_errs(
        "std-log-bare-type",
        "import std.log\nfn take(logger: logger) {}\n",
    ));
    assert!(check_errs(
        "std-log-collection",
        "import std.log\nfn take(loggers: array<log.logger>) {}\n",
    ));
    assert!(check_errs(
        "std-log-temporary",
        "import std.io\nimport std.log\nfn main() { log.new(io.stdout, log.level.Info).line(log.level.Info, \"x\") }\n",
    ));
    assert!(check_errs(
        "std-log-message",
        "import std.io\nimport std.log\nfn main() { logger := log.new(io.stdout, log.level.Info); logger.line(log.level.Info, 7) }\n",
    ));
    assert!(check_errs(
        "std-log-use-after-move",
        "import std.io\nimport std.log\nfn main() { logger := log.new(io.stdout, log.level.Info); moved := logger; logger.line(log.level.Info, \"x\") }\n",
    ));
    assert!(check_errs(
        "std-log-writer-after-new",
        "import std.io\nimport std.log\nfn main() -> Result<(), Error> { output := io.stdout; logger := log.new(output, log.level.Info); output.flush()?; return logger.flush() }\n",
    ));
    assert!(check_errs(
        "std-log-writer-rebound-before-new-action",
        "import std.io\nimport std.log\nfn main() { mut output := io.stdout; logger := log.new(output, { output = io.stderr; log.level.Info }) }\n",
    ));
    assert!(check_errs(
        "std-log-connection-escape",
        "import std.net\nimport std.log\nfn steal() -> Result<log.logger, Error> { conn := tcp.connect(\"127.0.0.1\", 80)?; return Ok(log.new(conn.writer(), log.level.Info)) }\nfn main() -> Result<(), Error> { logger := steal()?; return Ok(()) }\n",
    ));
    assert!(check_errs(
        "std-log-shared-line",
        "import std.log\nfn bad(borrow logger: log.logger) { logger.line(log.level.Info, \"x\") }\nfn main() -> i32 = 0\n",
    ));
    assert!(check_errs(
        "std-log-shared-flush",
        "import std.log\nfn bad(borrow logger: log.logger) -> Result<(), Error> = logger.flush()\nfn main() -> i32 = 0\n",
    ));
}

#[test]
fn logger_and_level_cross_whole_program_and_per_unit_interfaces() {
    let files = &[
        (
            "logging.align",
            "module logging\nimport std.log\npub Holder { logger: log.logger }\npub DirectCarrier { Active(log.logger), Empty }\npub Carrier<T> { Active(T), Empty }\npub fn make(output: writer, minimum: log.level) -> log.logger = log.new(output, minimum)\npub fn keep<T>(value: T) -> T = value\npub fn maybe(logger: log.logger) -> Option<log.logger> = Some(logger)\npub fn ready(logger: log.logger) -> Result<log.logger, Error> = Ok(logger)\npub fn observe(borrow logger: log.logger) -> bool = logger.enabled(log.level.Info)\npub fn emit_borrowed(borrow mut logger: log.logger) { logger.line(log.level.Info, \"borrowed\") }\npub fn checkpoint(borrow mut logger: log.logger) -> Result<(), Error> = logger.flush()\npub fn wrap(logger: log.logger) -> Holder = Holder { logger: logger }\npub fn direct(logger: log.logger) -> DirectCarrier = DirectCarrier.Active(logger)\npub fn carry(logger: log.logger) -> Carrier<log.logger> = Carrier.Active(logger)\npub fn emit(carrier: Carrier<log.logger>) -> Result<(), Error> = match carrier {\n  Active(logger) => {\n    logger.line(log.level.Info, \"interface\")\n    return logger.flush()\n  }\n  Empty => Ok(())\n}\n",
        ),
        (
            "main.align",
            "import std.io\nimport std.log\nimport logging\npub fn main() -> Result<(), Error> {\n  mut logger := logging.make(io.stdout, log.level.Info)\n  print(logging.observe(logger))\n  logging.emit_borrowed(logger)\n  logging.checkpoint(logger)?\n  logger2 := logging.keep(logger)\n  holder := logging.wrap(logger2)\n  logger3 := holder.logger\n  logging.emit(logging.carry(logger3))?\n  return Ok(())\n}\n",
        ),
    ];
    let differential = diff_check_multi("std-log-interface", files, "main.align");
    assert!(
        !differential.whole_errors && !differential.per_unit_errors,
        "whole:\n{}\nper-unit:\n{}",
        differential.whole_diags,
        differential.per_unit_diags,
    );
    let checked = differential.per_unit;
    let logging = checked
        .summaries
        .iter()
        .find(|summary| summary.unit == "logging")
        .expect("logging summary");
    let make = logging
        .fns
        .iter()
        .find(|function| function.name == "make")
        .expect("make interface");
    assert_eq!(
        make.params[1].ty,
        align_interface::IType::Named {
            path: "log.level".to_string(),
            args: Vec::new()
        },
    );
    assert_eq!(
        make.ret,
        align_interface::IType::Named {
            path: "log.logger".to_string(),
            args: Vec::new()
        },
    );

    if backend_available() {
        let output =
            build_per_unit_multi("std-log-interface-run", files, "main.align").link_and_run();
        assert_eq!(output.status.code(), Some(0));
        assert_eq!(
            String::from_utf8_lossy(&output.stdout),
            "true\n[INFO] borrowed\n[INFO] interface\n"
        );
    }
}

#[test]
fn llvm_extracts_level_tag_only_at_each_logger_runtime_call() {
    if !backend_available() {
        return;
    }
    let mut sources = SourceMap::new();
    let checked = check(
        &mut sources,
        "std-log-abi",
        r#"import std.io
import std.log
pub fn main() -> Result<(), Error> {
  logger := log.new(io.stdout, log.level.Debug)
  if logger.enabled(log.level.Info) { logger.line(log.level.Info, "x") }
  logger.flush()?
  return Ok(())
}
"#,
    );
    assert!(
        !checked.diags.has_errors(),
        "{}",
        align_driver::format_diagnostics(&sources, &checked.diags),
    );
    let mir =
        align_driver::try_lower_to_mir(&checked.hir).expect("checked logger HIR must validate");
    let llvm = emit_llvm_ir(&mir, BuildTarget::Baseline, false, &[], false).expect("LLVM IR");
    assert!(llvm.contains("call ptr @align_rt_log_new(ptr"));
    assert!(llvm.contains("call i32 @align_rt_log_enabled(ptr"));
    assert!(llvm.contains("call i32 @align_rt_log_line(ptr"));
    assert!(llvm.contains("call i32 @align_rt_log_flush(ptr"));
    assert!(llvm.contains("call void @align_rt_log_free(ptr"));
    assert!(
        llvm.matches("extractvalue").count() >= 3,
        "each log.level operand must remain an aggregate until its runtime call:\n{llvm}",
    );
}
