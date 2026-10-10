//! Plan164: fixed owned-string arrays transfer and release their inline elements.
#![cfg(unix)]
#[path = "helpers/owned_fixture.rs"]
mod owned_fixture;
use align_driver::{
    BuildTarget, Profile, build_per_unit, check, emit_object_file, link_objects, lower_to_mir,
};
use align_span::SourceMap;
use std::path::Path;

const MODEL: &str = r#"module model
pub Batch<T> { marker: T, values: [string; 2] }
pub Nested { batches: [Batch<i64>; 1] }
pub Empty { values: [string; 0] }
pub Generic<T> { marker: T, values: [T; 2] }
pub fn generic_strings() -> Generic<string> = Generic { marker: "tag".clone(), values: ["alice".clone(), "bob".clone()] }
pub fn at(borrow values: [string; 2], index: i64) -> str = values[index]
pub fn same_view<T>(value: T) -> T = value
pub Choice { Full(Batch<i64>), Empty(Empty) }
pub fn make() -> Batch<i64> {
  return Batch { marker: 0, values: ["alice".clone(), "bob".clone()] }
}
pub fn make_array() -> [string; 2] {
  values := ["alice".clone(), "bob".clone()]
  return values
}
pub fn first(values: slice<string>) -> str = values[0]
pub fn view_count(values: slice<string>) -> i64 = values.len()
pub fn identity<T>(value: T) -> T = value
pub fn count(borrow value: Batch<i64>) -> i64 = value.values[0].len() + value.values[1].len()
fn guard(fail: bool) -> Result<(), i64> { if fail { return Err(7) }; return Ok(()) }
fn map_error(error: i64) -> i64 = error
pub fn fallible(fail: bool) -> Result<Batch<i64>, i64> {
  value := make()
  guard(fail).map_err(map_error)?
  return Ok(value)
}
pub fn word(fail: bool) -> Result<string, i64> { guard(fail)?; return Ok("bob".clone()) }
pub fn partial(fail: bool) -> Result<i64, i64> {
  value := Batch { marker: 0, values: ["alice".clone(), word(fail)?] }
  return Ok(value.values[0].len() + value.values[1].len())
}
pub fn partial_bound(fail: bool) -> Result<i64, i64> {
  first := "alice".clone()
  values := [first, word(fail)?]
  return Ok(values[0].len() + values[1].len())
}
pub fn partial_wrapped(fail: bool) -> Result<i64, i64> {
  first := "alice".clone()
  values := [unsafe { first }, word(fail)?]
  return Ok(values[0].len() + values[1].len())
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
fn root() -> i32 {
  values := model.make_array()
  if live() != 8 || model.first(values) != "alice" { return 40 }
  duplicated := [values[0], values[0]]
  if duplicated[1] != "alice" { return 69 }
  view := values[..]
  if view[1] != "bob" || model.view_count(view) != 2 { return 41 }
  if model.view_count([]) != 0 { return 42 }
  return 0
}
fn moved() -> i32 {
  mut value := model.make()
  if live() != 8 || model.count(value) != 8 { return 1 }
  value = model.chosen(true)
  if live() != 8 || model.count(value) != 8 { return 2 }
  transferred := model.identity(value)
  if live() != 8 || model.count(transferred) != 8 { return 3 }
  return 0
}
fn replace_root(flag: bool) -> i32 {
  mut values := model.make_array()
  values = values
  values = if flag { values } else { model.make_array() }
  if live() != 8 || values[1] != "bob" { return 44 }
  return 0
}
fn replace_field() -> i32 {
  mut value := model.make()
  value.values = ["abcde".clone(), "xyz".clone()]
  if live() != 8 || value.values[0] != "abcde" { return 45 }
  return 0
}
fn replace_partial(fail: bool) -> Result<i64, i64> {
  mut values := model.make_array()
  values = ["alice".clone(), model.word(fail)?]
  return Ok(values[0].len() + values[1].len())
}
fn selected(flag: bool) -> i32 {
  text := "selected".clone()
  values := [if flag { text } else { text }]
  if values[0] != "selected" || live() != 8 { return 46 }
  return 0
}
fn generic_and_views() -> i32 {
  value := model.generic_strings()
  if live() != 11 || value.values[0] != "alice" { return 63 }
  view := value.values[1..]
  read := fn { view[0].len() }
  forwarded := model.same_view(view)
  if forwarded[0] != "bob" { return 68 }
  if read() != 3 || model.at(value.values, 1) != "bob" { return 64 }
  copied := view[0].clone()
  if copied != "bob" || live() != 14 { return 65 }
  if model.first(["literal".clone()]) != "literal" { return 66 }
  return 0
}
fn replacement_controls() -> i32 {
  mut numbers := [1, 2]
  numbers = [3, 4]
  mut views := ["a", "b"]
  views = ["c", "d"]
  mut records := [model.Batch { marker: 0, values: ["alice".clone(), "bob".clone()] }]
  records = [model.Batch { marker: 1, values: ["abcde".clone(), "xyz".clone()] }]
  mut empty: [string; 0] := []
  empty = []
  mut empty_copy: [i64; 0] := []
  empty_copy = []
  if numbers[0] != 3 || views[1] != "d" || records.len() != 1 || empty.len() != 0 || empty_copy.len() != 0 || live() != 8 { return 53 }
  return 0
}
fn interrupted(flag: bool) -> i32 {
  values := ["alice".clone(), if flag { return 0 } else { "bob".clone() }]
  if values[0] != "alice" || live() != 8 { return 54 }
  return 0
}
fn interrupted_loop(flag: bool) -> i32 {
  loop {
    first := "alice".clone()
    values := [unsafe { first }, if flag { break } else { "bob".clone() }]
    if values[1] != "bob" || live() != 8 { return 55 }
    break
  }
  return 0
}
fn arena_clones() -> i32 {
  value := arena { values := ["alice".clone(), "bob".clone()]; model.identity(values) }
  if value[0] != "alice" || live() != 8 { return 56 }
  return 0
}
fn nested() -> i32 {
  value := model.Nested { batches: [model.Batch { marker: 0, values: ["alice".clone(), "bob".clone()] }] }
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
  empty: [string; 0] := []
  moved := model.identity(empty)
  if moved.len() != 0 { print("bad empty length") }
}
fn exercise() -> i32 {
  mut status := root()
  if status != 0 { return status }
  if live() != 0 { print(live()); return 43 }
  status = moved()
  if status != 0 { return status }
  if live() != 0 { return 4 }
  status = replace_root(true)
  if status != 0 || live() != 0 { return 47 }
  status = replace_root(false)
  if status != 0 || live() != 0 { return 48 }
  status = replace_field()
  if status != 0 || live() != 0 { return 49 }
  status = selected(true)
  if status != 0 || live() != 0 { return 50 }
  status = selected(false)
  if status != 0 || live() != 0 { return 51 }
  status = generic_and_views()
  if status != 0 || live() != 0 { return 67 }
  status = replacement_controls()
  if status != 0 || live() != 0 { return 57 }
  status = interrupted(true)
  if status != 0 || live() != 0 { return 58 }
  status = interrupted(false)
  if status != 0 || live() != 0 { return 59 }
  status = interrupted_loop(true)
  if status != 0 || live() != 0 { return 60 }
  status = interrupted_loop(false)
  if status != 0 || live() != 0 { return 61 }
  status = arena_clones()
  if status != 0 || live() != 0 { return 62 }
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
  mut partial_index := 0
  loop {
    if partial_index == 2 { break }
    fail := partial_index == 0
    bound := model.partial_bound(fail) else { 7 }
    wrapped := model.partial_wrapped(fail) else { 7 }
    replaced := replace_partial(fail) else { 7 }
    expected := if fail { 7 } else { 8 }
    if bound != expected || wrapped != expected || replaced != expected || live() != 0 { return 52 }
    partial_index = partial_index + 1
  }
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
  if allocated != 68 { return 30 }
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
        output.stdout.is_empty() && output.stderr.is_empty(),
        "{label}: {output:?}"
    );
}

