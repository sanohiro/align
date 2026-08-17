//! Request 9: direct records whose text leaves are free-standing owners.

mod common;
use common::*;
use std::fmt::Write as _;
use std::process::{Command, Stdio};

#[test]
fn owned_json_decode_encode_and_bounded_round_trip_without_an_arena() {
    if !backend_available() {
        return;
    }
    let source = r#"
import core.json
OwnedTask {
  id: string,
  priority: i64,
  attempts: u16,
  limit: u64,
  enabled: bool,
  argv: array<string>,
  note: Option<string>,
}
fn main() -> Result<(), Error> {
  value: OwnedTask := json.decode("{\"id\":\"task-1\",\"priority\":-7,\"attempts\":3,\"limit\":18446744073709551615,\"enabled\":true,\"argv\":[\"\",\"quote:\\\" slash:\\/ backslash:\\\\ controls:\\b\\f\\n\\r\\t\",\"nul:\\u0000\",\"emoji:\\ud83d\\ude00\"],\"note\":\"\\u20ac\"}")?
  plain := json.encode(value)
  bounded := json.encode_bounded(value, plain.len())?
  rejected := json.encode_bounded(value, plain.len() - 1)
  negative := json.encode_bounded(value, -1)
  print(plain)
  print(bounded)
  print(match rejected { Ok(_) => 0, Err(_) => 1 })
  print(match negative { Ok(_) => 0, Err(_) => 1 })
  print(value.id.len())
  print(value.argv.len())
  return Ok(())
}
"#;
    let out = build_and_run("owned-json-round-trip", source);
    assert_eq!(
        out.status.code(),
        Some(0),
        "stderr: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    let canonical = "{\"id\":\"task-1\",\"priority\":-7,\"attempts\":3,\"limit\":18446744073709551615,\"enabled\":true,\"argv\":[\"\",\"quote:\\\" slash:/ backslash:\\\\ controls:\\b\\f\\n\\r\\t\",\"nul:\\u0000\",\"emoji:😀\"],\"note\":\"€\"}";
    assert_eq!(
        String::from_utf8_lossy(&out.stdout),
        format!("{canonical}\n{canonical}\n1\n1\n6\n4\n")
    );
}

#[test]
fn owned_json_optional_null_and_missing_are_canonical_none() {
    if !backend_available() {
        return;
    }
    let source = r#"
import core.json
Value { text: string, note: Option<string> }
fn main() -> Result<(), Error> {
  missing: Value := json.decode("{\"text\":\"a\"}")?
  null_value: Value := json.decode("{\"text\":\"b\",\"note\":null}")?
  empty: Value := json.decode("{\"text\":\"c\",\"note\":\"\"}")?
  print(json.encode(missing))
  print(json.encode(null_value))
  print(json.encode(empty))
  return Ok(())
}
"#;
    let out = build_and_run("owned-json-option-states", source);
    assert_eq!(out.status.code(), Some(0));
    assert_eq!(
        String::from_utf8_lossy(&out.stdout),
        "{\"text\":\"a\"}\n{\"text\":\"b\"}\n{\"text\":\"c\",\"note\":\"\"}\n"
    );
}

#[test]
fn owned_json_integer_widths_generic_substitution_and_unsigned_templates() {
    if !backend_available() {
        return;
    }
    let source = r#"
import core.json
Numbers {
  text: string,
  i8v: i8, u8v: u8, i16v: i16, u16v: u16,
  i32v: i32, u32v: u32, i64v: i64, u64v: u64,
}
Generic<T> { value: T, note: Option<string> }
fn main() -> Result<(), Error> {
  numbers: Numbers := json.decode("{\"text\":\"n\",\"i8v\":-128,\"u8v\":255,\"i16v\":-32768,\"u16v\":65535,\"i32v\":-2147483648,\"u32v\":4294967295,\"i64v\":-9223372036854775808,\"u64v\":18446744073709551615}")?
  print(json.encode(numbers))
  print(template "{numbers.u64v}")
  generic: Generic<string> := json.decode("{\"value\":\"owned\",\"note\":\"ok\"}")?
  print(json.encode(generic))
  return Ok(())
}
"#;
    let out = build_and_run("owned-json-integer-widths", source);
    assert_eq!(
        out.status.code(),
        Some(0),
        "stderr: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert_eq!(
        String::from_utf8_lossy(&out.stdout),
        concat!(
            "{\"text\":\"n\",\"i8v\":-128,\"u8v\":255,\"i16v\":-32768,\"u16v\":65535,",
            "\"i32v\":-2147483648,\"u32v\":4294967295,\"i64v\":-9223372036854775808,",
            "\"u64v\":18446744073709551615}\n",
            "18446744073709551615\n",
            "{\"value\":\"owned\",\"note\":\"ok\"}\n",
        )
    );
}

#[test]
fn owned_decode_is_free_standing_across_input_drop_and_arena_exit() {
    if !backend_available() {
        return;
    }
    let source = r#"
import core.json
Value { text: string, tags: array<string> }
fn from_owned_input() -> Result<Value, Error> {
  input := "{\"text\":\"detached\",\"tags\":[\"a\",\"b\"]}".clone()
  return Ok(json.decode(input)?)
}
fn from_arena() -> Result<Value, Error> {
  arena {
    value: Value := json.decode("{\"text\":\"outside\",\"tags\":[\"x\"]}")?
    return Ok(value)
  }
}
fn main() -> Result<(), Error> {
  first := from_owned_input()?
  second := from_arena()?
  print(first.text)
  print(first.tags.len())
  print(second.text)
  return Ok(())
}
"#;
    let out = build_and_run("owned-json-free-standing", source);
    assert_eq!(
        out.status.code(),
        Some(0),
        "stderr: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert_eq!(
        String::from_utf8_lossy(&out.stdout),
        "detached\n2\noutside\n"
    );
}

#[test]
fn owned_json_result_transfer_control_flow_replacement_and_drop_matrix() {
    if !backend_available() {
        return;
    }
    let source = r#"
import core.json
Value { text: string, tags: array<string>, note: Option<string> }
fn decoded(text: str) -> Result<Value, Error> = json.decode(text)
fn pass(value: Value) -> Value = value
fn keep_error(value: Error) -> Error = value
fn raw(text: str) -> Result<Value, Error> {
  result: Result<Value, Error> := decoded(text)
  return result
}
fn via_try(text: str) -> Result<Value, Error> {
  value := decoded(text)?
  return Ok(value)
}
fn via_map(text: str) -> Result<Value, Error> {
  value := decoded(text).map_err(keep_error)?
  return Ok(value)
}
fn via_if(flag: bool) -> Result<Value, Error> = if flag {
  decoded("{\"text\":\"if-a\",\"tags\":[]}")
} else {
  decoded("{\"text\":\"if-b\",\"tags\":[]}")
}
fn via_loop() -> Result<Value, Error> {
  value := loop {
    break decoded("{\"text\":\"loop\",\"tags\":[]}")?
  }
  return Ok(value)
}
fn via_else(text: str) -> i64 {
  value := decoded(text) else { return 9 }
  return value.text.len()
}
fn via_match(text: str) -> i64 = match decoded(text) {
  Ok(value) => value.text.len()
  Err(_) => 8
}
fn replacement() -> Result<i64, Error> {
  mut value := decoded("{\"text\":\"old\",\"tags\":[\"a\",\"b\"],\"note\":\"drop\"}")?
  value = decoded("{\"text\":\"newer\",\"tags\":[]}")?
  return Ok(value.text.len())
}
fn early_error() -> Result<i64, Error> {
  live := decoded("{\"text\":\"live\",\"tags\":[\"x\"]}")?
  failed := decoded("{\"text\":7,\"tags\":[]}")?
  return Ok(live.text.len() + failed.text.len())
}
fn main() -> Result<(), Error> {
  a := pass(raw("{\"text\":\"raw\",\"tags\":[]}")?)
  b := via_try("{\"text\":\"try\",\"tags\":[]}")?
  c := via_map("{\"text\":\"map\",\"tags\":[]}")?
  d := via_if(false)?
  e := via_loop()?
  print(a.text.len() + b.text.len() + c.text.len() + d.text.len() + e.text.len())
  print(via_else("{\"text\":\"else\",\"tags\":[]}"))
  print(via_else("{\"text\":false,\"tags\":[]}"))
  print(via_match("{\"text\":\"match\",\"tags\":[]}"))
  print(via_match("{\"text\":null,\"tags\":[]}"))
  print(replacement()?)
  print(match early_error() { Ok(value) => value, Err(_) => 7 })
  return Ok(())
}
"#;
    let out = build_and_run("owned-json-control-flow", source);
    assert_eq!(
        out.status.code(),
        Some(0),
        "stderr: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert_eq!(
        String::from_utf8_lossy(&out.stdout),
        "17\n4\n9\n5\n8\n5\n7\n"
    );
}

#[test]
fn owned_json_rejects_mixed_and_nested_graphs_before_lowering() {
    for source in [
        "import core.json\nMixed { owned: string, borrowed: str }\nfn main() -> Result<(), Error> { value: Mixed := json.decode(\"{}\")?; return Ok(()) }\n",
        "import core.json\nInner { text: string }\nOuter { inner: Inner }\nfn main() -> Result<(), Error> { value: Outer := json.decode(\"{}\")?; return Ok(()) }\n",
        "import core.json\nBad { values: Option<array<string>> }\nfn main() -> Result<(), Error> { value: Bad := json.decode(\"{}\")?; return Ok(()) }\n",
    ] {
        let mut source_map = SourceMap::new();
        let checked = check(&mut source_map, "owned-json-reject", source);
        assert!(checked.diags.has_errors());
    }
}

#[test]
fn owned_json_formation_routing_and_multi_invalid_precedence_are_deterministic() {
    let diagnostics = |name: &str, source: &str| {
        let mut source_map = SourceMap::new();
        let checked = check(&mut source_map, name, source);
        assert!(checked.diags.has_errors(), "{name}: fixture must reject");
        align_driver::format_diagnostics(&source_map, &checked.diags)
    };
    let cases = [
        (
            "layout-before-fields",
            "import core.json\nlayout(C) Bad { first: str, owned: string }\nfn main() -> Result<(), Error> { value: Bad := json.decode(\"{}\")?; return Ok(()) }\n",
            "cannot use `layout(C)`",
            ["mixed borrowed", "unsupported type"],
        ),
        (
            "align-before-fields",
            "import core.json\nalign(16) Bad { first: str, owned: string }\nfn main() -> Result<(), Error> { value: Bad := json.decode(\"{}\")?; return Ok(()) }\n",
            "cannot use explicit `align(N)`",
            ["mixed borrowed", "unsupported type"],
        ),
        (
            "first-unsupported-field",
            "import core.json\nBad { first: f64, second: str, owned: string }\nfn main() -> Result<(), Error> { value: Bad := json.decode(\"{}\")?; return Ok(()) }\n",
            "field 'first' has unsupported type f64",
            ["field 'second'", "mixed borrowed"],
        ),
        (
            "first-mixed-field",
            "import core.json\nBad { first: str, second: f64, owned: string }\nfn main() -> Result<(), Error> { value: Bad := json.decode(\"{}\")?; return Ok(()) }\n",
            "field 'first' has mixed borrowed type str",
            ["field 'second'", "unsupported type f64"],
        ),
        (
            "graph-before-bounded-limit",
            "import core.json\nBad { first: f64, owned: string }\nfn main() -> i32 { value := Bad { first: 1.0, owned: \"x\".clone() }; json.encode_bounded(value, true); return 0 }\n",
            "field 'first' has unsupported type f64",
            ["max_bytes", "must be i64"],
        ),
        (
            "generic-substitution",
            "import core.json\nGeneric<T> { value: T, note: Option<string> }\nfn main() -> Result<(), Error> { value: Generic<str> := json.decode(\"{}\")?; return Ok(()) }\n",
            "field 'value' has mixed borrowed type str",
            ["field 'note'", "unsupported type"],
        ),
    ];
    for (name, source, expected, absent) in cases {
        let rendered = diagnostics(name, source);
        assert!(rendered.contains(expected), "{name}: {rendered}");
        for forbidden in absent {
            assert!(!rendered.contains(forbidden), "{name}: {rendered}");
        }
    }

    let mut source_map = SourceMap::new();
    let existing = check(
        &mut source_map,
        "owned-json-non-selected",
        "import core.json\nExisting { text: str, score: f64 }\nfn main() -> i32 { value := Existing { text: \"x\", score: 1.5 }; output := json.encode(value); return output.len() as i32 }\n",
    );
    assert!(
        !existing.diags.has_errors(),
        "a record without an owned leaf must retain the existing route:\n{}",
        align_driver::format_diagnostics(&source_map, &existing.diags),
    );
}

fn owned_json_concurrency_source() -> String {
    let mut source = String::from(
        r#"
import core.json
Owned { text: string, tags: array<string> }
Borrowed { text: str, n: i64 }
Fixed { n: i64 }
Choice { Text(str), Count(i64), Flag(bool) }
Row { n: i64 }

fn od() -> Result<i64, Error> {
  value: Owned := json.decode("{\"text\":\"x\",\"tags\":[\"y\"]}")?
  return Ok(value.text.len())
}
fn oe() -> Result<i64, Error> {
  value: Owned := json.decode("{\"text\":\"x\",\"tags\":[]}")?
  output := json.encode(value)
  return Ok(if output.len() > 0 { 1 } else { 0 })
}
fn oeb() -> Result<i64, Error> {
  value: Owned := json.decode("{\"text\":\"x\",\"tags\":[]}")?
  output := json.encode_bounded(value, 64)?
  return Ok(if output.len() > 0 { 1 } else { 0 })
}
fn bd() -> Result<i64, Error> {
  value: Borrowed := json.decode("{\"text\":\"x\",\"n\":1}")?
  return Ok(value.n)
}
fn sd() -> Result<i64, Error> {
  value: i64 := json.decode("1")?
  return Ok(value)
}
fn ad() -> Result<i64, Error> {
  value: array<bool> := json.decode("[true]")?
  return Ok(value.len())
}
fn be() -> Result<i64, Error> {
  value := Borrowed { text: "x", n: 1 }
  output := json.encode(value)
  return Ok(if output.len() > 0 { 1 } else { 0 })
}
fn beb() -> Result<i64, Error> {
  value := Borrowed { text: "x", n: 1 }
  output := json.encode_bounded(value, 64)?
  return Ok(if output.len() > 0 { 1 } else { 0 })
}
fn fe() -> Result<i64, Error> {
  value := [Fixed { n: 1 }]
  output := json.encode(value)
  return Ok(if output.len() > 0 { 1 } else { 0 })
}
fn feb() -> Result<i64, Error> {
  value := [Fixed { n: 1 }]
  output := json.encode_bounded(value, 64)?
  return Ok(if output.len() > 0 { 1 } else { 0 })
}
fn ud() -> Result<i64, Error> {
  value: Choice := json.decode("1")?
  return Ok(match value { Text(_) => 0, Count(n) => n, Flag(_) => 0 })
}
fn ue() -> Result<i64, Error> {
  value := Choice.Count(1)
  output := json.encode(value)
  return Ok(if output.len() > 0 { 1 } else { 0 })
}
fn ueb() -> Result<i64, Error> {
  value := Choice.Count(1)
  output := json.encode_bounded(value, 64)?
  return Ok(if output.len() > 0 { 1 } else { 0 })
}
fn doc() -> Result<i64, Error> {
  arena {
    value := json.doc("{\"n\":1}")?
    return Ok(value.get("n").as_i64() else 0)
  }
}
fn scan() -> Result<i64, Error> {
  rows: json.scanner<Row> := json.scan("[{\"n\":1}]")
  return Ok(rows.n.sum()?)
}
fn aos() -> Result<i64, Error> {
  rows: array<Row> := json.decode("[{\"n\":1}]")?
  return Ok(rows.len())
}
fn soa() -> Result<i64, Error> {
  arena {
    rows: soa<Row> := json.decode("[{\"n\":1}]")?
    return Ok(rows.n.sum())
  }
}

fn main() -> Result<(), Error> {
  mut total: i64 := 0
"#,
    );
    let operations = [
        "od", "oe", "oeb", "bd", "sd", "ad", "be", "beb", "fe", "feb", "ud", "ue", "ueb", "doc",
        "scan", "aos", "soa",
    ];
    let mut pairs = 0;
    for (left_index, left) in operations.iter().enumerate() {
        for right in &operations[left_index..] {
            pairs += 1;
            writeln!(
                source,
                "  task_group {{\n    left := spawn(fn {{ {left}() }})\n    right := spawn(fn {{ {right}() }})\n    wait()?\n    total = total + left.get() + right.get()\n  }}"
            )
            .unwrap();
        }
    }
    assert_eq!(pairs, 153);
    source.push_str("  print(total)\n  return Ok(())\n}\n");
    source
}

#[test]
fn all_json_operation_variants_overlap_in_the_full_153_pair_matrix() {
    if !backend_available() {
        return;
    }
    let out = build_and_run(
        "owned-json-concurrency-matrix",
        &owned_json_concurrency_source(),
    );
    assert_eq!(
        out.status.code(),
        Some(0),
        "stderr: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert_eq!(String::from_utf8_lossy(&out.stdout), "306\n");
}

#[test]
fn owned_json_calls_are_independent_in_two_concurrent_processes() {
    if !backend_available() {
        return;
    }
    let source = r#"
import core.json
Owned { text: string, tags: array<string> }
fn main() -> Result<(), Error> {
  value: Owned := json.decode("{\"text\":\"process\",\"tags\":[\"a\",\"b\"]}")?
  print(json.encode(value))
  return Ok(())
}
"#;
    let built = build_exe("owned-json-two-processes", source);
    let spawn = || {
        Command::new(&built.exe)
            .stdout(Stdio::piped())
            .spawn()
            .expect("spawn owned JSON process")
    };
    let first = spawn();
    let second = spawn();
    for child in [first, second] {
        let output = child.wait_with_output().expect("wait owned JSON process");
        assert_eq!(output.status.code(), Some(0));
        assert_eq!(
            String::from_utf8_lossy(&output.stdout),
            "{\"text\":\"process\",\"tags\":[\"a\",\"b\"]}\n"
        );
    }
}
