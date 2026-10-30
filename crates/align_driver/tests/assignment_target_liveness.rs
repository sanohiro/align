//! Plan171: indexed writes observe target backing after all eager operands.
#![cfg(unix)]
#[path = "helpers/owned_fixture.rs"]
mod owned_fixture;
use align_driver::{BuildTarget, Profile, check};
use align_sema::hir;
use align_span::SourceMap;
use std::path::Path;

fn assert_check(source: &str, valid: bool, label: &str) {
    let mut sources = SourceMap::new();
    let checked = check(&mut sources, "assignment-target.align", source);
    let diagnostics = align_driver::format_diagnostics(&sources, &checked.diags);
    assert_eq!(
        !checked.diags.has_errors(),
        valid,
        "{label}: {diagnostics}\n{source}"
    );
    if !valid {
        assert!(
            diagnostics.contains("use of moved value") || diagnostics.contains("borrow")
                || diagnostics.contains("value snapshot was invalidated"),
            "{label}: expected target lifetime diagnostic, got {diagnostics}"
        );
    }
}

#[test]
fn eager_moves_reject() {
    // Each row is a distinct admitted AssignIndex/AssignElemField/AssignElem place.
    for (label, declarations, init, target, result) in [
        (
            "dynamic",
            "fn consume(values: array<i64>) -> i64 = 0",
            "mut values := [1, 2].to_array()",
            "values[INDEX]",
            "9",
        ),
        (
            "fixed-field",
            "State { text: string, counts: [i64; 1] }\nfn consume(values: State) -> i64 = 0",
            "mut values := State {text: \"old\".clone(), counts: [1]}",
            "values.counts[INDEX]",
            "9",
        ),
        (
            "nested-fixed-field",
            "Inner { counts: [i64; 1] }\nState { text: string, inner: Inner }\nfn consume(values: State) -> i64 = 0",
            "mut values := State {text: \"old\".clone(), inner: Inner {counts: [1]}}",
            "values.inner.counts[INDEX]",
            "9",
        ),
        (
            "dynamic-struct-field",
            "Entry { n: i64 }\nfn consume(values: array<Entry>) -> i64 = 0",
            "mut values := [Entry {n: 1}].to_array()",
            "values[INDEX].n",
            "9",
        ),
        (
            "fixed-move-field",
            "Entry { text: string }\nfn consume(values: [Entry; 1]) -> i64 = 0",
            "mut values := [Entry {text: \"old\".clone()}]",
            "values[INDEX].text",
            "\"new\".clone()",
        ),
        (
            "fixed-move-element",
            "Entry { text: string }\nfn consume(values: [Entry; 1]) -> i64 = 0",
            "mut values := [Entry {text: \"old\".clone()}]",
            "values[INDEX]",
            "Entry {text: \"new\".clone()}",
        ),
    ] {
        for index_moves in [false, true] {
            for nested in [false, true] {
                for large_index in [false, true] {
                    let padding = if large_index {
                        " + 0 + 0 + 0 + 0 + 0 + 0"
                    } else {
                        ""
                    };
                    let index = if index_moves {
                        format!("consume(values){padding}")
                    } else {
                        format!("0{padding}")
                    };
                    let value = if index_moves {
                        result.to_owned()
                    } else {
                        format!("{{ consume(values); {result} }}")
                    };
                    let statement = format!("{} = {value}", target.replace("INDEX", &index));
                    let statement = if nested {
                        format!("unused := {{ {{ {statement} }} }}")
                    } else {
                        statement
                    };
                    assert_check(
                        &format!("{declarations}\nfn main() {{ {init}; {statement} }}"),
                        false,
                        &format!(
                            "{label}/index={index_moves}/nested={nested}/large-index={large_index}"
                        ),
                    );
                }
            }
        }
    }
}

#[test]
fn eager_owner_invalidation_rejects() {
    for action in [
        "consume(owner)",
        "{ owner = [3, 4].to_array(); 0 }",
        "if flag { consume(owner) } else { 0 }",
        "loop { break consume(owner) }",
    ] {
        for index_action in [false, true] {
            for nested in [false, true] {
                let (index, value) = if index_action {
                    (action, "9")
                } else {
                    ("0", action)
                };
                let statement = format!("values[{index}] = {value}");
                let statement = if nested {
                    format!("unused := {{ {statement} }}")
                } else {
                    statement
                };
                assert_check(
                    &format!(
                        "fn consume(values: array<i64>) -> i64 = 0\nfn run(flag: bool) {{ mut owner := [1,2].to_array(); mut values := owner[..]; {statement} }}\nfn main() {{}}"
                    ),
                    false,
                    action,
                );
            }
        }
    }
}

