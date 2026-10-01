//! Text builder type formation, borrowed mutation, ownership, and imported ABI parity.

mod common;
use common::*;

fn assert_checked(name: &str, source: &str) {
    let mut sources = SourceMap::new();
    let checked = check(&mut sources, name, source);
    assert!(
        !checked.diags.has_errors(),
        "{}",
        align_driver::format_diagnostics(&sources, &checked.diags)
    );
    assert!(align_mir::lower_program_checked(&checked.hir, false, None).is_ok());
}

#[test]
fn receiver_argument_invalidation_matrix() {
    for (method, argument, ret) in [
        ("write", "\"x\"", "str"),
        ("write_int", "1", "i64"),
        ("write_bool", "true", "bool"),
        ("write_char", "'x'", "char"),
        ("write_float", "1.5", "f64"),
    ] {
        for receiver in [
            "output",
            "({ output })",
            "(task_group { output })",
            "(if true { output } else { builder() })",
            "(match 0 { 0 => output, _ => builder() })",
        ] {
            for action in [
                "reset(output)".to_owned(),
                "consume(output)".to_owned(),
                format!("({{ output = builder(); {argument} }})"),
            ] {
                let source = format!(
                    "fn create() -> builder = builder()\n\
                     fn reset(borrow mut output: builder) -> {ret} {{ output = builder(); return {argument} }}\n\
                     fn consume(output: builder) -> {ret} {{ text := output.to_string(); return {argument} }}\n\
                     fn main() -> i32 {{ mut output := create(); {receiver}.{method}({action}); return 0 }}\n"
                );
                let diagnostics = check_diagnostics("text-builder-receiver-invalidation", &source);
                assert!(
                    diagnostics.contains("value snapshot was invalidated"),
                    "{receiver}.{method}({action}): {diagnostics}"
                );
            }
        }
        let borrowed = format!(
            "fn reset(borrow mut output: builder) -> {ret} {{ output = builder(); return {argument} }}\n\
             fn append(borrow mut output: builder) {{ output.{method}(reset(output)) }}\n\
             fn main() -> i32 = 0\n"
        );
        let diagnostics =
            check_diagnostics("text-builder-borrowed-receiver-invalidation", &borrowed);
        assert!(
            diagnostics.contains("value snapshot was invalidated"),
            "{diagnostics}"
        );
        let positive = format!(
            "fn read(borrow output: builder) -> {ret} = {argument}\n\
             fn reset(borrow mut output: builder) -> {ret} {{ output = builder(); return {argument} }}\n\
             fn main() -> i32 {{ mut output := builder(); mut other := builder();\n\
               output.{method}(read(output)); output.{method}(reset(other));\n\
               builder().{method}(reset(output)); return 0 }}\n"
        );
        assert_checked("text-builder-stable-receiver", &positive);
    }
}

#[test]
fn isolated_borrowed_replacement() {
    for body in [
        "mut output := builder(); reset(output); print(output.to_string())",
        "arena { mut output := builder(); reset(output); print(output.to_string()) }",
    ] {
        // Keep this independent of owned-return helpers: those conservatively disable stack
        // headers in the caller and would mask a borrowed-place escape from the stack proof.
        let source = format!(
            "fn reset(borrow mut output: builder) {{ output = builder(); output.write(\"reset\") }}\n\
             fn main() -> i32 {{ {body}; return 0 }}\n"
        );
        assert_checked("text-builder-isolated-replacement", &source);
        if backend_available() {
            let output = build_and_run("text-builder-isolated-replacement", &source);
            assert!(output.status.success());
            assert_eq!(String::from_utf8_lossy(&output.stdout), "reset\n");
        }
    }
}

#[test]
fn borrowed_append_all_methods() {
    let source = r#"
fn append(borrow mut output: builder, text: str) {
  output.write(text)
  output.write_int(42)
  output.write_bool(true)
  output.write_char('あ')
  output.write_float(1.5)
}
fn forward(borrow mut output: builder) {
  append(output, "hello ".clone())
  ({ output }).write("!")
}
fn main() -> i32 {
  mut output: builder := builder(1)
  forward(output)
  text := output.to_string()
  print(text)
  return 0
}
"#;
    assert_checked("text-builder-append", source);
    if backend_available() {
        let output = build_and_run("text-builder-append", source);
        assert!(output.status.success());
        assert_eq!(
            String::from_utf8_lossy(&output.stdout),
            "hello 42trueあ1.5!\n"
        );
    }
}

