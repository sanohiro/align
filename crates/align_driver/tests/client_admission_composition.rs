//! R84–R88 provider owners; consumer policy and managed-pin adoption remain external.
mod common;
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
fn owned_array_json_root_remains_explicitly_deferred() {
    let source = r#"import core.json
Row { text: string }
fn main() -> Result<(), Error> {
    mut rows: array_builder<Row> := array_builder()
    rows.push(Row { text: "owned".clone() })
    values := rows.build()
    json.encode_bounded(values, 1024)?
    Ok(())
}
"#;
    let checked = diff_check_multi("r87-root-deferred", &[("main.align", source)], "main.align");
    assert!(
        checked.whole_errors && checked.per_unit_errors,
        "{}\n{}",
        checked.whole_diags,
        checked.per_unit_diags
    );
    assert!(
        checked
            .whole_diags
            .contains("expects a struct or struct-array"),
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
    for source in [
        "fn main() { fs.open_regular(\"x\") }",
        "import std.fs\nfn main() { fs.open_regular(3) }",
        "import std.fs\nfn main() { fs.open_regular() }",
        "import std.fs\nfn main() { fs.open_regular(\"x\", 1) }",
    ] {
        let checked = diff_check_multi(
            "r88-invalid-source",
            &[("main.align", source)],
            "main.align",
        );
        assert!(
            checked.whole_errors && checked.per_unit_errors,
            "{}\n{}",
            checked.whole_diags,
            checked.per_unit_diags
        );
    }
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
