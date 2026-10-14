//! R84–R88 provider owners; consumer policy and managed-pin adoption remain external.
mod common;
#[path = "helpers/owned_fixture.rs"]
mod owned_fixture;
use common::*;

fn run_both(name: &str, helper: &str, main: &str, expected: &str) {
    let files = [("helper.align", helper), ("main.align", main)];
    let checked = diff_check_multi(name, &files, "main.align");
    assert!(
        !checked.whole_errors && !checked.per_unit_errors,
        "{}\n{}",
        checked.whole_diags,
        checked.per_unit_diags
    );
    if backend_available() {
        for output in [
            build_and_run_multi(name, &files, "main.align"),
            build_per_unit_multi(&format!("{name}-unit"), &files, "main.align").link_and_run(),
        ] {
            assert!(
                output.status.success(),
                "{}",
                String::from_utf8_lossy(&output.stderr)
            );
            assert_eq!(String::from_utf8_lossy(&output.stdout), expected);
        }
    }
}

#[test]
fn borrowed_optional_json_encoding_owns_result() {
    let helper = r#"module helper
import core.json
pub Record { text: string }
pub Optional { record: Option<Record> }
pub fn encode(borrow input: Optional, limit: i64) -> Result<string, Error> {
    match input.record {
        None => Err(Error.Invalid),
        Some(value) => json.encode_bounded(value, limit),
    }
}
pub fn produce() -> Result<string, Error> {
    input := Optional { record: Some(Record { text: "owned".clone() }) }
    result := encode(input, 16)?
    print(match encode(input, 15) { Err(_) => true, Ok(_) => false })
    empty := Optional { record: None }
    print(match encode(empty, 16) { Err(_) => true, Ok(_) => false })
    Ok(result)
}
"#;
    run_both(
        "r85-independent-encoding",
        helper,
        "import helper\nfn main() -> Result<(), Error> { print(helper.produce()?)\n Ok(()) }\n",
        "true\ntrue\n{\"text\":\"owned\"}\n",
    );
}

#[test]
fn record_initializers_snapshot_before_owner_move() {
    for (name, inner, parameter) in [
        ("ordinary", "pub Inner { text: string }", "Inner"),
        ("generic", "pub Inner<T> { text: T }", "Inner<string>"),
    ] {
        let helper = format!(
            r#"module helper
import core.json
{inner}
pub Outer {{ value: Option<{parameter}>, digest: Option<string> }}
pub fn build(value: {parameter}) -> Outer {{
    Outer {{ digest: Some(value.text.clone()), value: Some(value) }}
}}
pub fn produce() -> Result<string, Error> {{
    input := Inner {{ text: "owned".clone() }}
    output := build(input)
    json.encode(output)
}}
"#
        );
        run_both(
            &format!("r86-{name}"),
            &helper,
            "import helper\nfn main() -> Result<(), Error> { print(helper.produce()?)\n Ok(()) }\n",
            "{\"value\":{\"text\":\"owned\"},\"digest\":\"owned\"}\n",
        );
    }
}

#[test]
fn record_initializers_preserve_effect_order() {
    let helper = r#"module helper
pub Pair<T> { first: T, second: T }
fn observe(value: i64) -> i64 { print(value)\n value }
pub fn produce() -> i64 {
    result := Pair { second: observe(2), first: observe(1) }
    result.first + result.second
}
"#
    .replace("\\n", "\n");
    run_both(
        "r86-effects",
        &helper,
        "import helper\nfn main() { print(helper.produce()) }\n",
        "2\n1\n3\n",
    );
}

#[test]
fn record_initializers_reject_read_after_move() {
    let source = r#"module helper
pub Inner { text: string }
pub Outer { digest: string, value: Inner }
pub fn invalid(value: Inner) -> Outer {
    Outer { value: value, digest: value.text.clone() }
}
"#;
    let checked = diff_check_multi(
        "r86-use-after-move",
        &[
            ("helper.align", source),
            ("main.align", "import helper\nfn main() {}\n"),
        ],
        "main.align",
    );
    assert!(
        checked.whole_errors && checked.per_unit_errors,
        "{}\n{}",
        checked.whole_diags,
        checked.per_unit_diags
    );
    assert!(
        checked.whole_diags.contains("moved"),
        "{}",
        checked.whole_diags
    );
}

