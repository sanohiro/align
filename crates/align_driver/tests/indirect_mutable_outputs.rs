//! Plan199: a uniquely resolved function value consumes exact mutable-output facts.
#![cfg(unix)]
#[path = "helpers/owned_fixture.rs"]
mod owned_fixture;

use align_driver::{BuildTarget, Profile, check, lower_to_mir};
use align_span::SourceMap;

fn certify(source: &str) {
    let mut sources = SourceMap::new();
    let checked = check(&mut sources, "indirect-output.align", source);
    assert!(
        !checked.diags.has_errors(),
        "{}\n{source}",
        align_driver::format_diagnostics(&sources, &checked.diags)
    );
    assert!(align_sema::checked_hir_body_facts_are_valid(&checked.hir));
    let mir = lower_to_mir(&checked.hir);
    align_mir::producer::validate_mir_producers(&mir).unwrap();
    let defined = mir.fns.iter().map(|f| f.name.clone()).collect();
    align_mir::producer::validate_partition_resource_rvalues(&mir, &defined).unwrap();
    let mut sources = SourceMap::new();
    let checked = align_driver::check_per_unit(&mut sources, "indirect-output.align", source);
    assert!(
        !checked.diags.has_errors(),
        "{}\n{source}",
        align_driver::format_diagnostics(&sources, &checked.diags)
    );
}

#[test]
fn source_matrix() {
    for nested in [false, true] {
        let (ty, field, replacement) = if nested {
            (
                "Outer",
                "row.inner.values",
                "Outer { inner: Row { values: [value].to_array() } }",
            )
        } else {
            ("Row", "row.values", "Row { values: [value].to_array() }")
        };
        for body in [
            String::new(),
            format!("{field}[0] = value"),
            format!("{field} = [value].to_array()"),
            format!("row = {replacement}"),
        ] {
            for dispatch in [
                "callback := set; callback(row, 7)",
                "first := set; callback := first; callback(row, 7)",
                "callback := if flag { set } else { set }; callback(row, 7)",
            ] {
                for leaf_first in [false, true] {
                    let leaf = format!("fn set(borrow mut row: {ty}, value: i64) {{ {body} }}");
                    let caller = format!(
                        "fn exercise(borrow mut row: {ty}, flag: bool) -> i64 {{ {dispatch}; return {field}[0] }}"
                    );
                    let functions = if leaf_first {
                        format!("{leaf}\n{caller}")
                    } else {
                        format!("{caller}\n{leaf}")
                    };
                    certify(&format!(
                        "Row {{ values: array<i64> }}\nOuter {{ inner: Row }}\n{functions}\nfn main() {{}}"
                    ));
                }
            }
        }
    }
}

