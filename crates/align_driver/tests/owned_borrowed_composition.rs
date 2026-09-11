//! Regression coverage for the R77/R79/R80/R81/R82/R83 composition boundary.
//!
//! These cases exercise the same checked-HIR, whole-program, per-unit, and native paths that
//! the external align-llm request batch consumes. R78 remains deferred under the friction ledger.

mod common;
use common::*;

fn assert_clean(name: &str, source: &str) {
    let checked = diff_check_multi(name, &[("main.align", source)], "main.align");
    assert!(
        !checked.whole_errors && !checked.per_unit_errors,
        "{name}\nwhole: {}\nper-unit: {}",
        checked.whole_diags,
        checked.per_unit_diags
    );
}

fn assert_rejected(name: &str, source: &str) {
    let checked = diff_check_multi(name, &[("main.align", source)], "main.align");
    assert!(
        checked.whole_errors && checked.per_unit_errors,
        "{name} unexpectedly passed\nwhole: {}\nper-unit: {}",
        checked.whole_diags,
        checked.per_unit_diags
    );
}

#[test]
fn fixed_record_field_borrow_keeps_the_physical_string_owner() {
    let source = r#"module borrowed_fixed_record_projection
Row { text: string, score: i64 }
fn size(borrow value: str) -> i64 = value.len()
fn main() -> i32 {
  rows := [Row { text: "fixed".clone(), score: 7 }]
  value := size(rows[0].text)
  if value == 5 && rows[0].score == 7 { return 42 }
  return 0
}

"#;
    assert_clean("r77-fixed-record-field", source);
    if backend_available() {
        assert_eq!(
            build_and_run("r77-fixed-record-field", source)
                .status
                .code(),
            Some(42)
        );
        let files = &[("main.align", source)];
        assert_eq!(
            build_per_unit_multi("r77-fixed-record-field-unit", files, "main.align")
                .link_and_run()
                .status
                .code(),
            Some(42)
        );
    }
}

#[test]
fn fixed_move_record_field_borrow_requires_an_integer_literal_index() {
    let source = r#"module fixed_move_record_projection
Policy { label: string }
Task { generation: Policy }
fn inspect(borrow policy: Policy) -> i64 = policy.label.len()
fn main() -> i32 {
  tasks := [Task { generation: Policy { label: "fixed".clone() } }]
  index := 0
  return inspect(tasks[index].generation) as i32
}
"#;
    assert_rejected("r77-fixed-runtime-index", source);
}

#[test]
fn fixed_move_record_field_borrow_accepts_an_integer_literal_index() {
    let source = r#"module fixed_move_record_literal_projection
Policy { label: string }
Task { generation: Policy }
fn inspect(borrow policy: Policy) -> i64 = policy.label.len()
fn main() -> i32 {
  tasks := [Task { generation: Policy { label: "fixed".clone() } }]
  return inspect(tasks[0].generation) as i32
}
"#;
    assert_clean("r77-fixed-literal-index", source);
    if backend_available() {
        assert_eq!(
            build_and_run("r77-fixed-literal-index", source)
                .status
                .code(),
            Some(5)
        );
    }
}

#[test]
fn dynamic_record_field_borrow_keeps_the_physical_string_owner() {
    let source = r#"module borrowed_dynamic_record_projection
Record { body: string }
fn redact(borrow text: str) -> string = text.clone()
fn build(blocks: slice<Record>) -> string = redact(blocks[0].body)
fn main() -> i32 {
  value := build([Record { body: "dynamic".clone() }])
  return value.len() as i32
}
"#;
    assert_clean("r77-dynamic-record-field", source);
    if backend_available() {
        assert_eq!(
            build_and_run("r77-dynamic-record-field", source)
                .status
                .code(),
            Some(7)
        );
        let files = &[("main.align", source)];
        assert_eq!(
            build_per_unit_multi("r77-dynamic-record-field-unit", files, "main.align")
                .link_and_run()
                .status
                .code(),
            Some(7)
        );
    }
}

#[test]
fn dynamic_move_record_field_borrow_is_read_only_in_place() {
    let source = r#"module borrowed_dynamic_move_record_projection
Policy { label: string }
Task { generation: Policy }
fn inspect(borrow policy: Policy) -> i64 = policy.label.len()
fn main() -> i32 {
  tasks := [Task { generation: Policy { label: "dynamic".clone() } }]
  return inspect(tasks[0].generation) as i32
}
"#;
    assert_clean("r83-dynamic-move-field-borrow", source);
    if backend_available() {
        assert_eq!(
            build_and_run("r83-dynamic-move-field-borrow", source)
                .status
                .code(),
            Some(7)
        );
        let files = &[("main.align", source)];
        assert_eq!(
            build_per_unit_multi("r83-dynamic-move-field-borrow-unit", files, "main.align")
                .link_and_run()
                .status
                .code(),
            Some(7)
        );
    }
}

