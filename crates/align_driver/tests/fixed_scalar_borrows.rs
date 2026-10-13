//! Request148: complete local inline storage supports the ordinary borrow modes.
#![cfg(unix)]
#[path = "helpers/owned_fixture.rs"]
mod owned_fixture;
use align_driver::{BuildTarget, Profile, build_per_unit, check, lower_to_mir};
use align_span::SourceMap;
use std::path::Path;

fn checked(source: &str) -> align_mir::Program {
    let mut sources = SourceMap::new();
    let checked = check(&mut sources, "fixed-scalar-borrow.align", source);
    assert!(
        !checked.diags.has_errors(),
        "{}",
        align_driver::format_diagnostics(&sources, &checked.diags)
    );
    lower_to_mir(&checked.hir)
}

fn certify(program: &align_mir::Program, valid: bool, label: &str) {
    let result = align_mir::producer::validate_mir_producers(program);
    assert_eq!(result.is_ok(), valid, "{label}: {result:?}");
    let defined = program
        .fns
        .iter()
        .map(|function| function.name.clone())
        .collect();
    let result = align_mir::producer::validate_partition_resource_rvalues(program, &defined);
    assert_eq!(result.is_ok(), valid, "{label}/partition: {result:?}");
}

#[test]
fn plain_fixed_array_borrow_producer_matrix() {
    for (ty, first, second) in [
        ("i8", "1 as i8", "2 as i8"),
        ("u64", "1 as u64", "2 as u64"),
        ("i64", "1", "2"),
        ("f32", "1.0 as f32", "2.0 as f32"),
        ("f64", "1.0", "2.0"),
        ("bool", "true", "false"),
        ("char", "'a'", "'b'"),
    ] {
        for length in [0, 1, 4, 37] {
            let elements = vec![first; length].join(", ");
            let mutation = if length == 0 {
                String::new()
            } else {
                format!("values[0] = {second}")
            };
            let source = format!(
                "fn inspect(borrow values: [{ty}; {length}]) -> i64 = values.len()\n\
                 fn change(borrow mut values: [{ty}; {length}]) {{ {mutation} }}\n\
                 fn main() {{\n\
                   pooled: [{ty}; {length}] := [{elements}]\n\
                   print(inspect(pooled))\n\
                   mut values: [{ty}; {length}] := [{elements}]\n\
                   change(values)\n\
                   change(values)\n\
                   print(inspect(values))\n\
                 }}\n"
            );
            certify(&checked(&source), true, &format!("{ty}/{length}"));
        }
    }
}

const MODEL: &str = r#"module model
pub Table { values: [i64; 4] }
pub fn mark(borrow mut values: [i64; 4]) { values[0] = values[0] + 1 }
pub fn inspect(borrow values: [i64; 4]) -> i64 = values[0]
pub fn forward(borrow mut values: [i64; 4]) { mark(values) }
pub fn generic<T>(borrow mut values: [i64; 4], marker: T) { mark(values) }
pub fn copied(values: [i64; 4]) -> [i64; 4] = values
fn guard(fail: bool) -> Result<(), i64> { if fail { return Err(7) }; return Ok(()) }
fn error(value: i64) -> i64 = value
pub fn fallible(borrow mut values: [i64; 4], fail: bool) -> Result<i64, i64> {
  mark(values)
  guard(fail).map_err(error)?
  mark(values)
  return Ok(values[0])
}
pub fn label(borrow mut values: [i64; 4]) -> string { mark(values); return "owned".clone() }
pub fn empty(borrow mut values: [i64; 0]) -> i64 = values.len()
"#;

#[test]
fn fixed_scalar_borrow_keeps_inline_storage() {
    let program = checked(
        "fn mark(borrow mut values: [i64; 4]) { values[0] = 1 }\n\
         fn main() -> i32 { mut values := [0, 0, 0, 0]; mark(values); return values[0] as i32 }",
    );
    let llvm = align_driver::emit_llvm_ir(
        &program,
        BuildTarget::Baseline,
        Profile::Release,
        false,
        &[],
        false,
    )
    .unwrap();
    assert!(llvm.contains("alloca [4 x i64]"), "{llvm}");
    for symbol in [
        "align_rt_alloc",
        "align_rt_free",
        "align_rt_array_builder_push",
    ] {
        assert!(
            !llvm
                .lines()
                .any(|line| line.contains("call ") && line.contains(symbol)),
            "unexpected call to {symbol}: {llvm}"
        );
    }
}

