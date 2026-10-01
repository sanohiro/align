//! Checked decimal grammar, text borrowing, Result control flow, and interface parity.
mod common;
use common::*;

#[test]
fn grammar_range_and_imported_generic_parity() {
    if !backend_available() {
        return;
    }
    let library = "module decimal\npub fn parse(text: str) -> Result<i64, Error> = text.parse_i64()\npub fn generic<T>(value: T, text: str) -> Result<i64, Error> = text.parse_i64()\n";
    let mut body = String::new();
    for (text, value) in [
        ("0", "0"),
        ("-0", "0"),
        ("+0", "0"),
        ("00042", "42"),
        ("+0012", "12"),
        ("-0012", "-12"),
        ("9223372036854775807", "9223372036854775807"),
        ("-9223372036854775808", "-9223372036854775808"),
        ("+9223372036854775807", "9223372036854775807"),
        ("0000000000000000000000000000001", "1"),
    ] {
        body.push_str(&format!("if (decimal.parse(\"{text}\") else {{ return 1 }}) != {value} {{ return 2 }}\nif (decimal.generic(true, \"{text}\".clone()) else {{ return 3 }}) != {value} {{ return 4 }}\n"));
    }
    for text in [
        "",
        "+",
        "-",
        " 1",
        "1 ",
        "\\t1",
        "1\\n",
        "1\\0",
        "١",
        "１２",
        "é",
        "--1",
        "++1",
        "+-1",
        "1-2",
        "0x10",
        "1_000",
        "1.0",
        "1e2",
        "9223372036854775808",
        "-9223372036854775809",
        "+9223372036854775808",
        "999999999999999999999999999999999",
    ] {
        for call in [
            format!("decimal.parse(\"{text}\")"),
            format!("decimal.generic(7, \"{text}\".clone())"),
        ] {
            body.push_str(&format!("match {call} {{ Ok(_) => {{ return 5 }}, Err(e) => match e {{ Invalid => {{}}, _ => {{ return 6 }} }} }}\n"));
        }
    }
    body.push_str(
        "if (\" +42 \".trim().parse_i64() else { return 7 }) != 42 { return 8 }\nreturn 0\n",
    );
    let main = format!("import decimal\nfn main() -> i32 {{\n{body}}}\n");
    let files = [("decimal.align", library), ("main.align", main.as_str())];
    for output in [
        build_and_run_multi("decimal-whole", &files, "main.align"),
        build_per_unit_multi("decimal-units", &files, "main.align").link_and_run(),
    ] {
        assert_eq!(
            output.status.code(),
            Some(0),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
    }
}

#[test]
fn receiver_and_control_matrix() {
    if !backend_available() {
        return;
    }
    let source = r#"
R { text: string }
fn read(borrow r: R) -> Result<i64, Error> = r.text.parse_i64()
fn propagate(text: str) -> Result<i64, Error> { n := text.parse_i64()?; return Ok(n + 1) }
fn produce() -> string { print(1); return "17".clone() }
fn early() -> i64 { x := ({ return 19; "x" }).parse_i64(); return 0 }
fn main() -> i32 {
  mut owned := "42".clone()
  scalar := owned.parse_i64()
  print(owned)
  owned = "invalid".clone()
  print(scalar else -1)
  print(owned.parse_i64() else 23)
  r := R { text: "5".clone() }
  print(read(r) else -1)
  print(r.text)
  print(produce().parse_i64() else -1)
  print(early())
  print(propagate("8") else -1)
  match propagate("bad") { Ok(_) => { return 1 }, Err(e) => match e { Invalid => {}, _ => { return 2 } } }
  changed: Result<i64, i64> := "bad".clone().parse_i64().map_err(fn e: Error { 31 })
  match changed { Ok(_) => { return 3 }, Err(e) => print(e) }
  mut result := "1".parse_i64()
  result = "2".parse_i64()
  print(result else -1)
  a := "3".clone()
  b := "4".clone()
  print((if true { a } else { b }).parse_i64() else -1)
  print(a)
  print(b)
  print((match 0 { 0 => "6".clone(), _ => "bad".clone() }).parse_i64() else -1)
  print((loop { break "7".clone() }).parse_i64() else -1)
  print((arena { "9".clone() }).parse_i64() else -1)
  print((task_group { "10".clone() }).parse_i64() else -1)
  return 0
}
"#;
    for per_unit in [false, true] {
        let output = if per_unit {
            build_per_unit_multi(
                "decimal-control-unit",
                &[("main.align", source)],
                "main.align",
            )
            .link_and_run()
        } else {
            build_and_run("decimal-control", source)
        };
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        assert_eq!(
            String::from_utf8_lossy(&output.stdout),
            "42\n42\n23\n5\n5\n1\n17\n19\n9\n31\n2\n3\n3\n4\n6\n7\n9\n10\n"
        );
    }
}

#[test]
fn pure_parallel_reader() {
    if !backend_available() {
        return;
    }
    let source = r#"
fn parse(text: str) -> i64 = text.parse_i64() else -1
fn main() -> i32 {
  texts := ["1", "bad", "-2", "+3"]
  values := texts.par_map(fn text { parse(text) })
  print(values[0]); print(values[1]); print(values[2]); print(values[3])
  return 0
}
"#;
    let output = build_and_run("decimal-parallel", source);
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(String::from_utf8_lossy(&output.stdout), "1\n-1\n-2\n3\n");
}

#[test]
fn diagnostics() {
    for source in [
        "fn f(text: str) -> Result<i64, Error> = text.parse_i64(10)",
        "fn f(text: str) -> Result<i64, Error> = text.parse_i64(false, 1)",
        "fn f(text: slice<u8>) -> Result<i64, Error> = text.parse_i64()",
        "fn f(text: i64) -> Result<i64, Error> = text.parse_i64()",
        "fn f(text: str) -> i64 = text.parse_i64()",
        "fn f(text: str) -> Result<i32, Error> = text.parse_i64()",
        "fn f(text: str) -> Result<i64, i64> = text.parse_i64()",
    ] {
        for per_unit in [false, true] {
            let mut sm = SourceMap::new();
            let diags = if per_unit {
                check_per_unit(&mut sm, "decimal-invalid.align", source).diags
            } else {
                check(&mut sm, "decimal-invalid.align", source).diags
            };
            assert!(diags.has_errors(), "accepted {source}");
        }
    }
}
