//! Request 12: bounded canonical JSON encoding.

mod common;
use common::*;

#[test]
fn json_encode_bounded_exact_fit_matches_json_encode() {
    if !backend_available() {
        return;
    }
    let source = r#"
import core.json
Inner { score: i64, note: Option<str> }
Row { id: i64, text: str, active: bool, inner: Inner }
fn main() -> Result<(), Error> {
  row := Row { id: 7, text: "a\"b\n日本語", active: true, inner: Inner { score: -9, note: None } }
  ordinary := json.encode(row)
  bounded := json.encode_bounded(row, ordinary.len())?
  print(ordinary)
  print(bounded)
  return Ok(())
}
"#;
    let out = build_and_run("json-encode-bounded-exact", source);
    assert_eq!(out.status.code(), Some(0));
    let expected = "{\"id\":7,\"text\":\"a\\\"b\\n日本語\",\"active\":true,\"inner\":{\"score\":-9}}\n";
    assert_eq!(String::from_utf8_lossy(&out.stdout), format!("{expected}{expected}"));
}

#[test]
fn json_encode_bounded_matches_the_current_schema_matrix() {
    if !backend_available() {
        return;
    }
    let source = r#"
import core.json
Point { x: i64, label: str }
Part { kind: str, text: str }
Content { Text(str), Parts(array<Part>) }
Message { role: str, content: Content }
Chat { messages: array<Message> }
Metrics { name: str, ints: array<i64>, floats: array<f64>, flags: array<bool> }
fn main() -> Result<(), Error> {
  scalar_union := Content.Text("hello\n日本語")
  scalar_union_plain := json.encode(scalar_union)
  scalar_union_bounded := json.encode_bounded(scalar_union, scalar_union_plain.len())?
  scalar_union_spare := json.encode_bounded(scalar_union, scalar_union_plain.len() + 1)?
  scalar_union_short := json.encode_bounded(scalar_union, scalar_union_plain.len() - 1)
  print(scalar_union_plain)
  print(scalar_union_bounded)
  print(scalar_union_spare)
  print(match scalar_union_short { Ok(_) => 0, Err(_) => 1 })

  fixed := [Point { x: 1, label: "a" }, Point { x: -2, label: "b\"" }]
  fixed_plain := json.encode(fixed)
  fixed_bounded := json.encode_bounded(fixed, fixed_plain.len())?
  fixed_spare := json.encode_bounded(fixed, fixed_plain.len() + 1)?
  fixed_short := json.encode_bounded(fixed, fixed_plain.len() - 1)
  print(fixed_plain)
  print(fixed_bounded)
  print(fixed_spare)
  print(match fixed_short { Ok(_) => 0, Err(_) => 1 })

  arena {
    chat: Chat := json.decode("{\"messages\":[{\"role\":\"u\",\"content\":[{\"kind\":\"text\",\"text\":\"hello\"}]},{\"role\":\"a\",\"content\":\"done\"}]}")?
    chat_plain := json.encode(chat)
    chat_bounded := json.encode_bounded(chat, chat_plain.len())?
    chat_spare := json.encode_bounded(chat, chat_plain.len() + 1)?
    chat_short := json.encode_bounded(chat, chat_plain.len() - 1)
    print(chat_plain)
    print(chat_bounded)
    print(chat_spare)
    print(match chat_short { Ok(_) => 0, Err(_) => 1 })

    metrics: Metrics := json.decode("{\"name\":\"m\",\"ints\":[-1,0,2],\"floats\":[1.5,2.0],\"flags\":[true,false]}")?
    metrics_plain := json.encode(metrics)
    metrics_bounded := json.encode_bounded(metrics, metrics_plain.len())?
    metrics_spare := json.encode_bounded(metrics, metrics_plain.len() + 1)?
    metrics_short := json.encode_bounded(metrics, metrics_plain.len() - 1)
    print(metrics_plain)
    print(metrics_bounded)
    print(metrics_spare)
    print(match metrics_short { Ok(_) => 0, Err(_) => 1 })
  }
  return Ok(())
}
"#;
    let out = build_and_run("json-encode-bounded-schema-matrix", source);
    assert_eq!(out.status.code(), Some(0));
    let lines = String::from_utf8_lossy(&out.stdout)
        .lines()
        .map(str::to_owned)
        .collect::<Vec<_>>();
    assert_eq!(lines.len(), 16, "unexpected output: {}", String::from_utf8_lossy(&out.stdout));
    for group in lines.chunks_exact(4) {
        assert_eq!(group[0], group[1]);
        assert_eq!(group[0], group[2]);
        assert_eq!(group[3], "1");
    }
}