#[test]
fn retains_sources_and_rejects_uncertain_calls() {
    let declarations = r#"
Row { values: array<i64> }
View { text: str }
fn replace(borrow mut row: Row, value: i64) { row.values = [value].to_array() }
fn keep(borrow mut row: Row, value: i64) {}
fn put(borrow mut view: View, text: str) { view.text = text }
fn consume(value: string) {}
"#;
    for (name, body, diagnostic) in [
        (
            "replaced-source",
            "fn main(){mut owner:=\"live\".clone();mut view:=View{text:\"old\"};f:=put;f(view,owner);owner=\"new\".clone();print(view.text)}",
            "invalidated",
        ),
        (
            "moved-source",
            "fn main(){owner:=\"live\".clone();mut view:=View{text:\"old\"};f:=put;f(view,owner);consume(owner);print(view.text)}",
            "invalidated",
        ),
        (
            "short-source",
            "fn bad(borrow mut view:View){arena {n:=7;short:=template \"{n}\";f:=put;f(view,short)}}\nfn main(){}",
            "shorter-lived",
        ),
        (
            "overlap",
            "fn main(){mut view:=View{text:\"old\"};f:=put;f(view,view.text)}",
            "aliases",
        ),
        (
            "eager-replacement",
            "fn main(){mut row:=Row{values:[1].to_array()};f:=replace;f(row,{row=Row{values:[2].to_array()};7})}",
            "invalidated",
        ),
        (
            "old-allocation",
            "fn main(){mut row:=Row{values:[1].to_array()};view:=row.values[..];f:=replace;f(row,7);print(view[0])}",
            "invalidated",
        ),
        (
            "unknown-target",
            "fn apply(f:fn(borrow mut Row,i64)->()){mut row:=Row{values:[1].to_array()};f(row,7);print(row.values[0])}\nfn main(){}",
            "invalidated",
        ),
        (
            "mixed-targets",
            "fn apply(flag:bool){mut row:=Row{values:[1].to_array()};f:=if flag {keep} else {replace};f(row,7);print(row.values[0])}\nfn main(){}",
            "invalidated",
        ),
        (
            "mixed-target-old-view",
            "fn apply(flag:bool){mut row:=Row{values:[1].to_array()};view:=row.values[..];f:=if flag {keep} else {replace};f(row,7);print(view[0])}\nfn main(){}",
            "invalidated",
        ),
    ] {
        let source = format!("{declarations}\n{body}");
        for per_unit in [false, true] {
            let mut sources = SourceMap::new();
            let diags = if per_unit {
                align_driver::check_per_unit(&mut sources, "indirect-negative.align", &source).diags
            } else {
                check(&mut sources, "indirect-negative.align", &source).diags
            };
            let rendered = align_driver::format_diagnostics(&sources, &diags);
            assert!(
                diags.has_errors() && rendered.contains(diagnostic),
                "{name}/{per_unit}: {rendered}"
            );
        }
    }
}

#[test]
fn recursive_forwarding_keeps_late_retention() {
    let source = r#"
View { text: str }
fn first(borrow mut dst: View, text: str, n: i64) {
  if n <= 0 { dst.text = text; return }
  callback := second
  callback(dst, text, n - 1)
}
fn second(borrow mut dst: View, text: str, n: i64) {
  callback := first
  callback(dst, text, n)
}
fn main() {
  owner := "live".clone()
  mut view := View { text: "old" }
  callback := second
  callback(view, owner, 2)
  print(view.text)
}
"#;
    certify(source);
    let invalid = source.replace("  print(view.text)", "  consume(owner)\n  print(view.text)")
        + "\nfn consume(value:string){}\n";
    for per_unit in [false, true] {
        let mut sources = SourceMap::new();
        let diags = if per_unit {
            align_driver::check_per_unit(&mut sources, "recursive-output.align", &invalid).diags
        } else {
            check(&mut sources, "recursive-output.align", &invalid).diags
        };
        let rendered = align_driver::format_diagnostics(&sources, &diags);
        assert!(
            diags.has_errors() && rendered.contains("invalidated"),
            "{per_unit}: {rendered}"
        );
    }
}

