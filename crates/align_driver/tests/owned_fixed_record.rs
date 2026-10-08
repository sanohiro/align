//! Request 136: certify inline Copy arrays beside owned record fields.
#![cfg(unix)]
#[path = "helpers/owned_fixture.rs"]
mod owned_fixture;
use align_driver::{
    BuildTarget, Profile, build_per_unit, check, emit_object_file, link_objects, lower_to_mir,
};
use align_span::SourceMap;
use std::path::Path;

#[test]
fn owned_fixed_record_producer_matrix() {
    for (name, definitions, ty, value) in [
        ("constant", "", "[i64; 2]", "[0, 7]"),
        ("computed", "", "[i64; 2]", "[n, n + 1]"),
        ("empty", "", "[i64; 0]", "[]"),
        ("bool", "", "[bool; 2]", "[true, false]"),
        ("float", "", "[f64; 2]", "[1.0, 2.0]"),
        ("char", "", "[char; 2]", "['a', 'b']"),
        (
            "nested",
            "Meta { counts: [i64; 2] }",
            "Meta",
            "Meta { counts: [n, 7] }",
        ),
    ] {
        let source = format!(
            "{definitions}\nValue {{ bytes: buffer, counts: {ty} }}\nfn make(n: i64) -> Value {{ return Value {{ bytes: buffer.filled(8, 0 as u8), counts: {value} }} }}\nfn main() {{}}\n"
        );
        let mut sources = SourceMap::new();
        let checked = check(&mut sources, "owned-fixed-record.align", &source);
        assert!(
            !checked.diags.has_errors(),
            "{name}: {}",
            align_driver::format_diagnostics(&sources, &checked.diags)
        );
        let mir = lower_to_mir(&checked.hir);
        assert!(
            align_mir::producer::validate_mir_producers(&mir).is_ok(),
            "{name}: {}",
            align_mir::print::program_to_string(&mir)
        );
    }
}