const MAIN: &str = r#"module main
import model
fn computed() -> i64 = 1
fn choice(flag: bool) -> i64 {
  first := [1, 2, 3, 4]
  second := [5, 6, 7, 8]
  mut values := if flag { first } else { second }
  model.mark(values)
  return values[0]
}
fn exercise(iterations: i64, early: bool) -> i64 {
  mut values := [computed(), 2, 3, 4]
  values[1] = 12
  model.mark(values)
  model.forward(values)
  model.generic(values, true)
  copy := model.copied(values)
  stored := model.Table { values: values }
  mut at := 0
  loop {
    if at == iterations { break }
    model.mark(values)
    values[2] = at
    at = at + 1
  }
  if model.inspect(copy) != 4 || stored.values[0] != 4 || values[1] != 12 { return -1 }
  if early { return model.inspect(values) }
  model.mark(values)
  return model.inspect(values)
}
fn main() -> i32 {
  if choice(true) != 2 || choice(false) != 6 { return 10 }
  if exercise(0, true) != 4 || exercise(1, false) != 6 || exercise(3, true) != 7 { return 1 }
  mut values := [computed(), 2, 3, 4]
  match model.fallible(values, true) { Err(error) => { if error != 7 { return 2 } } Ok(value) => { return 3 } }
  if values[0] != 2 { return 4 }
  answer := model.fallible(values, false) else { -1 }
  if answer != 4 { return 5 }
  label := model.label(values)
  if label.len() != 5 || values[0] != 5 { return 6 }
  mut table := model.Table { values: [7, 2, 3, 4] }
  model.mark(table.values)
  if table.values[0] != 8 { return 7 }
  values = if answer == 4 { model.copied(table.values) } else { model.copied(values) }
  values[3] = 11
  model.mark(values)
  if values[0] != 9 || values[3] != 11 { return 8 }
  mut empty: [i64; 0] := []
  if model.empty(empty) != 0 || model.empty(empty) != 0 { return 9 }
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
        .unwrap_or_else(|error| {
            panic!(
                "{label}: {error}\n{}",
                align_mir::print::program_to_string(program)
            )
        });
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
    // owned_fixture owns the complete compile/link/run process group and deadline.
    let result = std::process::Command::new(binary).output().unwrap();
    assert!(
        result.status.success() && result.stdout.is_empty() && result.stderr.is_empty(),
        "{label}: {result:?}"
    );
}

#[test]
fn imported_fixed_scalar_borrows_execute_and_replay() {
    owned_fixture::run(
        "imported_fixed_scalar_borrows_execute_and_replay",
        |stage| {
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
            let mut first = Vec::new();
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
                assert!(
                    built
                        .units
                        .iter()
                        .all(|unit| unit.frontend.as_ref().unwrap().hit == (round == 1))
                );
                let programs = (0..built.units.len())
                    .map(|i| built.materialize(i).unwrap().clone())
                    .collect::<Vec<_>>();
                let snapshot = programs
                    .iter()
                    .map(align_mir::print::program_to_string)
                    .collect::<Vec<_>>();
                if round == 0 {
                    first = snapshot;
                } else {
                    assert_eq!(first, snapshot);
                    execute(stage, &programs, "replay");
                }
            }
        },
    );
}