#[test]
fn borrowed_optional_command_shared_lifecycle() {
    let helper = r#"module helper
import std.process
pub Carrier { selected: Option<command> }
pub fn start(borrow input: Carrier) -> Result<process.child_scope, Error> {
    match input.selected { None => Err(Error.Invalid), Some(prepared) => prepared.start_scope() }
}
pub fn absent() -> bool {
    input := Carrier { selected: None }
    match start(input) { Err(_) => true, Ok(_) => false }
}
pub fn selected() -> Carrier {
    executable := "/bin/true".clone()
    Carrier { selected: Some(process.command(executable, ["true"])) }
}
pub fn finish(borrow input: Carrier) -> Result<(), Error> {
    mut scope := start(input)?
    loop {
        scope.reap(16)?
        if scope.release()? { break }
    }
    Ok(())
}
"#;
    let main = if cfg!(target_os = "linux") {
        "import helper\nfn main() -> Result<(), Error> { print(helper.absent())\n input := helper.selected()\n helper.finish(input)?\n helper.finish(input)?\n print(true)\n Ok(()) }\n"
    } else {
        "import helper\nfn main() { print(helper.absent()) }\n"
    };
    run_both(
        "r84-shared-command",
        helper,
        main,
        if cfg!(target_os = "linux") {
            "true\ntrue\n"
        } else {
            "true\n"
        },
    );
}

#[test]
fn borrowed_optional_command_rejects_mutation_and_transfer() {
    for (result, none, body, diagnostic) in [
        ("Option<command>", "None", "Some(prepared)", "borrow"),
        (
            "()",
            "{}",
            "{ prepared.env_clear() }",
            "cannot configure borrowed match payload",
        ),
        ("()", "{}", "{ take(prepared) }", "cannot move"),
    ] {
        let helper = format!(
            r#"module helper
import std.process
fn take(value: command) {{}}
pub fn invalid(borrow input: Option<command>) -> {result} {{
    match input {{ None => {none}, Some(prepared) => {body} }}
}}
"#
        );
        let checked = diff_check_multi(
            "r84-shared-command-negative",
            &[
                ("helper.align", &helper),
                ("main.align", "import helper\nfn main() {}\n"),
            ],
            "main.align",
        );
        assert!(
            checked.whole_errors && checked.per_unit_errors,
            "{}\n{}",
            checked.whole_diags,
            checked.per_unit_diags
        );
        assert!(
            checked.whole_diags.contains(diagnostic),
            "{}",
            checked.whole_diags
        );
    }
}

#[test]
fn record_initializers_stop_at_early_exit() {
    let helper = r#"module helper
pub Row { first: string, second: string, third: string }
fn observed(label: str) -> string { print(label)\n label.clone() }
fn fail() -> Result<string, Error> { print("fail")\n Err(Error.Invalid) }
pub fn produce() -> Result<Row, Error> {
    Ok(Row { third: observed("first"), second: fail()?, first: observed("unreached") })
}
"#
    .replace("\\n", "\n");
    run_both(
        "r86-early-exit",
        &helper,
        "import helper\nfn main() { print(match helper.produce() { Err(_) => true, Ok(_) => false }) }\n",
        "first\nfail\ntrue\n",
    );
}

#[test]
fn record_initializers_preserve_copy_snapshot_and_nested_order() {
    let helper = r#"module helper
pub Pair { first: i64, second: i64 }
pub Outer { nested: Pair, last: i64 }
fn replace(borrow mut value: i64) -> i64 { value = 9\n 1 }
pub fn produce() -> i64 {
    mut value := 3
    result := Outer { last: value, nested: Pair { second: value, first: replace(value) } }
    print(result.last)
    print(result.nested.second)
    value
}
"#
    .replace("\\n", "\n");
    run_both(
        "r86-copy-snapshots",
        &helper,
        "import helper\nfn main() { print(helper.produce()) }\n",
        "3\n3\n9\n",
    );
}