#[test]
fn dynamic_move_record_field_borrow_from_stable_field_keeps_the_root() {
    let source = r#"module borrowed_dynamic_move_record_field_base
Policy { label: string }
Task { generation: Policy }
Container { tasks: slice<Task> }
fn inspect(borrow policy: Policy) -> i64 = policy.label.len()
fn call(borrow container: Container) -> i64 {
  index := 0
  return inspect(container.tasks[index].generation)
}
fn main() -> i32 {
  tasks := [Task { generation: Policy { label: "dynamic".clone() } }]
  container := Container { tasks: tasks }
  return call(container) as i32
}
"#;
    assert_clean("r83-dynamic-move-field-base", source);
    if backend_available() {
        assert_eq!(
            build_and_run("r83-dynamic-move-field-base", source)
                .status
                .code(),
            Some(7)
        );
    }
}

#[test]
fn mixed_owned_and_borrowed_record_leaves_keep_per_leaf_authority() {
    let source = r#"module mixed_owned_borrowed_record
Mixed { owned: string, view: str }
fn consume(value: Mixed) -> i64 = value.view.len()
fn main() -> i32 {
  source := "borrowed".clone()
  view: str := source
  value := Mixed { owned: "owned".clone(), view: view }
  result := consume(value)
  return result as i32
}
"#;
    assert_clean("r83-mixed-owned-borrowed-record", source);
    if backend_available() {
        assert_eq!(
            build_and_run("r83-mixed-owned-borrowed-record", source)
                .status
                .code(),
            Some(8)
        );
    }
}

#[test]
fn nested_copy_process_signal_payload_is_read_without_transfer() {
    let source = r#"module fixture_setup_nested_signal
import std.process
Stop { Cancelled(process.signal) }
Report { stop: Stop, data: string }
Attempt { report: Option<Report> }
fn inspect(borrow attempt: Attempt) -> i64 {
  return match attempt.report {
    None => 0,
    Some(report) => match report.stop {
      Cancelled(signal) => process.signal_number(signal),
    },
  }
}
fn main() -> i32 {
  value := Attempt { report: Some(Report { stop: Stop.Cancelled(process.signal.Terminate), data: "".clone() }) }
  return inspect(value) as i32
}
"#;
    assert_clean("r79-nested-process-signal", source);
    if backend_available() {
        let output = build_and_run("r79-nested-process-signal", source);
        assert_eq!(output.status.code(), Some(15));
    }
}

#[test]
fn scalar_to_array_replacement_does_not_retain_the_loop_iteration_owner() {
    let source = r#"module scalar_copy_loop
Report { stdout: array<u8> }
fn report() -> Report {
  mut values: array_builder<u8> := array_builder()
  values.push(65 as u8)
  return Report { stdout: values.build() }
}
fn collect() -> array<u8> {
  empty: array_builder<u8> := array_builder()
  mut output := empty.build()
  mut index := 0
  loop {
    if index == 2 { break }
    value := report()
    bytes: slice<u8> := value.stdout
    output = bytes.to_array()
    index = index + 1
  }
  return output
}
fn main() -> i32 {
  result := collect()
  return result.len() as i32
}
"#;
    assert_clean("r80-scalar-to-array-loop", source);
    if backend_available() {
        assert_eq!(
            build_and_run("r80-scalar-to-array-loop", source)
                .status
                .code(),
            Some(1)
        );
        let files = &[("main.align", source)];
        assert_eq!(
            build_per_unit_multi("r80-scalar-to-array-loop-unit", files, "main.align")
                .link_and_run()
                .status
                .code(),
            Some(1)
        );
    }
}

#[test]
fn borrowed_record_array_field_is_a_shared_view_in_both_build_modes() {
    let source = r#"module borrowed_array_return_test
Row { path: string }
Observation { tree: array<Row> }
fn changes(left: slice<Row>, right: slice<Row>) -> array<string> {
  mut result: array_builder<string> := array_builder()
  if left.len() != 0 { result.push(left[0].path.clone()) }
  if right.len() != 0 { result.push(right[0].path.clone()) }
  return result.build()
}
fn inspect(borrow value: Observation) -> array<string> = {
  mut rows: array_builder<Row> := array_builder()
  rows.push(Row { path: "b".clone() })
  right := rows.build()
  return changes(value.tree, right)
}
fn main() -> i32 {
  mut rows: array_builder<Row> := array_builder()
  rows.push(Row { path: "a".clone() })
  value := Observation { tree: rows.build() }
  direct := changes(value.tree, value.tree)
  actual := inspect(value)
  if direct.len() == 2 && direct[0] == "a" && direct[1] == "a" &&
  actual.len() == 2 && actual[0] == "a" && actual[1] == "b" &&
  value.tree[0].path == "a" { return 42 }
  return 0
}
"#;
    assert_clean("r81-borrowed-record-array", source);
    if backend_available() {
        assert_eq!(
            build_and_run("r81-borrowed-record-array", source)
                .status
                .code(),
            Some(42)
        );
        let files = &[("main.align", source)];
        assert_eq!(
            build_per_unit_multi("r81-borrowed-record-array-unit", files, "main.align")
                .link_and_run()
                .status
                .code(),
            Some(42)
        );
    }
}