#[test]
fn unchanged_sibling_sources_survive_mutating_calls() {
    for (shape, declaration, initializer, write, read) in [
        (
            "copy",
            "Row { number: i64, text: str }",
            "Row { number: 1, text: view }",
            "row.number = 7",
            "row.text",
        ),
        (
            "move",
            "Row { numbers: array<i64>, text: str }",
            "Row { numbers: [1].to_array(), text: view }",
            "row.numbers[0] = 7",
            "row.text",
        ),
        (
            "elements",
            "Row { numbers: array<i64>, words: array<str> }",
            "Row { numbers: [1].to_array(), words: [view].to_array() }",
            "row.numbers = [7].to_array()",
            "row.words[0]",
        ),
    ] {
        for (control, body) in [
            ("plain", write.to_owned()),
            ("if", format!("if flag {{ {write} }}")),
            (
                "match",
                format!("match if flag {{ 1 }} else {{ 0 }} {{ 1 => {{ {write} }} _ => {{}} }}"),
            ),
            ("loop", format!("loop {{ if flag {{ {write} }}; break }}")),
            ("early-return", format!("if flag {{ return }}; {write}")),
        ] {
            for indirect in [false, true] {
                let call = if indirect {
                    "callback := modify; callback(row, flag)"
                } else {
                    "modify(row, flag)"
                };
                let prefix = format!(
                    "{declaration}\nfn modify(borrow mut row: Row, flag: bool) {{ {body} }}\nfn consume(value: string) {{}}\n"
                );
                let main = format!(
                    "owner := \"alive\".clone(); view: str := owner; mut row := {initializer}; {call};"
                );
                certify(&format!(
                    "{prefix}fn run(flag: bool) {{ {main} print({read}) }}\nfn main() {{ run(false) }}"
                ));
                for per_unit in [false, true] {
                    let source = format!(
                        "{prefix}fn run(flag: bool) {{ {main} consume(owner); print({read}) }}\nfn main() {{ run(false) }}"
                    );
                    let mut sources = SourceMap::new();
                    let diags = if per_unit {
                        align_driver::check_per_unit(
                            &mut sources,
                            "sibling-negative.align",
                            &source,
                        )
                        .diags
                    } else {
                        check(&mut sources, "sibling-negative.align", &source).diags
                    };
                    let rendered = align_driver::format_diagnostics(&sources, &diags);
                    assert!(
                        diags.has_errors() && rendered.contains("invalidated"),
                        "{shape}/{control}/{indirect}/{per_unit}: {rendered}\n{source}"
                    );
                }
            }
        }
    }
}

#[test]
fn newly_formed_self_view_cannot_outlive_its_owned_field() {
    for indirect in [false, true] {
        let call = if indirect {
            "f := link; f(row)"
        } else {
            "link(row)"
        };
        let source = format!(
            r#"
Row {{ numbers: array<i64>, view: slice<i64> }}
fn link(borrow mut row: Row) {{ row.view = row.numbers[..] }}
fn main() {{
  mut row := Row {{ numbers: [1].to_array(), view: [0] }}
  {call}
  saved := row.view
  row.numbers = [2].to_array()
  print(saved[0])
}}
"#
        );
        for per_unit in [false, true] {
            let mut sources = SourceMap::new();
            let diags = if per_unit {
                align_driver::check_per_unit(&mut sources, "self-view-negative.align", &source)
                    .diags
            } else {
                check(&mut sources, "self-view-negative.align", &source).diags
            };
            let rendered = align_driver::format_diagnostics(&sources, &diags);
            assert!(
                diags.has_errors() && rendered.contains("invalidated"),
                "{indirect}/{per_unit}: {rendered}"
            );
        }
    }
}

#[test]
fn mutation_flags_cover_aliases_callbacks_and_returning_paths() {
    for (name, body, expected) in [
        ("unchanged", "_ := flag", false),
        ("out", "install(row.numbers)", true),
        ("by-value", "change(row.numbers)", true),
        (
            "open-target",
            "writer := if flag { ignore } else { action }; writer(row.numbers)",
            true,
        ),
        (
            "captured",
            "view := row.numbers[..]; writer := fn { change(view) }; writer()",
            true,
        ),
        (
            "pipeline",
            "view := row.numbers[..]; _ := [1].map(fn x { change(view); x }).sum()",
            true,
        ),
        (
            "map-err",
            "view := row.numbers[..]; result: Result<(), i64> := Err(1); _ := result.map_err(fn e: i64 { change(view); e })",
            true,
        ),
        (
            "non-returning",
            "if flag { row.numbers[0] = 7; loop {} }",
            false,
        ),
    ] {
        let source = format!(
            r#"
Row {{ numbers: array<i64>, text: str }}
fn install(out values: slice<i64>) {{ values[0] = 7 }}
fn change(values: slice<i64>) {{ mut alias := values; alias[0] = 7 }}
fn ignore(values: slice<i64>) {{}}
fn forward(borrow mut row: Row, flag: bool, action: fn(slice<i64>) -> ()) {{ {body} }}
fn main() {{}}
"#
        );
        let mut sources = SourceMap::new();
        let checked = check(&mut sources, "mutation-flags.align", &source);
        assert!(
            !checked.diags.has_errors(),
            "{name}: {}",
            align_driver::format_diagnostics(&sources, &checked.diags)
        );
        let function = checked
            .hir
            .fns
            .iter()
            .find(|function| function.name == "forward")
            .unwrap();
        assert_eq!(
            function.mutable_retention.as_ref().unwrap()[0].may_mutate,
            expected,
            "{name}"
        );
        assert!(align_sema::checked_hir_body_facts_are_valid(&checked.hir));
    }
}