#[test]
fn reordered_record_initializers_in_fixed_arrays() {
    for generic in [false, true] {
        let declaration = if generic {
            "User<T> { name: T, age: i64 }"
        } else {
            "User { name: string, age: i64 }"
        };
        for early in [false, true] {
            let age = if early { "fail()?" } else { "2" };
            let helper = format!(
                r#"module helper
{declaration}
fn fail() -> Result<i64, Error> = Err(Error.Invalid)
pub fn produce() -> Result<(), Error> {{
    mut users := [User {{ age: 1, name: "a".clone() }}, User {{ name: "b".clone(), age: {age} }}]
    users[0].name = "c".clone()
    print(users[0].name)
    print(users[1].name)
    Ok(())
}}
"#
            );
            let main = format!(
                r#"import helper
extern "C" fn align_rt_alloc_count() -> i64
extern "C" fn align_rt_free_count() -> i64
fn main() {{
    before_alloc := unsafe {{ align_rt_alloc_count() }}
    before_free := unsafe {{ align_rt_free_count() }}
    print(match helper.produce() {{ Err(_) => true, Ok(_) => false }})
    print(unsafe {{ align_rt_alloc_count() }} - before_alloc)
    print(unsafe {{ align_rt_free_count() }} - before_free)
}}
"#
            );
            run_both(
                "r86-fixed-array",
                &helper,
                &main,
                if early {
                    "true\n2\n2\n"
                } else {
                    "c\nb\nfalse\n3\n3\n"
                },
            );
        }
    }
}

#[test]
fn regular_reader_survives_path_owner_and_keeps_normal_resolution() {
    let fixture = private_fixture();
    let root = std::fs::canonicalize(&fixture.dir).expect("fixture root");
    std::fs::write(root.join("data"), b"hello").expect("write data");
    std::fs::create_dir(root.join("directory")).expect("create directory");
    std::os::unix::fs::symlink("data", root.join("link")).expect("create link");
    let helper = r#"module helper
import std.fs
pub fn open(path: str) -> Result<reader, Error> = fs.open_regular(path)
pub fn observe(path: str) -> Result<(), Error> {
    input := { owned_path := path.clone()\n open(owned_path)? }
    metadata := input.metadata()?
    print(match metadata.kind { Regular => true, _ => false })
    mut bytes := buffer(16)
    print(input.read(bytes)?)
    print(bytes.bytes().as_str()?)
    Ok(())
}
"#
    .replace("\\n", "\n");
    let main = format!(
        r#"import helper
import std.fs
fn main() -> Result<(), Error> {{
    helper.observe({:?})?
    helper.observe({:?})?
    print(match fs.open_regular({:?}) {{ Err(error) => match error {{ Invalid => true, _ => false }}, Ok(_) => false }})
    Ok(())
}}
"#,
        root.join("./data").to_str().expect("path"),
        root.join("link").to_str().expect("path"),
        root.join("directory").to_str().expect("path")
    );
    run_both(
        "r88-reader",
        &helper,
        &main,
        "true\n5\nhello\ntrue\n5\nhello\ntrue\n",
    );
}

#[test]
fn regular_reader_requires_import_and_exact_path_type() {
    owned_fixture::run(
        "regular_reader_requires_import_and_exact_path_type",
        |stage| {
            for constructor in ["open_regular", "open_ro_regular"] {
                for source in [
                    "fn main() { fs.open_regular(\"x\") }",
                    "import std.fs\nfn main() { fs.open_regular(3) }",
                    "import std.fs\nfn main() { fs.open_regular() }",
                    "import std.fs\nfn main() { fs.open_regular(\"x\", 1) }",
                ] {
                    regular_source_errors(stage, &source.replace("open_regular", constructor));
                }
            }
        },
    );
}

fn private_fixture() -> Proj {
    use std::os::unix::fs::DirBuilderExt;
    static NEXT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    let nonce = NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    let dir = std::env::temp_dir().join(format!("align-admission-{}-{nonce}", std::process::id()));
    std::fs::DirBuilder::new()
        .mode(0o700)
        .create(&dir)
        .expect("acquire private fixture");
    Proj {
        dir,
        entry: "main.align".to_string(),
    }
}

