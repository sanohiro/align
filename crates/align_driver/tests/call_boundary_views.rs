//! K1 call authority, temporal observations and Request160 retained copy loops.
#![cfg(unix)]
mod common;
#[path = "helpers/owned_fixture.rs"]
mod owned_fixture;
use common::*;

fn verdict(name: &str, provider: &str, body: &str, error: Option<&str>) {
    let source = format!("module main\nimport helper\n{body}");
    let provider = format!("module helper\n{provider}");
    let checked = diff_check_multi(name, &[("helper.align", &provider), ("main.align", &source)], "main.align");
    for (mode, failed, diagnostics) in [
        ("whole", checked.whole_errors, checked.whole_diags),
        ("units", checked.per_unit_errors, checked.per_unit_diags),
    ] {
        assert_eq!(failed, error.is_some(), "{name}/{mode}: {diagnostics}");
        if let Some(error) = error { assert!(diagnostics.contains(error), "{name}/{mode}: {diagnostics}"); }
    }
}

#[test]
fn imported_boundary_and_source_timing() {
    owned_fixture::run("imported_boundary_and_source_timing", |_| {
        let writer = "pub fn write(borrow mut bytes: slice<u8>) { bytes[0] = 255 }";
        for inside in [false, true] {
            for indirect in [false, true] {
                let action = if indirect { "action := helper.write; action(alias)" } else { "helper.write(alias)" };
                let use_text = if inside { format!("print(items[{{ {action}; 0 }}].good)") }
                    else { format!("{action}; print(items[0].good)") };
                let source = format!("Pair {{ bad: str, good: str }}\nfn main() {{ owner := [(65 as u8)].to_array(); mut alias: slice<u8> := owner; text := alias.as_str() else {{ return }}; items := [Pair {{ bad: text, good: \"safe\" }}]; {use_text} }}");
                verdict(&format!("timing-{inside}-{indirect}"), writer, &source,
                    (!inside).then_some("validated bytes were modified"));
            }
        }
        for (name, provider, source, error) in [
            ("uncalled-writer", "pub fn write(bytes: slice<u8>) { mut local := bytes; local[0] = 1 }", "fn main() {}", Some("read-only view")),
            ("returned-view", "pub fn identity<T>(value: T) -> T = value", "fn main() { owner := [(0 as u8)].to_array(); view: slice<u8> := owner; mut returned := helper.identity(view); returned[0] = 1 }", Some("read-only view")),
            ("retained-authority", "pub fn identity<T>(value: T) -> T = value", "fn main() { owner := [(0 as u8)].to_array(); mut view: slice<u8> := owner; returned := helper.identity(view); view[0] = 1; print(returned[0]) }", None),
            ("fresh-text", "pub fn text(borrow mut bytes: slice<u8>) -> Result<str, Error> { bytes[0] = 65; bytes.as_str() }", "fn main() { owner := [(0 as u8)].to_array(); mut view: slice<u8> := owner; fresh := helper.text(view) else { return }; print(fresh) }", None),
            ("stale-text", "pub fn text(borrow mut bytes: slice<u8>) -> Result<str, Error> { bytes[0] = 65; bytes.as_str() }", "fn main() { owner := [(0 as u8)].to_array(); mut view: slice<u8> := owner; fresh := helper.text(view) else { return }; view[0] = 255; print(fresh) }", Some("validated bytes were modified")),
            ("old-and-new", "pub fn text(borrow mut bytes: slice<u8>) -> Result<str, Error> { bytes[0] = 65; bytes.as_str() }", "fn main() { owner := [(65 as u8)].to_array(); mut view: slice<u8> := owner; old := view.as_str() else { return }; fresh := helper.text(view) else { return }; print(fresh); print(old) }", Some("validated bytes were modified")),
        ] { verdict(name, provider, source, error); }
    });
}

#[test]
fn imported_publication_domains() {
    owned_fixture::run("imported_publication_domains", |_| {
        for dynamic in [false, true] {
            for stale in [false, true] {
                let (ty, place, initial) = if dynamic {
                    ("array<str>", "target[0]", "[\"initial\"].to_array()")
                } else { ("Holder", "target.text", "helper.Holder { text: \"initial\" }") };
                let provider = format!("pub Holder {{ text: str }}\npub fn publish(borrow mut target: {ty}, borrow mut bytes: slice<u8>) {{ bytes[0] = 65; {place} = bytes.as_str() else {{ return }} }}");
                let write = if stale { "alias[0] = 255;" } else { "" };
                let source = format!("fn main() {{ owner := [(65 as u8)].to_array(); mut alias: slice<u8> := owner; mut target := {initial}; helper.publish(target, alias); {write} print({place}) }}");
                verdict("mutable-publication", &provider, &source,
                    stale.then_some("validated bytes were modified"));
            }
        }
        for stale in [false, true] {
            let write = if stale { "alias[0] = 255;" } else { "" };
            verdict("codec-return", "import core.codec\npub fn open(bytes: slice<u8>) -> Result<codec.batch, Error> = codec.open(bytes)",
                &format!("fn main() -> Result<(), Error> {{ owner := buffer.filled(64, 0); mut alias := owner.bytes(); batch := helper.open(alias)?; {write} print(batch.rows()); Ok(()) }}"),
                stale.then_some("validated codec bytes were modified"));
        }
        for exit in ["return Err(1)", "Err(1)"] {
            verdict("stale-error-publication", &format!("pub Holder {{ text: str }}\npub fn publish(borrow mut target: Holder, borrow mut bytes: slice<u8>) -> Result<(), i64> {{ target.text = bytes.as_str() else {{ return Err(0) }}; bytes[0] = 255; {exit} }}"),
                "fn main() {}", Some("validated bytes were modified"));
        }
    });
}