#[test]
fn out_forwarding_retains_imported_input() {
    owned_fixture::run("out_forwarding_retains_imported_input", |stage| {
        std::fs::write(
            stage.join("helper.align"),
            r#"module helper
fn install(out values: slice<str>, value: str) { values[0] = value }
fn forward(borrow mut values: array<str>, value: str) { install(values, value) }
pub fn run(borrow mut values: array<str>, value: str) { forward(values, value) }
"#,
        )
        .unwrap();
        for indirect in [false, true] {
            let call = if indirect {
                "f := helper.run; f(values, owner)"
            } else {
                "helper.run(values, owner)"
            };
            for invalid in [false, true] {
                let end = if invalid { "consume(owner)" } else { "" };
                let source = format!(
                    r#"import helper
fn consume(value: string) {{}}
fn main() {{ owner := "live".clone(); mut values := ["old"].to_array(); {call}; {end}; print(values[0]) }}
"#
                );
                let entry = stage.join("main.align");
                std::fs::write(&entry, &source).unwrap();
                for per_unit in [false, true] {
                    let mut sources = SourceMap::new();
                    let diags = if per_unit {
                        align_driver::check_per_unit(&mut sources, entry.to_str().unwrap(), &source)
                            .diags
                    } else {
                        check(&mut sources, entry.to_str().unwrap(), &source).diags
                    };
                    let rendered = align_driver::format_diagnostics(&sources, &diags);
                    assert_eq!(
                        diags.has_errors(),
                        invalid,
                        "{indirect}/{invalid}/{per_unit}: {rendered}"
                    );
                    if invalid {
                        assert!(rendered.contains("invalidated"), "{rendered}");
                    }
                }
            }
        }
    });
}

const MODEL: &str = r#"module model
pub Row { numbers: array<i64>, words: array<str> }
pub fn patch(borrow mut row: Row, number: i64, word: str) {
  row.numbers[0] = number
  row.words[0] = word
}
pub fn replace(borrow mut row: Row, number: i64, word: str) {
  row = Row { numbers: [number].to_array(), words: [word].to_array() }
}
pub fn keep(borrow mut row: Row, ignored: str) {}
pub fn number(borrow mut row: Row, value: i64) { row.numbers[0] = value }
pub fn generic<T>(borrow mut row: Row, marker: T) {
  callback := number
  callback(row, 17)
}
fn guard(fail: bool) -> Result<(), i64> { if fail { return Err(7) }; return Ok(()) }
fn translate(value: i64) -> i64 = value + 1
pub fn run(borrow mut row: Row, word: str, fail: bool) -> Result<i64, i64> {
  callback := replace
  callback(row, 9, word)
  generic(row, 1)
  noop := keep
  noop(row, "ignored")
  guard(fail).map_err(translate)?
  return Ok(row.numbers[0])
}
"#;
const MAIN: &str = r#"import model
extern "C" fn align_rt_requested_live_reset()
extern "C" fn align_rt_requested_live_bytes() -> i64
extern "C" fn align_rt_alloc_count() -> i64
extern "C" fn align_rt_free_count() -> i64
fn exercise(fail: bool) -> Result<i64, i64> {
  owner := "alive".clone()
  next_owner := "next".clone()
  mut row := model.Row { numbers: [1].to_array(), words: ["old"].to_array() }
  if unsafe { align_rt_requested_live_bytes() } <= 0 { return Err(1) }
  before_alloc := unsafe { align_rt_alloc_count() }
  before_free := unsafe { align_rt_free_count() }
  patch := model.patch
  patch(row, 7, owner)
  number := model.number
  mut index := 0
  loop { if index == 3 { break }; number(row, 7); index = index + 1 }
  if row.numbers[0] != 7 || row.words[0] != "alive" { return Err(2) }
  if unsafe { align_rt_alloc_count() } != before_alloc || unsafe { align_rt_free_count() } != before_free { return Err(3) }
  run := model.run
  result := run(row, next_owner, fail)?
  if result != 17 || row.words[0] != "next" { return Err(4) }
  return Ok(result)
}
fn main() -> i32 {
  unsafe { align_rt_requested_live_reset() }
  baseline := unsafe { align_rt_requested_live_bytes() }
  if (exercise(false) else { return 1 }) != 17 { return 2 }
  if unsafe { align_rt_requested_live_bytes() } != baseline { return 3 }
  match exercise(true) { Ok(_) => { return 4 } Err(code) => { if code != 8 { return 5 } } }
  if unsafe { align_rt_requested_live_bytes() } != baseline { return 6 }
  return 0
}
"#;