const MODEL: &str = r#"module model
pub Counts { values: [i64; 2] }
pub Meta<T> { values: [T; 2], nested: Counts }
pub Value {
  bytes: buffer,
  counts: [i64; 2], widths: [i64; 2], phases: [i64; 2], emits: [i64; 2],
  metadata: Meta<i64>, empty: [i64; 0],
}
pub fn make(n: i64) -> Value {
  return Value {
    bytes: buffer.filled(8, n as u8),
    counts: [n, 7], widths: [2, 3], phases: [4, 5], emits: [6, 7],
    metadata: Meta { values: [n, 9], nested: Counts { values: [10, 11] } },
    empty: [],
  }
}
pub fn copied(borrow source: Value) -> Value {
  mut value := make(source.counts[0])
  value.counts = source.counts
  return value
}
"#;
const OPERATIONS: &str = r#"module operations
import model
pub fn identity<T>(value: T) -> T = value
pub fn observe(borrow value: model.Value) -> i64 {
  if value.bytes.len() != 8 || value.bytes.bytes().u8(0) != value.counts[0] as u8 { return -1 }
  if value.counts[1] != 7 || value.widths[0] != 2 || value.phases[1] != 5 || value.emits[0] != 6 { return -2 }
  if value.metadata.values[1] != 9 || value.metadata.nested.values[0] != 10 || value.empty.len() != 0 { return -3 }
  return value.counts[0]
}
pub fn advance(borrow mut value: model.Value) {
  { mut bytes := value.bytes.bytes(); bytes.set_u8(0, (value.counts[0] + 1) as u8) }
  value.counts[0] = value.counts[0] + 1
}
fn map_error(error: i64) -> i64 = error
fn guard(fail: bool) -> Result<(), i64> { if fail { return Err(12) }; return Ok(()) }
pub fn scoped(fail: bool) -> Result<i64, i64> {
  value := model.make(3)
  guard(fail).map_err(map_error)?
  return Ok(observe(value))
}
pub fn chosen(flag: bool) -> model.Value {
  return if flag { identity(model.make(4)) } else { model.make(5) }
}
"#;
const MAIN: &str = r#"module main
import model
import operations
extern "C" fn align_rt_requested_live_reset()
extern "C" fn align_rt_requested_live_bytes() -> i64
fn exercise() -> i32 {
  mut value := model.make(1)
  live := unsafe { align_rt_requested_live_bytes() }
  if live != 72 { return 11 }
  if operations.observe(value) != 1 { return 1 }
  mut iteration := 0
  loop {
    if iteration == 2 { break }
    operations.advance(value)
    iteration = iteration + 1
  }
  if operations.observe(value) != 3 { return 2 }
  value = operations.chosen(true)
  if operations.observe(value) != 4 { return 3 }
  if unsafe { align_rt_requested_live_bytes() } != live { return 12 }
  from_copy := operations.identity(model.copied(value))
  if operations.observe(from_copy) != 4 { return 4 }
  local := model.Value {
    bytes: buffer.filled(8, 6), counts: [6, 7], widths: [2, 3], phases: [4, 5], emits: [6, 7],
    metadata: model.Meta { values: [6, 9], nested: model.Counts { values: [10, 11] } }, empty: [],
  }
  if operations.observe(local) != 6 { return 5 }
  match operations.scoped(false) { Ok(n) => { if n != 3 { return 6 } } Err(e) => { return 7 } }
  match operations.scoped(true) { Ok(n) => { return 8 } Err(e) => { if e != 12 { return 9 } } }
  fallback := operations.scoped(true) else { 12 }
  if fallback != 12 { return 10 }
  if unsafe { align_rt_requested_live_bytes() } != live * 3 { return 13 }
  return 0
}
fn main() -> i32 {
  unsafe { align_rt_requested_live_reset() }
  status := exercise()
  if status != 0 { return status }
  if unsafe { align_rt_requested_live_bytes() } != 0 { return 21 }
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
    let output = std::process::Command::new(executable).output().unwrap();
    assert!(output.status.success(), "{label}: {output:?}");
    assert!(
        output.stdout.is_empty() && output.stderr.is_empty(),
        "{output:?}"
    );
}

#[test]
fn imported_owned_fixed_records_execute_and_replay_with_exact_cleanup() {
    owned_fixture::run(
        "imported_owned_fixed_records_execute_and_replay_with_exact_cleanup",
        |stage| {
            assert!(
                align_driver::backend_available(),
                "native owner requires LLVM"
            );
            let producer = "module producer\npub Value { bytes: buffer, counts: [i64; 2] }\npub fn make() -> Value { return Value { bytes: buffer.filled(8, 0 as u8), counts: [0, 0] } }\n";
            let main = "module main\nimport producer\nfn main() -> i32 { value := producer.make(); return if value.bytes.len() == 8 { 0 } else { 1 } }\n";
            std::fs::write(stage.join("producer.align"), producer).unwrap();
            std::fs::write(stage.join("main.align"), main).unwrap();
            let entry = stage.join("main.align");
            let mut sources = SourceMap::new();
            let checked = check(&mut sources, entry.to_str().unwrap(), main);
            assert!(
                !checked.diags.has_errors(),
                "{}",
                align_driver::format_diagnostics(&sources, &checked.diags)
            );
            execute(stage, &[lower_to_mir(&checked.hir)], "exact-whole");
            let mut sources = SourceMap::new();
            let built = build_per_unit(&mut sources, entry.to_str().unwrap(), main);
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
                "exact-units",
            );
            for (name, source) in [
                ("model.align", MODEL),
                ("operations.align", OPERATIONS),
                ("main.align", MAIN),
            ] {
                std::fs::write(stage.join(name), source).unwrap();
            }
            let entry = stage.join("main.align");
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
            let mut snapshots = Vec::new();
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
                assert_eq!(built.units.len(), 3);
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
                    snapshots = snapshot;
                } else {
                    assert_eq!(snapshots, snapshot);
                    execute(stage, &programs, "replay");
                }
            }
        },
    );
}