#[test]
fn fixed_owned_strings_lifecycle_and_replay() {
    owned_fixture::run("fixed_owned_strings_lifecycle_and_replay", |stage| {
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
        for round in 0..3 {
            let source = if round == 2 {
                std::fs::write(stage.join("model.align"), MODEL.replace("alice", "ALICE")).unwrap();
                let changed = MAIN.replace("alice", "ALICE");
                std::fs::write(entry, &changed).unwrap();
                changed
            } else {
                MAIN.to_string()
            };
            let mut sources = SourceMap::new();
            let mut built = align_driver::build_package(
                &mut sources,
                entry,
                &source,
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
            } else if round == 1 {
                assert_eq!(previous, snapshot);
                execute(stage, &programs, "replay");
            } else {
                assert_ne!(previous, snapshot);
                execute(stage, &programs, "body-change");
            }
        }
    });
}

#[test]
fn fixed_owned_strings_reject_invalid_transfers_and_consumers() {
    owned_fixture::run(
        "fixed_owned_strings_reject_invalid_transfers_and_consumers",
        |stage| {
            std::fs::write(stage.join("model.align"), MODEL).unwrap();
            let prefix =
                "import model\nKeys { values: [string; 2] }\nfn take(values: [string; 2]) {}\n";
            let mut cases = vec![
            ("twice", "fn main() { values := [\"a\".clone(), \"b\".clone()]; take(values); take(values) }".to_string()),
            ("source-twice", "fn main() { text := \"a\".clone(); values := [text, text] }".into()),
            ("moved-element-source", "fn main() { text := \"a\".clone(); values := [text, \"b\".clone()]; print(text) }".into()),
            ("borrowed-owner", "fn bad(borrow values: [string; 2]) { take(values) }\nfn main() {}".into()),
            ("owned-index", "fn main() { values := [\"a\".clone(), \"b\".clone()]; text: string := values[0] }".into()),
            ("nested-move", "fn main() { value := Keys { values: [\"a\".clone(), \"b\".clone()] }; taken := value.values }".into()),
            ("write-element", "fn main() { mut values := [\"a\".clone(), \"b\".clone()]; values[0] = \"c\".clone() }".into()),
            ("write-view", "fn bad(out values: slice<string>) { values[0] = \"c\".clone() }\nfn main() {}".into()),
            ("returned-index", "fn bad() -> str { values := [\"a\".clone(), \"b\".clone()]; return values[0] }\nfn main() {}".into()),
            ("returned-range", "fn bad() -> slice<string> { values := [\"a\".clone(), \"b\".clone()]; return values[..] }\nfn main() {}".into()),
            ("replaced-view", "fn main() { mut values := [\"a\".clone(), \"b\".clone()]; view := values[0]; values = [\"c\".clone(), \"d\".clone()]; print(view) }".into()),
            ("moved-view", "fn main() { values := [\"a\".clone(), \"b\".clone()]; view := values[0]; take(values); print(view) }".into()),
            ("capture-owner", "fn main() { values := [\"a\".clone(), \"b\".clone()]; f := fn { values.len() }; print(f()) }".into()),
            ("generic-owned-read", "fn first<T>(values: slice<T>) -> T = values[0]\nfn main() { values := [\"a\".clone(), \"b\".clone()]; text := first(values) }".into()),
            ("zero-twice", "fn empty(values: [string; 0]) {}\nfn main() { values: [string; 0] := []; empty(values); empty(values) }".into()),
            ("cardinality", "fn main() { values: [string; 2] := [\"a\".clone()] }".into()),
            ("nested-array", "fn main() { values: [[string; 1]; 1] := [[\"a\".clone()]] }".into()),
            ("dynamic-owner", "fn main() { values := [[1].to_array(), [2].to_array()] }".into()),
        ];
            for (name, body) in [
                (
                    "record-duplicate",
                    "Pair { a: string, b: string }\nfn main() { text := \"owned\".clone(); value := Pair { a: text, b: text } }",
                ),
                (
                    "tuple-duplicate",
                    "fn main() { text := \"owned\".clone(); value := (text, text) }",
                ),
                (
                    "call-duplicate",
                    "fn consume(a: string, b: string) {}\nfn main() { text := \"owned\".clone(); consume(text, text) }",
                ),
                (
                    "indirect-duplicate",
                    "fn consume(a: string, b: string) {}\nfn main() { text := \"owned\".clone(); f := consume; f(text, text) }",
                ),
                (
                    "nested-duplicate",
                    "Pair { a: string, b: string }\nfn accept(value: Pair) {}\nfn main() { text := \"owned\".clone(); accept(Pair { a: text, b: text }) }",
                ),
                (
                    "field-duplicate",
                    "Cell { text: string }\nfn main() { cell := Cell { text: \"owned\".clone() }; values := [cell.text, cell.text] }",
                ),
                (
                    "wrapped-duplicate",
                    "fn main() { text := \"owned\".clone(); values := [{ text }, unsafe { text }] }",
                ),
                (
                    "branch-duplicate",
                    "fn run(flag: bool) { a := \"a\".clone(); b := \"b\".clone(); values := [if flag { a } else { b }, a] }\nfn main() {}",
                ),
            ] {
                cases.push((name, body.into()));
            }
            for (name, body) in [
                (
                    "replaced-range",
                    r#"fn main() { mut values := ["a".clone(), "b".clone()]; view := values[..]; values = ["c".clone(), "d".clone()]; print(view[0]) }"#,
                ),
                (
                    "replaced-field-index",
                    r#"fn main() { mut value := Keys { values: ["a".clone(), "b".clone()] }; view := value.values[0]; value.values = ["c".clone(), "d".clone()]; print(view) }"#,
                ),
                (
                    "replaced-field-range",
                    r#"fn main() { mut value := Keys { values: ["a".clone(), "b".clone()] }; view := value.values[..]; value.values = ["c".clone(), "d".clone()]; print(view[0]) }"#,
                ),
                (
                    "moved-mixed-owner",
                    r#"Mixed { values: [string; 2], numbers: array<i64> }
fn consume(value: Mixed) {}
fn main() { value := Mixed { values: ["a".clone(), "b".clone()], numbers: [1].to_array() }; view := value.values[0]; consume(value); print(view) }"#,
                ),
                (
                    "temporary-escape",
                    r#"fn bad() -> str { view := ["owned".clone()][0]; return view }
fn main() {}"#,
                ),
            ] {
                cases.push((name, body.into()));
            }
            for (name, body) in [
                (
                    "imported-return-replaced",
                    r#"fn main() { mut values := ["a".clone(), "b".clone()]; view := model.first(values); values = ["c".clone(), "d".clone()]; print(view) }"#,
                ),
                (
                    "generic-view-moved",
                    r#"fn main() { values := ["a".clone(), "b".clone()]; view := model.same_view(values[..]); take(values); print(view[0]) }"#,
                ),
                (
                    "eager-invalidation",
                    r#"fn consume(view: str, count: i64) {}
fn kill(values: [string; 2]) -> i64 = values.len()
fn main() { values := ["a".clone(), "b".clone()]; consume(values[0], kill(values)) }"#,
                ),
                (
                    "exclusive-view",
                    r#"fn change(borrow mut values: [string; 2]) { values = ["c".clone(), "d".clone()] }
fn main() { mut values := ["a".clone(), "b".clone()]; view := values[0]; change(values); print(view) }"#,
                ),
                (
                    "mixed-mode",
                    r#"Mixed { values: [string; 2], numbers: array<i64> }
fn main() { arena { value := Mixed { values: ["a".clone(), "b".clone()], numbers: [1].to_array() }; print(value.values[0]) } }"#,
                ),
                (
                    "generic-invalid-element",
                    r#"Bad<T> { marker: T, values: [T; 2] }
fn main() { value := Bad { marker: [0].to_array(), values: [[1].to_array(), [2].to_array()] } }"#,
                ),
                (
                    "sum-duplicate",
                    r#"Choice { Both(string, string), Empty }
fn main() { text := "owned".clone(); value := Choice.Both(text, text) }"#,
                ),
            ] {
                cases.push((name, body.into()));
            }
            for source in ["values", "view"] {
                for (name, operation) in [
                    ("materialize", "result := SOURCE.to_array()"),
                    ("chunks", "result := SOURCE.chunks(1)"),
                    (
                        "shuffle",
                        "mut rng := rand.seed_with(1); rng.shuffle(SOURCE)",
                    ),
                    (
                        "sample",
                        "mut rng := rand.seed_with(1); result := rng.sample(SOURCE, 1)",
                    ),
                    ("map-into", "SOURCE.map_into(SOURCE)"),
                    ("map", "result := SOURCE.map(identity).count()"),
                ] {
                    let operation = operation.replace("SOURCE", source);
                    cases.push((name, format!(
                    "import std.rand\nfn identity(value: string) -> string = value\nfn main() {{ mut values := [\"a\".clone(), \"b\".clone()]; mut view := values[..]; {operation} }}"
                )));
                }
            }
            for (index, (name, body)) in cases.iter().enumerate() {
                let source = format!("{prefix}{body}\n");
                let entry = stage.join(format!("negative-{index}.align"));
                std::fs::write(&entry, &source).unwrap();
                for per_unit in [false, true] {
                    let mut sources = SourceMap::new();
                    let diagnostics = if per_unit {
                        build_per_unit(&mut sources, entry.to_str().unwrap(), &source).diags
                    } else {
                        check(&mut sources, entry.to_str().unwrap(), &source).diags
                    };
                    let rendered = align_driver::format_diagnostics(&sources, &diagnostics);
                    assert!(
                        diagnostics.has_errors(),
                        "{name}/{per_unit}: accepted\n{source}"
                    );
                    assert!(
                        !rendered.contains("internal error")
                            && !rendered.contains("cannot certify MIR"),
                        "{name}/{per_unit}: {rendered}"
                    );
                    assert!(
                        !rendered.contains("expected expression")
                            && !rendered.contains("undefined name"),
                        "{name}/{per_unit}: invalid fixture: {rendered}"
                    );
                }
            }
        },
    );
}

