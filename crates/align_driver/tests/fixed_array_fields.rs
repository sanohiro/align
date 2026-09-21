//! Plan 73 owner: nameable fixed arrays and inline record-field operations.

mod common;
use common::*;
use std::process::Command;

#[test]
fn fixed_array_field_read_mutate_slice_len_and_pipeline() {
    if !backend_available() {
        return;
    }
    let source = concat!(
        "Table { values: [i64; 4] }\n",
        "fn main() -> i32 {\n",
        "  mut table := Table{values: [10, 20, 30, 40]}\n",
        "  table.values[1] = 2\n",
        "  direct := table.values[2]\n",
        "  window := table.values[1..3].sum()\n",
        "  total := table.values.sum()\n",
        "  if table.values.len() == 4 && direct == 30 && window == 32 && total == 82 { return 42 }\n",
        "  return 1\n",
        "}\n",
    );
    let output = build_and_run("fixed-array-field-ops", source);
    assert_eq!(output.status.code(), Some(42));
}

#[test]
fn fixed_array_field_whole_replacement_executes() {
    if !backend_available() {
        return;
    }
    let source = concat!(
        "User { name: string }\n",
        "Users { values: [User; 2] }\n",
        "fn main() -> i32 {\n",
        "  mut users := Users{values: [User{name: \"old-a\".clone()}, User{name: \"old-b\".clone()}]}\n",
        "  users.values = [User{name: \"new-a\".clone()}, User{name: \"new-bb\".clone()}]\n",
        "  if users.values[0].name.len() == 5 && users.values[1].name.len() == 6 { return 42 }\n",
        "  return 1\n",
        "}\n",
    );
    let output = build_and_run("fixed-array-field-replacement", source);
    assert_eq!(output.status.code(), Some(42));
}

#[test]
fn fixed_array_type_rejects_newline_as_separator() {
    let mut sources = SourceMap::new();
    let checked = check(
        &mut sources,
        "newline-fixed-array.align",
        "Bad { values: [i64\n4] }\nfn main() {}\n",
    );
    assert!(checked.diags.has_errors());
}

#[test]
fn fixed_array_parameters_results_zero_and_37_fields_execute() {
    if !backend_available() {
        return;
    }
    let values = (1..=37)
        .map(|value| value.to_string())
        .collect::<Vec<_>>()
        .join(", ");
    let source = format!(
        concat!(
            "Bounds {{ empty: [i64; 0], values: [i64; 37] }}\n",
            "fn identity(xs: [i64; 37]) -> [i64; 37] = xs\n",
            "fn total(xs: [i64; 37]) -> i64 = xs.sum()\n",
            "fn main() -> i32 {{\n",
            "  bounds := Bounds{{empty: [], values: [{values}]}}\n",
            "  copied := identity(bounds.values)\n",
            "  if bounds.empty.len() == 0 && copied.len() == 37 && copied[36] == 37 && total(copied) == 703 {{ return 42 }}\n",
            "  return 1\n",
            "}}\n",
        ),
        values = values,
    );
    let output = build_and_run("fixed-array-field-boundaries", &source);
    assert_eq!(output.status.code(), Some(42));
}