fn assert_certification(program: &align_mir::Program, valid: bool, label: &str) {
    let verdict = align_mir::producer::validate_mir_producers(program);
    assert_eq!(verdict.is_ok(), valid, "{label}: {verdict:?}");
    let defined = program
        .fns
        .iter()
        .map(|function| function.name.clone())
        .collect();
    let partition = align_mir::producer::validate_partition_resource_rvalues(program, &defined);
    assert_eq!(partition.is_ok(), valid, "{label}/partition: {partition:?}");
}

#[test]
fn fixed_record_array_producers_reject_malformed_initialization() {
    use align_mir::{Const, ConstElem, Operand, Rvalue, Stmt};
    use align_sema::{IntTy, Ty};
    let i64_ty = Ty::Int(IntTy {
        bits: 64,
        signed: true,
    });
    let source = "Value { bytes: buffer, counts: [i64; 2] }\nfn make(n: i64) -> Value { return Value { bytes: buffer.filled(8, 0 as u8), counts: [n, 7] } }\nfn main() {}\n";
    let mut sources = SourceMap::new();
    let checked = check(&mut sources, "invalid-fixed-record.align", source);
    assert!(!checked.diags.has_errors());
    let mir = lower_to_mir(&checked.hir);
    assert_certification(&mir, true, "original");
    for mutation in 0..12 {
        let mut malformed = mir.clone();
        let function = malformed
            .fns
            .iter_mut()
            .find(|f| f.name.as_str() == "make")
            .unwrap();
        let block_index = function
            .blocks
            .iter()
            .position(|b| b.stmts.iter().any(|s| matches!(s, Stmt::StoreIndex(..))))
            .unwrap();
        let stores = function.blocks[block_index]
            .stmts
            .iter()
            .enumerate()
            .filter_map(|(i, s)| matches!(s, Stmt::StoreIndex(..)).then_some(i))
            .collect::<Vec<_>>();
        let Stmt::StoreIndex(slot, _, _) = function.blocks[block_index].stmts[stores[0]] else {
            unreachable!()
        };
        let (load_index, loaded) = function.blocks[block_index]
            .stmts
            .iter()
            .enumerate()
            .find_map(|(i, s)| match s {
                Stmt::Let(v, Rvalue::Load(s)) if *s == slot => Some((i, *v)),
                _ => None,
            })
            .unwrap();
        match mutation {
            0 => {
                function.blocks[block_index].stmts.remove(stores[0]);
            }
            1..=3 => {
                let Stmt::StoreIndex(_, index, value) =
                    &mut function.blocks[block_index].stmts[stores[0]]
                else {
                    unreachable!()
                };
                match mutation {
                    1 => *index = Operand::Const(Const::Int(1, i64_ty)),
                    2 => *index = Operand::Const(Const::Int(2, i64_ty)),
                    3 => *value = Operand::Const(Const::Bool(true)),
                    _ => unreachable!(),
                }
            }
            4 => {
                let store = function.blocks[block_index].stmts.remove(stores[0]);
                function.blocks[block_index].stmts.insert(load_index, store);
            }
            5 => {
                let store = function.blocks[block_index].stmts.remove(stores[0]);
                let other = (block_index + 1) % function.blocks.len();
                function.blocks[other].stmts.push(store);
            }
            6 => {
                let load = function.blocks[block_index].stmts[load_index].clone();
                function.blocks[block_index].stmts.push(load);
            }
            7 => {
                function.blocks[block_index]
                    .stmts
                    .retain(|s| !matches!(s, Stmt::StoreIndex(s, ..) if *s == slot));
                function.blocks[block_index]
                    .stmts
                    .insert(0, Stmt::Store(slot, Operand::Value(loaded)));
            }
            8..=10 => {
                // Exercise the bulk constant-store sibling independently of StoreIndex.
                function.blocks[block_index]
                    .stmts
                    .retain(|s| !matches!(s, Stmt::StoreIndex(s, ..) if *s == slot));
                let (elems, elem) = match mutation {
                    8 => (vec![ConstElem::Int(1)], i64_ty),
                    9 => (vec![ConstElem::Bool(true), ConstElem::Bool(false)], i64_ty),
                    10 => (vec![ConstElem::Int(1), ConstElem::Int(7)], Ty::Bool),
                    _ => unreachable!(),
                };
                function.blocks[block_index]
                    .stmts
                    .insert(stores[0], Stmt::StoreConstArray { slot, elems, elem });
            }
            11 => {
                function.param_modes[0] = align_ast::ParamMode::Out;
            }
            _ => unreachable!(),
        }
        assert_certification(&malformed, false, &format!("mutation {mutation}"));
    }
    let mut bulk = mir.clone();
    let function = bulk
        .fns
        .iter_mut()
        .find(|f| f.name.as_str() == "make")
        .unwrap();
    let block = &mut function.blocks[0];
    let (position, slot) = block
        .stmts
        .iter()
        .enumerate()
        .find_map(|(i, s)| match s {
            Stmt::StoreIndex(slot, ..) => Some((i, *slot)),
            _ => None,
        })
        .unwrap();
    block
        .stmts
        .retain(|s| !matches!(s, Stmt::StoreIndex(s, ..) if *s == slot));
    block.stmts.insert(
        position,
        Stmt::StoreConstArray {
            slot,
            elems: vec![ConstElem::Int(1), ConstElem::Int(7)],
            elem: i64_ty,
        },
    );
    assert_certification(&bulk, true, "complete bulk constants");
}