#[test]
fn copy_operand_identities_survive_later_rebinding() {
    owned_fixture::run("copy_operand_identities_survive_later_rebinding", |_| {
        for same in [false, true] {
            let input = if same { "left" } else { "right" };
            verdict("completed-copy-destination", "",
                &format!("fn main() {{ mut left := buffer.filled(4, 65); mut right := buffer.filled(4, 0); mut output := left.bytes(); input := {input}.bytes(); output.copy_from({{ output = right.bytes(); input }}) }}"),
                same.then_some("overlap"));
        }
    });
}

fn copy_program(intrinsic: bool, reverse: bool, nested: bool, fresh: bool) -> (String, String) {
    let parameters = if reverse { "borrow mut target: slice<u8>, borrow source: slice<u8>, at: i64" }
        else { "borrow source: slice<u8>, borrow mut target: slice<u8>, at: i64" };
    let arguments = if reverse { "output, input, at" } else { "input, output, at" };
    let declaration = if nested { "State { rows: buffer }\nPair { left: State, right: State }" }
        else { "Pair { left: buffer, right: buffer }" };
    let (left, right) = if nested { ("pair.left.rows", "pair.right.rows") } else { ("pair.left", "pair.right") };
    let views = format!("input := {left}.bytes(); mut output := {right}.bytes()");
    let action = if intrinsic { "output[at..at+1].copy_from(input[at..at+1])".to_string() }
        else { format!("helper.copy({arguments})") };
    let provider = format!("module helper\npub fn copy({parameters}) {{ target[at] = source.u8(at) }}");
    let initialization = if nested { "Pair { left: State { rows: buffer.filled(8, 65) }, right: State { rows: buffer.filled(8, 0) } }" }
        else { "Pair { left: buffer.filled(8, 65), right: buffer.filled(8, 0) }" };
    let retained = if fresh { "" } else { &views };
    let repeated = if fresh { &views } else { "" };
    let source = format!(r#"module main
import helper
{declaration}
fn run(borrow mut pair: Pair, count: i64) {{
  {retained}
  mut at := 0
  loop {{ if at >= count {{ break }}; {repeated}; {action}; at = at + 1 }}
}}
fn check_count(count: i64) -> bool {{
  mut pair := {initialization}
  run(pair, count)
  result := {right}.bytes()
  mut at := 0
  loop {{
    if at >= 8 {{ break }}
    expected: u8 := if at < count {{ 65 }} else {{ 0 }}
    if result[at] != expected {{ return false }}
    at = at + 1
  }}
  return true
}}
fn main() -> i32 {{ if check_count(0) && check_count(1) && check_count(8) {{ 0 }} else {{ 1 }} }}
"#);
    (provider, source)
}

#[test]
fn retained_copy_loops_cross_units() {
    owned_fixture::run("retained_copy_loops_cross_units", |_| {
        for intrinsic in [false, true] {
            for reverse in [false, true] {
                for nested in [false, true] {
                    for fresh in [false, true] {
                        let (provider, source) = copy_program(intrinsic, reverse, nested, fresh);
                        let files = [("helper.align", provider.as_str()), ("main.align", source.as_str())];
                        let result = diff_check_multi("retained-copy", &files, "main.align");
                        assert!(!result.whole_errors && !result.per_unit_errors,
                            "intrinsic={intrinsic}/reverse={reverse}/nested={nested}/fresh={fresh}: {}\n{}",
                            result.whole_diags, result.per_unit_diags);
                    }
                }
            }
        }
    });
}

#[test]
fn retained_copy_native_and_cache() {
    owned_fixture::run("retained_copy_native_and_cache", |stage| {
        for intrinsic in [false, true] {
            let (provider, source) = copy_program(intrinsic, false, true, false);
            let files = [("helper.align", provider.as_str()), ("main.align", source.as_str())];
            for output in [build_and_run_multi("copy-whole", &files, "main.align"),
                build_per_unit_multi("copy-units", &files, "main.align").link_and_run()]
            { assert_eq!(output.status.code(), Some(0), "{output:?}"); }
            std::fs::write(stage.join("helper.align"), &provider).unwrap();
            std::fs::write(stage.join("main.align"), &source).unwrap();
            let entry = stage.join("main.align").to_string_lossy().into_owned();
            let cache = CacheContext::at(stage.join(format!("cache-{intrinsic}")));
            for warm in [false, true] {
                let mut sources = SourceMap::new();
                let mut package = align_driver::build_package(&mut sources, &entry, &source,
                    &cache, align_driver::UnitReuse::Allowed);
                assert!(!package.diags.has_errors(), "{}", align_driver::format_diagnostics(&sources, &package.diags));
                assert_eq!(package.units.len(), 2);
                for index in 0..package.units.len() {
                    assert_eq!(package.units[index].frontend.as_ref().unwrap().hit, warm);
                    assert!(package.materialize(index).is_ok(), "rehydrate {index}");
                }
            }
        }
    });
}