fn execute(stage: &std::path::Path, programs: &[align_mir::Program], label: &str) {
    let mut objects = Vec::new();
    let mut libraries = Vec::new();
    for (index, program) in programs.iter().enumerate() {
        let object = stage.join(format!("{label}-{index}.o"));
        align_driver::emit_object_file(
            program,
            &object,
            BuildTarget::Baseline,
            Profile::Release,
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
        &objects
            .iter()
            .map(|path| path.as_path())
            .collect::<Vec<_>>(),
        &binary,
        &libraries,
        Profile::Release,
    )
    .unwrap();
    let output = std::process::Command::new(binary).output().unwrap();
    assert!(
        output.status.success() && output.stdout.is_empty() && output.stderr.is_empty(),
        "{label}: {output:?}"
    );
}

#[test]
fn native_lifecycle_and_replay() {
    owned_fixture::run("native_lifecycle_and_replay", |stage| {
        let model = stage.join("model.align");
        let entry = stage.join("main.align");
        std::fs::write(&model, MODEL).unwrap();
        std::fs::write(&entry, MAIN).unwrap();
        let entry = entry.to_str().unwrap();
        let mut sources = SourceMap::new();
        let checked = check(&mut sources, entry, MAIN);
        assert!(
            !checked.diags.has_errors(),
            "{}",
            align_driver::format_diagnostics(&sources, &checked.diags)
        );
        execute(stage, &[lower_to_mir(&checked.hir)], "whole");
        let context = align_driver::CacheContext::at(stage.join("cache"));
        let mut original = Vec::new();
        for round in 0..4 {
            let body = if round == 2 {
                MODEL.replace("  row.numbers[0] = number", "  row.numbers[0] = number + 0")
            } else {
                MODEL.to_owned()
            };
            std::fs::write(&model, body).unwrap();
            let mut sources = SourceMap::new();
            let mut built = align_driver::build_package(
                &mut sources,
                entry,
                MAIN,
                &context,
                align_driver::UnitReuse::Allowed,
            );
            assert!(
                !built.diags.has_errors(),
                "{}",
                align_driver::format_diagnostics(&sources, &built.diags)
            );
            assert_eq!(built.units.len(), 2);
            if round < 2 {
                assert!(
                    built
                        .units
                        .iter()
                        .all(|unit| unit.frontend.as_ref().unwrap().hit == (round == 1))
                );
            }
            let programs = (0..built.units.len())
                .map(|index| built.materialize(index).unwrap().clone())
                .collect::<Vec<_>>();
            let snapshot = programs
                .iter()
                .map(align_mir::print::program_to_string)
                .collect::<Vec<_>>();
            if round == 0 {
                original = snapshot;
            } else if round == 2 {
                assert_ne!(snapshot, original);
            } else {
                assert_eq!(snapshot, original);
            }
            execute(stage, &programs, &format!("replay-{round}"));
        }
    });
}