#[test]
fn fixed_record_ownership_and_alias_negatives_keep_both_modes() {
    owned_fixture::run(
        "fixed_record_ownership_and_alias_negatives_keep_both_modes",
        |stage| {
            let definitions = "Value { bytes: buffer, counts: [i64; 2] }\nfn make() -> Value = Value { bytes: buffer.filled(8, 0), counts: [1, 2] }\nfn take(value: Value) {}\n";
            for (name, body) in [
                (
                    "moved",
                    "fn main() -> i64 { value := make(); take(value); return value.bytes.len() }",
                ),
                (
                    "borrowed-move",
                    "fn bad(borrow value: Value) { take(value) }\nfn main() {}",
                ),
                (
                    "escaped-buffer",
                    "fn bad() -> slice<u8> { value := make(); return value.bytes.bytes() }\nfn main() {}",
                ),
                (
                    "aliased-mutation",
                    "fn write(borrow old: slice<i64>, borrow mut values: slice<i64>) { values[0] = old[0] }\nfn main() { mut value := make(); old := value.counts[..]; mut writable := value.counts[..]; write(old, writable) }",
                ),
                (
                    "contained-view",
                    "Views { bytes: buffer, names: [str; 1] }\nfn bad() -> Views { text := \"local\".clone(); view: str := text; return Views { bytes: buffer.filled(8, 0), names: [text[..]] } }\nfn main() {}",
                ),
            ] {
                let source = format!("{definitions}{body}\n");
                let entry = stage.join(format!("{name}.align"));
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
                        diags.has_errors(),
                        "{name}/{per_unit}: expected ownership rejection"
                    );
                    assert!(
                        !text.contains("expected identifier")
                            && !text.contains("undefined")
                            && !text.contains("unknown"),
                        "{name}: {text}"
                    );
                    assert!(
                        text.contains("move")
                            || text.contains("borrow")
                            || text.contains("outlive")
                            || text.contains("escape"),
                        "{name}: {text}"
                    );
                }
            }
        },
    );
}