#[test]
fn json_dynamic_array_root_matrix() {
    for (name, declarations, element, values, encoded) in [
        (
            "unsigned",
            "",
            "u64",
            vec!["18446744073709551615", "0"],
            "[18446744073709551615,0]",
        ),
        ("signed", "", "i8", vec!["-128", "127"], "[-128,127]"),
        ("unsigned8", "", "u8", vec!["255", "0"], "[255,0]"),
        ("unsigned16", "", "u16", vec!["65535", "0"], "[65535,0]"),
        (
            "unsigned32",
            "",
            "u32",
            vec!["4294967295", "0"],
            "[4294967295,0]",
        ),
        (
            "signed16",
            "",
            "i16",
            vec!["-32768", "32767"],
            "[-32768,32767]",
        ),
        (
            "signed32",
            "",
            "i32",
            vec!["-2147483648", "2147483647"],
            "[-2147483648,2147483647]",
        ),
        (
            "signed64",
            "",
            "i64",
            vec!["-9223372036854775808", "9223372036854775807"],
            "[-9223372036854775808,9223372036854775807]",
        ),
        ("float32", "", "f32", vec!["1.5", "-0.0"], "[1.5,-0.0]"),
        ("float64", "", "f64", vec!["1.5", "-0.0"], "[1.5,-0.0]"),
        ("bool", "", "bool", vec!["true", "false"], "[true,false]"),
        (
            "view",
            "",
            "str",
            vec![r#""a\\b""#, r#""\"""#],
            r#"["a\\b","\""]"#,
        ),
        (
            "string",
            "",
            "string",
            vec![r#""a\\b".clone()"#, r#""\"".clone()"#],
            r#"["a\\b","\""]"#,
        ),
        (
            "copy-record",
            "pub Row { text: str, value: i64 }",
            "Row",
            vec![r#"Row { text: "view", value: 2 }"#],
            r#"[{"text":"view","value":2}]"#,
        ),
        (
            "owned-record",
            "pub Inner { text: string }\npub Row { inner: Inner, maybe: Option<string> }",
            "Row",
            vec![
                r#"Row { inner: Inner { text: "owned".clone() }, maybe: Some("yes".clone()) }"#,
                r#"Row { inner: Inner { text: "next".clone() }, maybe: None }"#,
            ],
            r#"[{"inner":{"text":"owned"},"maybe":"yes"},{"inner":{"text":"next"}}]"#,
        ),
    ] {
        for empty in [false, true] {
            let pushes = if empty {
                String::new()
            } else {
                values
                    .iter()
                    .map(|value| format!("    builder.push({value})\n"))
                    .collect()
            };
            let expected = if empty { "[]" } else { encoded };
            let count = if empty { 0 } else { values.len() };
            let limit = expected.len();
            let region = matches!(name, "view" | "copy-record");
            let begin = if region { "arena out {" } else { "" };
            let end = if region { "}" } else { "" };
            let region_arg = if region { "out" } else { "" };
            let helper = format!(
                r#"module helper
import core.json
{declarations}
pub Envelope {{ rows: array<{element}> }}
pub fn produce() -> Result<(), Error> {{
    {begin}
    mut builder: array_builder<{element}> := array_builder({region_arg})
{pushes}    values := builder.build()
    print(json.encode(values)?)
    print(json.encode_bounded(values, {limit})?)
    print(match json.encode_bounded(values, {under}) {{ Err(Invalid) => true, _ => false }})
    print(match json.encode_bounded(values, -1) {{ Err(Invalid) => true, _ => false }})
    print(values.len())
    envelope := Envelope {{ rows: values }}
    print(json.encode(envelope)?)
    {end}
    Ok(())
}}
"#,
                under = limit - 1
            );
            run_both(
                &format!("r87-{name}-{empty}"),
                &helper,
                "import helper\nfn main() -> Result<(), Error> = helper.produce()\n",
                &format!("{expected}\n{expected}\ntrue\ntrue\n{count}\n{{\"rows\":{expected}}}\n"),
            );
        }
    }
}

#[test]
fn json_array_root_output_survives_borrowed_optional_source() {
    let helper = r#"module helper
import core.json
pub Row<T> { text: T }
pub Carrier { rows: Option<array<Row<string>>> }
pub fn encode(borrow input: Carrier) -> Result<string, Error> {
    match input.rows { None => Err(Error.Invalid), Some(values) => json.encode_bounded(values, 128) }
}
pub fn produce() -> Result<string, Error> {
    mut builder: array_builder<Row<string>> := array_builder()
    builder.push(Row { text: "owned".clone() })
    source := Carrier { rows: Some(builder.build()) }
    empty := Carrier { rows: None }
    print(match encode(empty) { Err(Invalid) => true, _ => false })
    print(encode(source)?)
    encode(source)
}
"#;
    run_both(
        "r87-borrowed-optional",
        helper,
        "import helper\nfn main() -> Result<(), Error> { print(helper.produce()?)\n Ok(()) }\n",
        "true\n[{\"text\":\"owned\"}]\n[{\"text\":\"owned\"}]\n",
    );
}

#[test]
fn json_array_root_rejects_unsupported_elements_and_source_replacement() {
    for source in [
        "import core.json\nfn bad(borrow rows: array<Option<i64>>) { json.encode(rows) }\nfn main() {}",
        "import core.json\nfn bad(borrow rows: array<array<i64>>) { json.encode(rows) }\nfn main() {}",
        "import core.json\nTag { A, B }\nfn bad(borrow rows: array<Tag>) { json.encode(rows) }\nfn main() {}",
        "import core.json\nRow { value: i64 }\nfn bad(borrow rows: soa<Row>) { json.encode(rows) }\nfn main() {}",
        r#"import core.json
fn replace(borrow mut rows: array<i64>) -> i64 {
    mut builder: array_builder<i64> := array_builder()
    rows = builder.build()
    128
}
fn main() {
    mut builder: array_builder<i64> := array_builder()
    builder.push(7)
    mut rows := builder.build()
    output := json.encode_bounded(rows, replace(rows))
}
"#,
    ] {
        let result = diff_check_multi("r87-negative", &[("main.align", source)], "main.align");
        assert!(
            result.whole_errors && result.per_unit_errors,
            "accepted unsupported source: {source}"
        );
        if source.contains("fn replace") {
            assert!(
                result
                    .whole_diags
                    .contains("JSON source place was invalidated"),
                "{}",
                result.whole_diags
            );
        }
    }
}

#[test]
fn json_array_root_nonfinite_failure_and_cleanup() {
    let helper = r#"module helper
import core.json
pub Row { text: string, value: f64 }
extern "C" fn align_rt_requested_live_bytes() -> i64
pub fn exercise() {
    mut builder: array_builder<Row> := array_builder()
    builder.push(Row { text: "owned".clone(), value: 1.0 / 0.0 })
    values := builder.build()
    print(unsafe { align_rt_requested_live_bytes() } > 0)
    print(match json.encode(values) { Err(Invalid) => true, _ => false })
    print(match json.encode_bounded(values, 128) { Err(Invalid) => true, _ => false })
    print(values.len())
    mut floats: array_builder<f64> := array_builder()
    floats.push(0.0 / 0.0)
    numbers := floats.build()
    print(match json.encode(numbers) { Err(Invalid) => true, _ => false })
    print(numbers.len())
}
"#;
    let main = r#"import helper
extern "C" fn align_rt_requested_live_reset()
extern "C" fn align_rt_requested_live_bytes() -> i64
fn main() {
    unsafe { align_rt_requested_live_reset() }
    helper.exercise()
    print(unsafe { align_rt_requested_live_bytes() })
}
"#;
    run_both(
        "r87-failure-cleanup",
        helper,
        main,
        "true\ntrue\ntrue\n1\ntrue\n1\n0\n",
    );
}

#[test]
fn json_array_root_arena_output_has_independent_cleanup() {
    let helper = r#"module helper
import core.json
extern "C" fn align_rt_requested_live_bytes() -> i64
pub fn produce() -> Result<string, Error> {
    arena out {
        mut builder: array_builder<str> := array_builder(out)
        builder.push("view")
        values := builder.build()
        return json.encode(values)
    }
}
pub fn exercise() -> Result<(), Error> {
    output := produce()?
    print(unsafe { align_rt_requested_live_bytes() } > 0)
    print(output)
    Ok(())
}
"#;
    let main = r#"import helper
extern "C" fn align_rt_requested_live_reset()
extern "C" fn align_rt_requested_live_bytes() -> i64
fn main() -> Result<(), Error> {
    unsafe { align_rt_requested_live_reset() }
    helper.exercise()?
    print(unsafe { align_rt_requested_live_bytes() })
    Ok(())
}
"#;
    run_both("r87-arena-result", helper, main, "true\n[\"view\"]\n0\n");
}

// Plan185: imported construction, temporary path lifetime and cached discriminator replay.
#[test]
fn regular_file_positional_lifecycle_and_replay() {
    owned_fixture::run("regular_file_positional_lifecycle_and_replay", |stage| {
        let path = stage.join("input");
        std::fs::write(&path, b"0123456789").unwrap();
        let helper = r#"module helper
import std.fs
pub fn open<T>(path:str, tag:T, way:i64)->Result<file,Error> {
  if way==0 { return fs.open_ro_regular(path.clone()) }
  if way==1 { return fs.open_ro_regular(if true {path.clone()} else {"unused".clone()}) }
  if way==2 { return fs.open_ro_regular(match way {2=>path.clone(),_=>"unused".clone()}) }
  if way==3 { return fs.open_ro_regular(loop {break path.clone()}) }
  return arena {fs.open_ro_regular(path.clone())}
}
pub fn skipped(path:str)->Result<file,Error> {
  pending:=fs.open_ro_regular({return Err(Error.Invalid);path.clone()})
  return pending
}
pub fn mapped(error:Error)->Error = error
pub fn observe(borrow input:file)->Result<(),Error> {
  mut bytes:=buffer(8)
  if input.len()?!=10 {return Err(Error.Invalid)}
  if input.pread_into(bytes,0,4,6)?!=4 {return Err(Error.Invalid)}
  if bytes.bytes().as_str()?!="6789" {return Err(Error.Invalid)}
  if input.pread_into(bytes,4,4,0)?!=4 {return Err(Error.Invalid)}
  if bytes.bytes().as_str()?!="67890123" {return Err(Error.Invalid)}
  if input.pread_into(bytes,0,8,10)?!=0 || bytes.len()!=8 {return Err(Error.Invalid)}
  if input.pread(bytes,8)?!=2 {return Err(Error.Invalid)}
  if bytes.bytes().as_str()?!="89" {return Err(Error.Invalid)}
  match input.pwrite("",0) {Err(error)=>{match error {Denied=>{},_=>{return Err(error)}}},Ok(count)=>{return Err(Error.Invalid)}}
  return Ok(())
}
"#;
        let main = format!(
            r#"module main
import helper
import std.fs
fn exercise()->Result<(),Error> {{
  path:={path:?}
  mut way:=0
  loop {{
    if way==5 {{break}}
    mut input:=helper.open(path,way,way).map_err(helper.mapped)?
    helper.observe(input)?
    input=helper.open(path,true,way) else {{return Err(Error.Invalid)}}
    helper.observe(input)?
    match helper.open(path,"tag",way) {{Ok(selected)=>{{helper.observe(selected)?}},Err(error)=>{{return Err(error)}}}}
    discarded:=helper.open(path,way,way)
    way=way+1
  }}
  match helper.skipped(path) {{Err(error)=>{{match error {{Invalid=>{{}},_=>{{return Err(error)}}}}}},Ok(unexpected)=>{{return Err(Error.Invalid)}}}}
  match fs.open_ro_regular({directory:?}) {{Err(error)=>{{match error {{Invalid=>{{}},_=>{{return Err(error)}}}}}},Ok(unexpected)=>{{return Err(Error.Invalid)}}}}
  match fs.open_ro_regular({missing:?}) {{Err(error)=>{{match error {{NotFound=>{{}},_=>{{return Err(error)}}}}}},Ok(unexpected)=>{{return Err(Error.Invalid)}}}}
  return Ok(())
}}
fn main()->i32 {{return match exercise() {{Ok(value)=>0,Err(error)=>1}}}}
"#,
            path = path.to_str().unwrap(),
            directory = stage.to_str().unwrap(),
            missing = stage.join("missing").to_str().unwrap()
        );
        std::fs::write(stage.join("helper.align"), helper).unwrap();
        let entry = stage.join("main.align");
        std::fs::write(&entry, &main).unwrap();
        let entry = entry.to_str().unwrap();
        let mut sources = align_span::SourceMap::new();
        let checked = align_driver::check(&mut sources, entry, &main);
        assert!(
            !checked.diags.has_errors(),
            "{}",
            align_driver::format_diagnostics(&sources, &checked.diags)
        );
        let whole = align_mir::lower_program_checked(&checked.hir, false, Some(&sources)).unwrap();
        regular_file_execute(stage, &[whole], "whole");
        let mut sources = align_span::SourceMap::new();
        let built = align_driver::build_per_unit(&mut sources, entry, &main);
        assert!(
            !built.diags.has_errors(),
            "{}",
            align_driver::format_diagnostics(&sources, &built.diags)
        );
        regular_file_execute(
            stage,
            &built
                .units
                .into_iter()
                .map(|unit| unit.mir)
                .collect::<Vec<_>>(),
            "units",
        );
        let cache = align_driver::CacheContext::at(stage.join("cache"));
        for round in 0..2 {
            let mut sources = align_span::SourceMap::new();
            let mut built = align_driver::build_package(
                &mut sources,
                entry,
                &main,
                &cache,
                align_driver::UnitReuse::Allowed,
            );
            assert!(
                !built.diags.has_errors(),
                "{}",
                align_driver::format_diagnostics(&sources, &built.diags)
            );
            assert_eq!(built.units.len(), 2);
            assert!(
                built
                    .units
                    .iter()
                    .all(|unit| unit.frontend.as_ref().unwrap().hit == (round == 1))
            );
            let programs = (0..built.units.len())
                .map(|i| built.materialize(i).unwrap().clone())
                .collect::<Vec<_>>();
            if round == 1 {
                regular_file_execute(stage, &programs, "cache-hit")
            }
        }
    });
}

fn regular_file_execute(stage: &std::path::Path, programs: &[align_mir::Program], label: &str) {
    let mut objects = Vec::new();
    let mut libraries = Vec::new();
    for (i, program) in programs.iter().enumerate() {
        let object = stage.join(format!("{label}-{i}.o"));
        align_driver::emit_object_file(
            program,
            &object,
            align_driver::BuildTarget::Baseline,
            align_driver::Profile::Release,
            &[],
            false,
        )
        .unwrap();
        objects.push(object);
        for library in &program.link_libs {
            if !libraries.contains(library) {
                libraries.push(library.clone());
            }
        }
    }
    let binary = stage.join(label);
    align_driver::link_objects(
        &align_driver::CDriver::default(),
        &objects.iter().map(|p| p.as_path()).collect::<Vec<_>>(),
        &binary,
        &libraries,
        align_driver::Profile::Release,
    )
    .unwrap();
    let output = std::process::Command::new(binary).output().unwrap();
    assert!(
        output.status.success() && output.stdout.is_empty() && output.stderr.is_empty(),
        "{label}: {output:?}"
    );
}

#[test]
fn regular_file_preserves_effect_type_and_move_rules() {
    owned_fixture::run(
        "regular_file_preserves_effect_type_and_move_rules",
        |stage| {
            for (source, diagnostic) in [
                (
                    "fn inspect(x:i64)->i64 { result:=fs.open_ro_regular(\"input\"); return x }\nfn main(){ values:=[1,2].par_map(inspect); print(values.sum()) }",
                    "requires a Pure function",
                ),
                (
                    "fn take(value:file) {}\nfn main()->Result<(),Error> { value:=fs.open_ro_regular(\"input\")?; take(value); value.len()?; return Ok(()) }",
                    "moved",
                ),
                (
                    "fn main()->Result<(),Error> { value:reader:=fs.open_ro_regular(\"input\")?; return Ok(()) }",
                    "type mismatch",
                ),
            ] {
                let source = format!("import std.fs\n{source}\n");
                for errors in regular_source_errors(stage, &source) {
                    assert!(errors.contains(diagnostic), "{errors}");
                }
            }
        },
    );
}

fn regular_source_errors(stage: &std::path::Path, source: &str) -> [String; 2] {
    let entry = stage.join("main.align");
    std::fs::write(&entry, source).unwrap();
    let entry = entry.to_str().unwrap();
    let mut sources = align_span::SourceMap::new();
    let whole = align_driver::check(&mut sources, entry, source);
    assert!(whole.diags.has_errors(), "whole-program accepted {source}");
    let whole_errors = align_driver::format_diagnostics(&sources, &whole.diags);
    let mut sources = align_span::SourceMap::new();
    let units = align_driver::check_per_unit(&mut sources, entry, source);
    assert!(units.diags.has_errors(), "per-unit accepted {source}");
    [
        whole_errors,
        align_driver::format_diagnostics(&sources, &units.diags),
    ]
}