#[test]
fn json_encode_bounded_owned_result_composes_with_existing_result_control_flow() {
    if !backend_available() {
        return;
    }
    let source = r#"
import core.json
Row { id: i64, text: str }
fn keep_error(e: Error) -> Error = e
fn returned(row: Row, cap: i64) -> Result<string, Error> = json.encode_bounded(row, cap)
fn main() -> Result<(), Error> {
  row := Row { id: 7, text: "owned" }
  plain := json.encode(row)

  via_return := returned(row, plain.len())?
  print(via_return)

  matched := json.encode_bounded(row, plain.len())
  matched_len := match matched { Ok(value) => value.len(), Err(_) => -1 }
  print(matched_len)

  via_else := json.encode_bounded(row, plain.len()) else { return Err(Error.Invalid) }
  print(via_else)

  mut replaced: Result<string, Error> := json.encode_bounded(row, plain.len())
  replaced = json.encode_bounded(row, plain.len())
  via_map := replaced.map_err(keep_error)?
  print(via_map)

  unused_ok := json.encode_bounded(row, plain.len())
  unused_err := json.encode_bounded(row, 0)
  print(row.id)
  return Ok(())
}
"#;
    let out = build_and_run("json-encode-bounded-result-flow", source);
    assert_eq!(out.status.code(), Some(0), "stderr: {}", String::from_utf8_lossy(&out.stderr));
    let value = "{\"id\":7,\"text\":\"owned\"}";
    assert_eq!(
        String::from_utf8_lossy(&out.stdout),
        format!("{value}\n{}\n{value}\n{value}\n7\n", value.len())
    );
}

#[test]
fn json_encode_bounded_limit_failures_are_invalid() {
    if !backend_available() {
        return;
    }
    for (name, limit) in [("negative", -1), ("zero", 0), ("one", 1), ("exact-minus-one", 1)] {
        let source = format!(
            "import core.json\nEmpty {{ omitted: Option<i64> }}\nfn main() -> Result<(), Error> {{\n  value := Empty {{ omitted: None }}\n  json.encode_bounded(value, {limit})?\n  return Ok(())\n}}\n"
        );
        let out = build_and_run(&format!("json-encode-bounded-{name}"), &source);
        assert_eq!(
            out.status.code(),
            Some(2),
            "limit {limit} must propagate Error.Invalid; stderr: {}",
            String::from_utf8_lossy(&out.stderr)
        );
    }

    let exact = r#"
import core.json
Empty { omitted: Option<i64> }
fn main() -> Result<(), Error> {
  value := Empty { omitted: None }
  text := json.encode_bounded(value, 2)?
  print(text)
  return Ok(())
}
"#;
    let out = build_and_run("json-encode-bounded-empty-exact", exact);
    assert_eq!(out.status.code(), Some(0));
    assert_eq!(String::from_utf8_lossy(&out.stdout), "{}\n");
}

