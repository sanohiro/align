//! Plan246: inactive temporary payloads do not release a retained active owner.
mod common;
use common::*;
#[path = "helpers/indexed_array_field.rs"]
mod native;
#[path = "helpers/owned_fixture.rs"]
mod owned_fixture;

fn verdict(label: &str, source: &str, rejected: bool) {
    let result = diff_check_multi(label, &[("main.align", source)], "main.align");
    assert_eq!(result.whole_errors, rejected, "{source}\n{}", result.whole_diags);
    assert_eq!(result.per_unit_errors, rejected, "{source}\n{}", result.per_unit_diags);
    if rejected {
        for diagnostics in [&result.whole_diags, &result.per_unit_diags] {
            assert!(diagnostics.contains("invalidated") || diagnostics.contains("borrow") || diagnostics.contains("outlive"), "{diagnostics}");
        }
    }
}

fn program(ty: &str, value: &str, binding: &str, source: &str) -> String {
    format!(r#"
Record {{ values: array<i64> }}
fn fresh() -> {ty} {{ {value} }}
fn make(flag: bool) -> Result<{ty}, Error> = if flag {{ Ok(fresh()) }} else {{ Err(Error.Invalid) }}
fn run(flag: bool) -> Option<{ty}> {{
 mut retained: Option<{ty}> := None
 {binding}
 match {source} {{ Ok(value) => {{ retained = Some(value) }}, Err(_) => {{}} }}
 retained
}}
fn main() {{ value := run(true) }}
"#)
}

#[test]
fn source_and_type_matrix() {
    for (ty, value) in [
        ("array<i64>", "[7, 9].to_array()"),
        ("Record", "Record { values: [7, 9].to_array() }"),
        ("array<Record>", "mut b: array_builder<Record> := array_builder(); b.push(Record { values: [7].to_array() }); b.build()"),
        ("string", "\"owned\".clone()"),
        ("i64", "7"),
    ] {
        for source in ["make(flag)", "{ make(flag) }", "if flag { make(true) } else { make(false) }"] {
            verdict("owned-match-source", &program(ty, value, "", source), false);
        }
    }
    for (binding, source) in [("source := make(flag)", "source"), ("", "loop { break make(flag) }")] {
        verdict("owned-match-source-control", &program("array<i64>", "[7].to_array()", binding, source), false);
    }
    verdict("owned-match-returned-result", r#"
fn make(flag: bool) -> Result<array<i64>, Error> = if flag { Ok([7].to_array()) } else { Err(Error.Invalid) }
fn run(flag: bool) -> Option<array<i64>> = match make(flag) { Ok(value) => Some(value), Err(_) => None }
fn main() { result := run(true) }
"#, false);
    for (tagged_ty, make, arms) in [
        ("Result<i64, array<i64>>", "Err([7].to_array())", "Err(value) => { retained = Some(value) }, Ok(_) => {}"),
        ("Option<array<i64>>", "Some([7].to_array())", "Some(value) => { retained = Some(value) }, None => {}"),
        ("Choice", "Choice.Left([7].to_array())", "Left(value) => { retained = Some(value) }, _ => {}"),
        ("Choice", "Choice.Right([7].to_array())", "Left(_) => {}, Right(value) => { retained = Some(value) }, Empty => {}"),
    ] {
        let source = format!("Choice {{ Left(array<i64>), Right(array<i64>), Empty }}\nfn make() -> {tagged_ty} = {make}\nfn run() -> Option<array<i64>> {{ mut retained: Option<array<i64>> := None; match make() {{ {arms} }}; retained }}\nfn main() {{ result := run() }}");
        verdict("owned-match-tag-symmetry", &source, false);
    }
    verdict("owned-match-or-pattern", r#"
Tag { A, B, C(array<i64>) }
fn make() -> Tag = Tag.C([7].to_array())
fn run() -> Option<array<i64>> { mut retained: Option<array<i64>> := None; match make() { A | B => {}, C(value) => { retained = Some(value) } }; retained }
fn main() { result := run() }
"#, false);
}

#[test]
fn control_and_expiration() {
    let prelude = "fn make(flag: bool) -> Result<array<i64>, Error> = if flag { Ok([7].to_array()) } else { Err(Error.Invalid) }\nfn wrap(value: array<i64>) -> Result<array<i64>, Error> = Ok(value)\n";
    for source in [
        "make(flag).map_err(fn error: Error { error })",
        "wrap(make(flag)?)",
    ] {
        let program = format!("{prelude}fn run(flag: bool) -> Result<Option<array<i64>>, Error> {{ mut retained: Option<array<i64>> := None; match {source} {{ Ok(value) => {{ retained = Some(value) }}, Err(_) => {{}} }}; Ok(retained) }}\nfn main() {{ result := run(true) }}");
        verdict("owned-match-fallible-control", &program, false);
    }
    verdict("owned-match-else-control", &format!("{prelude}fn run(flag: bool) -> Option<array<i64>> {{ value := make(flag) else {{ return None }}; Some(value) }}\nfn main() {{ result := run(false) }}"), false);
    for (limit, exit) in [(0, ""), (1, ""), (3, ""), (3, "if i == 1 { break }"), (3, "if i == 1 { return retained }")] {
        let source = format!("{prelude}fn run() -> Option<array<i64>> {{ mut retained: Option<array<i64>> := Some([1].to_array()); mut i := 0; loop {{ if i >= {limit} {{ break }}; match make(i != 1) {{ Ok(value) => {{ retained = Some(value) }}, Err(_) => {{}} }}; {exit}; i = i + 1 }}; retained }}\nfn main() {{ result := run() }}");
        verdict("owned-match-loop-replacement", &source, false);
    }
    let negative = r#"
fn make(text: str) -> Result<array<str>, Error> = Ok([text].to_array())
fn bad() -> Option<array<str>> {
 owner := "a local borrowed member".clone()
 mut retained: Option<array<str>> := None
 match make(owner) { Ok(value) => { retained = Some(value) }, Err(_) => {} }
 retained
}
fn main() { result := bad() }
"#;
    verdict("owned-match-retains-borrowed-member", negative, true);
    let negative = format!("{prelude}fn consume(value: array<i64>) {{}}\nfn main() {{ owner := [7].to_array(); stale := owner[..]; consume(owner); mut retained: Option<array<i64>> := None; match make(false) {{ Ok(value) => {{ retained = Some(value) }}, Err(_) => {{}} }}; print(stale[0]) }}");
    verdict("owned-match-keeps-ended-owner", &negative, true);
    let negative = format!("{prelude}fn main() {{ seed := [0]; mut stale := seed[..]; mut i := 0; loop {{ if i >= 2 {{ break }}; print(stale[0]); match make(true) {{ Ok(value) => {{ stale = value[..] }}, Err(_) => {{}} }}; i = i + 1 }}; print(stale[0]) }}");
    verdict("owned-match-prior-iteration", &negative, true);
}

#[test]
fn native_lifecycle_and_cache() {
    owned_fixture::run("native_lifecycle_and_cache", |stage| {
        let module = r#"module payloads
pub fn make<T>(flag: bool, marker: T) -> Result<array<i64>, array<i64>> =
 if flag { Ok([7, 9].to_array()) } else { Err([11, 13].to_array()) }
pub fn collect(flag: bool, replace: bool) -> Option<array<i64>> {
 mut retained: Option<array<i64>> := if replace { Some([1, 2].to_array()) } else { None }
 match make(flag, 1) { Ok(value) => { retained = Some(value) }, Err(_) => {} }
 retained
}
pub fn inspect(value: Option<array<i64>>) -> i64 = match value { Some(data) => data[0] + data[1], None => 0 }
pub fn repeat(count: i64) -> Option<array<i64>> {
 mut retained: Option<array<i64>> := None
 mut i := 0
 loop { if i >= count { break }; match make(i % 2 == 0, 1) { Ok(value) => { retained = Some(value) }, Err(_) => {} }; i = i + 1 }
 retained
}
"#;
        let source = r#"import payloads
extern "C" fn align_rt_alloc_count() -> i64
extern "C" fn align_rt_free_count() -> i64
fn round() -> bool {
 payloads.inspect(payloads.collect(true, false)) == 16 &&
 payloads.inspect(payloads.collect(false, false)) == 0 &&
 payloads.inspect(payloads.collect(true, true)) == 16 &&
 payloads.inspect(payloads.collect(false, true)) == 3 &&
 payloads.inspect(payloads.repeat(0)) == 0 &&
 payloads.inspect(payloads.repeat(1)) == 16 &&
 payloads.inspect(payloads.repeat(3)) == 16
}
fn main() -> i32 {
 allocated := unsafe { align_rt_alloc_count() }
 freed := unsafe { align_rt_free_count() }
 mut i := 0
 loop { if i >= 4 { break }; if !round() { return 1 }; i = i + 1 }
 if unsafe { align_rt_alloc_count() } - allocated != 40 { return 2 }
 if unsafe { align_rt_free_count() } - freed != 40 { return 3 }
 0
}
"#;
        std::fs::write(stage.join("payloads.align"), module).unwrap();
        native::execute(stage, source, "owned-match-native").unwrap();
        let cache = align_driver::CacheContext::at(stage.join("cache"));
        let entry = stage.join("main.align");
        let mut first = Vec::new();
        for round in 0..2 {
            let mut sources = align_span::SourceMap::new();
            let mut package = align_driver::build_package(&mut sources, entry.to_str().unwrap(), source,
                &cache, align_driver::UnitReuse::Allowed);
            assert!(!package.diags.has_errors(), "{}", align_driver::format_diagnostics(&sources, &package.diags));
            let helper = package.units.iter().find(|unit| unit.unit == "payloads").unwrap();
            assert_eq!(helper.frontend.as_ref().unwrap().hit, round == 1);
            let programs = (0..package.units.len()).map(|index| package.materialize(index).unwrap().clone()).collect::<Vec<_>>();
            let printed = programs.iter().map(align_mir::print::program_to_string).collect::<Vec<_>>();
            if round == 0 { first = printed; } else { assert_eq!(first, printed); }
            let output = native::run_programs(stage, &programs, &format!("cached-{round}"), false).unwrap();
            assert!(output.status.success() && output.stdout.is_empty() && output.stderr.is_empty(), "{output:?}");
        }
    });
}
