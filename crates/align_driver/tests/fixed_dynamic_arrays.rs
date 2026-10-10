//! Plan165: inline dynamic-array owners retain exact shared access and cleanup.
#![cfg(unix)]
#[path = "helpers/owned_fixture.rs"]
mod owned_fixture;
use align_driver::{
    BuildTarget, Profile, build_per_unit, check, emit_object_file, link_objects, lower_to_mir,
};
use align_span::SourceMap;
use std::path::Path;

const MODEL: &str = r#"module model
pub Batch { values: [array<i64>; 2] }
pub Row { name: string, number: i64 }
pub Generic<T> { marker: T, values: [T; 2] }
pub fn identity<T>(value: T) -> T = value
pub fn count(borrow value: array<i64>) -> i64 = value.len()
pub fn first(borrow value: array<i64>) -> i64 = value[0]
pub fn view(borrow value: array<i64>) -> slice<i64> = value[..]
pub fn view_count(values: slice<array<i64>>, index: i64) -> i64 = count(values[index])
pub fn fixed_count(borrow values: [array<i64>; 2], index: i64) -> i64 = count(values[index])
pub fn make_values() -> [array<i64>; 2] { values := [[11, 12].to_array(), [13].to_array()]; return values }
pub fn make() -> Batch = Batch { values: [[11, 12].to_array(), [13].to_array()] }
pub fn strings() -> array<string> {
  mut builder: array_builder<string> := array_builder()
  builder.push("alpha".clone()); builder.push("beta".clone())
  return builder.build()
}
pub fn string_first(borrow values: array<string>) -> str = values[0]
pub fn rows() -> array<Row> {
  mut builder: array_builder<Row> := array_builder()
  builder.push(Row { name: "alpha".clone(), number: 11 })
  return builder.build()
}
pub fn row_first(borrow values: array<Row>) -> str = values[0].name
pub fn guard(fail: bool) -> Result<(), i64> { if fail { return Err(7) }; return Ok(()) }
pub fn number(fail: bool) -> Result<array<i64>, i64> { guard(fail)?; return Ok([13].to_array()) }
pub fn partial(fail: bool) -> Result<i64, i64> {
  values := [[11, 12].to_array(), number(fail)?]
  return Ok(count(values[0]) + count(values[1]))
}
pub fn partial_bound(fail: bool) -> Result<i64, i64> {
  first_value := [11, 12].to_array()
  values := [first_value, number(fail)?]
  return Ok(count(values[0]) + count(values[1]))
}
pub fn partial_wrapped(fail: bool) -> Result<i64, i64> {
  first_value := [11, 12].to_array()
  values := [unsafe { first_value }, number(fail)?]
  return Ok(count(values[0]) + count(values[1]))
}
pub fn flow(flag: bool, fail: bool) -> Result<i64, i64> {
  a := if flag { [11].to_array() } else { [12].to_array() }
  b := number(fail).map_err(fn error: i64 { error + 1 })?
  mut values := [a, b]
  values = match (if flag { 1 } else { 0 }) { 1 => values, _ => values }
  values = make_values()
  loop { if count(values[0]) == 2 { break } }
  fallback := number(true) else [15].to_array()
  final_values := [fallback, [16].to_array()]
  return Ok(first(values[0]) + first(final_values[1]))
}

pub fn early(fail: bool) -> i64 {
  values: [array<i64>; 2] := [[11].to_array(), { if fail { return 0 }; [12].to_array() }]
  return count(values[0]) + count(values[1])
}
pub fn partial_break(fail: bool) -> i64 {
  loop {
    values: [array<i64>; 2] := [[11].to_array(), { if fail { break 0 }; [12].to_array() }]
    break count(values[0]) + count(values[1])
  }
}

"#;