#[test]
fn fixed_owned_strings_reject_forged_hir_and_mir() {
    use align_mir::{Const, Operand, Rvalue, Stmt};
    use align_sema::{Scalar, Ty, hir};
    let source = "fn empty() -> [string; 0] { first: [string; 0] := []; second := first; return second }\nfn borrow_at(borrow values: [string; 2], index: i64) -> str = values[index]\nfn main() { values := [\"a\".clone(), \"b\".clone()]; view := values[0] }\n";
    let mut sources = SourceMap::new();
    let checked = check(&mut sources, "forged-strings.align", source);
    assert!(
        !checked.diags.has_errors(),
        "{}",
        align_driver::format_diagnostics(&sources, &checked.diags)
    );
    for mutation in 0..5 {
        let mut bad = checked.hir.clone();
        let function = bad.fns.iter_mut().find(|f| f.name == "main").unwrap();
        let mut changed = false;
        for statement in &mut function.body.stmts {
            let hir::Stmt::Let { local, init } = statement else {
                continue;
            };
            if mutation == 0 && matches!(init.kind, hir::ExprKind::Index { .. }) {
                init.ty = Ty::String;
                function.locals[*local as usize].ty = Ty::String;
                changed = true;
            } else if let hir::ExprKind::ArrayLit {
                elems,
                elem,
                pooled,
            } = &mut init.kind
            {
                match mutation {
                    1 => {
                        *pooled = true;
                    }
                    2 => {
                        elems.pop();
                    }
                    3 => {
                        *elem = Ty::Str;
                    }
                    4 => {
                        init.ty = Ty::Array(Scalar::String, 3);
                        function.locals[*local as usize].ty = init.ty;
                    }
                    _ => continue,
                }
                changed = true;
            }
        }
        assert!(changed, "HIR mutation {mutation}");
        for per_unit in [false, true] {
            for located in [false, true] {
                assert!(
                    align_mir::lower_program_checked(&bad, per_unit, located.then_some(&sources))
                        .is_err(),
                    "HIR mutation {mutation}/{per_unit}/{located}"
                );
            }
        }
    }
    let mir = lower_to_mir(&checked.hir);
    let certify = |program: &align_mir::Program, valid, label: &str| {
        let verdict = align_mir::producer::validate_mir_producers(program);
        assert_eq!(verdict.is_ok(), valid, "{label}: {verdict:?}");
        let defined = program.fns.iter().map(|f| f.name.clone()).collect();
        let verdict = align_mir::producer::validate_partition_resource_rvalues(program, &defined);
        assert_eq!(verdict.is_ok(), valid, "{label}/partition: {verdict:?}");
    };
    certify(&mir, true, "original");
    for mutation in 0..4 {
        let mut bad = mir.clone();
        let function = bad
            .fns
            .iter_mut()
            .find(|f| f.name.as_str() == "empty")
            .unwrap();
        let loads = function.blocks[0]
            .stmts
            .iter()
            .filter_map(|s| match s {
                Stmt::Let(value, Rvalue::Load(slot))
                    if matches!(function.slots[*slot as usize], Ty::Array(Scalar::String, 0)) =>
                {
                    Some((*value, *slot))
                }
                _ => None,
            })
            .collect::<Vec<_>>();
        assert!(loads.len() >= 2);
        let (value, slot) = loads[0];
        match mutation {
            0 => function.blocks[0]
                .stmts
                .insert(0, Stmt::Store(slot, Operand::Value(loads[1].0))),
            1 => function.blocks[0].stmts.insert(
                0,
                Stmt::StoreIndex(
                    slot,
                    Operand::Const(Const::Int(
                        0,
                        Ty::Int(align_sema::IntTy {
                            bits: 64,
                            signed: true,
                        }),
                    )),
                    Operand::Const(Const::Bool(true)),
                ),
            ),
            2 => function.blocks[0]
                .stmts
                .push(Stmt::Let(value, Rvalue::Load(slot))),
            3 => {
                function.slots[slot as usize] = Ty::Array(Scalar::Bool, 0);
            }
            _ => unreachable!(),
        }
        certify(&bad, false, &format!("empty mutation {mutation}"));
    }
    let mut bad = mir.clone();
    let function = bad
        .fns
        .iter_mut()
        .find(|f| f.name.as_str() == "borrow_at")
        .unwrap();
    let value = function
        .blocks
        .iter()
        .flat_map(|b| &b.stmts)
        .find_map(|s| match s {
            Stmt::Let(value, Rvalue::Index(..)) => Some(*value),
            _ => None,
        })
        .unwrap();
    function.value_tys[value as usize] = Ty::String;
    function.ret = Ty::String;
    certify(&bad, false, "owned index result");
    assert!(
        align_driver::emit_llvm_ir(
            &bad,
            BuildTarget::Baseline,
            Profile::Release,
            false,
            &[],
            false
        )
        .is_err()
    );
    let ir = align_driver::emit_llvm_ir(
        &mir,
        BuildTarget::Baseline,
        Profile::Release,
        false,
        &[],
        false,
    )
    .unwrap();
    for line in ir.lines().filter(|line| line.contains("call ")) {
        assert!(
            !line.contains("@align_rt_array_")
                && !line.contains("@align_rt_builder_")
                && !line.contains("@align_rt_alloc("),
            "outer allocation: {line}"
        );
    }
}