#[test]
fn fixed_scalar_borrows_reject_incomplete_or_forged_storage() {
    use align_mir::{Const, ConstElem, Operand, Rvalue, Stmt};
    use align_sema::{IntTy, Scalar, Ty};
    let i64_ty = Ty::Int(IntTy {
        bits: 64,
        signed: true,
    });
    let source = "fn touch(borrow mut values: [i64; 2]) { values[0] = 9 }\n\
        fn exercise(n: i64, flag: bool) { mut values := [n, 7]; touch(values); if flag { print(1) } else { print(2) } }\n\
        fn main() {}\n";
    let original = checked(source);
    certify(&original, true, "original indexed initialization");
    for mutation in 0..15 {
        let mut program = original.clone();
        let function = program
            .fns
            .iter_mut()
            .find(|function| function.name.as_str() == "exercise")
            .unwrap();
        let block = function
            .blocks
            .iter()
            .position(|block| {
                block
                    .stmts
                    .iter()
                    .any(|statement| matches!(statement, Stmt::StoreIndex(..)))
            })
            .unwrap();
        let stores = function.blocks[block]
            .stmts
            .iter()
            .enumerate()
            .filter_map(|(index, statement)| {
                matches!(statement, Stmt::StoreIndex(..)).then_some(index)
            })
            .collect::<Vec<_>>();
        let Stmt::StoreIndex(slot, _, _) = function.blocks[block].stmts[stores[0]] else {
            unreachable!()
        };
        let (call_position, call_value) = function.blocks[block]
            .stmts
            .iter()
            .enumerate()
            .find_map(|(index, statement)| match statement {
                Stmt::Let(value, Rvalue::Call(_, _)) => Some((index, *value)),
                _ => None,
            })
            .unwrap();
        match mutation {
            0 => {
                function.blocks[block].stmts.remove(stores[0]);
            }
            1..=3 => {
                let Stmt::StoreIndex(_, index, value) =
                    &mut function.blocks[block].stmts[stores[0]]
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
                let store = function.blocks[block].stmts.remove(stores[0]);
                function.blocks[block].stmts.insert(call_position, store);
            }
            5 => {
                let store = function.blocks[block].stmts.remove(stores[0]);
                let other = (block + 1) % function.blocks.len();
                function.blocks[other].stmts.insert(0, store);
            }
            6 => {
                let call = function.blocks[block].stmts[call_position].clone();
                function.blocks[block].stmts.push(call);
            }
            7 | 8 => {
                let Stmt::Let(_, Rvalue::Call(_, args)) =
                    &mut function.blocks[block].stmts[call_position]
                else {
                    unreachable!()
                };
                let Operand::BorrowedPlace(place) = &mut args[0] else {
                    unreachable!()
                };
                if mutation == 7 {
                    place.ty = Ty::Array(
                        Scalar::Int(IntTy {
                            bits: 64,
                            signed: true,
                        }),
                        3,
                    );
                } else {
                    place.slot = u32::MAX;
                }
            }
            9 => {
                function.blocks[block].stmts.retain(
                    |statement| !matches!(statement, Stmt::StoreIndex(s, ..) if *s == slot),
                );
                let value = u32::try_from(function.value_tys.len()).unwrap();
                function.value_tys.push(function.slots[slot as usize]);
                function.blocks[block]
                    .stmts
                    .insert(0, Stmt::Let(value, Rvalue::Load(slot)));
                function.blocks[block]
                    .stmts
                    .insert(1, Stmt::Store(slot, Operand::Value(value)));
            }
            10..=13 => {
                function.blocks[block].stmts.retain(
                    |statement| !matches!(statement, Stmt::StoreIndex(s, ..) if *s == slot),
                );
                let (elems, elem) = match mutation {
                    10 => (vec![ConstElem::Int(1)], i64_ty),
                    11 => (vec![ConstElem::Int(1), ConstElem::Bool(false)], i64_ty),
                    12 => (vec![ConstElem::Int(1), ConstElem::Int(7)], Ty::Bool),
                    13 => (vec![ConstElem::Int(1), ConstElem::Int(7)], i64_ty),
                    _ => unreachable!(),
                };
                let position = if mutation == 13 {
                    function.blocks[block].stmts.iter().position(|statement| matches!(statement, Stmt::Let(value, _) if *value == call_value)).unwrap() + 1
                } else {
                    stores[0]
                };
                function.blocks[block]
                    .stmts
                    .insert(position, Stmt::StoreConstArray { slot, elems, elem });
            }
            14 => {
                function.param_modes[0] = align_ast::ParamMode::Out;
            }
            _ => unreachable!(),
        }
        certify(&program, false, &format!("mutation {mutation}"));
        assert!(
            align_driver::emit_llvm_ir(
                &program,
                BuildTarget::Baseline,
                Profile::Dev,
                false,
                &[],
                false
            )
            .is_err(),
            "codegen accepted mutation {mutation}"
        );
    }
    for length in [0, 2] {
        let values = if length == 0 { "" } else { "1, 2" };
        let source = format!(
            "fn touch(borrow mut values: [i64; {length}]) {{ print(values.len()) }}\n\
            fn forward(borrow mut values: [i64; {length}]) {{ touch(values) }}\n\
            fn main() {{ mut values: [i64; {length}] := [{values}]; forward(values) }}\n"
        );
        let program = checked(&source);
        certify(&program, true, "parameter forwarding");
        for mode in [align_ast::ParamMode::Borrow, align_ast::ParamMode::Out] {
            let mut bad = program.clone();
            bad.fns
                .iter_mut()
                .find(|function| function.name.as_str() == "forward")
                .unwrap()
                .param_modes[0] = mode;
            certify(&bad, false, &format!("{length}/{mode:?}"));
        }
        if length == 0 {
            for (name, statement, valid) in [
                (
                    "exact empty bulk",
                    Stmt::StoreConstArray {
                        slot: 0,
                        elems: vec![],
                        elem: i64_ty,
                    },
                    true,
                ),
                (
                    "mistyped empty bulk",
                    Stmt::StoreConstArray {
                        slot: 0,
                        elems: vec![],
                        elem: Ty::Bool,
                    },
                    false,
                ),
                (
                    "nonempty bulk",
                    Stmt::StoreConstArray {
                        slot: 0,
                        elems: vec![ConstElem::Int(1)],
                        elem: i64_ty,
                    },
                    false,
                ),
                (
                    "empty indexed write",
                    Stmt::StoreIndex(
                        0,
                        Operand::Const(Const::Int(0, i64_ty)),
                        Operand::Const(Const::Int(1, i64_ty)),
                    ),
                    false,
                ),
            ] {
                let mut modified = program.clone();
                let function = modified
                    .fns
                    .iter_mut()
                    .find(|function| function.name.as_str() == "main")
                    .unwrap();
                assert!(matches!(function.slots[0], Ty::Array(_, 0)));
                function.blocks[0].stmts.insert(0, statement);
                certify(&modified, valid, name);
                assert_eq!(
                    align_driver::emit_llvm_ir(
                        &modified,
                        BuildTarget::Baseline,
                        Profile::Dev,
                        false,
                        &[],
                        false
                    )
                    .is_ok(),
                    valid,
                    "{name}/codegen"
                );
            }
            let mut bad = program;
            let function = bad
                .fns
                .iter_mut()
                .find(|function| function.name.as_str() == "main")
                .unwrap();
            let slot = function
                .slots
                .iter()
                .position(|ty| matches!(ty, Ty::Array(_, 0)))
                .unwrap();
            let value = u32::try_from(function.value_tys.len()).unwrap();
            function.value_tys.push(function.slots[slot]);
            function.blocks[0].stmts.insert(
                0,
                Stmt::Let(value, Rvalue::Load(u32::try_from(slot).unwrap())),
            );
            function.blocks[0].stmts.insert(
                1,
                Stmt::Store(u32::try_from(slot).unwrap(), Operand::Value(value)),
            );
            certify(&bad, false, "empty root-store cycle");
        }
    }
    let joined = checked(
        "fn touch(borrow mut values: [i64; 2]) { values[0] = 9 }\n\
        fn exercise(flag: bool) { first := [1, 2]; second := [3, 4]; mut values := if flag { first } else { second }; touch(values) }\nfn main() {}\n",
    );
    certify(&joined, true, "both branches initialize");
    let mut missing = joined.clone();
    let function = missing
        .fns
        .iter_mut()
        .find(|function| function.name.as_str() == "exercise")
        .unwrap();
    let mut stores = Vec::new();
    for block in &function.blocks {
        for statement in &block.stmts {
            if let Stmt::Store(slot, _) = statement
                && matches!(function.slots.get(*slot as usize), Some(Ty::Array(_, 2)))
            {
                stores.push((block.id, *slot));
            }
        }
    }
    let (block, slot) = stores
        .iter()
        .find(|(_, slot)| stores.iter().filter(|(_, other)| slot == other).count() == 2)
        .copied()
        .unwrap();
    function.blocks[block as usize].stmts.retain(
        |statement| !matches!(statement, Stmt::Store(destination, _) if *destination == slot),
    );
    certify(&missing, false, "one branch omits initialization");
}