const MAIN: &str = r#"module main
import model
ZeroFirst { empty: [array<i64>; 0], text: string }
ZeroLast { text: string, empty: [array<i64>; 0] }
extern "C" fn align_rt_requested_live_reset()
extern "C" fn align_rt_requested_live_bytes() -> i64
extern "C" fn align_rt_alloc_count() -> i64
extern "C" fn align_rt_free_count() -> i64
fn live() -> i64 = unsafe { align_rt_requested_live_bytes() }
fn allocations() -> i64 = unsafe { align_rt_alloc_count() }
fn direct(index: i64) -> i32 {
  values := [[11, 12].to_array(), [13].to_array()]
  mut seen := 0
  if live() != 24 || model.count(values[0]) != 2 || model.first(values[{ seen = seen + 1; index }]) != 13 || seen != 1 { return 1 }
  view := model.view(values[index])
  if view[0] != 13 { return 2 }
  return 0
}
fn transported() -> i32 {
  mut batch := model.make()
  if live() != 24 || model.first(batch.values[0]) != 11 { return 3 }
  batch.values = model.make_values()
  if live() != 24 || model.view_count(batch.values[..], 1) != 1 { return 4 }
  moved := model.identity(batch)
  if model.first(moved.values[1]) != 13 { return 5 }
  return 0
}
fn shared_whole() -> i32 {
  values := model.make_values()
  if model.fixed_count(values, 1) != 1 { return 6 }
  return 0
}
fn replacement(flag: bool) -> i32 {
  mut values := model.make_values()
  values = values
  values = if flag { values } else { model.make_values() }
  values = model.make_values()
  if live() != 24 || model.first(values[1]) != 13 { return 7 }
  return 0
}
fn deep() -> i32 {
  values := [model.strings(), model.strings()]
  if model.string_first(values[0]) != "alpha" || model.string_first(values[1]) != "alpha" { return 8 }
  rows := [model.rows(), model.rows()]
  if model.row_first(rows[0]) != "alpha" || model.row_first(rows[1]) != "alpha" { return 9 }
  if live() <= 0 { return 10 }
  return 0
}
fn empty_siblings() -> i32 {
  empty_view: slice<array<i64>> := []
  if empty_view.len() != 0 { return 41 }
  left: Option<ZeroFirst> := Some(ZeroFirst { empty: [], text: "x".clone() })
  right: Result<ZeroLast, i64> := Ok(ZeroLast { text: "x".clone(), empty: [] })
  if live() != 2 { return 11 }
  return 0
}
fn partials() -> i32 {
  a := model.partial(true) else 0
  b := model.partial(false) else 0
  c := model.partial_bound(true) else 0
  d := model.partial_bound(false) else 0
  e := model.partial_wrapped(true) else 0
  f := model.partial_wrapped(false) else 0
  if a != 0 || c != 0 || e != 0 || b != 3 || d != 3 || f != 3 { return 12 }
  return 0
}
fn morphology() -> i32 {
  generic := model.Generic { marker: [1].to_array(), values: [[2].to_array(), [3].to_array()] }
  moved := model.identity(generic)
  callback := model.first
  if callback(moved.values[1]) != 3 { return 30 }
  nested := [model.Batch { values: [[6].to_array(), [7].to_array()] }]
  if nested.len() != 1 { return 42 }
  pair := ([4].to_array(), [5].to_array())
  tuple_values := [pair.0, pair.1]
  if model.first(tuple_values[1]) != 5 { return 31 }
  strings := ["x".clone(), "yy".clone()]
  records := [model.Row { name: "x".clone(), number: 1 }, model.Row { name: "yy".clone(), number: 2 }]
  if inspect_string(strings[1]) != 2 || inspect_record(records[1]) != 2 { return 32 }
  return 0
}
fn inspect_string(borrow value: string) -> i64 = value.len()
fn inspect_record(borrow value: model.Row) -> i64 = value.name.len()
fn flows() -> i32 {
  a := model.flow(true, true) else 0
  b := model.flow(false, true) else 0
  c := model.flow(true, false) else 0
  d := model.flow(false, false) else 0
  if a != 0 || b != 0 || c != 27 || d != 27 { return 33 }
  if model.early(true) != 0 || model.early(false) != 2 || model.partial_break(true) != 0 || model.partial_break(false) != 2 { return 37 }
  return 0
}
fn bool_count(borrow value: array<bool>) -> i64 = value.len()
fn float_count(borrow value: array<f64>) -> i64 = value.len()
fn char_count(borrow value: array<char>) -> i64 = value.len()
fn str_first(borrow value: array<str>) -> str = value[0]
fn domains() -> i32 {
  bools := [[true].to_array()]
  floats := [[1.5].to_array()]
  chars := [['x'].to_array()]
  text := "input".clone()
  view: str := text
  strings := [[view].to_array()]
  if bool_count(bools[0]) != 1 || float_count(floats[0]) != 1 || char_count(chars[0]) != 1 || str_first(strings[0]) != "input" { return 38 }
  arena {
    values := [[11].to_array(), [12].to_array()]
    if model.first(values[0]) != 11 || model.first(values[1]) != 12 { return 39 }
  }
  return 0
}
fn main() -> i32 {
  unsafe { align_rt_requested_live_reset() }
  allocated_before := allocations()
  freed_before := unsafe { align_rt_free_count() }
  a := direct(1)
  if a != 0 { return a }
  if allocations() - allocated_before != 2 || live() != 0 { return 20 }
  b := transported()
  if b != 0 { return b }
  if live() != 0 { return 21 }
  c := shared_whole()
  if c != 0 { return c }
  if live() != 0 { return 22 }
  d := replacement(true)
  if d != 0 { return d }
  if live() != 0 { return 23 }
  e := replacement(false)
  if e != 0 { return e }
  if live() != 0 { return 24 }
  f := deep()
  if f != 0 { return f }
  if live() != 0 { return 25 }
  g := empty_siblings()
  if g != 0 { return g }
  if live() != 0 { return 26 }
  h := partials()
  if h != 0 { return h }
  allocated := allocations() - allocated_before
  freed := unsafe { align_rt_free_count() } - freed_before
  // alloc_count counts align_rt_alloc only. Four builders acquire their buffers through
  // realloc(null), then each frozen buffer is retired by align_rt_free alongside 35 allocations.
  if allocated != 35 || freed != 39 || live() != 0 { return 27 }
  before := allocations()
  m := morphology()
  if m != 0 { return m }
  if allocations() - before != 11 || live() != 0 { return 34 }
  flow_result := flows()
  if flow_result != 0 { return flow_result }
  if allocations() - before != 31 || live() != 0 { return 35 }
  if unsafe { align_rt_free_count() } - freed_before != 70 { return 36 }
  domain_allocations := allocations()
  domain_result := domains()
  if domain_result != 0 { return domain_result }
  if allocations() <= domain_allocations || live() != 0 { return 40 }

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
fn fixed_dynamic_arrays_lifecycle_and_replay() {
    owned_fixture::run("fixed_dynamic_arrays_lifecycle_and_replay", |stage| {
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
                std::fs::write(stage.join("model.align"), MODEL.replace("alpha", "ALPHA")).unwrap();
                let changed = MAIN.replace("alpha", "ALPHA");
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
fn fixed_dynamic_arrays_reject_invalid_transfers_and_views() {
    owned_fixture::run(
        "fixed_dynamic_arrays_reject_invalid_transfers_and_views",
        |stage| {
            std::fs::write(stage.join("model.align"), MODEL).unwrap();
            let prefix = "import model\nimport std.rand\nfn take(values: [array<i64>; 2]) {}\nfn consume(value: array<i64>) {}\n";
            let mut cases = vec![
            ("loop-owner-export", "fn bad() -> [array<i64>; 2] { loop { break model.make_values() } }\nfn main() {}".into()),
            ("loop-owner-replacement", "fn bad(flag: bool) -> i64 { a := [1].to_array(); b := [2].to_array(); mut values := [a, b]; values = match (if flag { 1 } else { 0 }) { 1 => values, _ => values }; mut again := true; loop { if !again { break }; values = model.make_values(); again = false }; return model.count(values[0]) }\nfn main() {}".into()),
            ("duplicate", "fn main() { a := [1].to_array(); values := [a, a] }".to_string()),
            ("tuple-duplicate", "fn main() { pair := ([1].to_array(), [2].to_array()); values := [pair.0, pair.0] }".into()),
            ("source-after-transfer", "fn main() { a := [1].to_array(); values := [a, [2].to_array()]; print(a.len()) }".into()),
            ("whole-twice", "fn main() { values := model.make_values(); take(values); take(values) }".into()),
            ("empty-twice", "fn empty(value: [array<i64>; 0]) {}\nfn main() { values: [array<i64>; 0] := []; empty(values); empty(values) }".into()),
            ("owned-index", "fn main() { values := model.make_values(); consume(values[0]) }".into()),
            ("indexed-store", "fn main() { mut values := model.make_values(); values[0] = [3].to_array() }".into()),
            ("indexed-exclusive", "fn mutate(borrow mut value: array<i64>) {}\nfn main() { mut values := model.make_values(); mutate(values[0]) }".into()),
            ("borrowed-owner-move", "fn bad(borrow values: [array<i64>; 2]) { take(values) }\nfn main() {}".into()),
            ("nested-field-move", "fn main() { batch := model.make(); values := batch.values }".into()),
            ("returned-inner-view", "fn bad() -> slice<i64> { values := model.make_values(); return model.view(values[0]) }\nfn main() {}".into()),
            ("returned-outer-view", "fn bad() -> slice<array<i64>> { values := model.make_values(); return values[..] }\nfn main() {}".into()),
            ("retained-inner-view", "View { values: slice<i64> }\nfn retain(borrow value: array<i64>, borrow mut output: View) { output.values = value[..] }\nfn main() { values := model.make_values(); mut output := View { values: [] }; retain(values[0], output); take(values); print(output.values[0]) }".into()),
            ("replaced-inner-view", "fn main() { mut values := model.make_values(); view := model.view(values[0]); values = model.make_values(); print(view[0]) }".into()),
            ("replaced-outer-view", "fn main() { mut values := model.make_values(); view := values[..]; values = model.make_values(); print(model.view_count(view, 0)) }".into()),
            ("moved-inner-view", "fn main() { values := model.make_values(); view := model.view(values[0]); take(values); print(view[0]) }".into()),
            ("moved-outer-view", "fn main() { values := model.make_values(); view := values[..]; take(values); print(model.view_count(view, 0)) }".into()),
            ("generic-owned-read", "fn first<T>(values: slice<T>) -> T = values[0]\nfn main() { values := model.make_values(); consume(first(values)) }".into()),
            ("temporary-base", "fn main() { print(model.count(model.make_values()[0])) }".into()),
            ("nested-index-base", "fn first(borrow values: array<i64>) -> i64 = values[0]\nfn bad(values: slice<array<i64>>) -> i64 = first(values[0][0])\nfn main() {}".into()),
            ("capture-owner", "fn main() { values := model.make_values(); f := fn { values.len() }; print(f()) }".into()),
            ("cardinality", "fn main() { values: [array<i64>; 2] := [[1].to_array()] }".into()),
            ("nested-fixed", "fn main() { values := [[[1].to_array()]] }".into()),
            ("specialized-inner", "fn main() { values := [[1, 2].chunks(1)] }".into()),
            ("generic-nested-dynamic", "fn bad<T>(value: T) -> array<T> { mut b: array_builder<T> := array_builder(); b.push(value); return b.build() }\nfn main() { values := bad([1].to_array()) }".into()),
            ("nested-record-dynamic", "fn bad(values: array<array<model.Row>>) {}\nfn main() {}".into()),
            ("nested-dynamic", "fn bad(values: array<array<i64>>) {}\nfn main() {}".into()),
            ("mixed-mode", "fn main() { a := [1].to_array(); arena { values := [a, [2].to_array()] } }".into()),
            ("arena-escape", "fn bad() -> [array<i64>; 2] = arena { values := [[1].to_array(), [2].to_array()]; values }\nfn main() {}".into()),
            ("inner-str-escape", "fn bad() -> [array<str>; 1] { text := \"owned\".clone(); view: str := text; values := [[view].to_array()]; return values }\nfn main() {}".into()),
            ("arena-view-escape", "fn bad() -> slice<i64> = arena { values := [[1].to_array(), [2].to_array()]; model.view(values[0]) }\nfn main() {}".into()),
            ("eager-source-release", "fn main() { a := [1].to_array(); values := [a, model.identity(a)] }".into()),
            ("later-argument-release", "fn inspect(borrow value: array<i64>, other: ()) -> i64 = value.len()\nfn main() { values := model.make_values(); print(inspect(values[0], take(values))) }".into()),
            ("slice-header-rebind", "fn both(borrow value: array<i64>, later: ()) -> i64 = value.len()\nfn main() { values := model.make_values(); mut view := values[..]; print(both(view[0], { view = values[1..]; () })) }".into()),
            ("index-release", "fn main() { values := model.make_values(); print(model.count(values[{ take(values); 0 }])) }".into()),
        ].into_iter().map(|(name, body)| (name.to_string(), body)).collect::<Vec<_>>();
            for (label, source) in [("fixed", "values"), ("slice", "values[..]")] {
                for (operation, call) in [
                    ("to-array", "to_array()"),
                    ("chunks", "chunks(1)"),
                    ("map", "map(fn value { value }).to_array()"),
                ] {
                    cases.push((format!("{label}-{operation}"),
                    format!("fn main() {{ values := model.make_values(); result := {source}.{call} }}")));
                }
            }
            for (label, receiver) in [("fixed", "values"), ("slice", "view")] {
                for (operation, call) in [
                    ("shuffle", "rng.shuffle(SOURCE)"),
                    ("sample", "result := rng.sample(SOURCE, 1)"),
                    ("map-into", "SOURCE.map_into(SOURCE)"),
                ] {
                    let call = call.replace("SOURCE", receiver);
                    cases.push((format!("{label}-{operation}"), format!("fn main() {{ mut values := model.make_values(); mut view := values[..]; mut rng := rand.seed_with(1); {call} }}")));
                }
            }
            let mut failures = Vec::new();
            for (name, body) in cases {
                let source = format!("{prefix}{body}\n");
                let entry = stage.join(format!("{name}.align"));
                std::fs::write(&entry, &source).unwrap();
                for per_unit in [false, true] {
                    let mut sources = SourceMap::new();
                    let diagnostics = if per_unit {
                        build_per_unit(&mut sources, entry.to_str().unwrap(), &source).diags
                    } else {
                        check(&mut sources, entry.to_str().unwrap(), &source).diags
                    };
                    let rendered = align_driver::format_diagnostics(&sources, &diagnostics);
                    if !diagnostics.has_errors() {
                        failures.push(format!("{name}/{per_unit}: accepted\n{source}"));
                    } else if rendered.contains("internal error")
                        || rendered.contains("cannot certify MIR")
                        || rendered.contains("expected expression")
                        || rendered.contains("undefined name")
                        || rendered.contains("/model.align:")
                    {
                        failures.push(format!(
                            "{name}/{per_unit}: invalid boundary or fixture: {rendered}"
                        ));
                    }
                }
            }
            assert!(failures.is_empty(), "{}", failures.join("\n"));
        },
    );
}

#[test]
fn fixed_dynamic_arrays_reject_forged_construction() {
    use align_mir::{Const, Operand, Rvalue, Stmt, Term};
    use align_sema::{Scalar, Ty, hir};
    let source = "Row { n: i64 }\nfn records(borrow values: [array<Row>; 0]) {}\npub fn carry(value: [array<i64>; 2]) -> [array<i64>; 2] = value\nfn make() -> [array<i64>; 2] { values := [[11, 12].to_array(), [13].to_array()]; return values }\nfn empty() -> [array<i64>; 0] { first: [array<i64>; 0] := []; second := first; return second }\nfn main() {}\n";
    let mut sources = SourceMap::new();
    let checked = check(&mut sources, "forged-dynamic.align", source);
    assert!(
        !checked.diags.has_errors(),
        "{}",
        align_driver::format_diagnostics(&sources, &checked.diags)
    );
    for mutation in 0..5 {
        let mut bad = checked.hir.clone();
        let function = bad.fns.iter_mut().find(|f| f.name == "make").unwrap();
        let hir::Stmt::Let { local, init } = &mut function.body.stmts[0] else {
            panic!("literal binding")
        };
        let hir::ExprKind::ArrayLit {
            elems,
            elem,
            pooled,
        } = &mut init.kind
        else {
            panic!("outer literal")
        };
        match mutation {
            0 => *pooled = true,
            1 => {
                elems.pop();
            }
            2 => *elem = Ty::String,
            3 => {
                let Ty::Array(element, _) = init.ty else {
                    panic!("fixed type")
                };
                init.ty = Ty::Array(element, 3);
                function.locals[*local as usize].ty = init.ty;
            }
            _ => elems[0].ty = Ty::Bool,
        }
        for per_unit in [false, true] {
            for located in [false, true] {
                assert!(
                    align_mir::lower_program_checked(&bad, per_unit, located.then_some(&sources))
                        .is_err(),
                    "HIR {mutation}/{per_unit}/{located}"
                );
            }
        }
    }
    let mut over_aligned = checked.hir.clone();
    over_aligned
        .structs
        .iter_mut()
        .find(|record| record.source_name == "Row")
        .unwrap()
        .align = Some(16);
    for per_unit in [false, true] {
        for located in [false, true] {
            assert!(
                align_mir::lower_program_checked(
                    &over_aligned,
                    per_unit,
                    located.then_some(&sources)
                )
                .is_err(),
                "over-aligned nested dynamic header"
            );
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
    let ir = align_driver::emit_llvm_ir(
        &mir,
        BuildTarget::Baseline,
        Profile::Release,
        false,
        &[],
        false,
    )
    .unwrap();
    let name = &mir
        .fns
        .iter()
        .find(|function| function.name.as_str() == "carry")
        .unwrap()
        .name;
    let transport_symbol = format!(
        "@\"{}\"(",
        align_codegen_llvm::imported_program_symbol(name)
    );
    let transport = ir
        .split("\n}")
        .find(|body| {
            body.lines()
                .any(|line| line.starts_with("define ") && line.contains(&transport_symbol))
        })
        .unwrap_or_else(|| {
            panic!(
                "fixed owner transport function: {}",
                ir.lines()
                    .filter(|line| line.starts_with("define "))
                    .collect::<Vec<_>>()
                    .join("\n")
            )
        });
    for line in transport.lines().filter(|line| line.contains("call ")) {
        assert!(
            !line.contains("@align_rt_alloc(")
                && !line.contains("@align_rt_array_")
                && !line.contains("@align_rt_builder_"),
            "outer transport allocation: {line}"
        );
    }
    certify(&mir, true, "original");
    for mutation in 0..7 {
        let mut bad = mir.clone();
        let function = bad
            .fns
            .iter_mut()
            .find(|f| f.name.as_str() == "make")
            .unwrap();
        let (block, position, slot) = function
            .blocks
            .iter()
            .enumerate()
            .find_map(|(b, block)| {
                block
                    .stmts
                    .iter()
                    .enumerate()
                    .find_map(|(p, statement)| match statement {
                        Stmt::StoreIndex(slot, _, _)
                            if matches!(
                                function.slots[*slot as usize],
                                Ty::Array(Scalar::DynArray(_), 2)
                            ) =>
                        {
                            Some((b, p, *slot))
                        }
                        _ => None,
                    })
            })
            .unwrap();
        let (load_block, load_position, loaded) = function
            .blocks
            .iter()
            .enumerate()
            .find_map(|(b, block)| {
                block
                    .stmts
                    .iter()
                    .enumerate()
                    .find_map(|(p, statement)| match statement {
                        Stmt::Let(value, Rvalue::Load(candidate)) if *candidate == slot => {
                            Some((b, p, *value))
                        }
                        _ => None,
                    })
            })
            .unwrap();
        let store = function.blocks[block].stmts[position].clone();
        match mutation {
            0 => {
                function.blocks[block].stmts.remove(position);
            }
            1 => function.blocks[block].stmts.insert(position, store),
            2 => {
                let Stmt::StoreIndex(_, index, _) = &mut function.blocks[block].stmts[position]
                else {
                    panic!("store")
                };
                *index = Operand::Const(Const::Int(
                    2,
                    Ty::Int(align_sema::IntTy {
                        bits: 64,
                        signed: true,
                    }),
                ));
            }
            3 => {
                let Stmt::StoreIndex(_, _, value) = &mut function.blocks[block].stmts[position]
                else {
                    panic!("store")
                };
                *value = Operand::Const(Const::Bool(true));
            }
            4 => {
                function.blocks[block].stmts.remove(position);
                let shifted = usize::from(block == load_block && position < load_position);
                function.blocks[load_block]
                    .stmts
                    .insert(load_position + 1 - shifted, store);
            }
            5 => {
                assert_ne!(
                    block, load_block,
                    "inner materializers must split initialization and publication"
                );
                function.blocks[function.entry as usize].term =
                    Term::Goto(function.blocks[load_block].id);
            }
            _ => function.blocks[load_block]
                .stmts
                .push(Stmt::Let(loaded, Rvalue::Load(slot))),
        }
        certify(&bad, false, &format!("construction {mutation}"));
    }
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
                    if matches!(
                        function.slots[*slot as usize],
                        Ty::Array(Scalar::DynArray(_), 0)
                    ) =>
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
            _ => function.slots[slot as usize] = Ty::Array(Scalar::Bool, 0),
        }
        certify(&bad, false, &format!("empty {mutation}"));
    }
}