#[test]
fn conservative_view_backing_survives_without_header_metadata() {
    for (materializer, writable) in [
        ("copied := [Bytes {data: input}].to_array(); mut view := copied[0].data", true),
        ("mut builder: array_builder<Bytes> := array_builder(out); builder.push(Bytes {data: input}); copied := builder.build(); mut view := copied[0].data", true),
        ("result := [1].reduce(Bytes {data: input}, keep); mut view := result.data", false),
        ("mut view := choose_bytes(Bytes {data: input})", false),
    ] {
        for invalidates in [false, true] {
            for in_index in [false, true] {
                let operand = if invalidates { "consume(owner)" } else { "0" };
                let (index, value) = if in_index {
                    (operand.to_owned(), "65 as u8".to_owned())
                } else {
                    ("0".to_owned(), format!("{{ {operand}; 65 as u8 }}"))
                };
                let source = format!(
                    "Bytes {{ data: slice<u8> }}\nfn keep(acc: Bytes, value: i64) -> Bytes = acc\nfn choose_bytes(value: Bytes) -> slice<u8> = value.data\nfn consume(value: array<u8>) -> i64 = 0\nfn main() {{ owner := \"xx\".bytes().to_array(); input: slice<u8> := owner; arena out {{ {materializer}; view[{index}] = {value} }} }}"
                );
                if writable {
                    assert_check(&source, !invalidates, materializer);
                } else {
                    let mut sources = SourceMap::new();
                    let checked = check(&mut sources, "returned-view-write.align", &source);
                    let diagnostics = align_driver::format_diagnostics(&sources, &checked.diags);
                    assert!(checked.diags.has_errors() && diagnostics.contains("read-only"),
                        "{materializer}: {diagnostics}");
                    // Returned views still retain their source lifetime, even though the call
                    // boundary now forbids writing through them. Exercise that axis by reading.
                    let read = source.replace(&format!("view[{index}] = {value}"),
                        &format!("_ := view[{{ {operand}; 0 }}]"));
                    assert_check(&read, !invalidates, materializer);
                }
            }
        }
    }
}

#[test]
fn eager_control_paths() {
    for operand in [
        "consume(values)",
        "{ consume(values); 0 }",
        "if flag { consume(values) } else { 0 }",
        "match (if flag { 1 } else { 0 }) { 1 => consume(values), _ => 0 }",
        "loop { break consume(values) }",
        "maybe(values) else 0",
        "fallible(values)?",
        "fallible(values).map_err(fn error: i64 { error })?",
    ] {
        for in_index in [false, true] {
            let (index, value) = if in_index {
                (operand, "9")
            } else {
                ("0", operand)
            };
            let source = format!(
                "fn consume(values: array<i64>) -> i64 = 0\nfn maybe(values: array<i64>) -> Option<i64> = Some(0)\nfn fallible(values: array<i64>) -> Result<i64, i64> = Ok(0)\nfn run(flag: bool) -> Result<(), i64> {{ mut values := [1,2].to_array(); values[{index}] = {value}; return Ok(()) }}\nfn main() {{}}"
            );
            assert_check(&source, false, operand);
        }
    }
}

const MODEL: &str = r#"module model
pub fn consume(values: array<i64>) -> i64 = 0
pub fn text(value: string) -> i64 = 9
pub fn static_text(value: string) -> str = "new"
pub fn guard(fail: bool) -> Result<i64, i64> { if fail { return Err(7) }; return Ok(0) }
"#;

const VALID: &str = r#"module main
import model
State { text: string, counts: [i64; 1] }
Entry { text: string }
Point { x: i64, y: i64 }
fn terminate() -> i64 {
  mut values := [1, 2].to_array()
  values[model.consume(values)] = { return 17; 9 }
  return 1
}
fn break_child() -> i64 {
  mut values := [1, 2].to_array()
  return loop { values[model.consume(values)] = { break 19; 9 } }
}
fn fail_child() -> Result<i64, i64> {
  mut values := [1, 2].to_array()
  values[model.consume(values)] = { model.guard(true)?; return Ok(23); 9 }
  return Ok(1)
}
fn main() -> i32 {
  mut state := State {text: "old".clone(), counts: [1]}
  state.counts[0] = model.text(state.text)
  if state.counts[0] != 9 { return 1 }
  text := "old".clone()
  mut fixed: [str; 1] := [text[..]]
  fixed[0] = model.static_text(text)
  if fixed[0] != "new" { return 2 }
  second_text := "old".clone()
  original: [str; 1] := [second_text[..]]
  mut view := original[..]
  view[0] = model.static_text(second_text)
  mut values := [1,2].to_array()
  mut order := 0
  values[{ order = order + 1; model.consume(values) }] = {
    if order != 1 { return 3 }
    order = order + 1
    values = [3,4].to_array()
    9
  }
  if order != 2 || values[0] != 9 || values[1] != 4 { return 4 }
  first := [1,2].to_array()
  second := [3,4].to_array()
  mut rebound := first[..]
  rebound[model.consume(first)] = { rebound = second[..]; 9 }
  if rebound[0] != 9 || second[1] != 4 { return 5 }
  mut records := [Entry {text: "old".clone()}]
  records[0] = Entry {text: "new".clone()}
  records[0].text = "fresh".clone()
  if records[0].text != "fresh" { return 6 }
  mut at := 0
  loop {
    if at == 3 { break }
    mut local := [1,2].to_array()
    unused := { local[model.consume(local)] = { local = [5,6].to_array(); 7 } }
    if local[0] != 7 || local[1] != 6 { return 7 }
    at = at + 1
  }
  arena {
    mut rows := [Point {x: 1, y: 2}].to_soa()
    rows[0].x = 7
    unused := { rows[0] = Point {x: 8, y: 9} }
    if rows[0].x != 8 || rows[0].y != 9 { return 11 }
  }
  if terminate() != 17 || break_child() != 19 { return 8 }
  match fail_child() { Ok(_) => { return 9 }, Err(error) => { if error != 7 { return 10 } } }
  return 0
}
"#;