#[test]
fn shared_append_matrix() {
    for (method, argument) in [
        ("write", "\"x\""),
        ("write_int", "1"),
        ("write_bool", "true"),
        ("write_char", "'x'"),
        ("write_float", "1.5"),
    ] {
        for receiver in [
            "output",
            "({ output })",
            "({ 1; output })",
            "({ { output } })",
            "(unsafe { output })",
            "(float(reassoc) { output })",
            "(task_group { output })",
            "(arena { output })",
            "(arena out { output })",
        ] {
            let source = format!(
                "fn append(borrow output: builder) {{ {receiver}.{method}({argument}) }}\nfn main() -> i32 = 0\n"
            );
            let diagnostics = check_diagnostics("text-builder-shared-append", &source);
            assert!(
                diagnostics.contains("shared-borrowed builder"),
                "{receiver}.{method}: {diagnostics}"
            );
        }
    }
    for receiver in [
        "(if true { output } else { builder() })",
        "(match 0 { 0 => output, _ => builder() })",
        "(loop { break output })",
    ] {
        let source = format!(
            "fn append(borrow output: builder) {{ {receiver}.write(\"x\") }}\nfn main() -> i32 = 0\n"
        );
        let diagnostics = check_diagnostics("text-builder-shared-selected", &source);
        assert!(diagnostics.contains("borrow"), "{receiver}: {diagnostics}");
        assert!(check_errs("text-builder-shared-selected", &source));
    }
    for receiver in [
        "builder()",
        "({ builder() })",
        "(if true { builder() } else { builder() })",
    ] {
        let source = format!("fn main() -> i32 {{ {receiver}.write(\"x\"); return 0 }}\n");
        assert_checked("text-builder-owned-expression", &source);
    }
}

#[test]
fn borrowed_receiver_values_preserve_owner() {
    for (receiver, expected) in [
        ("({ output })", "beforeAAafter\n"),
        ("({ 0; output })", "beforeAAafter\n"),
        ("(unsafe { output })", "beforeAAafter\n"),
        ("(float(reassoc) { output })", "beforeAAafter\n"),
        ("(task_group { output })", "beforeAAafter\n"),
        ("(arena out { output })", "beforeAAafter\n"),
        (
            "(if select { output } else { builder() })",
            "beforeAafter\n",
        ),
        (
            "(match (if select { 0 } else { 1 }) { 0 => output, _ => builder() })",
            "beforeAafter\n",
        ),
    ] {
        let source = format!(
            r#"
fn append(borrow mut output: builder, select: bool) {{ {receiver}.write("A") }}
fn main() -> i32 {{
  mut output := builder()
  output.write("before")
  append(output, true)
  append(output, false)
  output.write("after")
  print(output.to_string())
  return 0
}}
"#
        );
        assert_checked("text-builder-receiver-values", &source);
        if backend_available() {
            let output = build_and_run("text-builder-receiver-values", &source);
            assert!(output.status.success(), "{receiver}: {output:?}");
            assert_eq!(
                String::from_utf8_lossy(&output.stdout),
                expected,
                "{receiver}"
            );
        }
    }
}

#[test]
fn borrowed_receiver_survives_terminating_argument() {
    let source = r#"
fn append(borrow mut output: builder, select: bool) -> Result<(), Error> {
  argument: Result<str, Error> := Err(Error.Invalid)
  (if select { output } else { builder() }).write(argument?)
  return Ok(())
}
fn main() -> i32 {
  mut output := builder()
  output.write("before")
  first := append(output, true)
  second := append(output, false)
  output.write("after")
  print(output.to_string())
  return 0
}
"#;
    assert_checked("text-builder-terminating-argument", source);
    if backend_available() {
        let output = build_and_run("text-builder-terminating-argument", source);
        assert!(output.status.success());
        assert_eq!(String::from_utf8_lossy(&output.stdout), "beforeafter\n");
    }
}

