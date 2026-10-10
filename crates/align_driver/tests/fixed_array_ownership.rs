//! Plan163: fixed Move-record arrays retain ownership through their containing types.
#![cfg(unix)]
#[path = "helpers/owned_fixture.rs"]
mod owned_fixture;
use align_driver::{
    BuildTarget, Profile, build_per_unit, check, emit_object_file, link_objects, lower_to_mir,
};
use align_span::SourceMap;
use std::path::Path;

const MODEL: &str = r#"module model
pub Item { text: string }
pub Batch<T> { marker: T, values: [Item; 2] }
pub Nested { batches: [Batch<i64>; 1] }
pub Empty { values: [Item; 0] }
pub Choice { Full(Batch<i64>), Empty(Empty) }
pub UnitCells { marker: i64, units: [(); 4] }
pub UnitChoice { Value(UnitCells) }
pub fn make() -> Batch<i64> {
  return Batch { marker: 0, values: [Item { text: "alice".clone() }, Item { text: "bob".clone() }] }
}
pub fn identity<T>(value: T) -> T = value
pub fn count(borrow value: Batch<i64>) -> i64 = value.values[0].text.len() + value.values[1].text.len()
fn guard(fail: bool) -> Result<(), i64> { if fail { return Err(7) }; return Ok(()) }
fn map_error(error: i64) -> i64 = error
pub fn fallible(fail: bool) -> Result<Batch<i64>, i64> {
  value := make()
  guard(fail).map_err(map_error)?
  return Ok(value)
}
fn word(fail: bool) -> Result<string, i64> { guard(fail)?; return Ok("bob".clone()) }
pub fn partial(fail: bool) -> Result<i64, i64> {
  value := Batch { marker: 0, values: [Item { text: "alice".clone() }, Item { text: word(fail)? }] }
  return Ok(value.values[0].text.len() + value.values[1].text.len())
}
pub fn chosen(flag: bool) -> Batch<i64> = if flag { identity(make()) } else { make() }
"#;

const MAIN: &str = r#"module main
import model
WithSibling { batch: model.Batch<i64>, other: string }
extern "C" fn align_rt_requested_live_reset()
extern "C" fn align_rt_requested_live_bytes() -> i64
extern "C" fn align_rt_alloc_count() -> i64
extern "C" fn align_rt_free_count() -> i64
fn live() -> i64 = unsafe { align_rt_requested_live_bytes() }
fn moved() -> i32 {
  mut value := model.make()
  if live() != 8 || model.count(value) != 8 { return 1 }
  value = model.chosen(true)
  if live() != 8 || model.count(value) != 8 { return 2 }
  transferred := model.identity(value)
  if live() != 8 || model.count(transferred) != 8 { return 3 }
  return 0
}
fn nested() -> i32 {
  value := model.Nested { batches: [model.Batch { marker: 0, values: [model.Item { text: "alice".clone() }, model.Item { text: "bob".clone() }] }] }
  if live() != 8 || value.batches.len() != 1 { return 5 }
  return 0
}
fn sibling() -> i32 {
  value := WithSibling { batch: model.make(), other: "extra".clone() }
  if live() != 13 || value.other.len() != 5 { return 7 }
  return 0
}
fn fallible(fail: bool) -> i32 {
  value := model.fallible(fail) else { model.chosen(false) }
  if live() != 8 || model.count(value) != 8 { return 9 }
  return 0
}
fn optional() -> i32 {
  value: Option<model.Batch<i64>> := Some(model.make())
  if live() != 8 { return 17 }
  return 0
}
fn failure() -> i32 {
  value: Result<(), model.Batch<i64>> := Err(model.make())
  if live() != 8 { return 19 }
  return 0
}
fn choice() -> i32 {
  value := model.Choice.Full(model.make())
  match value { Full(batch) => { if model.count(batch) != 8 { return 21 } } Empty(empty) => { return 22 } }
  return 0
}
fn empty() {
  value := model.Empty { values: [] }
  optional: Option<model.Empty> := Some(value)
  success: Result<model.Empty, i64> := Ok(model.Empty { values: [] })
  failure: Result<(), model.Empty> := Err(model.Empty { values: [] })
  choice := model.Choice.Empty(model.Empty { values: [] })
  units := model.UnitChoice.Value(model.UnitCells { marker: 7, units: [(), (), (), ()] })
  match units { Value(cells) => { print(cells.marker + cells.units.len()) } }
}
fn exercise() -> i32 {
  mut status := moved()
  if status != 0 { return status }
  if live() != 0 { return 4 }
  status = nested()
  if status != 0 { return status }
  if live() != 0 { return 6 }
  status = sibling()
  if status != 0 { return status }
  if live() != 0 { return 8 }
  mut iteration := 0
  loop {
    if iteration == 2 { break }
    status = fallible(iteration == 1)
    if status != 0 { return status }
    if live() != 0 { return 10 }
    iteration = iteration + 1
  }
  match model.partial(true) { Ok(value) => { return 11 } Err(error) => { if error != 7 { return 12 } } }
  if live() != 0 { return 13 }
  match model.partial(false) { Ok(value) => { if value != 8 { return 14 } } Err(error) => { return 15 } }
  if live() != 0 { return 16 }
  status = optional()
  if status != 0 { return status }
  if live() != 0 { return 18 }
  status = failure()
  if status != 0 { return status }
  if live() != 0 { return 20 }
  status = choice()
  if status != 0 { return status }
  if live() != 0 { return 23 }
  empty()
  return 0
}
fn main() -> i32 {
  unsafe { align_rt_requested_live_reset() }
  allocations := unsafe { align_rt_alloc_count() }
  frees := unsafe { align_rt_free_count() }
  status := exercise()
  if status != 0 { return status }
  allocated := unsafe { align_rt_alloc_count() } - allocations
  freed := unsafe { align_rt_free_count() } - frees
  if allocated != 24 { return 30 }
  if allocated != freed || live() != 0 { return 31 }
  return 0
}
"#;

