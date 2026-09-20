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