#[test]
fn borrowed_owner_cannot_escape() {
    for (name, source) in [
        (
            "finish",
            "fn bad(borrow mut output: builder) -> string = output.to_string()",
        ),
        (
            "return",
            "fn bad(borrow mut output: builder) -> builder = output",
        ),
        (
            "retain",
            "fn bad(borrow mut output: builder) { saved := output; saved.write(\"x\") }",
        ),
        (
            "consume",
            "fn take(output: builder) {}\nfn bad(borrow mut output: builder) { take(output) }",
        ),
        (
            "shared-replace",
            "fn bad(borrow output: builder) { output = builder() }",
        ),
        (
            "alias",
            "fn both(borrow mut left: builder, borrow mut right: builder) {}\nfn bad() { mut output := builder(); both(output, output) }",
        ),
        (
            "immutable",
            "fn append(borrow mut output: builder) {}\nfn bad() { output := builder(); append(output) }",
        ),
    ] {
        let source = format!("{source}\nfn main() -> i32 = 0\n");
        let diagnostics = check_diagnostics(name, &source);
        assert!(check_errs(name, &source), "{name}: {diagnostics}");
        assert!(
            !diagnostics.contains("unknown type"),
            "{name}: {diagnostics}"
        );
    }
}

#[test]
fn type_and_placement_matrix() {
    for (name, declaration) in [
        ("arguments", "fn bad(output: builder<i64>) {}"),
        ("field", "Holder { output: builder }"),
        ("tuple", "fn bad(output: (builder, i64)) {}"),
        ("array", "fn bad(output: array<builder>) {}"),
        ("fixed-array", "fn bad(output: [builder; 1]) {}"),
        ("slice", "fn bad(output: slice<builder>) {}"),
        ("option", "fn bad(output: Option<builder>) {}"),
        ("result", "fn bad(output: Result<builder, Error>) {}"),
        ("fn-parameter", "fn bad(callback: fn(builder) -> ()) {}"),
        (
            "fn-value-parameter",
            "fn append(borrow mut output: builder) {}\nfn bad() { callback := append }",
        ),
        (
            "fn-value-result",
            "fn make() -> builder = builder()\nfn bad() { callback := make }",
        ),
        (
            "capture",
            "fn bad() { output := builder(); callback := fn { output.to_string() } }",
        ),
        (
            "task-result",
            "fn bad() { task_group { task := spawn(fn { builder() }); wait() } }",
        ),
    ] {
        let source = format!("{declaration}\nfn main() -> i32 = 0\n");
        let diagnostics = check_diagnostics(name, &source);
        assert!(check_errs(name, &source), "{name}: {diagnostics}");
        assert!(
            !diagnostics.contains("unknown type: 'builder'"),
            "{name}: {diagnostics}"
        );
    }
    // Result spelling and function-value formation are separate existing admission gates.
    assert_checked(
        "text-builder-fn-result-annotation",
        "fn unused(callback: fn() -> builder) {}\nfn main() -> i32 = 0\n",
    );
}

#[test]
fn owned_transfer() {
    let source = r#"
fn create() -> builder = builder(1)
fn forward(output: builder) -> builder {
  output.write("owned")
  return output
}
fn finish(output: builder) -> string = output.to_string()
fn main() -> i32 {
  first: builder := create()
  second := forward(first)
  print(finish(second))
  unused: builder := create()
  unused.write("dropped")
  return 0
}
"#;
    assert_checked("text-builder-owned-transfer", source);
    if backend_available() {
        let output = build_and_run("text-builder-owned-transfer", source);
        assert!(output.status.success());
        assert_eq!(String::from_utf8_lossy(&output.stdout), "owned\n");
    }
    for operation in ["forward(output)", "output.to_string()"] {
        let source = format!(
            "fn forward(output: builder) -> builder = output\nfn main() -> i32 {{ output := builder(); moved := {operation}; output.write(\"bad\"); return 0 }}\n"
        );
        let diagnostics = check_diagnostics("text-builder-moved-source", &source);
        assert!(diagnostics.contains("moved"), "{operation}: {diagnostics}");
    }
}