fn execute(stage: &Path, programs: &[align_mir::Program], label: &str) {
    let mut objects = Vec::new();
    let mut libraries = Vec::new();
    for (index, program) in programs.iter().enumerate() {
        let object = stage.join(format!("{label}-{index}.o"));
        emit_object_file(
            program,
            &object,
            BuildTarget::Baseline,
            Profile::Release,
            &[],
            false,
        )
        .unwrap_or_else(|error| panic!("{label}: {error}"));
        objects.push(object);
        for library in &program.link_libs {
            if !libraries.contains(library) {
                libraries.push(library.clone());
            }
        }
    }
    let executable = stage.join(format!("run-{label}"));
    link_objects(
        &align_driver::CDriver::default(),
        &objects.iter().map(|p| p.as_path()).collect::<Vec<_>>(),
        &executable,
        &libraries,
        Profile::Release,
    )
    .unwrap();
    // The parent-owned fixture bounds this entire compile/link/run process group.
    let output = std::process::Command::new(executable).output().unwrap();
    assert!(output.status.success(), "{label}: {output:?}");
    assert!(
        output.stdout == b"11\n" && output.stderr.is_empty(),
        "{label}: {output:?}"
    );
}

#[test]
fn fixed_array_ownership_lifecycle_and_replay() {
    owned_fixture::run("fixed_array_ownership_lifecycle_and_replay", |stage| {
        assert!(
            align_driver::backend_available(),
            "native ownership owner requires LLVM"
        );
        std::fs::write(stage.join("model.align"), MODEL).unwrap();
        let entry = stage.join("main.align");
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
        let mut sources = SourceMap::new();
        let built = build_per_unit(&mut sources, entry, MAIN);
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
        let context = align_driver::CacheContext::at(stage.join("cache"));
        let mut previous = Vec::new();
        for round in 0..2 {
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
            for unit in &built.units {
                assert_eq!(
                    unit.frontend.as_ref().unwrap().hit,
                    round == 1,
                    "{}",
                    unit.unit
                );
            }
            let programs = (0..built.units.len())
                .map(|i| built.materialize(i).unwrap().clone())
                .collect::<Vec<_>>();
            let snapshot = programs
                .iter()
                .map(align_mir::print::program_to_string)
                .collect::<Vec<_>>();
            if round == 0 {
                previous = snapshot;
            } else {
                assert_eq!(previous, snapshot);
                execute(stage, &programs, "replay");
            }
        }
    });
}

#[test]
fn fixed_array_ownership_rejects_copy_and_borrow_consumption() {
    owned_fixture::run(
        "fixed_array_ownership_rejects_copy_and_borrow_consumption",
        |stage| {
            for (name, ty, value) in [
                (
                    "record",
                    "Batch",
                    "Batch { values: [Item { text: \"x\".clone() }] }",
                ),
                (
                    "option",
                    "Option<Batch>",
                    "Some(Batch { values: [Item { text: \"x\".clone() }] })",
                ),
                (
                    "result",
                    "Result<Batch, i64>",
                    "Ok(Batch { values: [Item { text: \"x\".clone() }] })",
                ),
                (
                    "sum",
                    "Choice",
                    "Choice.Value(Batch { values: [Item { text: \"x\".clone() }] })",
                ),
                ("empty", "Empty", "Empty { values: [] }"),
                (
                    "field",
                    "Batch",
                    "Batch { values: [Item { text: \"x\".clone() }] }",
                ),
            ] {
                for borrowed in [false, true] {
                    let body = if borrowed {
                        format!("fn bad(borrow value: {ty}) {{ take(value) }}\nfn main() {{}}")
                    } else {
                        format!("fn main() {{ value: {ty} := {value}; take(value); take(value) }}")
                    };
                    let body = if name == "field" {
                        if borrowed {
                            "fn bad(borrow value: Batch) { taken := value.values }\nfn main() {}"
                                .into()
                        } else {
                            format!("fn main() {{ value := {value}; taken := value.values }}")
                        }
                    } else {
                        body
                    };
                    let source = format!(
                        "Item {{ text: string }}\nBatch {{ values: [Item; 1] }}\nEmpty {{ values: [Item; 0] }}\nChoice {{ Value(Batch), Other }}\nfn take(value: {ty}) {{}}\n{body}\n"
                    );
                    let entry = stage.join(format!("{name}-{borrowed}.align"));
                    std::fs::write(&entry, &source).unwrap();
                    for per_unit in [false, true] {
                        let mut sources = SourceMap::new();
                        let diags = if per_unit {
                            build_per_unit(&mut sources, entry.to_str().unwrap(), &source).diags
                        } else {
                            check(&mut sources, entry.to_str().unwrap(), &source).diags
                        };
                        let text = align_driver::format_diagnostics(&sources, &diags);
                        assert!(
                            diags.has_errors()
                                && (text.contains("move")
                                    || text.contains("Move")
                                    || text.contains("borrow")),
                            "{name}/{borrowed}/{per_unit}: {text}"
                        );
                        assert!(
                            !text.contains("expected identifier") && !text.contains("unknown"),
                            "{text}"
                        );
                    }
                }
            }
        },
    );
}