#[test]
fn fixed_array_public_signature_round_trips_per_unit() {
    if !backend_available() {
        return;
    }
    let root =
        std::env::temp_dir().join(format!("align-fixed-array-per-unit-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(&root).expect("create fixed-array project");
    std::fs::write(
        root.join("dep.align"),
        "module dep\npub Table { values: [i64; 4] }\npub fn make() -> Table = Table{values: [10, 20, 3, 9]}\npub fn total(values: [i64; 4]) -> i64 = values.sum()\n",
    )
    .expect("write dependency");
    std::fs::write(
        root.join("main.align"),
        "module main\nimport dep\nfn main() -> i32 { table := dep.make(); return dep.total(table.values) as i32 }\n",
    )
    .expect("write entry");
    let output = Command::new(env!("CARGO_BIN_EXE_alignc"))
        .args(["run", "main.align", "--cc", "/usr/bin/cc"])
        .current_dir(&root)
        .env("ALIGNC_CACHE", "off")
        .output()
        .expect("run per-unit fixed-array program");
    let _ = std::fs::remove_dir_all(&root);
    assert_eq!(
        output.status.code(),
        Some(42),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
}

#[test]
fn borrowed_fixed_array_record_certifies_across_fallible_calls() {
    if !backend_available() {
        return;
    }
    let files = [
        (
            "table.align",
            concat!(
                "module table\n",
                "pub Table { values: [i64; 4] }\n",
                "pub fn positive() -> Table = Table{values: [1, 2, 3, 4]}\n",
                "pub fn negative() -> Table = Table{values: [-1, 2, 3, 4]}\n",
            ),
        ),
        (
            "walker.align",
            concat!(
                "module walker\n",
                "import table\n",
                "pub Fault { code: string, detail: string }\n",
                "LocalTable { values: [i64; 4] }\n",
                "NestedTable { inner: LocalTable }\n",
                "fn step(value: i64) -> Result<(), Fault> {\n",
                "  if value < 0 { return Err(Fault{code: \"NEGATIVE\".clone(), detail: \"value\".clone()}) }\n",
                "  return Ok(())\n",
                "}\n",
                "fn make(value: i64) -> LocalTable = LocalTable{values: [value, 2, 3, 4]}\n",
                "pub fn walk_imported(borrow input: table.Table) -> Result<(), Fault> {\n",
                "  values := input.values[0..input.values.len()]\n",
                "  step(values[0])?\n",
                "  return Ok(())\n",
                "}\n",
                "pub fn walk_direct(value: i64) -> Result<(), Fault> {\n",
                "  input := LocalTable{values: [value, 2, 3, 4]}\n",
                "  values := input.values[0..input.values.len()]\n",
                "  step(values[0])?\n",
                "  return Ok(())\n",
                "}\n",
                "pub fn walk_array(values: [i64; 4]) -> Result<(), Fault> {\n",
                "  view := values[0..values.len()]\n",
                "  step(view[0])?\n",
                "  return Ok(())\n",
                "}\n",
                "pub fn walk_constant() -> Result<(), Fault> {\n",
                "  input := LocalTable{values: [1, 2, 3, 4]}\n",
                "  values := input.values[0..input.values.len()]\n",
                "  step(values[0])?\n",
                "  return Ok(())\n",
                "}\n",
                "pub fn walk_mutated(value: i64, index: i64) -> Result<(), Fault> {\n",
                "  mut input := LocalTable{values: [1, 2, 3, 4]}\n",
                "  input.values[index] = value\n",
                "  values := input.values[0..input.values.len()]\n",
                "  step(values[index])?\n",
                "  return Ok(())\n",
                "}\n",
                "pub fn walk_replaced(value: i64) -> Result<(), Fault> {\n",
                "  mut input := LocalTable{values: [1, 2, 3, 4]}\n",
                "  input.values = [value, 6, 7, 8]\n",
                "  values := input.values[0..input.values.len()]\n",
                "  step(values[0])?\n",
                "  return Ok(())\n",
                "}\n",
                "pub fn walk_nested(value: i64) -> Result<(), Fault> {\n",
                "  input := NestedTable{inner: LocalTable{values: [value, 2, 3, 4]}}\n",
                "  values := input.inner.values[0..input.inner.values.len()]\n",
                "  step(values[0])?\n",
                "  return Ok(())\n",
                "}\n",
                "pub fn walk_nested_variable(value: i64) -> Result<(), Fault> {\n",
                "  inner := make(value)\n",
                "  input := NestedTable{inner: inner}\n",
                "  values := input.inner.values[0..input.inner.values.len()]\n",
                "  step(values[0])?\n",
                "  return Ok(())\n",
                "}\n",
                "pub fn walk_record_stores(value: i64, choose: bool) -> Result<(), Fault> {\n",
                "  returned := make(value)\n",
                "  copied := returned\n",
                "  mut input := if choose { copied } else { make(value) }\n",
                "  input = make(value)\n",
                "  values := input.values[0..input.values.len()]\n",
                "  step(values[0])?\n",
                "  return Ok(())\n",
                "}\n",
                "pub fn walk_array_stores(values: [i64; 4]) -> Result<(), Fault> {\n",
                "  mut selected := values\n",
                "  selected[1] = 2\n",
                "  input := LocalTable{values: selected}\n",
                "  view := input.values[0..input.values.len()]\n",
                "  step(view[0])?\n",
                "  return Ok(())\n",
                "}\n",
            ),
        ),
        (
            "main.align",
            concat!(
                "module main\n",
                "import table\n",
                "import walker\n",
                "pub fn main() -> Result<(), Error> {\n",
                "  positive := table.positive()\n",
                "  walker.walk_imported(positive) else { return Err(Error.Invalid) }\n",
                "  negative := table.negative()\n",
                "  match walker.walk_imported(negative) { Ok(_) => { return Err(Error.Invalid) } Err(_) => {} }\n",
                "  walker.walk_direct(1) else { return Err(Error.Invalid) }\n",
                "  match walker.walk_direct(-1) { Ok(_) => { return Err(Error.Invalid) } Err(_) => {} }\n",
                "  positive_values := [1, 2, 3, 4]\n",
                "  walker.walk_array(positive_values) else { return Err(Error.Invalid) }\n",
                "  negative_values := [-1, 2, 3, 4]\n",
                "  match walker.walk_array(negative_values) { Ok(_) => { return Err(Error.Invalid) } Err(_) => {} }\n",
                "  walker.walk_constant() else { return Err(Error.Invalid) }\n",
                "  walker.walk_mutated(9, 2) else { return Err(Error.Invalid) }\n",
                "  match walker.walk_mutated(-1, 2) { Ok(_) => { return Err(Error.Invalid) } Err(_) => {} }\n",
                "  walker.walk_replaced(9) else { return Err(Error.Invalid) }\n",
                "  match walker.walk_replaced(-1) { Ok(_) => { return Err(Error.Invalid) } Err(_) => {} }\n",
                "  walker.walk_nested(9) else { return Err(Error.Invalid) }\n",
                "  match walker.walk_nested(-1) { Ok(_) => { return Err(Error.Invalid) } Err(_) => {} }\n",
                "  walker.walk_nested_variable(9) else { return Err(Error.Invalid) }\n",
                "  match walker.walk_nested_variable(-1) { Ok(_) => { return Err(Error.Invalid) } Err(_) => {} }\n",
                "  walker.walk_record_stores(9, true) else { return Err(Error.Invalid) }\n",
                "  match walker.walk_record_stores(-1, false) { Ok(_) => { return Err(Error.Invalid) } Err(_) => {} }\n",
                "  positive_array_stores := [9, 0, 0, 0]\n",
                "  walker.walk_array_stores(positive_array_stores) else { return Err(Error.Invalid) }\n",
                "  negative_array_stores := [-1, 0, 0, 0]\n",
                "  match walker.walk_array_stores(negative_array_stores) { Ok(_) => { return Err(Error.Invalid) } Err(_) => {} }\n",
                "  return Ok(())\n",
                "}\n",
            ),
        ),
    ];

    let checked = diff_check_multi("fixed-array-fallible-borrow", &files, "main.align");
    assert!(
        !checked.whole_errors && !checked.per_unit_errors,
        "whole-program diagnostics:\n{}\nper-unit diagnostics:\n{}",
        checked.whole_diags,
        checked.per_unit_diags
    );
    let whole = build_and_run_multi("fixed-array-fallible-borrow-whole", &files, "main.align");
    assert_eq!(
        whole.status.code(),
        Some(0),
        "{}",
        String::from_utf8_lossy(&whole.stderr)
    );
    let per_unit = build_per_unit_multi("fixed-array-fallible-borrow-unit", &files, "main.align");
    let output = per_unit.link_and_run();
    assert_eq!(
        output.status.code(),
        Some(0),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
}

#[test]
fn fixed_move_record_array_field_constructs_and_drops_once() {
    if !backend_available() {
        return;
    }
    let source = concat!(
        "User { name: string }\n",
        "Users { values: [User; 2] }\n",
        "fn main() -> i32 {\n",
        "  users := Users{values: [User{name: \"alice\".clone()}, User{name: \"bob\".clone()}]}\n",
        "  if users.values.len() == 2 && users.values[0].name.len() == 5 && users.values[1].name.len() == 3 { return 42 }\n",
        "  return 1\n",
        "}\n",
    );
    let output = build_and_run("fixed-move-record-array-field", source);
    assert_eq!(output.status.code(), Some(42));
}

#[test]
fn thirteen_by_32_inline_fields_emit_no_dynamic_array_allocation() {
    if !backend_available() {
        return;
    }
    let fields = (0..13)
        .map(|index| format!("f{index}: [i64; 32]"))
        .collect::<Vec<_>>()
        .join(", ");
    let literal = (0..32)
        .map(|value| value.to_string())
        .collect::<Vec<_>>()
        .join(", ");
    let initializers = (0..13)
        .map(|index| format!("f{index}: [{literal}]"))
        .collect::<Vec<_>>()
        .join(", ");
    let source = format!(
        "NodeTable {{ {fields} }}\nfn main() -> i32 {{ table := NodeTable{{{initializers}}}; return (table.f0[0] + table.f12[31]) as i32 }}\n"
    );
    let mut sources = SourceMap::new();
    let checked = check(&mut sources, "fixed-array-13x32.align", &source);
    assert!(!checked.diags.has_errors());
    let mir = lower_to_mir(&checked.hir);
    let llvm = emit_llvm_ir(
        &mir,
        BuildTarget::Baseline,
        Profile::Release,
        true,
        &[],
        false,
    )
    .expect("emit optimized fixed-array LLVM");
    for forbidden in [
        "align_rt_array_builder_push",
        "align_rt_alloc",
        "align_rt_free",
    ] {
        assert!(
            !llvm.contains(forbidden),
            "unexpected allocation symbol {forbidden}:\n{llvm}"
        );
    }
}

#[test]
fn fixed_array_cardinality_and_excluded_elements_fail_before_lowering() {
    for (name, source) in [
        (
            "short",
            "Box { values: [i64; 2] }\nfn main() { value := Box{values: [1]} }\n",
        ),
        (
            "long",
            "Box { values: [i64; 1] }\nfn main() { value := Box{values: [1, 2]} }\n",
        ),
        ("nested", "Box { values: [[i64; 2]; 2] }\nfn main() {}\n"),
        (
            "owned-string",
            "Box { values: [string; 2] }\nfn main() {}\n",
        ),
        (
            "native-owner",
            "Box { values: [buffer; 2] }\nfn main() {}\n",
        ),
    ] {
        let mut sources = SourceMap::new();
        let checked = check(&mut sources, name, source);
        assert!(checked.diags.has_errors(), "must reject {name}");
    }
}

#[test]
fn fixed_array_field_slice_cannot_escape_local_record() {
    let mut sources = SourceMap::new();
    let checked = check(
        &mut sources,
        "fixed-field-slice-escape.align",
        "Table { values: [i64; 2] }\nfn bad() -> slice<i64> { table := Table{values: [20, 22]}; return table.values[0..2] }\nfn main() {}\n",
    );
    assert!(checked.diags.has_errors());
}

#[test]
fn fixed_array_field_element_store_preserves_borrow_lifetime() {
    let mut sources = SourceMap::new();
    let checked = check(
        &mut sources,
        "fixed-field-element-escape.align",
        concat!(
            "Holder { values: [str; 1] }\n",
            "fn bad() -> Holder {\n",
            "  mut output := Holder{values: [\"\"]}\n",
            "  arena {\n",
            "    owned := \"temporary\".clone()\n",
            "    view: str := owned\n",
            "    output.values[0] = view\n",
            "  }\n",
            "  return output\n",
            "}\n",
            "fn main() {}\n",
        ),
    );
    assert!(checked.diags.iter().any(|diagnostic| {
        diagnostic
            .message
            .contains("cannot be stored into a longer-lived array element")
    }), "a field-rooted element store must not hide an arena borrow");
}

#[test]
fn indexed_store_does_not_widen_nonfixed_field_places() {
    let mut sources = SourceMap::new();
    let checked = check(
        &mut sources,
        "nonfixed-field-index-store.align",
        concat!(
            "Holder { values: slice<i64> }\n",
            "fn bad(borrow mut holder: Holder) { holder.values[0] = 1 }\n",
            "fn main() {}\n",
        ),
    );
    assert!(checked.diags.iter().any(|diagnostic| {
        diagnostic
            .message
            .contains("only for fixed-array fields")
    }));
}

#[test]
fn generic_record_substitutes_fixed_array_element_once() {
    if !backend_available() {
        return;
    }
    let source = concat!(
        "Holder<T> { values: [T; 2] }\n",
        "fn main() -> i32 {\n",
        "  holder: Holder<i64> := Holder{values: [20, 22]}\n",
        "  return holder.values.sum() as i32\n",
        "}\n",
    );
    let output = build_and_run("generic-fixed-array-field", source);
    assert_eq!(output.status.code(), Some(42));
}

#[test]
fn malformed_fixed_field_slice_is_rejected_before_codegen() {
    let source = "Table { values: [i64; 2] }\nfn main() -> i32 { table := Table{values: [20, 22]}; return table.values.sum() as i32 }\n";
    let mut sources = SourceMap::new();
    let checked = check(&mut sources, "malformed-fixed-field.align", source);
    assert!(!checked.diags.has_errors());
    let mut mir = lower_to_mir(&checked.hir);
    let length = mir
        .fns
        .iter_mut()
        .flat_map(|function| &mut function.blocks)
        .flat_map(|block| &mut block.stmts)
        .find_map(|statement| match statement {
            align_mir::Stmt::Let(_, align_mir::Rvalue::MakeFieldSlice(_, _, length)) => {
                Some(length)
            }
            _ => None,
        })
        .expect("field pipeline emits a field slice");
    *length += 1;
    assert!(align_mir::producer::validate_mir_producers(&mir).is_err());
}

#[test]
fn incomplete_fixed_array_construction_cannot_certify_a_fallible_return() {
    let source = concat!(
        "Fault { code: string, detail: string }\n",
        "Table { values: [i64; 4] }\n",
        "fn step(value: i64) -> Result<(), Fault> {\n",
        "  if value < 0 { return Err(Fault{code: \"NEGATIVE\".clone(), detail: \"value\".clone()}) }\n",
        "  return Ok(())\n",
        "}\n",
        "fn walk(value: i64) -> Result<(), Fault> {\n",
        "  input := Table{values: [value, 2, 3, 4]}\n",
        "  values := input.values[0..input.values.len()]\n",
        "  step(values[0])?\n",
        "  return Ok(())\n",
        "}\n",
        "fn main() {}\n",
    );
    let mut sources = SourceMap::new();
    let checked = check(&mut sources, "incomplete-fixed-array.align", source);
    assert!(!checked.diags.has_errors());
    let mir = lower_to_mir(&checked.hir);
    assert!(align_mir::producer::validate_mir_producers(&mir).is_ok());
    for mutation in 0..5 {
        let mut malformed = mir.clone();
        let function = malformed
            .fns
            .iter_mut()
            .find(|function| function.name.as_str() == "walk")
            .expect("walk MIR");
        let block_index = function
            .blocks
            .iter()
            .position(|block| {
                block
                    .stmts
                    .iter()
                    .filter(|statement| matches!(statement, align_mir::Stmt::StoreIndex(..)))
                    .count()
                    >= 2
            })
            .expect("fixed-array construction block");
        let stores = function.blocks[block_index]
            .stmts
            .iter()
            .enumerate()
            .filter_map(|(index, statement)| {
                matches!(statement, align_mir::Stmt::StoreIndex(..)).then_some(index)
            })
            .collect::<Vec<_>>();
        let array_slot = match &function.blocks[block_index].stmts[stores[0]] {
            align_mir::Stmt::StoreIndex(slot, _, _) => *slot,
            _ => unreachable!(),
        };
        match mutation {
            0 => {
                function.blocks[block_index].stmts.remove(stores[0]);
            }
            1 => {
                let first = match &function.blocks[block_index].stmts[stores[0]] {
                    align_mir::Stmt::StoreIndex(_, index, _) => index.clone(),
                    _ => unreachable!(),
                };
                let align_mir::Stmt::StoreIndex(_, index, _) =
                    &mut function.blocks[block_index].stmts[stores[1]]
                else {
                    unreachable!();
                };
                *index = first;
            }
            2 => {
                let align_mir::Stmt::StoreIndex(_, index, _) =
                    &mut function.blocks[block_index].stmts[stores[0]]
                else {
                    unreachable!();
                };
                let align_mir::Operand::Const(align_mir::Const::Int(value, _)) = index else {
                    unreachable!();
                };
                *value = 4;
            }
            3 => {
                let store = function.blocks[block_index].stmts.remove(stores[0]);
                let load = function.blocks[block_index]
                    .stmts
                    .iter()
                    .position(|statement| {
                        matches!(statement,
                            align_mir::Stmt::Let(_, align_mir::Rvalue::Load(slot))
                                if *slot == array_slot)
                    })
                    .expect("fixed-array load");
                function.blocks[block_index].stmts.insert(load + 1, store);
            }
            4 => {
                let store = function.blocks[block_index].stmts.remove(stores[0]);
                let other = (block_index + 1) % function.blocks.len();
                function.blocks[other].stmts.push(store);
            }
            _ => unreachable!(),
        }
        assert!(
            align_mir::producer::validate_mir_producers(&malformed).is_err(),
            "accepted malformed fixed-array construction {mutation}"
        );
    }
}