#[test]
fn shared_reader_keeps_caller_owner_live() {
    let source = r#"
import std.io
fn show(borrow output: builder) -> Result<(), Error> = io.stdout.write(output)
fn main() -> i32 {
  mut output := builder()
  output.write("shared")
  first := show(output)
  second := show(output)
  output.write("!")
  print(output.to_string())
  return 0
}
"#;
    assert_checked("text-builder-shared-reader", source);
    if backend_available() {
        let output = build_and_run("text-builder-shared-reader", source);
        assert!(output.status.success());
        assert_eq!(
            String::from_utf8_lossy(&output.stdout),
            "sharedsharedshared!\n"
        );
    }
}

#[test]
fn borrowed_append_control_flow() {
    for (name, body, expected) in [
        (
            "branch",
            "if true { output.write(\"A\") } else { output.write(\"B\") }; return Ok(())",
            "A",
        ),
        (
            "match",
            "match 0 { 0 => { output.write(\"M\") }, _ => { output.write(\"N\") } }; return Ok(())",
            "M",
        ),
        (
            "loop",
            "mut index: i64 := 0; loop { if index == 2 { break }; output.write_int(index); index = index + 1 }; return Ok(())",
            "01",
        ),
        (
            "else",
            "value: Option<i64> := None; number := value else { output.write(\"E\"); return Ok(()) }; output.write_int(number); return Ok(())",
            "E",
        ),
        (
            "try",
            "output.write(\"T\"); value: Result<i64, Error> := Err(Error.Invalid); number := value?; output.write_int(number); return Ok(())",
            "T",
        ),
        (
            "map-err",
            "output.write(\"R\"); value: Result<i64, Error> := Err(Error.Invalid); number := value.map_err(fn problem: Error { problem })?; output.write_int(number); return Ok(())",
            "R",
        ),
        (
            "return",
            "output.write(\"X\"); return Err(Error.Invalid)",
            "X",
        ),
    ] {
        let source = format!(
            r#"
fn append(borrow mut output: builder) -> Result<(), Error> {{
  abandoned := builder()
  abandoned.write("temporary")
  {body}
}}
fn main() -> i32 {{
  mut output := builder()
  result := append(output)
  print(output.to_string())
  arena {{
    mut arena_output := builder()
    arena_result := append(arena_output)
    print(arena_output.to_string())
  }}
  return 0
}}
"#
        );
        assert_checked(name, &source);
        if backend_available() {
            let output = build_and_run(name, &source);
            assert!(output.status.success(), "{name}: {output:?}");
            assert_eq!(
                String::from_utf8_lossy(&output.stdout),
                format!("{expected}\n{expected}\n"),
                "{name}"
            );
        }
    }
}

#[test]
fn imported_builder_helpers_match_whole_program() {
    let files = &[
        (
            "builders.align",
            r#"module builders
pub fn create() -> builder = builder()
pub fn identity<T>(value: T) -> T = value
pub fn append(borrow mut output: builder, text: str) { output.write(text) }
pub fn forward<T>(borrow mut output: builder, value: T) { output.write("generic"); append(output, "!") }
pub fn reset(borrow mut output: builder) { output = builder(); output.write("reset:") }
pub fn finish(output: builder) -> string = output.to_string()
"#,
        ),
        (
            "main.align",
            r#"import builders
fn main() -> i32 {
  mut output: builder := builder()
  builders.append(output, "discarded")
  builders.reset(output)
  builders.append(output, "module:")
  builders.forward(output, 42)
  print(builders.finish(output))
  mut created: builder := builders.identity(builders.create())
  builders.append(created, "created")
  print(builders.finish(created))
  return 0
}
"#,
        ),
    ];
    let checked = diff_check_multi("text-builder-import", files, "main.align");
    assert!(
        !checked.whole_errors && !checked.per_unit_errors,
        "whole: {}\nper-unit: {}",
        checked.whole_diags,
        checked.per_unit_diags
    );
    if backend_available() {
        let whole = build_and_run_multi("text-builder-import-whole", files, "main.align");
        let per_unit =
            build_per_unit_multi("text-builder-import-units", files, "main.align").link_and_run();
        for output in [whole, per_unit] {
            assert!(output.status.success());
            assert_eq!(
                String::from_utf8_lossy(&output.stdout),
                "reset:module:generic!\ncreated\n"
            );
        }
    }
}