fn execute(stage: &Path, programs: &[align_mir::Program], label: &str) {
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
    // The parent-owned fixture bounds compilation and this entire descendant process group.
    let output = std::process::Command::new(binary).output().unwrap();
    assert!(
        output.status.success() && output.stdout.is_empty() && output.stderr.is_empty(),
        "{label}: {output:?}"
    );
}

#[test]
fn valid_replacements_execute() {
    owned_fixture::run("valid_replacements_execute", |stage| {
        std::fs::write(stage.join("model.align"), MODEL).unwrap();
        let entry = stage.join("main.align");
        std::fs::write(&entry, VALID).unwrap();
        let mut sources = SourceMap::new();
        let checked = check(&mut sources, entry.to_str().unwrap(), VALID);
        assert!(
            !checked.diags.has_errors(),
            "{}",
            align_driver::format_diagnostics(&sources, &checked.diags)
        );
        let program =
            align_mir::lower_program_checked(&checked.hir, false, Some(&sources)).unwrap();
        execute(stage, &[program], "whole");
        let mut sources = SourceMap::new();
        let built = align_driver::build_per_unit(&mut sources, entry.to_str().unwrap(), VALID);
        assert!(
            !built.diags.has_errors(),
            "{}",
            align_driver::format_diagnostics(&sources, &built.diags)
        );
        execute(
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
            let mut sources = SourceMap::new();
            let mut built = align_driver::build_package(
                &mut sources,
                entry.to_str().unwrap(),
                VALID,
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
                .map(|index| built.materialize(index).unwrap().clone())
                .collect::<Vec<_>>();
            if round == 1 {
                execute(stage, &programs, "cache-hit");
            }
        }
    });
}

#[test]
fn imported_assignment_liveness() {
    owned_fixture::run("imported_assignment_liveness", |stage| {
        std::fs::write(stage.join("model.align"), MODEL).unwrap();
        let entry = stage.join("main.align");
        for expression in [
            "values[model.consume(values)] = 9",
            "values[0] = model.consume(values)",
        ] {
            let source = format!(
                "module main\nimport model\nfn main() {{ mut values := [1,2].to_array(); {expression} }}"
            );
            std::fs::write(&entry, &source).unwrap();
            for per_unit in [false, true] {
                let mut sources = SourceMap::new();
                let diagnostics = if per_unit {
                    align_driver::check_per_unit(&mut sources, entry.to_str().unwrap(), &source)
                        .diags
                } else {
                    check(&mut sources, entry.to_str().unwrap(), &source).diags
                };
                assert!(diagnostics.has_errors());
                assert!(
                    align_driver::format_diagnostics(&sources, &diagnostics)
                        .contains("use of moved value 'values'")
                );
            }
        }
    });
}

#[test]
fn forged_assignment_replay_rejects() {
    for in_index in [false, true] {
        let source = "fn consume(values: array<i64>) -> i64 = 0\nfn run() { mut values := [1,2].to_array(); values[0] = 9; consume(values); return }\nfn main() {}";
        let mut sources = SourceMap::new();
        let checked = check(&mut sources, "forged-assignment.align", source);
        assert!(
            !checked.diags.has_errors(),
            "{}",
            align_driver::format_diagnostics(&sources, &checked.diags)
        );
        assert!(align_sema::checked_hir_body_facts_are_valid(&checked.hir));
        let mut bad = checked.hir.clone();
        let function = bad
            .fns
            .iter_mut()
            .find(|function| function.name == "run")
            .unwrap();
        let call_position = function.body.stmts.len() - 2;
        let call = match function.body.stmts.remove(call_position) {
            hir::Stmt::Expr(call) => call,
            other => panic!("expected consuming call, got {other:?}"),
        };
        let hir::Stmt::AssignIndex { index, value, .. } = &mut function.body.stmts[1] else {
            panic!("fixture lost indexed assignment");
        };
        if in_index {
            *index = call;
        } else {
            *value = call;
        }
        assert!(!align_sema::checked_hir_body_facts_are_valid(&bad));
        for per_unit in [false, true] {
            assert!(align_mir::lower_program_checked(&bad, per_unit, Some(&sources)).is_err());
        }
    }
}