#[test]
fn fixed_scalar_borrow_source_boundaries_remain_closed() {
    owned_fixture::run(
        "fixed_scalar_borrow_source_boundaries_remain_closed",
        |stage| {
            for (name, source, diagnostic) in [
                (
                    "shared-write",
                    "fn bad(borrow values: [i64; 2]) { values[0] = 1 }\nfn main() {}",
                    "immutable",
                ),
                (
                    "shared-upgrade",
                    "fn change(borrow mut values: [i64; 2]) {}\nfn bad(borrow values: [i64; 2]) { change(values) }\nfn main() {}",
                    "borrow",
                ),
                (
                    "overlapping-arguments",
                    "fn change(borrow prior: slice<i64>, borrow mut values: [i64; 2]) { values[0] = prior[0] }\nfn main() { mut values := [1, 2]; view := values[..]; change(view, values) }",
                    "borrow",
                ),
                (
                    "escape",
                    "fn bad() -> slice<i64> { values := [1, 2]; return values[..] }\nfn main() {}",
                    "local array",
                ),
                (
                    "function-value",
                    "fn change(borrow mut values: [i64; 2]) {}\nfn main() { action := change }",
                    "function value",
                ),
                (
                    "generic-element",
                    "fn change<T>(borrow mut values: [T; 2], replacement: T) { values[0] = replacement }\nfn main() {}",
                    "element assignment",
                ),
            ] {
                let entry = stage.join(format!("{name}.align"));
                std::fs::write(&entry, source).unwrap();
                for per_unit in [false, true] {
                    let mut sources = SourceMap::new();
                    let diags = if per_unit {
                        build_per_unit(&mut sources, entry.to_str().unwrap(), source).diags
                    } else {
                        check(&mut sources, entry.to_str().unwrap(), source).diags
                    };
                    let text = align_driver::format_diagnostics(&sources, &diags);
                    assert!(
                        diags.has_errors() && text.contains(diagnostic),
                        "{name}/{per_unit}: {text}"
                    );
                }
            }
        },
    );
}