#[test]
fn json_encode_bounded_surface_and_schema_fail_closed() {
    let missing_import = check_diagnostics(
        "json-encode-bounded-import",
        "Row { value: i64 }\nfn main() -> i32 {\n  row := Row { value: 1 }\n  json.encode_bounded(row, 8)\n  return 0\n}\n",
    );
    assert!(missing_import.contains("import core.json"), "{missing_import}");

    let arity = check_diagnostics(
        "json-encode-bounded-arity",
        "import core.json\nRow { value: i64 }\nfn main() -> i32 {\n  row := Row { value: 1 }\n  json.encode_bounded(row)\n  return 0\n}\n",
    );
    assert!(arity.contains("expects 2 arguments"), "{arity}");

    let limit = check_diagnostics(
        "json-encode-bounded-limit-type",
        "import core.json\nRow { value: i64 }\nfn main() -> i32 {\n  row := Row { value: 1 }\n  cap: i32 := 8\n  json.encode_bounded(row, cap)\n  return 0\n}\n",
    );
    assert!(limit.contains("i64"), "{limit}");

    let ordinary = check_diagnostics(
        "json-encode-schema-control",
        "import core.json\nBad { value: char }\nfn main() -> i32 {\n  value := Bad { value: 'x' }\n  json.encode(value)\n  return 0\n}\n",
    );
    let bounded = check_diagnostics(
        "json-encode-bounded-schema",
        "import core.json\nBad { value: char }\nfn main() -> i32 {\n  value := Bad { value: 'x' }\n  json.encode_bounded(value, 32)\n  return 0\n}\n",
    );
    assert!(ordinary.contains("unsupported type char"), "{ordinary}");
    assert!(bounded.contains("unsupported type char"), "{bounded}");
    assert!(bounded.contains("json.encode_bounded"), "{bounded}");

    for (name, field_ty) in [("owned-string", "string"), ("owned-string-array", "array<string>")] {
        let ordinary_source = format!(
            "import core.json\nUnsupported {{ value: {field_ty} }}\nfn reject(value: Unsupported) -> i32 {{\n  json.encode(value)\n  return 0\n}}\nfn main() -> i32 = 0\n"
        );
        let bounded_source = format!(
            "import core.json\nUnsupported {{ value: {field_ty} }}\nfn reject(value: Unsupported) -> i32 {{\n  json.encode_bounded(value, 64)\n  return 0\n}}\nfn main() -> i32 = 0\n"
        );
        let ordinary = check_diagnostics(
            &format!("json-encode-{name}-control"),
            &ordinary_source,
        );
        let bounded = check_diagnostics(
            &format!("json-encode-bounded-{name}"),
            &bounded_source,
        );
        assert!(ordinary.contains("unsupported type"), "{ordinary}");
        assert!(bounded.contains("unsupported type"), "{bounded}");
        assert!(bounded.contains("json.encode_bounded"), "{bounded}");
    }
}

#[test]
fn json_encode_bounded_mir_and_llvm_use_the_bounded_stack_abi() {
    let source = r#"
import core.json
Row { value: i64 }
fn encode(row: Row, cap: i64) -> Result<string, Error> = json.encode_bounded(row, cap)
fn main() -> i32 = 0
"#;
    let mut source_map = SourceMap::new();
    let checked = check(&mut source_map, "json-encode-bounded-shape", source);
    assert!(
        !checked.diags.has_errors(),
        "{}",
        align_driver::format_diagnostics(&source_map, &checked.diags)
    );
    let mir = align_mir::print::program_to_string(&lower_to_mir(&checked.hir));
    assert!(mir.contains("json_encode_bounded"), "{mir}");

    if !backend_available() {
        return;
    }
    let llvm = emit_llvm(source);
    assert!(llvm.contains("call ptr @align_rt_builder_init_bounded_stack"), "{llvm}");
    assert!(llvm.contains("call i32 @align_rt_builder_finish_bounded_stack"), "{llvm}");
    assert!(!llvm.contains("call { ptr, i64 } @align_rt_builder_into_string_stack"), "{llvm}");
}

#[test]
fn json_encode_bounded_matches_whole_and_per_unit_compilation() {
    let library = r#"
module bounded
import core.json
pub Row { id: i64, text: str }
pub fn make() -> Row = Row { id: 9, text: "unit" }
pub fn encode(row: Row, cap: i64) -> Result<string, Error> = json.encode_bounded(row, cap)
"#;
    let main = r#"
module main
import bounded
fn main() -> Result<(), Error> {
  row := bounded.make()
  text := bounded.encode(row, 64)?
  print(text)
  return Ok(())
}
"#;
    let files = [("bounded.align", library), ("main.align", main)];
    let checked = assert_same_verdict(
        "json-encode-bounded-per-unit-check",
        &files,
        "main.align",
    );
    assert!(!checked.diags.has_errors(), "unexpected per-unit diagnostics");
    if !backend_available() {
        return;
    }
    let out = build_per_unit_multi(
        "json-encode-bounded-per-unit-run",
        &files,
        "main.align",
    )
    .link_and_run();
    assert_eq!(out.status.code(), Some(0), "stderr: {}", String::from_utf8_lossy(&out.stderr));
    assert_eq!(String::from_utf8_lossy(&out.stdout), "{\"id\":9,\"text\":\"unit\"}\n");
}