#[test]
fn borrowed_process_namespace_payload_is_admitted_and_keeps_the_call_boundary() {
    let source = r#"module borrowed_namespace_command
import std.process
fn attach(borrow mut command: command, borrow namespace: Option<process.user_namespace>, slot: i64) -> Result<(), Error> {
  return match namespace {
    None => if slot == 0 { Ok(()) } else { Err(Error.Invalid) },
    Some(value) => command.inherit_namespace(value, slot),
  }
}
fn main() -> Result<(), Error> {
  mut command := process.command("/bin/echo", ["echo", "namespace absent"])
  absent: Option<process.user_namespace> := None
  attach(command, absent, 0)?
  return Ok(())
}
"#;
    assert_clean("r82-borrowed-process-namespace", source);
}

#[test]
fn nested_result_option_owned_return_preserves_the_view_read_and_owner() {
    let source = r#"module owned_document_return_test
Document { text: string }
Template { content: string }
Manifest { path: Option<string> }
fn bound(value: str) -> Result<Document, Error> {
  return Ok(Document { text: value.clone() })
}
fn decode(source: str) -> Result<Template, Error> {
  return Ok(Template { content: source.clone() })
}
fn wrap(borrow manifest: Manifest) -> Result<Option<Template>, Error> {
  return match manifest.path {
    None => Ok(None),
    Some(path) => {
      source := bound(path)?
      value := decode(source.text)?
      return Ok(Some(value))
    },
  }
}
fn main() -> i32 {
  manifest := Manifest { path: Some("payload".clone()) }
  result := wrap(manifest)
  length := match result {
    Err(_) => 0,
    Ok(option) => match option {
      None => 0,
      Some(value) => value.content.len() as i32,
    },
  }
  if length == 7 { return 42 }
  return 0
}
"#;
    assert_clean("r83-nested-owned-return", source);
    if backend_available() {
        assert_eq!(
            build_and_run("r83-nested-owned-return", source)
                .status
                .code(),
            Some(42)
        );
        let files = &[("main.align", source)];
        assert_eq!(
            build_per_unit_multi("r83-nested-owned-return-unit", files, "main.align")
                .link_and_run()
                .status
                .code(),
            Some(42)
        );
    }
}

#[test]
fn faithful_owned_document_return_keeps_all_result_leaves_certified() {
    // This retains the shape of the R83 provider reproduction: an optional borrowed manifest,
    // a local owned Document returned through Result, a field borrow into a decoder, and an owned
    // seven-field Template returned through Result<Option<_>>. It is deliberately broader than a
    // one-field Document control so a regression in a later owned leaf remains visible.
    let source = r#"module faithful_owned_document_return
Document { text: string, sha256: string, size: i64 }
Template { a: string, b: string, c: string, d: string, e: string, f: string, g: string }
Manifest { path: Option<string>, hash: Option<string> }
fn read_document(path: str) -> Result<Document, Error> = Ok(Document { text: path.clone(), sha256: "digest".clone(), size: path.len() })
fn bound(path: str, hash: str) -> Result<Document, Error> {
  source := read_document(path)?
  if source.sha256 != hash { return Err(Error.Invalid) }
  return Ok(source)
}
fn decode(source: str) -> Result<Template, Error> = Ok(Template {
  a: source.clone(), b: source.clone(), c: source.clone(), d: source.clone(),
  e: source.clone(), f: source.clone(), g: source.clone(),
})
fn wrap(borrow manifest: Manifest) -> Result<Option<Template>, Error> {
  return match manifest.path {
    None => Ok(None),
    Some(path) => {
      hash := match manifest.hash {
        None => { return Err(Error.Invalid) },
        Some(value) => { hash: str := value; hash },
      }
      source := bound(path, hash)?
      value := decode(source.text)?
      return Ok(Some(value))
    },
  }
}
fn main() -> i32 {
  manifest := Manifest { path: Some("payload".clone()), hash: Some("digest".clone()) }
  result := wrap(manifest)
  return match result {
    Err(_) => 0,
    Ok(option) => match option {
      None => 0,
      Some(value) => value.g.len() as i32,
    },
  }
}
"#;
    assert_clean("r83-faithful-owned-document-return", source);
    if backend_available() {
        assert_eq!(
            build_and_run("r83-faithful-owned-document-return", source)
                .status
                .code(),
            Some(7)
        );
        let files = &[("main.align", source)];
        assert_eq!(
            build_per_unit_multi("r83-faithful-owned-document-return-unit", files, "main.align")
                .link_and_run()
                .status
                .code(),
            Some(7)
        );
    }
}
