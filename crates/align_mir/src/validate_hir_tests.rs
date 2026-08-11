use super::*;
use crate::validate_hir::{body_core_metadata_is_valid, body_ty_mangle};
use align_sema::{
    AggregateArrayElem, ArrayBuilderElem, FloatTy, FnEffect, IntTy, Layout, PrimScalar, Scalar, Ty,
    hir::{
        self, EnumDef, EnumVariant, FieldDef, FnTy, ImportedFn, ResourceDef,
        ReturnBorrowSummary, ReturnRegionSummary, StructDef, TaggedType, TupleDef,
    },
};
use align_diag::Diagnostics;
use align_lexer::tokenize;
use align_parser::parse_file;
use std::cell::Cell;

fn direct_program_name(call: &DirectCall) -> Option<&str> {
    match call {
        DirectCall::Program(target) => Some(target.as_str()),
        DirectCall::Runtime(_) => None,
    }
}

#[test]
fn aggregate_region_builder_source_survives_the_complete_hir_gate() {
    for (name, source) in [
        (
            "vector-new",
            "fn main() -> i32 { arena out { mut values: array_builder<vec4<i32>> := array_builder(out)\n return 0 } }\n",
        ),
        (
            "vector-push",
            "fn main() -> i32 { arena out { value: vec4<i32> := [1, 2, 3, 4]\n mut values: array_builder<vec4<i32>> := array_builder(out)\n values.push(value)\n return 0 } }\n",
        ),
        (
            "vector-build",
            "fn main() -> i32 { arena out { value: vec4<i32> := [1, 2, 3, 4]\n mut values: array_builder<vec4<i32>> := array_builder(out)\n values.push(value)\n built := values.build()\n return built.len() as i32 } }\n",
        ),
        (
            "vector-index",
            "fn main() -> i32 { arena out { value: vec4<i32> := [1, 2, 3, 4]\n mut values: array_builder<vec4<i32>> := array_builder(out)\n values.push(value)\n built := values.build()\n return built[0][1] } }\n",
        ),
        (
            "mask-build",
            "fn main() -> i32 { arena out { a: vec4<i32> := [1, 2, 3, 4]\n b: vec4<i32> := [0, 3, 2, 5]\n mut values: array_builder<mask4<i32>> := array_builder(out)\n values.push(a > b)\n built := values.build()\n selected := select(built[0], a, b)\n return selected[1] } }\n",
        ),
    ] {
        let program = checked_source_program(source);
        assert!(
            validate_hir::body_only_metadata_is_valid(&program),
            "{name}: bodies"
        );
        assert_eq!(lower_program(&program).fns.len(), 1, "{name}: lowering");
    }
}

#[test]
fn tagged_copy_fields_from_dynamic_struct_arrays_survive_the_hir_gate() {
    let source = r#"
Row {
  optional: Option<str>,
}

Fallible {
  value: Result<str, Error>,
}

fn main() -> i32 {
  arena out {
    mut rows: array_builder<Row> := array_builder(out)
    rows.push(Row { optional: Some("x") })
    built := rows.build()
    optional := built[0].optional else { "" }
    fixed := [Fallible { value: Ok("yz") }]
    fallible := fixed[0].value else { "" }
    return (optional.len() + fallible.len()) as i32
  }
}
"#;
    let program = checked_source_program(source);
    assert!(
        validate_hir::body_only_metadata_is_valid(&program),
        "Copy Option/Result element fields must survive the fail-closed HIR gate",
    );
    assert_eq!(lower_program(&program).fns.len(), 1);
}

fn declaration_header_program() -> hir::Program {
    let mut program = baseline_program();
    let slice_i32 = Ty::Slice(scalar_int(32));
    program.externs.push(hir::ExternFn {
        name: "c_read".to_string(),
        params: vec![int(64)],
        param_modes: vec![align_ast::ParamMode::ByValue],
        ret: int(64),
        return_borrow: ReturnBorrowSummary::None,
        return_region: ReturnRegionSummary::None,
        return_cleanup: hir::ReturnCleanupAbi::None,
    });
    program.imported_fns.push(ImportedFn {
        name: "dep$read".to_string(),
        params: vec![slice_i32],
        param_modes: vec![align_ast::ParamMode::Out],
        ret: slice_i32,
        return_provenance_known: true,
        return_borrow: ReturnBorrowSummary::Roots {
            params: vec![0],
            captures: Vec::new(),
        },
        return_region: ReturnRegionSummary::Roots {
            params: vec![0],
            captures: Vec::new(),
        },
        return_cleanup: hir::ReturnCleanupAbi::None,
        effect: FnEffect::Pure,
    });
    let span = align_span::Span::new(0, 0, 0);
    program.fns.push(hir::Fn {
        name: "worker".to_string(),
        origin: hir::FnOrigin::Source {
            is_entry: false,
            is_public: true,
        },
        params: vec![0],
        param_modes: vec![align_ast::ParamMode::ByValue],
        ret: Ty::Str,
        return_borrow: ReturnBorrowSummary::Roots {
            params: vec![0],
            captures: Vec::new(),
        },
        return_region: ReturnRegionSummary::Roots {
            params: vec![0],
            captures: Vec::new(),
        },
        return_cleanup: hir::ReturnCleanupAbi::None,
        locals: vec![
            hir::Local {
                id: 0,
                name: "value".to_string(),
                ty: Ty::Str,
                is_mut: false,
                is_param: true,
                align: None,
            },
            hir::Local {
                id: 1,
                name: "copy".to_string(),
                ty: Ty::Str,
                is_mut: false,
                is_param: false,
                align: None,
            },
        ],
        body: hir::Block {
            stmts: vec![hir::Stmt::Let {
                local: 1,
                init: hir::Expr {
                    kind: hir::ExprKind::Local(0),
                    ty: Ty::Str,
                    span,
                },
            }],
            value: Some(Box::new(hir::Expr {
                kind: hir::ExprKind::Local(0),
                ty: Ty::Str,
                span,
            })),
        },
        span,
        drop_locals: Vec::new(),
        drop_individual_locals: Vec::new(),
        drop_individual_exprs: Default::default(),
    });
    program
}

fn checked_source_program(source: &str) -> hir::Program {
    let mut diagnostics = Diagnostics::new();
    let tokens = tokenize(0, source, &mut diagnostics);
    let file = parse_file(tokens, &mut diagnostics);
    let program = align_sema::check_file(&file, &mut diagnostics);
    assert!(
        !diagnostics.has_errors(),
        "replay fixture must check before mutation: {:?}",
        diagnostics
            .iter()
            .map(|diagnostic| &diagnostic.message)
            .collect::<Vec<_>>()
    );
    program
}

#[test]
fn hir_body_validator_accepts_signed_minimum_only_under_direct_negation() {
    let source = "fn signed_minimum() -> i64 = -9223372036854775808\nfn main() -> i32 = 0\n";
    let valid = checked_source_program(source);
    assert!(
        validate_hir::body_only_metadata_is_valid(&valid),
        "the checked INT_MIN unary representation must survive the HIR gate",
    );

    let mut malformed = valid;
    let expression = body_value_expression_mut(&mut malformed, "signed_minimum");
    let hir::ExprKind::Unary { expr, .. } = &expression.kind else {
        panic!("signed minimum lost its unary HIR representation")
    };
    *expression = (**expr).clone();
    assert!(
        !validate_hir::body_only_metadata_is_valid(&malformed),
        "the one-past signed magnitude must fail closed outside direct unary negation",
    );
}

fn checked_interface_program(
    provenance: Option<(ReturnBorrowSummary, ReturnRegionSummary)>,
    effect: FnEffect,
) -> hir::Program {
    let dependency = "module dep\npub fn identity(value: str) -> str {}\n";
    let consumer = "import dep\nfn consume(value: str) -> str = dep.identity(value)\nfn main() -> i32 = 0\n";
    let mut diagnostics = Diagnostics::new();
    let dependency_tokens = tokenize(0, dependency, &mut diagnostics);
    let dependency_file = parse_file(dependency_tokens, &mut diagnostics);
    let consumer_tokens = tokenize(1, consumer, &mut diagnostics);
    let consumer_file = parse_file(consumer_tokens, &mut diagnostics);
    assert!(!diagnostics.has_errors(), "interface fixture must parse");
    let modules = [
        align_sema::Module {
            path: "dep".to_string(),
            file: &dependency_file,
            is_entry: false,
            interface_only: true,
        },
        align_sema::Module {
            path: "main".to_string(),
            file: &consumer_file,
            is_entry: true,
            interface_only: false,
        },
    ];
    let mut external_effects = std::collections::HashMap::new();
    external_effects.insert("dep$identity".to_string(), effect);
    let mut external_provenance = align_sema::ExternalReturnProvenance::new();
    if let Some(provenance) = provenance {
        external_provenance.insert(
            "dep$identity".to_string(),
            (provenance.0, provenance.1, hir::ReturnCleanupAbi::None),
        );
    }
    let program = if external_provenance.is_empty() {
        align_sema::check_program_with_effects(&modules, &external_effects, &mut diagnostics)
    } else {
        align_sema::check_program_with_interface_facts(
            &modules,
            &external_effects,
            &external_provenance,
            &mut diagnostics,
        )
    };
    assert!(
        !diagnostics.has_errors(),
        "interface fixture must check"
    );
    program
}

fn assert_replay_rejects_without_mutating(program: hir::Program, message: &str) {
    let before = format!("{program:#?}");
    assert!(!align_sema::checked_hir_body_facts_are_valid(&program), "{message}");
    assert_eq!(
        format!("{program:#?}"),
        before,
        "rejected replay must not mutate input HIR: {message}"
    );
}

fn assert_body_entrypoints_empty(label: &str, program: &hir::Program) {
    assert!(
        !body_core_metadata_is_valid(program),
        "{label}: body validator accepted malformed root"
    );
    let source_map = SourceMap::new();
    for lowered in [
        lower_program(program),
        lower_program_located(program, &source_map),
        lower_program_per_unit(program),
        lower_program_per_unit_located(program, &source_map),
    ] {
        assert!(
            is_empty(&lowered),
            "{label}: malformed root published partial MIR"
        );
    }
}

fn fn_type_header_program() -> hir::Program {
    let mut program = baseline_program();
    program.fn_types[0] = FnTy {
        params: vec![
            (align_ast::ParamMode::ByValue, Scalar::Str),
            (align_ast::ParamMode::ByValue, Scalar::Str),
        ],
        ret: Ty::Str,
        return_borrow: ReturnBorrowSummary::Roots {
            params: vec![0, 1],
            captures: Vec::new(),
        },
        return_region: ReturnRegionSummary::Roots {
            params: vec![0, 1],
            captures: Vec::new(),
        },
        return_cleanup: hir::ReturnCleanupAbi::None,
        effect: Cell::new(FnEffect::Pure),
    };
    program
}

fn summary_header_program() -> hir::Program {
    let mut program = declaration_header_program();
    program.fns[0].params = vec![0, 1];
    program.fns[0].param_modes = vec![
        align_ast::ParamMode::ByValue,
        align_ast::ParamMode::ByValue,
    ];
    program.fns[0].locals[1].ty = Ty::Str;
    program.fns[0].locals[1].is_param = true;
    program.fns[0].body.stmts.clear();
    program.fns[0].return_borrow = ReturnBorrowSummary::Roots {
        params: vec![0, 1],
        captures: Vec::new(),
    };
    program.fns[0].return_region = ReturnRegionSummary::Roots {
        params: vec![0, 1],
        captures: Vec::new(),
    };
    program
}

fn main_header_program(params: Vec<Ty>, param_modes: Vec<align_ast::ParamMode>, ret: Ty) -> hir::Program {
    let mut program = baseline_program();
    push_builtin_error(&mut program);
    let span = align_span::Span::new(0, 0, 0);
    let local_params = params
        .iter()
        .enumerate()
        .map(|(id, &ty)| hir::Local {
            id: id as u32,
            name: format!("arg{id}"),
            ty,
            is_mut: false,
            is_param: true,
            align: None,
        })
        .collect();
    program.fns.push(hir::Fn {
        name: "main".to_string(),
        origin: hir::FnOrigin::Source {
            is_entry: true,
            is_public: false,
        },
        params: (0..params.len()).map(|id| id as u32).collect(),
        param_modes,
        ret,
        return_borrow: ReturnBorrowSummary::None,
        return_region: ReturnRegionSummary::None,
        return_cleanup: hir::ReturnCleanupAbi::None,
        locals: local_params,
        body: hir::Block {
            stmts: Vec::new(),
            value: None,
        },
        span,
        drop_locals: Vec::new(),
        drop_individual_locals: Vec::new(),
        drop_individual_exprs: Default::default(),
    });
    program
}

fn assert_header_rejected(label: &str, program: &hir::Program) {
    assert!(
        validate_hir::global_type_metadata_is_valid(program),
        "{label}: header fixture must remain graph-valid"
    );
    assert!(
        validate_hir::type_placement_metadata_is_valid(program),
        "{label}: header fixture must remain placement-valid"
    );
    assert!(
        validate_hir::nominal_link_metadata_is_valid(program),
        "{label}: header fixture must remain nominal/link-valid"
    );
    assert!(
        !validate_hir::declaration_header_metadata_is_valid(program),
        "{label}: declaration/header validator accepted malformed metadata"
    );
    let source_map = SourceMap::new();
    for lowered in [
        lower_program(program),
        lower_program_located(program, &source_map),
        lower_program_per_unit(program),
        lower_program_per_unit_located(program, &source_map),
    ] {
        assert!(is_empty(&lowered), "{label}: invalid header published MIR");
    }
}

#[test]
fn malformed_hir_declaration_header_metadata_fails_closed() {
    let base = declaration_header_program();
    assert_one_header_mutation("extern-name", &base, |program| {
        program.externs[0].name.push('\0');
    });
    assert_one_header_mutation("extern-main", &base, |program| {
        program.externs[0].name = "main".to_string();
    });
    assert_one_header_mutation("extern-arity", &base, |program| {
        program.externs[0].param_modes.clear();
    });
    assert_one_header_mutation("extern-mode", &base, |program| {
        program.externs[0].param_modes[0] = align_ast::ParamMode::Out;
    });
    assert_one_header_mutation("extern-summary", &base, |program| {
        program.externs[0].return_borrow = ReturnBorrowSummary::Roots {
            params: vec![0],
            captures: Vec::new(),
        };
    });
    assert_one_header_mutation("extern-duplicate-name", &base, |program| {
        program.externs.push(program.externs[0].clone());
    });
    assert_one_header_mutation("import-name", &base, |program| {
        program.imported_fns[0].name.clear();
    });
    assert_one_header_mutation("import-main", &base, |program| {
        program.imported_fns[0].name = "main".to_string();
    });
    let mut imported_copy_borrow = base.clone();
    imported_copy_borrow.imported_fns[0].param_modes[0] = align_ast::ParamMode::Borrow;
    assert!(
        validate_hir::declaration_header_metadata_is_valid(&imported_copy_borrow),
        "an imported shared borrow of Copy slice storage is valid declaration metadata"
    );
    assert_one_header_mutation("import-out-type", &base, |program| {
        program.imported_fns[0].params[0] = Ty::Str;
    });
    assert_one_header_mutation("import-summary-order", &base, |program| {
        program.imported_fns[0].return_region = ReturnRegionSummary::None;
    });
    assert_one_header_mutation("import-summary-captures", &base, |program| {
        program.imported_fns[0].return_borrow = ReturnBorrowSummary::Roots {
            params: vec![0],
            captures: vec![0],
        };
    });
    assert_one_header_mutation("import-summary-range", &base, |program| {
        program.imported_fns[0].return_borrow = ReturnBorrowSummary::Roots {
            params: vec![1],
            captures: Vec::new(),
        };
        program.imported_fns[0].return_region = ReturnRegionSummary::Roots {
            params: vec![1],
            captures: Vec::new(),
        };
    });
    assert_one_header_mutation("import-summary-non-borrowable", &base, |program| {
        program.imported_fns[0].params[0] = int(64);
        program.imported_fns[0].return_borrow = ReturnBorrowSummary::Roots {
            params: vec![0],
            captures: Vec::new(),
        };
        program.imported_fns[0].return_region = ReturnRegionSummary::Roots {
            params: vec![0],
            captures: Vec::new(),
        };
    });
    assert_one_header_mutation("import-duplicate-name", &base, |program| {
        program.imported_fns.push(program.imported_fns[0].clone());
    });
    assert_one_header_mutation("stored-name", &base, |program| {
        program.fns[0].name.push('\0');
    });
    assert_one_header_mutation("stored-duplicate-name", &base, |program| {
        program.fns.push(program.fns[0].clone());
    });
    assert_one_header_mutation("stored-span", &base, |program| {
        program.fns[0].span = align_span::Span::new(0, 2, 1);
    });
    assert_one_header_mutation("stored-parameter-id", &base, |program| {
        program.fns[0].params[0] = 1;
    });
    assert_one_header_mutation("stored-parameter-mode", &base, |program| {
        program.fns[0].param_modes[0] = align_ast::ParamMode::BorrowMut;
    });

    let mut mutable_region = declaration_header_program();
    let function = &mut mutable_region.fns[0];
    function.locals.truncate(1);
    function.locals[0].ty = Ty::ArenaHandle;
    function.locals[0].is_mut = true;
    function.param_modes[0] = align_ast::ParamMode::BorrowMut;
    function.ret = Ty::Unit;
    function.return_borrow = ReturnBorrowSummary::None;
    function.return_region = ReturnRegionSummary::None;
    function.body.stmts.clear();
    function.body.value = Some(Box::new(hir::Expr {
        kind: hir::ExprKind::Unit,
        ty: Ty::Unit,
        span: function.span,
    }));
    assert_header_rejected("mutable-region-parameter", &mutable_region);
    assert_one_header_mutation("stored-parameter-name", &base, |program| {
        program.fns[0].locals[0].name = "bad-name".to_string();
    });
    assert_one_header_mutation("stored-local-id", &base, |program| {
        program.fns[0].locals[0].id = 1;
    });
    assert_one_header_mutation("stored-local-name", &base, |program| {
        program.fns[0].locals[0].name.clear();
    });
    assert_one_header_mutation("stored-local-parameter-bit", &base, |program| {
        program.fns[0].locals[0].is_param = false;
    });
    assert_one_header_mutation("stored-local-alignment", &base, |program| {
        program.fns[0].locals.push(hir::Local {
            id: 2,
            name: "aligned".to_string(),
            ty: int(32),
            is_mut: false,
            is_param: false,
            align: Some(3),
        });
    });
    assert_one_header_mutation("stored-drop-order", &base, |program| {
        program.fns[0].drop_locals = vec![0, 0];
    });
    assert_one_header_mutation("stored-drop-subset", &base, |program| {
        program.fns[0].drop_individual_locals = vec![0];
        program.fns[0].drop_locals.clear();
    });
    assert_one_header_mutation("stored-drop-range", &base, |program| {
        program.fns[0].drop_locals = vec![2];
    });
    assert_one_header_mutation("stored-individual-drop-range", &base, |program| {
        program.fns[0].drop_individual_locals = vec![2];
    });
    assert_one_header_mutation("lifted-origin", &base, |program| {
        program.fns[0].origin = hir::FnOrigin::Lifted { capture_count: 2 };
    });
    assert_one_header_mutation("lifted-origin-mode", &base, |program| {
        program.fns[0].origin = hir::FnOrigin::Lifted { capture_count: 0 };
        program.fns[0].locals[0].is_param = false;
        program.fns[0].param_modes[0] = align_ast::ParamMode::Out;
    });
    let mut lifted_summary = base.clone();
    lifted_summary.fns[0].origin = hir::FnOrigin::Lifted { capture_count: 1 };
    lifted_summary.fns[0].locals[0].is_param = false;
    lifted_summary.fns[0].return_borrow = ReturnBorrowSummary::Roots {
        params: Vec::new(),
        captures: vec![0],
    };
    lifted_summary.fns[0].return_region = ReturnRegionSummary::Roots {
        params: Vec::new(),
        captures: vec![0],
    };
    assert!(
        validate_hir::declaration_header_metadata_is_valid(&lifted_summary),
        "an in-range lifted capture summary is valid declaration metadata"
    );
    assert!(
        align_sema::checked_hir_body_facts_are_valid(&lifted_summary),
        "the lifted capture summary must agree with producer replay"
    );
    assert_one_header_mutation("lifted-origin-summary-range", &lifted_summary, |program| {
        program.fns[0].return_borrow = ReturnBorrowSummary::Roots {
            params: Vec::new(),
            captures: vec![1],
        };
        program.fns[0].return_region = ReturnRegionSummary::Roots {
            params: Vec::new(),
            captures: vec![1],
        };
    });

    let fn_base = fn_type_header_program();
    assert!(validate_hir::declaration_header_metadata_is_valid(&fn_base));
    let mut copy_borrow_fn = fn_base.clone();
    copy_borrow_fn.fn_types[0].params[0].0 = align_ast::ParamMode::Borrow;
    assert!(
        validate_hir::declaration_header_metadata_is_valid(&copy_borrow_fn),
        "a function-value shared borrow of Copy str storage is valid declaration metadata"
    );
    assert_one_header_mutation("fn-type-mode", &fn_base, |program| {
        program.fn_types[0].params[0].0 = align_ast::ParamMode::Out;
    });
    assert_one_header_mutation("fn-type-summary-order", &fn_base, |program| {
        program.fn_types[0].return_borrow = ReturnBorrowSummary::Roots {
            params: vec![1, 0],
            captures: Vec::new(),
        };
        program.fn_types[0].return_region = ReturnRegionSummary::Roots {
            params: vec![1, 0],
            captures: Vec::new(),
        };
    });
    assert_one_header_mutation("fn-type-summary-duplicate", &fn_base, |program| {
        program.fn_types[0].return_borrow = ReturnBorrowSummary::Roots {
            params: vec![0, 0],
            captures: Vec::new(),
        };
        program.fn_types[0].return_region = ReturnRegionSummary::Roots {
            params: vec![0, 0],
            captures: Vec::new(),
        };
    });
    assert_one_header_mutation("fn-type-summary-range", &fn_base, |program| {
        program.fn_types[0].return_borrow = ReturnBorrowSummary::Roots {
            params: vec![2],
            captures: Vec::new(),
        };
        program.fn_types[0].return_region = ReturnRegionSummary::Roots {
            params: vec![2],
            captures: Vec::new(),
        };
    });
    let mut stale_fn_capture = fn_base.clone();
    stale_fn_capture.fn_types[0].return_borrow = ReturnBorrowSummary::Roots {
            params: vec![0, 1],
            captures: vec![0],
    };
    stale_fn_capture.fn_types[0].return_region = ReturnRegionSummary::Roots {
            params: vec![0, 1],
            captures: vec![0],
    };
    assert!(
        validate_hir::declaration_header_metadata_is_valid(&stale_fn_capture),
        "a canonical function-type capture summary is structurally valid"
    );
    assert_replay_rejects_without_mutating(
        stale_fn_capture.clone(),
        "a function-type capture summary without a concrete producer target must fail replay",
    );
    let source_map = SourceMap::new();
    for lowered in [
        lower_program(&stale_fn_capture),
        lower_program_located(&stale_fn_capture, &source_map),
        lower_program_per_unit(&stale_fn_capture),
        lower_program_per_unit_located(&stale_fn_capture, &source_map),
    ] {
        assert!(
            is_empty(&lowered),
            "fn-type-summary-captures: stale producer fact published MIR"
        );
    }

    let summary_base = summary_header_program();
    assert!(validate_hir::declaration_header_metadata_is_valid(&summary_base));
    assert_one_header_mutation("stored-summary-order", &summary_base, |program| {
        program.fns[0].return_borrow = ReturnBorrowSummary::Roots {
            params: vec![1, 0],
            captures: Vec::new(),
        };
        program.fns[0].return_region = ReturnRegionSummary::Roots {
            params: vec![1, 0],
            captures: Vec::new(),
        };
    });
    assert_one_header_mutation("stored-summary-range", &summary_base, |program| {
        program.fns[0].return_borrow = ReturnBorrowSummary::Roots {
            params: vec![2],
            captures: Vec::new(),
        };
        program.fns[0].return_region = ReturnRegionSummary::Roots {
            params: vec![2],
            captures: Vec::new(),
        };
    });
    assert_one_header_mutation("stored-summary-empty", &summary_base, |program| {
        program.fns[0].return_borrow = ReturnBorrowSummary::Roots {
            params: Vec::new(),
            captures: Vec::new(),
        };
        program.fns[0].return_region = ReturnRegionSummary::Roots {
            params: Vec::new(),
            captures: Vec::new(),
        };
    });
    assert_one_header_mutation("stored-summary-non-borrowable", &summary_base, |program| {
        program.fns[0].locals[1].ty = Ty::String;
        program.fns[0].return_borrow = ReturnBorrowSummary::Roots {
            params: vec![1],
            captures: Vec::new(),
        };
        program.fns[0].return_region = ReturnRegionSummary::Roots {
            params: vec![1],
            captures: Vec::new(),
        };
    });
    assert_one_header_mutation("stored-summary-captures", &summary_base, |program| {
        program.fns[0].return_borrow = ReturnBorrowSummary::Roots {
            params: vec![0, 1],
            captures: vec![0],
        };
        program.fns[0].return_region = ReturnRegionSummary::Roots {
            params: vec![0, 1],
            captures: vec![0],
        };
    });
}

#[test]
fn malformed_hir_callable_namespace_fails_closed() {
    fn assert_unpublished(label: &str, program: &hir::Program) {
        let source_map = SourceMap::new();
        for lowered in [
            lower_program(program),
            lower_program_located(program, &source_map),
            lower_program_per_unit(program),
            lower_program_per_unit_located(program, &source_map),
        ] {
            assert!(is_empty(&lowered), "{label}: malformed callable namespace published MIR");
        }
    }

    let mut stored = checked_source_program(
        "fn helper(value: i64) -> i64 = value\n\
         fn main() -> i32 {\n  unused := helper(1)\n  return 0\n}\n",
    );
    stored.fns[0].name.push('\0');
    assert_unpublished("stored-name-nul", &stored);

    let mut direct = checked_source_program(
        "fn helper(value: i64) -> i64 = value\n\
         fn main() -> i32 {\n  unused := helper(1)\n  return 0\n}\n",
    );
    let call = direct.fns[1]
        .body
        .stmts
        .iter_mut()
        .find_map(|statement| match statement {
            hir::Stmt::Let { init, .. } => match &mut init.kind {
                hir::ExprKind::Call { func, .. } => Some(func),
                _ => None,
            },
            _ => None,
        })
        .expect("fixture contains a direct call");
    call.clear();
    assert_unpublished("empty-direct-target", &direct);

    let mut declarations = declaration_header_program();
    declarations.imported_fns[0].name.push('\0');
    declarations.externs[0].name.clear();
    assert_unpublished("import-before-extern-name", &declarations);
}

#[test]
fn main_header_abi_matrix_is_exhaustive() {
    let result = Ty::Result(Scalar::Unit, Scalar::Enum(1));
    let argv = Ty::DynArray(Scalar::Str);
    for (label, params, modes, ret) in [
        ("main-unit", Vec::new(), Vec::new(), Ty::Unit),
        ("main-i32", Vec::new(), Vec::new(), int(32)),
        ("main-result", Vec::new(), Vec::new(), result),
        (
            "main-argv-result",
            vec![argv],
            vec![align_ast::ParamMode::ByValue],
            result,
        ),
    ] {
        let program = main_header_program(params, modes, ret);
        assert!(
            validate_hir::global_type_metadata_is_valid(&program)
                && validate_hir::type_placement_metadata_is_valid(&program)
                && validate_hir::nominal_link_metadata_is_valid(&program)
                && validate_hir::declaration_header_metadata_is_valid(&program),
            "{label}: valid main ABI rejected"
        );
    }

    assert_header_rejected(
        "main-no-arg-unsigned-i32",
        &main_header_program(
            Vec::new(),
            Vec::new(),
            Ty::Int(IntTy {
                bits: 32,
                signed: false,
            }),
        ),
    );
    assert_header_rejected(
        "main-no-arg-float",
        &main_header_program(Vec::new(), Vec::new(), Ty::Float(FloatTy { bits: 64 })),
    );
    assert_header_rejected(
        "main-argv-unit",
        &main_header_program(vec![argv], vec![align_ast::ParamMode::ByValue], Ty::Unit),
    );
    assert_header_rejected(
        "main-argv-i32",
        &main_header_program(vec![argv], vec![align_ast::ParamMode::ByValue], int(32)),
    );
    assert_header_rejected(
        "main-argv-mode",
        &main_header_program(vec![argv], vec![align_ast::ParamMode::Out], result),
    );
    assert_header_rejected(
        "main-argv-type",
        &main_header_program(
            vec![Ty::DynArray(Scalar::Int(IntTy { bits: 64, signed: true }))],
            vec![align_ast::ParamMode::ByValue],
            result,
        ),
    );
    assert_header_rejected(
        "main-argument-count",
        &main_header_program(
            vec![argv, argv],
            vec![align_ast::ParamMode::ByValue, align_ast::ParamMode::ByValue],
            result,
        ),
    );

    let source_non_entry = main_header_program(Vec::new(), Vec::new(), Ty::Unit);
    assert_one_header_mutation("main-non-entry-origin", &source_non_entry, |program| {
        program.fns[0].origin = hir::FnOrigin::Source {
            is_entry: false,
            is_public: true,
        };
    });
    assert_one_header_mutation("main-monomorph-origin", &source_non_entry, |program| {
        program.fns[0].origin = hir::FnOrigin::Monomorph;
    });
    assert_one_header_mutation("main-lifted-origin", &source_non_entry, |program| {
        program.fns[0].origin = hir::FnOrigin::Lifted { capture_count: 0 };
    });

    let valid_error = main_header_program(vec![argv], vec![align_ast::ParamMode::ByValue], result);
    assert_one_header_mutation("main-error-code-width", &valid_error, |program| {
        program.enums[1].variants[4].payload = vec![Scalar::Int(IntTy {
            bits: 64,
            signed: true,
        })];
    });
    assert_one_header_mutation("main-error-variant-order", &valid_error, |program| {
        program.enums[1].variants.swap(0, 1);
    });
    assert_one_header_mutation("main-error-variant-name", &valid_error, |program| {
        program.enums[1].variants[0].name = "Unknown".to_string();
    });
    assert_one_header_mutation("main-error-name", &valid_error, |program| {
        program.enums[1].name = "OtherError".to_string();
    });
    assert_one_header_mutation("main-error-source-name", &valid_error, |program| {
        program.enums[1].source_name = "OtherError".to_string();
    });
    assert_one_header_mutation("main-error-variant-count", &valid_error, |program| {
        program.enums[1].variants.pop();
    });
    assert_one_header_mutation("main-error-extra-variant", &valid_error, |program| {
        program.enums[1].variants.insert(4, EnumVariant {
            name: "Extra".to_string(),
            payload: Vec::new(),
            field_base: 1,
        });
    });
}

#[test]
fn valid_hir_declaration_header_preflight_is_mir_identity() {
    let mut base = declaration_header_program();
    for effect in [FnEffect::Pure, FnEffect::Impure, FnEffect::Unknown] {
        base.imported_fns[0].effect = effect;
        assert!(validate_hir::declaration_header_metadata_is_valid(&base));
        let source_map = SourceMap::new();
        let checked = lower_program_per_unit(&base);
        let unchecked = lower_program_unchecked(&base, None, true);
        assert_eq!(format!("{checked:#?}"), format!("{unchecked:#?}"));
        let located = lower_program_per_unit_located(&base, &source_map);
        let located_unchecked = lower_program_unchecked(
            &base,
            Some(Rc::new(SourceLines::from_map(&source_map))),
            true,
        );
        assert_eq!(format!("{located:#?}"), format!("{located_unchecked:#?}"));
        assert_eq!(checked.imported_fns.len(), 1);
        assert_eq!(checked.imported_fns[0].name.as_str(), "dep$read");
    }

    let mut lifted = declaration_header_program();
    lifted.fns[0].origin = hir::FnOrigin::Lifted { capture_count: 0 };
    lifted.fns[0].locals[0].is_param = false;
    lifted.fns[0].return_borrow = ReturnBorrowSummary::None;
    lifted.fns[0].return_region = ReturnRegionSummary::None;
    assert!(validate_hir::declaration_header_metadata_is_valid(&lifted));
}

#[test]
fn valid_header_does_not_consume_body_facts() {
    let mut program = declaration_header_program();
    program.fns[0]
        .drop_individual_exprs
        .insert(align_span::Span::new(999, 4, 4), false);
    assert!(validate_hir::declaration_header_metadata_is_valid(&program));
    assert!(!align_sema::checked_hir_body_facts_are_valid(&program));
    assert!(is_empty(&lower_program(&program)));

    let mut body_local_type = declaration_header_program();
    body_local_type.fns[0].locals.push(hir::Local {
        id: 2,
        name: "task_value".to_string(),
        ty: Ty::Task(Scalar::Int(IntTy { bits: 64, signed: true })),
        is_mut: false,
        is_param: false,
        align: None,
    });
    assert!(validate_hir::declaration_header_metadata_is_valid(&body_local_type));
}

#[test]
fn hir_body_validator_json_scan_copy_row() {
    let input = body_test_expr(hir::ExprKind::Str("[]".to_string()), Ty::Str);
    let scanner = body_test_expr(
        hir::ExprKind::JsonScan {
            struct_id: 0,
            input: Box::new(input),
        },
        Ty::JsonScanner(0),
    );
    let mut program = baseline_program();
    program
        .fns
        .push(body_unit_case("json_scan_copy_row", scanner.clone()));
    assert!(validate_hir::json_scan_copy_rows_are_valid(&program));

    program.structs[0].fields[0].ty = Ty::DynArray(scalar_int(64));
    assert!(!validate_hir::json_scan_copy_rows_are_valid(&program));

    let nested_scanner = body_test_expr(
        hir::ExprKind::Block(hir::Block {
            stmts: Vec::new(),
            value: Some(Box::new(scanner.clone())),
        }),
        Ty::JsonScanner(0),
    );
    let mut nested_program = baseline_program();
    nested_program
        .fns
        .push(body_unit_case("json_scan_nested_copy_row", nested_scanner));
    assert!(validate_hir::json_scan_copy_rows_are_valid(&nested_program));
    nested_program.structs[0].fields[0].ty = Ty::Char;
    assert!(!validate_hir::json_scan_copy_rows_are_valid(&nested_program));

    let mut missing_struct = baseline_program();
    missing_struct.fns.push(body_unit_case(
        "json_scan_missing_row",
        body_test_expr(
            hir::ExprKind::JsonScan {
                struct_id: 99,
                input: Box::new(body_test_expr(hir::ExprKind::Str("[]".to_string()), Ty::Str)),
            },
            Ty::JsonScanner(99),
        ),
    ));
    assert!(!validate_hir::json_scan_copy_rows_are_valid(&missing_struct));

    let mut cyclic_struct = baseline_program();
    cyclic_struct.structs[0].fields[0].ty = Ty::Struct(0);
    cyclic_struct
        .fns
        .push(body_unit_case("json_scan_cyclic_row", scanner.clone()));
    assert!(!validate_hir::json_scan_copy_rows_are_valid(&cyclic_struct));

    let mut optional_owned = baseline_program();
    optional_owned.structs.push(StructDef {
        name: "OwnedRecord".to_string(),
        source_name: "OwnedRecord".to_string(),
        fields: vec![FieldDef {
            name: "xs".to_string(),
            ty: Ty::DynArray(scalar_int(64)),
        }],
        align: None,
        c_repr: false,
    });
    optional_owned.structs[0].fields[0].ty = Ty::Option(Scalar::Struct(1));
    optional_owned
        .fns
        .push(body_unit_case("json_scan_optional_owned_row", scanner.clone()));
    assert!(!validate_hir::json_scan_copy_rows_are_valid(&optional_owned));

    let mut union_owned = baseline_program();
    union_owned.structs.push(StructDef {
        name: "OwnedRecord".to_string(),
        source_name: "OwnedRecord".to_string(),
        fields: vec![FieldDef {
            name: "xs".to_string(),
            ty: Ty::DynArray(scalar_int(64)),
        }],
        align: None,
        c_repr: false,
    });
    union_owned.enums.push(EnumDef {
        name: "OwnedChoice".to_string(),
        source_name: "OwnedChoice".to_string(),
        variants: vec![EnumVariant {
            name: "Value".to_string(),
            payload: vec![Scalar::Struct(1)],
            field_base: 1,
        }],
    });
    union_owned.structs[0].fields[0].ty = Ty::Enum(1);
    union_owned
        .fns
        .push(body_unit_case("json_scan_union_owned_row", scanner));
    assert!(!validate_hir::json_scan_copy_rows_are_valid(&union_owned));
}

#[test]
fn hir_program_json_scan_copy_row() {
    let input = body_test_expr(hir::ExprKind::Str("[]".to_string()), Ty::Str);
    let scanner = body_test_expr(
        hir::ExprKind::JsonScan {
            struct_id: 0,
            input: Box::new(input),
        },
        Ty::JsonScanner(0),
    );
    let mut program = baseline_program();
    program.fns.push(body_unit_case("json_scan_copy_row", scanner));
    assert!(validate_hir::json_scan_copy_rows_are_valid(&program));
    assert_eq!(
        validate_hir::json_scan_validation_reason(&program),
        Ok(()),
        "the accepted scanner envelope must pass the reason seam"
    );
    assert!(align_sema::checked_hir_body_depth_is_valid(&program));
    assert!(validate_hir::global_type_metadata_is_valid(&program));
    assert!(validate_hir::type_placement_metadata_is_valid(&program));
    assert!(validate_hir::nominal_link_metadata_is_valid(&program));
    assert!(validate_hir::declaration_header_metadata_is_valid(&program));

    let source_map = SourceMap::new();
    let mut invalid_schema = program.clone();
    invalid_schema.structs[0].fields[1].ty = Ty::Char;
    assert!(!validate_hir::json_scan_copy_rows_are_valid(&invalid_schema));
    for lowered in [
        lower_program(&invalid_schema),
        lower_program_located(&invalid_schema, &source_map),
        lower_program_per_unit(&invalid_schema),
        lower_program_per_unit_located(&invalid_schema, &source_map),
    ] {
        assert!(is_empty(&lowered), "invalid scanner schema published MIR");
    }

    let mut direct_owned = program.clone();
    direct_owned.structs[0].fields[0].ty = Ty::DynArray(scalar_int(64));
    let mut transitive_owned = program.clone();
    transitive_owned.structs.push(StructDef {
        name: "OwnedRecord".to_string(),
        source_name: "OwnedRecord".to_string(),
        fields: vec![FieldDef {
            name: "xs".to_string(),
            ty: Ty::DynArray(scalar_int(64)),
        }],
        align: None,
        c_repr: false,
    });
    transitive_owned.structs[0].fields[0].ty = Ty::Struct(1);
    for owned in [direct_owned, transitive_owned] {
        assert!(!validate_hir::json_scan_copy_rows_are_valid(&owned));
        for lowered in [
            lower_program(&owned),
            lower_program_located(&owned, &source_map),
            lower_program_per_unit(&owned),
            lower_program_per_unit_located(&owned, &source_map),
        ] {
            assert!(is_empty(&lowered), "owned scanner row published MIR");
        }
    }

    let mut missing_row = program.clone();
    missing_row.fns[0].body = hir::Block {
        stmts: vec![hir::Stmt::Expr(body_test_expr(
            hir::ExprKind::JsonScan {
                struct_id: 99,
                input: Box::new(body_test_expr(hir::ExprKind::Str("[]".to_string()), Ty::Str)),
            },
            Ty::JsonScanner(99),
        ))],
        value: Some(Box::new(body_test_expr(hir::ExprKind::Unit, Ty::Unit))),
    };
    assert!(!validate_hir::json_scan_copy_rows_are_valid(&missing_row));

    let mut cyclic_row = program.clone();
    cyclic_row.structs[0].fields[0].ty = Ty::Struct(0);
    assert!(!validate_hir::json_scan_copy_rows_are_valid(&cyclic_row));

    let mut invalid_span = program.clone();
    if let hir::Stmt::Expr(expression) = &mut invalid_span.fns[0].body.stmts[0] {
        expression.span = align_span::Span::new(0, 2, 1);
    }
    assert_eq!(
        validate_hir::json_scan_validation_reason(&invalid_span),
        Err(validate_hir::JsonScanValidationReason::InvalidSpan)
    );

    let mut wrong_stored_type = program.clone();
    if let hir::Stmt::Expr(expression) = &mut wrong_stored_type.fns[0].body.stmts[0] {
        expression.ty = Ty::JsonScanner(99);
    }
    assert_eq!(
        validate_hir::json_scan_validation_reason(&wrong_stored_type),
        Err(validate_hir::JsonScanValidationReason::StoredType)
    );

    let mut unknown_row = program.clone();
    if let hir::Stmt::Expr(expression) = &mut unknown_row.fns[0].body.stmts[0] {
        if let hir::ExprKind::JsonScan { struct_id, .. } = &mut expression.kind {
            *struct_id = 99;
        }
        expression.ty = Ty::JsonScanner(99);
    }
    assert_eq!(
        validate_hir::json_scan_validation_reason(&unknown_row),
        Err(validate_hir::JsonScanValidationReason::UnknownRow)
    );

    let mut wrong_input_type = program.clone();
    if let hir::Stmt::Expr(expression) = &mut wrong_input_type.fns[0].body.stmts[0]
        && let hir::ExprKind::JsonScan { input, .. } = &mut expression.kind
    {
        input.ty = Ty::String;
    }
    assert_eq!(
        validate_hir::json_scan_validation_reason(&wrong_input_type),
        Err(validate_hir::JsonScanValidationReason::InputType)
    );

    let mut invalid_schema_reason = program.clone();
    invalid_schema_reason.structs[0].fields[1].ty = Ty::Char;
    assert_eq!(
        validate_hir::json_scan_validation_reason(&invalid_schema_reason),
        Err(validate_hir::JsonScanValidationReason::Schema)
    );

    let mut invalid_copy = program.clone();
    invalid_copy.structs[0].fields[0].ty = Ty::DynArray(scalar_int(64));
    assert_eq!(
        validate_hir::json_scan_validation_reason(&invalid_copy),
        Err(validate_hir::JsonScanValidationReason::Copy)
    );

    // The scanner envelope has one explicit exception to the universal span rule: its enclosing
    // expression span wins even when a later field is independently malformed. Keep every pair in
    // the precedence matrix so a future validator cannot accidentally reorder these checks while
    // preserving the one-fault cases above.
    let paired_reason = |mut candidate: hir::Program, expected| {
        if let hir::Stmt::Expr(expression) = &mut candidate.fns[0].body.stmts[0] {
            expression.span = align_span::Span::new(0, 2, 1);
        }
        assert_eq!(
            validate_hir::json_scan_validation_reason(&candidate),
            Err(expected)
        );
    };
    paired_reason(
        {
            let mut candidate = program.clone();
            if let hir::Stmt::Expr(expression) = &mut candidate.fns[0].body.stmts[0] {
                expression.ty = Ty::JsonScanner(99);
            }
            candidate
        },
        validate_hir::JsonScanValidationReason::InvalidSpan,
    );
    paired_reason(
        {
            let mut candidate = program.clone();
            if let hir::Stmt::Expr(expression) = &mut candidate.fns[0].body.stmts[0] {
                if let hir::ExprKind::JsonScan { struct_id, .. } = &mut expression.kind {
                    *struct_id = 99;
                }
                expression.ty = Ty::JsonScanner(99);
            }
            candidate
        },
        validate_hir::JsonScanValidationReason::InvalidSpan,
    );
    paired_reason(
        {
            let mut candidate = program.clone();
            if let hir::Stmt::Expr(expression) = &mut candidate.fns[0].body.stmts[0]
                && let hir::ExprKind::JsonScan { input, .. } = &mut expression.kind
            {
                input.ty = Ty::String;
            }
            candidate
        },
        validate_hir::JsonScanValidationReason::InvalidSpan,
    );
    paired_reason(
        {
            let mut candidate = program.clone();
            candidate.structs[0].fields[1].ty = Ty::Char;
            candidate
        },
        validate_hir::JsonScanValidationReason::InvalidSpan,
    );
    paired_reason(
        {
            let mut candidate = program.clone();
            candidate.structs[0].fields[0].ty = Ty::DynArray(scalar_int(64));
            candidate
        },
        validate_hir::JsonScanValidationReason::InvalidSpan,
    );
}

#[test]
fn hir_program_json_scan_envelope_mismatch() {
    let scanner = body_test_expr(
        hir::ExprKind::JsonScan {
            struct_id: 0,
            input: Box::new(body_test_expr(hir::ExprKind::Str("[]".to_string()), Ty::Str)),
        },
        Ty::JsonScanner(0),
    );
    let mut program = baseline_program();
    program.fns.push(body_unit_case("json_scan_envelope_mismatch", scanner));
    if let hir::Stmt::Expr(expression) = &mut program.fns[0].body.stmts[0]
        && let hir::ExprKind::JsonScan { input, .. } = &mut expression.kind
    {
        input.ty = Ty::String;
    }
    assert_eq!(
        validate_hir::json_scan_validation_reason(&program),
        Err(validate_hir::JsonScanValidationReason::InputType)
    );
}

#[test]
fn hir_program_json_scan_envelope_precedence_matrix() {
    let scanner = body_test_expr(
        hir::ExprKind::JsonScan {
            struct_id: 0,
            input: Box::new(body_test_expr(hir::ExprKind::Str("[]".to_string()), Ty::Str)),
        },
        Ty::JsonScanner(0),
    );
    let mut program = baseline_program();
    program.fns.push(body_unit_case("json_scan_envelope_precedence", scanner));

    let set_fault = |candidate: &mut hir::Program, fault: usize| {
        match fault {
            0 => {
                if let hir::Stmt::Expr(expression) = &mut candidate.fns[0].body.stmts[0] {
                    expression.ty = Ty::JsonScanner(1);
                }
            }
            1 => {
                if let hir::Stmt::Expr(expression) = &mut candidate.fns[0].body.stmts[0] {
                    if let hir::ExprKind::JsonScan { struct_id, .. } = &mut expression.kind {
                        *struct_id = 99;
                    }
                    expression.ty = Ty::JsonScanner(99);
                }
            }
            2 => {
                if let hir::Stmt::Expr(expression) = &mut candidate.fns[0].body.stmts[0]
                    && let hir::ExprKind::JsonScan { input, .. } = &mut expression.kind
                {
                    input.ty = Ty::String;
                }
            }
            3 => candidate.structs[0].fields[1].ty = Ty::Char,
            4 => candidate.structs[0].fields[0].ty = Ty::DynArray(scalar_int(64)),
            _ => unreachable!("five scanner-envelope fault classes"),
        }
    };
    let reasons = [
        validate_hir::JsonScanValidationReason::StoredType,
        validate_hir::JsonScanValidationReason::UnknownRow,
        validate_hir::JsonScanValidationReason::InputType,
        validate_hir::JsonScanValidationReason::Schema,
        validate_hir::JsonScanValidationReason::Copy,
    ];
    for first in 0..reasons.len() {
        for second in (first + 1)..reasons.len() {
            let mut candidate = program.clone();
            set_fault(&mut candidate, second);
            // Apply the lower-priority mutation first: the stored-type and unknown-row cases share
            // the expression's type field, and the higher-priority fault must remain observable.
            set_fault(&mut candidate, first);
            assert_eq!(
                validate_hir::json_scan_validation_reason(&candidate),
                Err(reasons[first]),
                "valid-Span scanner envelope precedence must choose fault {first} over {second}"
            );
        }
    }
}


#[test]
fn malformed_hir_body_metadata_fails_closed() {
    let mut program = declaration_header_program();
    program.fns[0].return_borrow = ReturnBorrowSummary::None;
    program.fns[0].return_region = ReturnRegionSummary::None;
    assert!(validate_hir::global_type_metadata_is_valid(&program));
    assert!(validate_hir::type_placement_metadata_is_valid(&program));
    assert!(validate_hir::nominal_link_metadata_is_valid(&program));
    assert!(validate_hir::declaration_header_metadata_is_valid(&program));
    assert!(!align_sema::checked_hir_body_facts_are_valid(&program));

    let source_map = SourceMap::new();
    for lowered in [
        lower_program(&program),
        lower_program_located(&program, &source_map),
        lower_program_per_unit(&program),
        lower_program_per_unit_located(&program, &source_map),
    ] {
        assert!(is_empty(&lowered), "malformed body published partial MIR");
    }
}

#[test]
fn malformed_hir_body_structure_precedes_fact_replay() {
    let mut program = declaration_header_program();
    program.fns[0].return_borrow = ReturnBorrowSummary::None;
    program.fns[0].return_region = ReturnRegionSummary::None;
    program.fns[0]
        .body
        .value
        .as_mut()
        .expect("declaration fixture has a body value")
        .kind = hir::ExprKind::Local(99);
    assert!(validate_hir::global_type_metadata_is_valid(&program));
    assert!(validate_hir::type_placement_metadata_is_valid(&program));
    assert!(validate_hir::nominal_link_metadata_is_valid(&program));
    assert!(validate_hir::declaration_header_metadata_is_valid(&program));
    assert!(!validate_hir::body_core_metadata_is_valid(&program));

    let source_map = SourceMap::new();
    for lowered in [
        lower_program(&program),
        lower_program_located(&program, &source_map),
        lower_program_per_unit(&program),
        lower_program_per_unit_located(&program, &source_map),
    ] {
        assert!(is_empty(&lowered), "malformed body structure published MIR");
    }
}

#[test]
fn malformed_hir_unused_local_record_fails_closed() {
    let mut program = declaration_header_program();
    program.fns[0].locals.push(hir::Local {
        id: 2,
        name: "orphan".to_string(),
        ty: Ty::Str,
        is_mut: false,
        is_param: false,
        align: None,
    });
    assert!(validate_hir::global_type_metadata_is_valid(&program));
    assert!(validate_hir::type_placement_metadata_is_valid(&program));
    assert!(validate_hir::nominal_link_metadata_is_valid(&program));
    assert!(validate_hir::declaration_header_metadata_is_valid(&program));
    assert!(!validate_hir::body_only_metadata_is_valid(&program));

    let source_map = SourceMap::new();
    for lowered in [
        lower_program(&program),
        lower_program_located(&program, &source_map),
        lower_program_per_unit(&program),
        lower_program_per_unit_located(&program, &source_map),
    ] {
        assert!(is_empty(&lowered), "unused orphan local published MIR");
    }
}

#[test]
fn malformed_hir_visible_local_name_collisions_fail_closed() {
    let program = checked_source_program(
        "Choice { Pair(i64, i64), Single(i64) }\n\
         fn scope_names(left: i64, right: i64, flag: bool) -> i64 {\n\
           first := left\n\
           second := right\n\
           (tuple_left, tuple_right) := (first, second)\n\
           if flag { inner := tuple_left; return inner }\n\
           return tuple_right\n\
         }\n\
         fn match_names(choice: Choice) -> i64 = match choice {\n\
           Pair(item_left, item_right) => item_left + item_right\n\
           Single(item) => item\n\
         }\n\
         fn sibling_blocks(flag: bool) -> i64 {\n\
           if flag { value := 1; return value }\n\
           if !flag { value := 2; return value }\n\
           return 0\n\
         }\n\
         fn sibling_arms(choice: Choice) -> i64 = match choice {\n\
           Pair(value, _) => value\n\
           Single(value) => value\n\
         }\n\
         fn owned_pair() -> (string, string) {\n\
           return (\"owned\".clone(), \"value\".clone())\n\
         }\n\
         fn hidden_tuple_discards() -> i64 {\n\
           _drop0 := \"user\".clone()\n\
           (_, first) := owned_pair()\n\
           (_, second) := owned_pair()\n\
           return _drop0.len() + first.len() + second.len()\n\
         }\n\
         fn hidden_scope() -> i64 {\n\
           result := {\n\
             (_, kept) := owned_pair()\n\
             kept.len()\n\
           }\n\
           return result\n\
         }\n\
         fn main() -> i32 = 0\n",
    );
    assert_accepted("disjoint sibling local names", &program);

    let hidden_name = align_sema::tuple_drop_local_name(0);
    let hidden_function = program
        .fns
        .iter()
        .find(|function| function.name.as_str() == "hidden_tuple_discards")
        .expect("hidden tuple fixture");
    let hidden_ids = hidden_function
        .locals
        .iter()
        .filter(|local| local.name == hidden_name)
        .map(|local| local.id)
        .collect::<Vec<_>>();
    assert_eq!(hidden_ids.len(), 2, "both owned discards need hidden locals");
    for &hidden in &hidden_ids {
        assert_eq!(hidden_function.drop_locals.iter().filter(|&&id| id == hidden).count(), 1);
        assert_eq!(
            hidden_function
                .drop_individual_locals
                .iter()
                .filter(|&&id| id == hidden)
                .count(),
            1
        );
    }
    let source_map = SourceMap::new();
    for lowered in [
        lower_program(&program),
        lower_program_located(&program, &source_map),
        lower_program_per_unit(&program),
        lower_program_per_unit_located(&program, &source_map),
    ] {
        let function = lowered
            .fns
            .iter()
            .find(|function| function.name.as_str() == "hidden_tuple_discards")
            .expect("hidden tuple MIR function");
        for &hidden in &hidden_ids {
            assert_eq!(
                function
                    .blocks
                    .iter()
                    .flat_map(|block| &block.stmts)
                    .filter(|statement| matches!(statement, Stmt::Drop(slot) if *slot == hidden))
                    .count(),
                1,
                "hidden tuple local {hidden} must drop exactly once"
            );
        }
    }

    let rename = |program: &mut hir::Program, function: &str, from: &str, to: &str| {
        let function = program
            .fns
            .iter_mut()
            .find(|candidate| candidate.name == function)
            .expect("scope fixture function");
        let local = function
            .locals
            .iter_mut()
            .find(|local| local.name == from)
            .expect("scope fixture local");
        local.name = to.to_string();
    };
    for (label, function, from, to) in [
        ("duplicate parameters", "scope_names", "right", "left"),
        ("parameter shadow", "scope_names", "first", "left"),
        ("same-block rebind", "scope_names", "second", "first"),
        (
            "tuple-pattern duplicate",
            "scope_names",
            "tuple_right",
            "tuple_left",
        ),
        ("inner-block shadow", "scope_names", "inner", "first"),
        (
            "match-pattern duplicate",
            "match_names",
            "item_right",
            "item_left",
        ),
        ("match binding shadow", "match_names", "item_left", "choice"),
    ] {
        let mut malformed = program.clone();
        rename(&mut malformed, function, from, to);
        assert_body_entrypoints_empty(label, &malformed);
    }

    let accept_visible_rename = |label: &str, function_name: &str, from: &str, spelling: &str| {
        let mut accepted = program.clone();
        accepted
            .fns
            .iter_mut()
            .find(|function| function.name.as_str() == function_name)
            .expect("visible-name acceptance function")
            .locals
            .iter_mut()
            .find(|local| local.name == from)
            .expect("visible-name acceptance local")
            .name = spelling.to_string();
        assert_accepted(label, &accepted);
    };
    for spelling in ["_drop0", "$tuple_drop00", "$tuple_drop1"] {
        accept_visible_rename(spelling, "hidden_scope", &hidden_name, spelling);
    }
    for spelling in [hidden_name.as_str(), "_drop0", "$tuple_drop00", "$tuple_drop1"] {
        accept_visible_rename(spelling, "scope_names", "tuple_left", spelling);
    }
    for spelling in [hidden_name.as_str(), "_drop0", "$tuple_drop00"] {
        accept_visible_rename(spelling, "scope_names", "first", spelling);
    }
    accept_visible_rename(
        "owned Let near spelling without collision",
        "hidden_tuple_discards",
        "_drop0",
        "$tuple_drop00",
    );
    accept_visible_rename(
        "visible canonical name before hidden tuple locals",
        "hidden_tuple_discards",
        "_drop0",
        &hidden_name,
    );

    let reject_hidden_spelling = |label: &str, spelling: &str| {
        let mut malformed = program.clone();
        let function = malformed
            .fns
            .iter_mut()
            .find(|function| function.name.as_str() == "hidden_tuple_discards")
            .expect("hidden tuple fixture");
        for &hidden in &hidden_ids {
            function.locals[hidden as usize].name = spelling.to_string();
        }
        assert_body_entrypoints_empty(label, &malformed);
    };
    reject_hidden_spelling("source-visible _drop spelling", "_drop0");
    reject_hidden_spelling("leading-zero hidden spelling", "$tuple_drop00");
    reject_hidden_spelling("wrong-ordinal hidden spelling", "$tuple_drop1");

    let reject_visible_pair = |label: &str,
                               function_name: &str,
                               first: &str,
                               second: &str,
                               spelling: &str| {
        let mut malformed = program.clone();
        let function = malformed
            .fns
            .iter_mut()
            .find(|function| function.name.as_str() == function_name)
            .expect("visible-name matrix function");
        for original in [first, second] {
            function
                .locals
                .iter_mut()
                .find(|local| local.name == original)
                .expect("visible-name matrix local")
                .name = spelling.to_string();
        }
        assert_body_entrypoints_empty(label, &malformed);
    };
    reject_visible_pair(
        "Copy tuple canonical spelling",
        "scope_names",
        "second",
        "tuple_left",
        &hidden_name,
    );
    reject_visible_pair(
        "Copy tuple source spelling",
        "scope_names",
        "second",
        "tuple_left",
        "_drop0",
    );
    reject_visible_pair(
        "Copy tuple leading-zero spelling",
        "scope_names",
        "second",
        "tuple_left",
        "$tuple_drop00",
    );
    reject_visible_pair(
        "Copy tuple wrong-ordinal spelling",
        "scope_names",
        "second",
        "tuple_left",
        "$tuple_drop1",
    );
    reject_visible_pair(
        "ordinary owned Let canonical spelling",
        "hidden_tuple_discards",
        "_drop0",
        "first",
        &hidden_name,
    );
    reject_visible_pair(
        "ordinary owned Let near spelling",
        "hidden_tuple_discards",
        "_drop0",
        "first",
        "$tuple_drop00",
    );

    let hidden_len = |local| {
        body_test_expr(
            hir::ExprKind::Len(Box::new(body_test_expr(
                hir::ExprKind::Local(local),
                Ty::String,
            ))),
            int(64),
        )
    };
    let insert_hidden_read = |program: &mut hir::Program, position: usize| {
        let function = program
            .fns
            .iter_mut()
            .find(|function| function.name.as_str() == "hidden_scope")
            .expect("hidden scope fixture");
        let hir::Stmt::Let { init, .. } = &mut function.body.stmts[0] else {
            panic!("hidden scope outer binding")
        };
        let hir::ExprKind::Block(block) = &mut init.kind else {
            panic!("hidden scope block")
        };
        let hir::Stmt::LetTuple { locals, .. } = &block.stmts[0] else {
            panic!("hidden scope tuple binding")
        };
        let hidden = locals[0].expect("owned discard hidden local");
        block.stmts.insert(position, hir::Stmt::Expr(hidden_len(hidden)));
        hidden
    };
    let mut initialized = program.clone();
    insert_hidden_read(&mut initialized, 1);
    assert!(
        validate_hir::body_only_metadata_is_valid(&initialized),
        "hidden local id must initialize after its tuple binding"
    );
    let mut before_binding = program.clone();
    insert_hidden_read(&mut before_binding, 0);
    assert_body_entrypoints_empty("hidden local before tuple binding", &before_binding);

    let mut after_scope = program.clone();
    let hidden = {
        let function = after_scope
            .fns
            .iter()
            .find(|function| function.name.as_str() == "hidden_scope")
            .expect("hidden scope fixture");
        let hir::Stmt::Let { init, .. } = &function.body.stmts[0] else {
            panic!("hidden scope outer binding")
        };
        let hir::ExprKind::Block(block) = &init.kind else {
            panic!("hidden scope block")
        };
        let hir::Stmt::LetTuple { locals, .. } = &block.stmts[0] else {
            panic!("hidden scope tuple binding")
        };
        locals[0].expect("owned discard hidden local")
    };
    after_scope
        .fns
        .iter_mut()
        .find(|function| function.name.as_str() == "hidden_scope")
        .expect("hidden scope fixture")
        .body
        .stmts
        .insert(1, hir::Stmt::Expr(hidden_len(hidden)));
    assert_body_entrypoints_empty("hidden local after block exit", &after_scope);
}

#[test]
fn capturing_partition_and_par_map_reach_all_lowerers() {
    let program = checked_source_program(
        "fn captured() -> i64 {\n\
           t := 2\n\
           (big, small) := [1, 2, 3, 4].partition(fn x { x > t })\n\
           b := 10\n\
           ys := [1, 2, 3].par_map(fn x { x + b })\n\
           return big.len() + small.len() + ys.len()\n\
         }\n\
         fn main() -> i32 = 0\n",
    );
    assert_accepted("capturing partition/par_map terminals", &program);
}

#[test]
fn valid_hir_body_preflight_is_mir_identity() {
    let program = declaration_header_program();
    assert!(align_sema::checked_hir_body_facts_are_valid(&program));
    let source_map = SourceMap::new();
    for (checked, unchecked) in [
        (
            lower_program(&program),
            lower_program_unchecked(&program, None, false),
        ),
        (
            lower_program_located(&program, &source_map),
            lower_program_unchecked(
                &program,
                Some(Rc::new(SourceLines::from_map(&source_map))),
                false,
            ),
        ),
        (
            lower_program_per_unit(&program),
            lower_program_unchecked(&program, None, true),
        ),
        (
            lower_program_per_unit_located(&program, &source_map),
            lower_program_unchecked(
                &program,
                Some(Rc::new(SourceLines::from_map(&source_map))),
                true,
            ),
        ),
    ] {
        assert!(!is_empty(&checked), "valid body did not reach MIR");
        assert_eq!(format!("{checked:#?}"), format!("{unchecked:#?}"));
    }
}

#[test]
fn body_contract_function_return_none() {
    let integer = int(64);
    let mut unit = baseline_program();
    unit.fns.push(body_test_named_function(
        "unit_return_none",
        hir::Block {
            stmts: vec![hir::Stmt::Return(None)],
            value: None,
        },
        Vec::new(),
        Ty::Unit,
    ));
    assert!(body_core_metadata_is_valid(&unit));

    let mut non_unit = unit.clone();
    non_unit.fns[0].ret = integer;
    assert_body_entrypoints_empty("Return(None) in non-Unit function", &non_unit);

    let mut value = baseline_program();
    value.fns.push(body_test_named_function(
        "integer_return_some",
        hir::Block {
            stmts: vec![hir::Stmt::Return(Some(body_test_expr(
                hir::ExprKind::Int(1),
                integer,
            )))],
            value: None,
        },
        Vec::new(),
        integer,
    ));
    assert!(body_core_metadata_is_valid(&value));

    let mut missing = value.clone();
    let hir::Stmt::Return(return_value) = &mut missing.fns[0].body.stmts[0] else {
        unreachable!("return fixture lost its return statement");
    };
    *return_value = None;
    assert_body_entrypoints_empty("missing value in non-Unit Return", &missing);
}

#[test]
fn body_contract_function_root_completion() {
    let integer = int(64);

    let mut unit_empty = baseline_program();
    unit_empty.fns.push(body_test_named_function(
        "unit_empty_body",
        hir::Block {
            stmts: Vec::new(),
            value: None,
        },
        Vec::new(),
        Ty::Unit,
    ));
    assert!(body_core_metadata_is_valid(&unit_empty));

    let mut non_unit_missing = baseline_program();
    non_unit_missing.fns.push(body_test_named_function(
        "non_unit_missing_tail",
        hir::Block {
            stmts: Vec::new(),
            value: None,
        },
        Vec::new(),
        integer,
    ));
    assert_body_entrypoints_empty("reachable non-Unit body without a tail", &non_unit_missing);

    let mut non_unit_statement_fallthrough = baseline_program();
    non_unit_statement_fallthrough.fns.push(body_test_named_function(
        "non_unit_statement_fallthrough",
        hir::Block {
            stmts: vec![hir::Stmt::Expr(body_test_expr(
                hir::ExprKind::Int(1),
                integer,
            ))],
            value: None,
        },
        Vec::new(),
        integer,
    ));
    assert_body_entrypoints_empty(
        "reachable non-Unit statement fallthrough without a tail",
        &non_unit_statement_fallthrough,
    );

    let mut non_unit_return = baseline_program();
    non_unit_return.fns.push(body_test_named_function(
        "non_unit_return_completion",
        hir::Block {
            stmts: vec![hir::Stmt::Return(Some(body_test_expr(
                hir::ExprKind::Int(1),
                integer,
            )))],
            value: None,
        },
        Vec::new(),
        integer,
    ));
    assert!(body_core_metadata_is_valid(&non_unit_return));
}


#[test]
fn checked_hir_body_fact_replay_rejects_stale_producer_facts() {
    let base = declaration_header_program();
    // The baseline function-type entry is annotation-only in this fixture; its producer value is
    // the conservative Unknown state, not a synthesized Pure result.
    base.fn_types[0].effect.set(FnEffect::Unknown);
    let before = format!("{base:#?}");
    assert!(
        align_sema::checked_hir_body_facts_are_valid(&base),
        "a producer-valid body must reproduce its stored facts"
    );
    assert_eq!(format!("{base:#?}"), before, "replay must not mutate input HIR");

    let mut bad_return_borrow = base.clone();
    bad_return_borrow.fns[0].return_borrow = ReturnBorrowSummary::None;
    assert_replay_rejects_without_mutating(
        bad_return_borrow,
        "stale return-borrow provenance must fail closed",
    );

    let mut bad_return_region = base.clone();
    bad_return_region.fns[0].return_region = ReturnRegionSummary::None;
    assert_replay_rejects_without_mutating(
        bad_return_region,
        "stale return-region provenance must fail closed",
    );

    let mut bad_drop_locals = base.clone();
    bad_drop_locals.fns[0].drop_locals = vec![99];
    assert_replay_rejects_without_mutating(bad_drop_locals, "stale Drop locals must fail closed");

    let mut bad_drop_individual_locals = base.clone();
    bad_drop_individual_locals.fns[0].drop_individual_locals = vec![99];
    assert_replay_rejects_without_mutating(
        bad_drop_individual_locals,
        "stale individual Drop locals must fail closed",
    );

    let mut bad_drop_exprs = base.clone();
    bad_drop_exprs.fns[0]
        .drop_individual_exprs
        .insert(align_span::Span::new(0, 99, 100), true);
    assert_replay_rejects_without_mutating(
        bad_drop_exprs,
        "stale individual Drop expression map must fail closed",
    );

    let mut malformed_local = base.clone();
    malformed_local.fns[0]
        .body
        .value
        .as_mut()
        .expect("declaration fixture has a body value")
        .kind = hir::ExprKind::Local(99);
    assert_replay_rejects_without_mutating(
        malformed_local,
        "a malformed local ordinal must fail closed instead of panicking",
    );

    let bad_effect = base.clone();
    bad_effect.fn_types[0].effect.set(FnEffect::Pure);
    assert_replay_rejects_without_mutating(
        bad_effect,
        "stale annotation-only effect must fail closed",
    );
}

#[test]
fn checked_hir_body_fact_replay_preserves_imported_fact_presence() {
    let roots = ReturnBorrowSummary::Roots {
        params: vec![0],
        captures: Vec::new(),
    };
    let regions = ReturnRegionSummary::Roots {
        params: vec![0],
        captures: Vec::new(),
    };
    for effect in [FnEffect::Pure, FnEffect::Unknown, FnEffect::Impure] {
        let unknown = checked_interface_program(None, effect);
        assert_eq!(unknown.imported_fns.len(), 1);
        assert!(
            !unknown.imported_fns[0].return_provenance_known,
            "compatibility API omission must remain distinguishable from exact None"
        );
        let unknown_consumer = unknown
            .fns
            .iter()
            .find(|function| function.name.as_str() == "consume")
            .expect("consumer function");
        assert_eq!(unknown_consumer.return_borrow, roots);
        assert_eq!(unknown_consumer.return_region, regions);
        assert!(align_sema::checked_hir_body_facts_are_valid(&unknown));

        for (return_borrow, return_region) in [
            (ReturnBorrowSummary::None, ReturnRegionSummary::None),
            (roots.clone(), regions.clone()),
        ] {
            let program = checked_interface_program(
                Some((return_borrow.clone(), return_region.clone())),
                effect,
            );
            assert!(program.imported_fns[0].return_provenance_known);
            let consumer = program
                .fns
                .iter()
                .find(|function| function.name.as_str() == "consume")
                .expect("consumer function");
            if matches!(return_borrow, ReturnBorrowSummary::None) {
                assert_eq!(consumer.return_borrow, ReturnBorrowSummary::None);
                assert_eq!(consumer.return_region, ReturnRegionSummary::None);
            } else {
                assert_eq!(consumer.return_borrow, roots);
                assert_eq!(consumer.return_region, regions);
            }
            assert!(
                align_sema::checked_hir_body_facts_are_valid(&program),
                "known imported provenance/effect combination must replay: {effect:?}"
            );
        }
    }
}

#[test]
fn checked_hir_body_fact_replay_covers_cleanup_and_function_effects() {
    let base = checked_source_program(
        "fn quiet(x: i64) -> i64 = x + 1
fn loud(x: i64) -> i64 {
  print(x)
  return x
}
fn replace() -> i32 {
  mut value := \"first\".clone()
  value = \"second\".clone()
  return value.len() as i32
}
fn main() -> i32 {
  pure_value := quiet
  impure_value := loud
  return replace()
}
",
    );
    let before = format!("{base:#?}");
    assert!(
        align_sema::checked_hir_body_facts_are_valid(&base),
        "producer facts for cleanup and concrete function values must replay"
    );
    assert_eq!(format!("{base:#?}"), before, "replay must not mutate input HIR");

    let mut malformed_fn_id = base.clone();
    let main_index = malformed_fn_id
        .fns
        .iter()
        .position(|function| function.name.as_str() == "main")
        .expect("main function");
    let local = malformed_fn_id.fns[main_index]
        .locals
        .iter_mut()
        .find(|local| matches!(local.ty, Ty::Fn(_)))
        .expect("function-value local");
    local.ty = Ty::Fn(u32::MAX);
    assert_replay_rejects_without_mutating(
        malformed_fn_id,
        "a malformed local function-type id must fail closed",
    );

    let replace_index = base
        .fns
        .iter()
        .position(|function| function.name.as_str() == "replace")
        .expect("replace function");
    let assignment_index = base.fns[replace_index]
        .body
        .stmts
        .iter()
        .position(|statement| matches!(statement, hir::Stmt::Assign { .. }))
        .expect("string replacement assignment");

    let bad_assignment = base.clone();
    let hir::Stmt::Assign {
        drop_old,
        drop_new,
        ..
    } = &bad_assignment.fns[replace_index].body.stmts[assignment_index]
    else {
        unreachable!("assignment index was selected from the same body");
    };
    let old_drop_old = drop_old.get();
    let old_drop_new = drop_new.get();
    drop_old.set(!old_drop_old);
    assert_replay_rejects_without_mutating(
        bad_assignment,
        "stale assignment drop-old fact must fail closed",
    );

    let bad_assignment_new = base.clone();
    let hir::Stmt::Assign { drop_new, .. } =
        &bad_assignment_new.fns[replace_index].body.stmts[assignment_index]
    else {
        unreachable!("assignment index was selected from the same body");
    };
    drop_new.set(!old_drop_new);
    assert_replay_rejects_without_mutating(
        bad_assignment_new,
        "stale assignment drop-new fact must fail closed",
    );

    let main = base
        .fns
        .iter()
        .find(|function| function.name.as_str() == "main")
        .expect("main function");
    let function_value_ids: Vec<u32> = main
        .locals
        .iter()
        .filter_map(|local| match local.ty {
            Ty::Fn(id) => Some(id),
            _ => None,
        })
        .collect();
    assert_eq!(function_value_ids.len(), 2, "fixture must publish two function values");
    let bad_effect = base.clone();
    let effect_id = function_value_ids[0] as usize;
    let effect = bad_effect.fn_types[effect_id].effect.get();
    bad_effect.fn_types[effect_id].effect.set(match effect {
        FnEffect::Pure => FnEffect::Unknown,
        FnEffect::Unknown => FnEffect::Pure,
        FnEffect::Impure => FnEffect::Pure,
    });
    assert_replay_rejects_without_mutating(
        bad_effect,
        "stale concrete function-value effect must fail closed",
    );
}

#[test]
fn deep_hir_header_type_dag_is_stack_bounded() {
    let mut program = baseline_program();
    program.structs = (0..4_096)
        .map(|index| StructDef {
            name: format!("HeaderDeep{index}"),
            source_name: format!("HeaderDeep{index}"),
            fields: vec![FieldDef {
                name: "next".to_string(),
                ty: if index == 4_095 {
                    Ty::Str
                } else {
                    Ty::Struct(index + 1)
                },
            }],
            align: None,
            c_repr: false,
        })
        .collect();
    program.imported_fns.push(ImportedFn {
        name: "deep$header".to_string(),
        params: vec![Ty::Struct(0)],
        param_modes: vec![align_ast::ParamMode::ByValue],
        ret: Ty::Struct(0),
        return_provenance_known: true,
        return_borrow: ReturnBorrowSummary::Roots {
            params: vec![0],
            captures: Vec::new(),
        },
        return_region: ReturnRegionSummary::Roots {
            params: vec![0],
            captures: Vec::new(),
        },
        return_cleanup: hir::ReturnCleanupAbi::None,
        effect: FnEffect::Unknown,
    });
    assert!(validate_hir::declaration_header_metadata_is_valid(&program));
    assert!(!is_empty(&lower_program_per_unit(&program)));

    let mut malformed = program.clone();
    malformed.imported_fns.push(ImportedFn {
        name: String::new(),
        params: Vec::new(),
        param_modes: Vec::new(),
        ret: Ty::Unit,
        return_provenance_known: false,
        return_borrow: ReturnBorrowSummary::None,
        return_region: ReturnRegionSummary::None,
        return_cleanup: hir::ReturnCleanupAbi::None,
        effect: FnEffect::Impure,
    });
    assert_header_rejected("deep-header-later-sibling", &malformed);
}

fn assert_one_header_mutation(
    label: &str,
    base: &hir::Program,
    mutate: impl FnOnce(&mut hir::Program),
) {
    let mut program = base.clone();
    mutate(&mut program);
    assert_header_rejected(label, &program);
}

fn int(bits: u8) -> Ty {
    Ty::Int(IntTy { bits, signed: true })
}

fn scalar_int(bits: u8) -> Scalar {
    Scalar::Int(IntTy { bits, signed: true })
}

fn fn_type(ret: Ty) -> FnTy {
    FnTy {
        params: Vec::new(),
        ret,
        return_borrow: ReturnBorrowSummary::None,
        return_region: ReturnRegionSummary::None,
        return_cleanup: hir::ReturnCleanupAbi::None,
        effect: Cell::new(FnEffect::Unknown),
    }
}

fn body_fn_type(params: Vec<(align_ast::ParamMode, Scalar)>, ret: Ty) -> FnTy {
    FnTy {
        params,
        ret,
        return_borrow: ReturnBorrowSummary::None,
        return_region: ReturnRegionSummary::None,
        return_cleanup: hir::ReturnCleanupAbi::None,
        effect: Cell::new(FnEffect::Pure),
    }
}

pub(super) fn baseline_program() -> hir::Program {
    hir::Program {
        fns: Vec::new(),
        externs: Vec::new(),
        link_libs: Vec::new(),
        structs: vec![StructDef {
            name: "Record".to_string(),
            source_name: "Record".to_string(),
            fields: vec![
                FieldDef {
                    name: "key".to_string(),
                    ty: Ty::Str,
                },
                FieldDef {
                    name: "value".to_string(),
                    ty: int(64),
                },
            ],
            align: None,
            c_repr: false,
        }],
        enums: vec![EnumDef {
            name: "Choice".to_string(),
            source_name: "Choice".to_string(),
            variants: vec![
                EnumVariant {
                    name: "Empty".to_string(),
                    payload: Vec::new(),
                    field_base: 1,
                },
                EnumVariant {
                    name: "Value".to_string(),
                    payload: vec![scalar_int(64)],
                    field_base: 1,
                },
            ],
        }],
        resources: Vec::new(),
        tagged_types: vec![TaggedType::Option(scalar_int(64))],
        tuples: vec![TupleDef {
            elems: vec![scalar_int(64), Scalar::Bool],
        }],
        fn_types: vec![fn_type(Ty::Unit)],
        imported_fns: Vec::new(),
    }
}

fn push_builtin_error(program: &mut hir::Program) -> u32 {
    let id = program.enums.len() as u32;
    program.enums.push(EnumDef {
        name: "Error".to_string(),
        source_name: "Error".to_string(),
        variants: vec![
            EnumVariant {
                name: "NotFound".to_string(),
                payload: Vec::new(),
                field_base: 1,
            },
            EnumVariant {
                name: "Invalid".to_string(),
                payload: Vec::new(),
                field_base: 1,
            },
            EnumVariant {
                name: "Denied".to_string(),
                payload: Vec::new(),
                field_base: 1,
            },
            EnumVariant {
                name: "Timeout".to_string(),
                payload: Vec::new(),
                field_base: 1,
            },
            EnumVariant {
                name: "Code".to_string(),
                payload: vec![Scalar::Int(IntTy {
                    bits: 32,
                    signed: true,
                })],
                field_base: 1,
            },
        ],
    });
    id
}

fn push_builtin_regex_match(program: &mut hir::Program) -> u32 {
    let id = program.structs.len() as u32;
    let i64_ty = int(64);
    program.structs.push(StructDef {
        name: "regex_match".to_string(),
        source_name: "regex_match".to_string(),
        fields: vec![
            FieldDef {
                name: "start".to_string(),
                ty: i64_ty,
            },
            FieldDef {
                name: "end".to_string(),
                ty: i64_ty,
            },
        ],
        align: None,
        c_repr: false,
    });
    id
}

fn push_builtin_argon2_params(program: &mut hir::Program) -> u32 {
    let id = program.structs.len() as u32;
    let i64_ty = int(64);
    program.structs.push(StructDef {
        name: "argon2_params".to_string(),
        source_name: "argon2_params".to_string(),
        fields: ["m_cost", "t_cost", "parallelism", "len"]
            .into_iter()
            .map(|name| FieldDef {
                name: name.to_string(),
                ty: i64_ty,
            })
            .collect(),
        align: None,
        c_repr: false,
    });
    id
}

fn native_local(id: u32, ty: Ty) -> hir::Expr {
    body_test_expr(hir::ExprKind::Local(id), ty)
}

fn native_str() -> hir::Expr {
    body_test_expr(hir::ExprKind::Str("value".to_string()), Ty::Str)
}

fn native_i64() -> hir::Expr {
    body_test_expr(hir::ExprKind::Int(1), int(64))
}

fn native_result(ok: Ty, error: u32) -> Ty {
    Ty::Result(
        align_sema::ty_to_scalar(ok).expect("native result payload is scalar"),
        Scalar::Enum(error),
    )
}

fn push_builtin_json_kind(program: &mut hir::Program) -> u32 {
    let id = program.enums.len() as u32;
    program.enums.push(EnumDef {
        name: "json.kind".to_string(),
        source_name: "json.kind".to_string(),
        variants: ["Object", "Array", "Str", "Number", "Bool", "Null", "Missing"]
            .into_iter()
            .map(|name| EnumVariant {
                name: name.to_string(),
                payload: Vec::new(),
                field_base: 1,
            })
            .collect(),
    });
    id
}

fn imported_fn(name: &str, params: Vec<Ty>, ret: Ty) -> ImportedFn {
    ImportedFn {
        name: name.to_string(),
        param_modes: vec![align_ast::ParamMode::ByValue; params.len()],
        params,
        ret,
        return_provenance_known: false,
        return_borrow: ReturnBorrowSummary::None,
        return_region: ReturnRegionSummary::None,
        return_cleanup: hir::ReturnCleanupAbi::None,
        effect: FnEffect::Pure,
    }
}

fn unary_int_expr_depth(depth: usize, ty: Ty) -> hir::Expr {
    assert!(depth >= 1);
    let span = align_span::Span::new(0, 0, 0);
    let mut expr = hir::Expr {
        kind: hir::ExprKind::Int(0),
        ty,
        span,
    };
    for _ in 1..depth {
        expr = hir::Expr {
            kind: hir::ExprKind::Unary {
                op: align_ast::UnOp::Neg,
                expr: Box::new(expr),
            },
            ty,
            span,
        };
    }
    expr
}

fn str_trim_expr_depth(depth: usize) -> hir::Expr {
    assert!(depth >= 1);
    let span = align_span::Span::new(0, 0, 0);
    let mut expr = hir::Expr {
        kind: hir::ExprKind::Str("x".to_string()),
        ty: Ty::Str,
        span,
    };
    for _ in 1..depth {
        expr = hir::Expr {
            kind: hir::ExprKind::StrTrim {
                kind: hir::StrTrimKind::Both,
                recv: Box::new(expr),
            },
            ty: Ty::Str,
            span,
        };
    }
    expr
}

fn individual_expr_span(
    next_offset: &mut u32,
    ownership: &mut std::collections::HashMap<align_span::Span, bool>,
) -> align_span::Span {
    let span = align_span::Span::new(0, *next_offset, *next_offset + 1);
    *next_offset += 1;
    ownership.insert(span, true);
    span
}

fn with_return(ty: Ty) -> hir::Program {
    let mut program = baseline_program();
    program.imported_fns.push(ImportedFn {
        name: "dep$value".to_string(),
        params: Vec::new(),
        param_modes: Vec::new(),
        ret: ty,
        return_provenance_known: false,
        return_borrow: ReturnBorrowSummary::None,
        return_region: ReturnRegionSummary::None,
        return_cleanup: hir::ReturnCleanupAbi::None,
        effect: FnEffect::Pure,
    });
    program
}

fn is_empty(program: &Program) -> bool {
    program.fns.is_empty()
        && program.externs.is_empty()
        && program.imported_fns.is_empty()
        && program.link_libs.is_empty()
        && program.structs.is_empty()
        && program.enums.is_empty()
        && program.tagged_types.is_empty()
        && program.tuples.is_empty()
}

fn assert_rejected(label: &str, program: &hir::Program) {
    assert!(
        !validate_hir::global_type_metadata_is_valid(program)
            || !validate_hir::type_placement_metadata_is_valid(program)
            || !validate_hir::nominal_link_metadata_is_valid(program)
            || !validate_hir::declaration_header_metadata_is_valid(program),
        "{label}: validator accepted malformed metadata"
    );
    let source_map = SourceMap::new();
    for lowered in [
        lower_program(program),
        lower_program_located(program, &source_map),
        lower_program_per_unit(program),
        lower_program_per_unit_located(program, &source_map),
    ] {
        assert!(
            is_empty(&lowered),
            "{label}: an entrypoint published partial MIR"
        );
    }
}

fn assert_placement_rejected(label: &str, program: &hir::Program) {
    assert!(
        validate_hir::global_type_metadata_is_valid(program),
        "{label}: placement fixture is not graph-valid"
    );
    assert!(
        !validate_hir::type_placement_metadata_is_valid(program),
        "{label}: placement validator accepted graph-valid malformed metadata"
    );
    assert_rejected(label, program);
}

fn assert_nominal_rejected(label: &str, program: &hir::Program) {
    assert!(
        validate_hir::global_type_metadata_is_valid(program),
        "{label}: nominal fixture is not graph-valid"
    );
    assert!(
        validate_hir::type_placement_metadata_is_valid(program),
        "{label}: nominal fixture is not placement-valid"
    );
    assert!(
        !validate_hir::nominal_link_metadata_is_valid(program),
        "{label}: nominal/link validator accepted malformed metadata"
    );
    let source_map = SourceMap::new();
    for lowered in [
        lower_program(program),
        lower_program_located(program, &source_map),
        lower_program_per_unit(program),
        lower_program_per_unit_located(program, &source_map),
    ] {
        assert!(is_empty(&lowered), "{label}: an entrypoint published partial MIR");
    }
}

fn assert_graph_accepted(label: &str, program: &hir::Program) {
    assert!(
        validate_hir::global_type_metadata_is_valid(program),
        "{label}: global graph validator rejected valid metadata"
    );
}

#[derive(Clone, Copy)]
enum MirOwner {
    Unary,
    MixedEager,
    StrTrim,
    Path,
    Reader,
    BytesStrTry,
    Regex,
    Template,
    File,
    ArrayBuilder,
    Command,
    Http,
    Match,
    Conditional,
    Scoped,
    Loop,
    Stage,
    BlockStatement,
    WildcardMatch,
    If,
}

#[derive(Clone, Copy, Default)]
struct HirOwnerEvidence {
    ifs: usize,
    binary_matches: usize,
    wildcard_matches: usize,
    short_circuits: usize,
    else_unwraps: usize,
}

fn hir_owner_evidence(program: &hir::Program) -> HirOwnerEvidence {
    let mut evidence = HirOwnerEvidence::default();
    let mut work = program
        .fns
        .iter()
        .filter_map(|function| function.body.value.as_deref())
        .collect::<Vec<_>>();
    while let Some(expression) = work.pop() {
        match &expression.kind {
            hir::ExprKind::If { .. } => evidence.ifs += 1,
            hir::ExprKind::Match { arms, .. } if arms.len() == 1 && arms[0].variants.is_empty() => {
                evidence.wildcard_matches += 1;
            }
            hir::ExprKind::Match { arms, .. } if arms.len() == 2 => {
                evidence.binary_matches += 1;
            }
            hir::ExprKind::Binary {
                op: align_ast::BinOp::And | align_ast::BinOp::Or,
                ..
            } => evidence.short_circuits += 1,
            hir::ExprKind::ElseUnwrap { .. } => evidence.else_unwraps += 1,
            _ => {}
        }
        work.extend(align_sema::direct_expr_children(expression));
    }
    evidence
}

fn rvalues(program: &Program) -> impl Iterator<Item = &Rvalue> {
    program
        .fns
        .iter()
        .flat_map(|function| &function.blocks)
        .flat_map(|block| &block.stmts)
        .filter_map(|stmt| match stmt {
            Stmt::Let(_, rvalue) => Some(rvalue),
            _ => None,
        })
}

fn result_payload_tys(program: &Program, ty: Ty) -> Option<(Ty, Ty)> {
    match ty {
        Ty::Result(ok, err) => Some((align_sema::scalar_to_ty(ok), align_sema::scalar_to_ty(err))),
        Ty::Tagged(id) => match program.tagged_types.get(id as usize) {
            Some(TaggedType::Result(ok, err)) => Some((
                align_sema::scalar_to_ty(*ok),
                align_sema::scalar_to_ty(*err),
            )),
            _ => None,
        },
        _ => None,
    }
}

fn assert_builtin_error_ty(label: &str, program: &Program, ty: Ty) {
    let Ty::Enum(id) = ty else {
        panic!("{label}: Result error payload is not the builtin Error enum: {ty:?}");
    };
    let error = program
        .enums
        .get(id as usize)
        .unwrap_or_else(|| panic!("{label}: builtin Error enum id {id} is out of range"));
    assert_eq!(error.name, "Error", "{label}: wrong builtin Error name");
    assert_eq!(
        error.source_name, "Error",
        "{label}: wrong builtin Error source name"
    );
    let expected = ["NotFound", "Invalid", "Denied", "Timeout", "Code"];
    assert_eq!(
        error.variants.len(),
        expected.len(),
        "{label}: wrong builtin Error variant count"
    );
    for (index, (variant, expected_name)) in error.variants.iter().zip(expected).enumerate() {
        assert_eq!(
            variant.name, expected_name,
            "{label}: wrong builtin Error variant at index {index}"
        );
        assert_eq!(
            variant.field_base, 1,
            "{label}: wrong builtin Error field base at index {index}"
        );
        if index == 4 {
            assert_eq!(
                variant.payload,
                vec![scalar_int(32)],
                "{label}: Error.Code must carry exact i32"
            );
        } else {
            assert!(
                variant.payload.is_empty(),
                "{label}: non-Code Error variant carries a payload"
            );
        }
    }
}

fn assert_result_rvalue_contract(label: &str, program: &Program) {
    for function in &program.fns {
        for block in &function.blocks {
            for stmt in &block.stmts {
                let Stmt::Let(value, rvalue) = stmt else {
                    continue;
                };
                let value_ty = function.value_tys[*value as usize];
                match rvalue {
                    Rvalue::ResultOk(ok) => {
                        let (ok_ty, _) = result_payload_tys(program, value_ty).unwrap_or_else(|| {
                            panic!(
                                "{label}: ResultOk construction has non-Result MIR type {value_ty:?}"
                            )
                        });
                        assert_eq!(
                            function.operand_ty(ok),
                            ok_ty,
                            "{label}: ResultOk operand disagrees with its Result payload"
                        );
                    }
                    Rvalue::ResultErr(err) => {
                        let (_, err_ty) = result_payload_tys(program, value_ty).unwrap_or_else(|| {
                            panic!(
                                "{label}: ResultErr construction has non-Result MIR type {value_ty:?}"
                            )
                        });
                        assert_eq!(
                            function.operand_ty(err),
                            err_ty,
                            "{label}: ResultErr operand disagrees with its Result payload"
                        );
                    }
                    Rvalue::ResultIsOk(result) => {
                        assert_eq!(
                            value_ty,
                            Ty::Bool,
                            "{label}: ResultIsOk destination is not bool"
                        );
                        let operand_ty = function.operand_ty(result);
                        assert!(
                            result_payload_tys(program, operand_ty).is_some(),
                            "{label}: ResultIsOk received non-Result MIR operand {operand_ty:?}"
                        );
                    }
                    Rvalue::ResultUnwrapOk(result) => {
                        let operand_ty = function.operand_ty(result);
                        let (ok_ty, _) = result_payload_tys(program, operand_ty).unwrap_or_else(|| {
                            panic!(
                                "{label}: ResultUnwrapOk received non-Result MIR operand {operand_ty:?}"
                            )
                        });
                        assert_eq!(
                            value_ty, ok_ty,
                            "{label}: ResultUnwrapOk destination disagrees with its Result payload"
                        );
                    }
                    Rvalue::ResultUnwrapErr(result) => {
                        let operand_ty = function.operand_ty(result);
                        let (_, err_ty) = result_payload_tys(program, operand_ty).unwrap_or_else(|| {
                            panic!(
                                "{label}: ResultUnwrapErr received non-Result MIR operand {operand_ty:?}"
                            )
                        });
                        assert_eq!(
                            value_ty, err_ty,
                            "{label}: ResultUnwrapErr destination disagrees with its Result payload"
                        );
                    }
                    _ => {}
                }
            }
        }
    }
}

fn assert_mir_owner(label: &str, program: &Program, owner: MirOwner, evidence: HirOwnerEvidence) {
    let values: Vec<&Rvalue> = rvalues(program).collect();
    let has = |predicate: fn(&Rvalue) -> bool| values.iter().copied().any(predicate);
    let count =
        |predicate: fn(&Rvalue) -> bool| values.iter().copied().filter(|rv| predicate(rv)).count();
    let bytes_str_counts = || {
        (
            count(|rv| matches!(rv, Rvalue::BytesAsStr { .. })),
            count(|rv| matches!(rv, Rvalue::ResultIsOk(..))),
            count(|rv| matches!(rv, Rvalue::ResultUnwrapOk(..))),
        )
    };
    let branch_count = program
        .fns
        .iter()
        .flat_map(|function| &function.blocks)
        .filter(|block| matches!(block.term, Term::Branch(..)))
        .count();
    let goto_count = program
        .fns
        .iter()
        .flat_map(|function| &function.blocks)
        .filter(|block| matches!(block.term, Term::Goto(..)))
        .count();
    let option_test_count = count(|rv| matches!(rv, Rvalue::OptionIsSome(..)));
    let has_call = |expected: &str| {
        values
            .iter()
            .copied()
            .any(|rv| matches!(rv, Rvalue::Call(name, _) if direct_program_name(name) == Some(expected)))
    };
    let owned = match owner {
        MirOwner::Unary => has(|rv| matches!(rv, Rvalue::Un(..))),
        MirOwner::MixedEager => {
            has(|rv| matches!(rv, Rvalue::Un(..)))
                && has(|rv| matches!(rv, Rvalue::Bin(..)))
                && has(|rv| matches!(rv, Rvalue::Cast { .. }))
                && has(|rv| matches!(rv, Rvalue::Call(..)))
        }
        MirOwner::StrTrim => has(|rv| matches!(rv, Rvalue::StrTrim { .. })),
        MirOwner::Path => {
            has(|rv| matches!(rv, Rvalue::PathJoin { .. }))
                && has(|rv| matches!(rv, Rvalue::PathComponent { .. }))
                && has(|rv| matches!(rv, Rvalue::PathNormalize { .. }))
        }
        MirOwner::Reader => has(|rv| matches!(rv, Rvalue::ReaderBuffered(..))),
        MirOwner::BytesStrTry => {
            let (conversions, result_tests, result_unwraps) = bytes_str_counts();
            let mut exact_builtin_result = false;
            for function in &program.fns {
                for block in &function.blocks {
                    for stmt in &block.stmts {
                        let Stmt::Let(value, Rvalue::ResultOk(ok)) = stmt else {
                            continue;
                        };
                        let value_ty = function.value_tys[*value as usize];
                        let Some((ok_ty, err_ty)) = result_payload_tys(program, value_ty) else {
                            continue;
                        };
                        if ok_ty != Ty::Str || function.operand_ty(ok) != Ty::Str {
                            continue;
                        }
                        assert_builtin_error_ty(label, program, err_ty);
                        exact_builtin_result = true;
                    }
                }
            }
            conversions > 0 && result_tests > 0 && result_unwraps > 0 && exact_builtin_result
        }
        MirOwner::Regex => has(|rv| matches!(rv, Rvalue::RegexReplace { .. })),
        MirOwner::Template => {
            has(|rv| matches!(rv, Rvalue::Template(..)))
                && has(|rv| matches!(rv, Rvalue::StrClone(..)))
        }
        MirOwner::File => has(|rv| matches!(rv, Rvalue::FileCreateRw { .. })),
        MirOwner::ArrayBuilder => has(|rv| matches!(rv, Rvalue::ArrayBuilderPush { .. })),
        MirOwner::Command => has(|rv| matches!(rv, Rvalue::Command { .. })),
        MirOwner::Http => has(|rv| matches!(rv, Rvalue::HttpRequest { .. })),
        MirOwner::Match => {
            assert!(
                evidence.binary_matches > 0,
                "{label}: binary-match fixture contains no binary Match node"
            );
            assert_eq!(
                option_test_count, evidence.binary_matches,
                "{label}: binary Match nodes did not each emit one Option test"
            );
            assert_eq!(
                branch_count, evidence.binary_matches,
                "{label}: binary Match nodes did not each emit one branch"
            );
            true
        }
        MirOwner::Conditional => {
            assert!(
                evidence.short_circuits > 0 && evidence.else_unwraps > 0,
                "{label}: conditional fixture lacks one of its independent control families"
            );
            assert_eq!(
                option_test_count, evidence.else_unwraps,
                "{label}: else-unwrap nodes did not each emit one Option test"
            );
            assert_eq!(
                branch_count,
                evidence.short_circuits + evidence.else_unwraps,
                "{label}: short-circuit and else-unwrap nodes did not each emit one branch"
            );
            true
        }
        MirOwner::Scoped => {
            has(|rv| matches!(rv, Rvalue::ArenaBegin)) && has(|rv| matches!(rv, Rvalue::TgBegin))
        }
        MirOwner::Loop => program.fns.iter().any(|function| {
            function
                .blocks
                .iter()
                .any(|block| matches!(block.term, Term::Goto(_)))
        }),
        MirOwner::Stage => has(|rv| {
            matches!(rv, Rvalue::Call(name, _) if direct_program_name(name) == Some("dep$stage_id"))
        }),
        // Transparent blocks and expression statements emit no instruction of their own, so their
        // fixture ends in a producer-valid imported sentinel. Reaching that call proves the whole
        // structural spine was traversed rather than merely publishing an empty function.
        MirOwner::BlockStatement => has_call("dep$block_stmt_sentinel"),
        // A wildcard-only match has no tag test. Its sentinel proves the selected arm was lowered,
        // and the join edge proves the wildcard-match spine completed its parent action.
        MirOwner::WildcardMatch => {
            assert!(
                evidence.wildcard_matches > 0,
                "{label}: wildcard fixture contains no wildcard Match node"
            );
            assert_eq!(
                goto_count, evidence.wildcard_matches,
                "{label}: wildcard Match nodes did not each complete their join edge"
            );
            has_call("dep$wildcard_sentinel")
        }
        MirOwner::If => {
            assert!(evidence.ifs > 0, "{label}: if fixture contains no If node");
            assert_eq!(
                branch_count, evidence.ifs,
                "{label}: If nodes did not each emit one branch"
            );
            true
        }
    };
    if !owned {
        let (conversions, result_tests, result_unwraps) = bytes_str_counts();
        let arenas = count(|rv| matches!(rv, Rvalue::ArenaBegin));
        let task_groups = count(|rv| matches!(rv, Rvalue::TgBegin));
        panic!(
            "{label}: expected specialized MIR owner was not emitted (bytes.as_str={conversions}, ResultIsOk={result_tests}, ResultUnwrapOk={result_unwraps}, arenas={arenas}, task_groups={task_groups})"
        );
    }
}

fn assert_hir_owner_contract(label: &str, program: &hir::Program, owner: MirOwner) {
    let function = program
        .fns
        .first()
        .unwrap_or_else(|| panic!("{label}: depth fixture has no function"));
    match owner {
        MirOwner::Path | MirOwner::Regex => {
            assert_eq!(
                function.ret,
                Ty::String,
                "{label}: borrowed owned temporary escaped as the function result"
            );
            assert!(
                function.drop_individual_exprs.len() > 1
                    && function
                        .drop_individual_exprs
                        .values()
                        .all(|individual| *individual),
                "{label}: owned string temporaries lack sema-equivalent individual ownership"
            );
        }
        MirOwner::Template => {
            assert_eq!(
                function.ret,
                Ty::String,
                "{label}: hidden template string view escaped as the function result"
            );
            assert!(
                !function.drop_individual_exprs.is_empty()
                    && function
                        .drop_individual_exprs
                        .values()
                        .all(|individual| *individual),
                "{label}: cloned template result lacks individual ownership"
            );
        }
        MirOwner::Reader | MirOwner::File | MirOwner::Command | MirOwner::Http => {
            let mut work = function.body.value.as_deref().into_iter().collect::<Vec<_>>();
            // A Move-producing expression may be an expression statement whose result is
            // discarded. Include those roots in the producer-fact contract just as MIR does.
            work.extend(function.body.stmts.iter().filter_map(|statement| match statement {
                hir::Stmt::Expr(expression) => Some(expression),
                _ => None,
            }));
            let mut owned_expressions = 0;
            while let Some(expression) = work.pop() {
                if align_sema::needs_drop_flag(
                    expression.ty,
                    &program.structs,
                    &program.tuples,
                    &program.enums,
                    &program.tagged_types,
                ) {
                    owned_expressions += 1;
                    assert_eq!(
                        function.drop_individual_exprs.get(&expression.span),
                        Some(&true),
                        "{label}: owned producer lacks sema-equivalent individual ownership"
                    );
                }
                work.extend(align_sema::direct_expr_children(expression));
            }
            assert!(
                owned_expressions > 0,
                "{label}: Move-producing fixture contains no owned expression"
            );
            assert_eq!(
                function.drop_individual_exprs.len(),
                owned_expressions,
                "{label}: ownership table contains stale or aliased producer facts"
            );
        }
        _ => {}
    }
    if matches!(owner, MirOwner::Regex | MirOwner::ArrayBuilder) {
        assert_eq!(
            function.drop_locals, function.drop_individual_locals,
            "{label}: Move parameters do not have individual Drop ownership"
        );
        assert!(
            !function.drop_locals.is_empty(),
            "{label}: Move parameter is missing Drop metadata"
        );
    }
}

fn assert_accepted_impl(label: &str, program: &hir::Program, owner: Option<MirOwner>) {
    assert!(
        validate_hir::global_type_metadata_is_valid(program),
        "{label}: validator rejected valid metadata"
    );
    assert!(
        validate_hir::nominal_link_metadata_is_valid(program),
        "{label}: nominal/link validator rejected valid metadata"
    );
    assert!(
        validate_hir::type_placement_metadata_is_valid(program),
        "{label}: placement validator rejected valid metadata"
    );
    assert!(
        validate_hir::declaration_header_metadata_is_valid(program),
        "{label}: declaration/header validator rejected valid metadata"
    );
    if let Some(owner) = owner {
        assert_hir_owner_contract(label, program, owner);
    }
    let evidence = hir_owner_evidence(program);
    let source_map = SourceMap::new();
    for lowered in [
        lower_program(program),
        lower_program_located(program, &source_map),
        lower_program_per_unit(program),
        lower_program_per_unit_located(program, &source_map),
    ] {
        assert!(
            !is_empty(&lowered),
            "{label}: valid metadata did not reach an entrypoint"
        );
        assert_result_rvalue_contract(label, &lowered);
        if let Some(owner) = owner {
            assert_mir_owner(label, &lowered, owner, evidence);
        }
    }
}

fn assert_accepted(label: &str, program: &hir::Program) {
    assert_accepted_impl(label, program, None);
}

fn assert_owned_accepted(label: &str, program: &hir::Program, owner: MirOwner) {
    assert_accepted_impl(label, program, Some(owner));
}

fn with_unary_body_depth(depth: usize) -> hir::Program {
    assert!(depth >= 2, "the root Block and leaf Expr need depth two");
    let span = align_span::Span::new(0, 0, 0);
    let mut expr = hir::Expr {
        kind: hir::ExprKind::Int(0),
        ty: int(64),
        span,
    };
    for _ in 2..depth {
        expr = hir::Expr {
            kind: hir::ExprKind::Unary {
                op: align_ast::UnOp::Neg,
                expr: Box::new(expr),
            },
            ty: int(64),
            span,
        };
    }
    let mut program = baseline_program();
    program.fns.push(hir::Fn {
        name: "deep".to_string(),
        origin: hir::FnOrigin::Source { is_entry: false, is_public: false },
        params: Vec::new(),
        param_modes: Vec::new(),
        ret: int(64),
        return_borrow: ReturnBorrowSummary::None,
        return_region: ReturnRegionSummary::None,
        return_cleanup: hir::ReturnCleanupAbi::None,
        locals: Vec::new(),
        body: hir::Block {
            stmts: Vec::new(),
            value: Some(Box::new(expr)),
        },
        span,
        drop_locals: Vec::new(),
        drop_individual_locals: Vec::new(),
        drop_individual_exprs: Default::default(),
    });
    program
}

fn with_mixed_eager_body_depth(depth: usize) -> hir::Program {
    assert!(depth >= 2, "the root Block and leaf Expr need depth two");
    let span = align_span::Span::new(0, 0, 0);
    let mut expr = hir::Expr {
        kind: hir::ExprKind::Int(0),
        ty: int(32),
        span,
    };
    for expression_depth in 2..depth {
        let ty = expr.ty;
        expr = match expression_depth % 4 {
            0 => hir::Expr {
                kind: hir::ExprKind::Binary {
                    op: align_ast::BinOp::Add,
                    lhs: Box::new(expr),
                    rhs: Box::new(hir::Expr {
                        kind: hir::ExprKind::Int(1),
                        ty,
                        span,
                    }),
                },
                ty,
                span,
            },
            1 => hir::Expr {
                kind: hir::ExprKind::Unary {
                    op: align_ast::UnOp::Neg,
                    expr: Box::new(expr),
                },
                ty,
                span,
            },
            2 => {
                let ty = if ty == int(32) { int(64) } else { int(32) };
                hir::Expr {
                    kind: hir::ExprKind::Cast(Box::new(expr)),
                    ty,
                    span,
                }
            }
            _ => hir::Expr {
                kind: hir::ExprKind::Call {
                    func: if ty == int(32) {
                        "dep$id_i32".to_string()
                    } else {
                        "dep$id_i64".to_string()
                    },
                    args: vec![expr],
                    type_args: Vec::new(),
                },
                ty,
                span,
            },
        };
    }
    let ret = expr.ty;
    let mut program = baseline_program();
    program
        .imported_fns
        .push(imported_fn("dep$id_i32", vec![int(32)], int(32)));
    program
        .imported_fns
        .push(imported_fn("dep$id_i64", vec![int(64)], int(64)));
    program.fns.push(hir::Fn {
        name: "deep_mixed_eager".to_string(),
        origin: hir::FnOrigin::Source { is_entry: false, is_public: false },
        params: Vec::new(),
        param_modes: Vec::new(),
        ret,
        return_borrow: ReturnBorrowSummary::None,
        return_region: ReturnRegionSummary::None,
        return_cleanup: hir::ReturnCleanupAbi::None,
        locals: Vec::new(),
        body: hir::Block {
            stmts: Vec::new(),
            value: Some(Box::new(expr)),
        },
        span,
        drop_locals: Vec::new(),
        drop_individual_locals: Vec::new(),
        drop_individual_exprs: Default::default(),
    });
    program
}

fn with_str_trim_body_depth(depth: usize) -> hir::Program {
    assert!(depth >= 2, "the root Block and leaf Expr need depth two");
    let span = align_span::Span::new(0, 0, 0);
    let expr = str_trim_expr_depth(depth - 1);
    let mut program = baseline_program();
    program.fns.push(hir::Fn {
        name: "deep_str_trim".to_string(),
        origin: hir::FnOrigin::Source { is_entry: false, is_public: false },
        params: Vec::new(),
        param_modes: Vec::new(),
        ret: Ty::Str,
        return_borrow: ReturnBorrowSummary::None,
        return_region: ReturnRegionSummary::None,
        return_cleanup: hir::ReturnCleanupAbi::None,
        locals: Vec::new(),
        body: hir::Block {
            stmts: Vec::new(),
            value: Some(Box::new(expr)),
        },
        span,
        drop_locals: Vec::new(),
        drop_individual_locals: Vec::new(),
        drop_individual_exprs: Default::default(),
    });
    program
}

fn with_path_string_body_depth(depth: usize) -> hir::Program {
    assert!(
        depth >= 8,
        "the root Block, path cycle, and owned endpoint need depth eight"
    );
    let span = align_span::Span::new(0, 0, 0);
    let target_expr_depth = depth - 1;
    let inner_depth = target_expr_depth - 1;
    let mut drop_individual_exprs = std::collections::HashMap::new();
    let mut next_offset = 1;
    let mut expr = hir::Expr {
        kind: hir::ExprKind::Str("x".to_string()),
        ty: Ty::Str,
        span,
    };
    let cycle_count = (inner_depth - 1) / 5;
    for _ in 0..(inner_depth - 1) % 5 {
        expr = hir::Expr {
            kind: hir::ExprKind::StrTrim {
                kind: hir::StrTrimKind::Both,
                recv: Box::new(expr),
            },
            ty: Ty::Str,
            span,
        };
    }
    for _ in 0..cycle_count {
        expr = hir::Expr {
            kind: hir::ExprKind::PathComponent {
                kind: hir::PathComponentKind::Base,
                path: Box::new(expr),
            },
            ty: Ty::Str,
            span,
        };
        expr = hir::Expr {
            kind: hir::ExprKind::PathJoin {
                a: Box::new(expr),
                b: Box::new(hir::Expr {
                    kind: hir::ExprKind::Str("y".to_string()),
                    ty: Ty::Str,
                    span,
                }),
            },
            ty: Ty::String,
            span: individual_expr_span(&mut next_offset, &mut drop_individual_exprs),
        };
        expr = hir::Expr {
            kind: hir::ExprKind::StrBorrow(Box::new(expr)),
            ty: Ty::Str,
            span,
        };
        expr = hir::Expr {
            kind: hir::ExprKind::PathNormalize {
                path: Box::new(expr),
            },
            ty: Ty::String,
            span: individual_expr_span(&mut next_offset, &mut drop_individual_exprs),
        };
        expr = hir::Expr {
            kind: hir::ExprKind::StrBorrow(Box::new(expr)),
            ty: Ty::Str,
            span,
        };
    }
    expr = hir::Expr {
        kind: hir::ExprKind::PathJoin {
            a: Box::new(expr),
            b: Box::new(hir::Expr {
                kind: hir::ExprKind::Str("z".to_string()),
                ty: Ty::Str,
                span,
            }),
        },
        ty: Ty::String,
        span: individual_expr_span(&mut next_offset, &mut drop_individual_exprs),
    };
    let mut program = baseline_program();
    program.fns.push(hir::Fn {
        name: "deep_path_string".to_string(),
        origin: hir::FnOrigin::Source { is_entry: false, is_public: false },
        params: Vec::new(),
        param_modes: Vec::new(),
        ret: Ty::String,
        return_borrow: ReturnBorrowSummary::None,
        return_region: ReturnRegionSummary::None,
        return_cleanup: hir::ReturnCleanupAbi::None,
        locals: Vec::new(),
        body: hir::Block {
            stmts: Vec::new(),
            value: Some(Box::new(expr)),
        },
        span,
        drop_locals: Vec::new(),
        drop_individual_locals: Vec::new(),
        drop_individual_exprs,
    });
    program
}

fn with_reader_buffered_body_depth(depth: usize) -> hir::Program {
    assert!(depth >= 2, "the root Block and leaf Expr need depth two");
    let span = align_span::Span::new(0, 0, 0);
    let mut drop_individual_exprs = std::collections::HashMap::new();
    let mut next_offset = 1;
    let mut expr = hir::Expr {
        kind: hir::ExprKind::ReaderStdin,
        ty: Ty::Reader,
        span: individual_expr_span(&mut next_offset, &mut drop_individual_exprs),
    };
    for _ in 2..depth {
        expr = hir::Expr {
            kind: hir::ExprKind::ReaderBuffered {
                reader: Box::new(expr),
            },
            ty: Ty::Reader,
            span: individual_expr_span(&mut next_offset, &mut drop_individual_exprs),
        };
    }
    let mut program = baseline_program();
    program.fns.push(hir::Fn {
        name: "deep_reader_buffered".to_string(),
        origin: hir::FnOrigin::Source { is_entry: false, is_public: false },
        params: Vec::new(),
        param_modes: Vec::new(),
        ret: Ty::Reader,
        return_borrow: ReturnBorrowSummary::None,
        return_region: ReturnRegionSummary::None,
        return_cleanup: hir::ReturnCleanupAbi::None,
        locals: Vec::new(),
        body: hir::Block {
            stmts: Vec::new(),
            value: Some(Box::new(expr)),
        },
        span,
        drop_locals: Vec::new(),
        drop_individual_locals: Vec::new(),
        drop_individual_exprs,
    });
    program
}

fn with_bytes_str_cycle_body_depth(depth: usize) -> hir::Program {
    assert!(depth >= 2, "the root Block and leaf Expr need depth two");
    let span = align_span::Span::new(0, 0, 0);
    let bytes_ty = Ty::Slice(Scalar::Int(IntTy {
        bits: 8,
        signed: false,
    }));
    let mut program = baseline_program();
    let error_id = push_builtin_error(&mut program);
    let result_ty = Ty::Result(Scalar::Str, Scalar::Enum(error_id));
    let target_expr_depth = depth - 1;
    let cycle_depth = (1..=target_expr_depth)
        .rev()
        .find(|candidate| candidate.is_multiple_of(3) && candidate % 2 == target_expr_depth % 2)
        .expect("every requested boundary depth has a Result-ending cycle prefix");
    let mut expr = hir::Expr {
        kind: hir::ExprKind::Str("x".to_string()),
        ty: Ty::Str,
        span,
    };
    for _ in 1..cycle_depth {
        expr = match expr.ty {
            Ty::Str => hir::Expr {
                kind: hir::ExprKind::StrBytes {
                    inner: Box::new(expr),
                },
                ty: bytes_ty,
                span,
            },
            ty if ty == bytes_ty => hir::Expr {
                kind: hir::ExprKind::BytesAsStr {
                    bytes: Box::new(expr),
                },
                ty: result_ty,
                span,
            },
            ty if ty == result_ty => hir::Expr {
                kind: hir::ExprKind::Try(Box::new(expr)),
                ty: Ty::Str,
                span,
            },
            other => panic!("unexpected bytes/string cycle type: {other:?}"),
        };
    }
    assert_eq!(expr.ty, result_ty);
    let mut expr_depth = cycle_depth;
    while expr_depth < target_expr_depth {
        expr = hir::Expr {
            kind: hir::ExprKind::Block(hir::Block {
                stmts: Vec::new(),
                value: Some(Box::new(expr)),
            }),
            ty: result_ty,
            span,
        };
        expr_depth += 2;
    }
    assert_eq!(expr_depth, target_expr_depth);
    program.fns.push(hir::Fn {
        name: "deep_bytes_str_cycle".to_string(),
        origin: hir::FnOrigin::Source { is_entry: false, is_public: false },
        params: Vec::new(),
        param_modes: Vec::new(),
        ret: result_ty,
        return_borrow: ReturnBorrowSummary::None,
        return_region: ReturnRegionSummary::None,
        return_cleanup: hir::ReturnCleanupAbi::None,
        locals: Vec::new(),
        body: hir::Block {
            stmts: Vec::new(),
            value: Some(Box::new(expr)),
        },
        span,
        drop_locals: Vec::new(),
        drop_individual_locals: Vec::new(),
        drop_individual_exprs: Default::default(),
    });
    program
}

fn with_regex_string_body_depth(depth: usize) -> hir::Program {
    assert!(
        depth >= 5,
        "the root Block, regex cycle, and owned endpoint need depth five"
    );
    let span = align_span::Span::new(0, 0, 0);
    let target_expr_depth = depth - 1;
    let inner_depth = target_expr_depth - 1;
    let mut drop_individual_exprs = std::collections::HashMap::new();
    let mut next_offset = 1;
    let mut expr = hir::Expr {
        kind: hir::ExprKind::Str("x".to_string()),
        ty: Ty::Str,
        span,
    };
    for _ in 0..(inner_depth - 1) % 2 {
        expr = hir::Expr {
            kind: hir::ExprKind::StrTrim {
                kind: hir::StrTrimKind::Both,
                recv: Box::new(expr),
            },
            ty: Ty::Str,
            span,
        };
    }
    for _ in 0..(inner_depth - 1) / 2 {
        expr = hir::Expr {
            kind: hir::ExprKind::RegexReplace {
                regex: Box::new(hir::Expr {
                    kind: hir::ExprKind::Local(0),
                    ty: Ty::Regex,
                    span,
                }),
                text: Box::new(expr),
                repl: Box::new(hir::Expr {
                    kind: hir::ExprKind::Str("y".to_string()),
                    ty: Ty::Str,
                    span,
                }),
                all: false,
            },
            ty: Ty::String,
            span: individual_expr_span(&mut next_offset, &mut drop_individual_exprs),
        };
        expr = hir::Expr {
            kind: hir::ExprKind::StrBorrow(Box::new(expr)),
            ty: Ty::Str,
            span,
        };
    }
    expr = hir::Expr {
        kind: hir::ExprKind::RegexReplace {
            regex: Box::new(hir::Expr {
                kind: hir::ExprKind::Local(0),
                ty: Ty::Regex,
                span,
            }),
            text: Box::new(expr),
            repl: Box::new(hir::Expr {
                kind: hir::ExprKind::Str("z".to_string()),
                ty: Ty::Str,
                span,
            }),
            all: false,
        },
        ty: Ty::String,
        span: individual_expr_span(&mut next_offset, &mut drop_individual_exprs),
    };
    // The regex parameter is a Move handle. Its synthetic Local expression has the zero span
    // used by this handcrafted boundary fixture, so the producer's DropProvenance map carries
    // the shared zero-span entry alongside the individually-spanned replacement nodes.
    drop_individual_exprs.insert(span, true);
    let mut program = baseline_program();
    program.fns.push(hir::Fn {
        name: "deep_regex_string".to_string(),
        origin: hir::FnOrigin::Source { is_entry: false, is_public: false },
        params: vec![0],
        param_modes: vec![align_ast::ParamMode::ByValue],
        ret: Ty::String,
        return_borrow: ReturnBorrowSummary::None,
        return_region: ReturnRegionSummary::None,
        return_cleanup: hir::ReturnCleanupAbi::None,
        locals: vec![hir::Local {
            id: 0,
            name: "regex".to_string(),
            ty: Ty::Regex,
            is_mut: false,
            is_param: true,
            align: None,
        }],
        body: hir::Block {
            stmts: Vec::new(),
            value: Some(Box::new(expr)),
        },
        span,
        drop_locals: vec![0],
        drop_individual_locals: vec![0],
        drop_individual_exprs,
    });
    program
}

fn with_template_body_depth(depth: usize) -> hir::Program {
    assert!(
        depth >= 4,
        "the root Block, template view, and owned clone need depth four"
    );
    let span = align_span::Span::new(0, 0, 0);
    let target_expr_depth = depth - 1;
    let inner_depth = target_expr_depth - 1;
    let mut expr = hir::Expr {
        kind: hir::ExprKind::Str("x".to_string()),
        ty: Ty::Str,
        span,
    };
    let mut expr_depth = 1;
    if inner_depth.is_multiple_of(2) {
        expr = hir::Expr {
            kind: hir::ExprKind::StrTrim {
                kind: hir::StrTrimKind::Both,
                recv: Box::new(expr),
            },
            ty: Ty::Str,
            span,
        };
        expr_depth += 1;
    }
    while expr_depth < inner_depth {
        expr = hir::Expr {
            kind: hir::ExprKind::Template(vec![hir::TemplatePart::Hole(expr)]),
            ty: Ty::Str,
            span,
        };
        expr_depth += 2;
    }
    let mut drop_individual_exprs = std::collections::HashMap::new();
    let mut next_offset = 1;
    expr = hir::Expr {
        kind: hir::ExprKind::StrClone(Box::new(expr)),
        ty: Ty::String,
        span: individual_expr_span(&mut next_offset, &mut drop_individual_exprs),
    };
    let mut program = baseline_program();
    program.fns.push(hir::Fn {
        name: "deep_template".to_string(),
        origin: hir::FnOrigin::Source { is_entry: false, is_public: false },
        params: Vec::new(),
        param_modes: Vec::new(),
        ret: Ty::String,
        return_borrow: ReturnBorrowSummary::None,
        return_region: ReturnRegionSummary::None,
        return_cleanup: hir::ReturnCleanupAbi::None,
        locals: Vec::new(),
        body: hir::Block {
            stmts: Vec::new(),
            value: Some(Box::new(expr)),
        },
        span,
        drop_locals: Vec::new(),
        drop_individual_locals: Vec::new(),
        drop_individual_exprs,
    });
    program
}

fn with_file_body_depth(depth: usize) -> hir::Program {
    assert!(
        depth >= 3,
        "the root Block, file Expr, and path need depth three"
    );
    let span = align_span::Span::new(0, 0, 0);
    let mut program = baseline_program();
    let error_id = push_builtin_error(&mut program);
    let result_ty = Ty::Result(Scalar::File, Scalar::Enum(error_id));
    let mut drop_individual_exprs = std::collections::HashMap::new();
    let mut next_offset = 1;
    let expr = hir::Expr {
        kind: hir::ExprKind::FileCreateRw {
            path: Box::new(str_trim_expr_depth(depth - 2)),
        },
        ty: result_ty,
        span: individual_expr_span(&mut next_offset, &mut drop_individual_exprs),
    };
    program.fns.push(hir::Fn {
        name: "deep_file".to_string(),
        origin: hir::FnOrigin::Source { is_entry: false, is_public: false },
        params: Vec::new(),
        param_modes: Vec::new(),
        ret: result_ty,
        return_borrow: ReturnBorrowSummary::None,
        return_region: ReturnRegionSummary::None,
        return_cleanup: hir::ReturnCleanupAbi::None,
        locals: Vec::new(),
        body: hir::Block {
            stmts: Vec::new(),
            value: Some(Box::new(expr)),
        },
        span,
        drop_locals: Vec::new(),
        drop_individual_locals: Vec::new(),
        drop_individual_exprs,
    });
    program
}

fn with_array_builder_body_depth(depth: usize) -> hir::Program {
    assert!(
        depth >= 3,
        "the root Block, array-builder Expr, and value need depth three"
    );
    let span = align_span::Span::new(0, 0, 0);
    let elem = ArrayBuilderElem::Scalar(scalar_int(64));
    let builder_ty = Ty::array_builder(elem);
    let mut drop_individual_exprs = std::collections::HashMap::new();
    drop_individual_exprs.insert(span, true);
    let expr = hir::Expr {
        kind: hir::ExprKind::ArrayBuilderPush {
            builder: Box::new(hir::Expr {
                kind: hir::ExprKind::Local(0),
                ty: builder_ty,
                span,
            }),
            value: Box::new(unary_int_expr_depth(depth - 2, int(64))),
            moves_value: false,
        },
        ty: Ty::Unit,
        span,
    };
    let mut program = baseline_program();
    program.fns.push(hir::Fn {
        name: "deep_array_builder".to_string(),
        origin: hir::FnOrigin::Source { is_entry: false, is_public: false },
        params: vec![0],
        param_modes: vec![align_ast::ParamMode::ByValue],
        ret: Ty::Unit,
        return_borrow: ReturnBorrowSummary::None,
        return_region: ReturnRegionSummary::None,
        return_cleanup: hir::ReturnCleanupAbi::None,
        locals: vec![hir::Local {
            id: 0,
            name: "builder".to_string(),
            ty: builder_ty,
            is_mut: true,
            is_param: true,
            align: None,
        }],
        body: hir::Block {
            stmts: Vec::new(),
            value: Some(Box::new(expr)),
        },
        span,
        drop_locals: vec![0],
        drop_individual_locals: vec![0],
        drop_individual_exprs,
    });
    program
}

fn with_process_command_body_depth(depth: usize) -> hir::Program {
    assert!(
        depth >= 4,
        "the root Block, expression statement, process-command Expr, and command need depth four"
    );
    let span = align_span::Span::new(0, 0, 0);
    let argv_ty = Ty::Slice(Scalar::Str);
    let mut drop_individual_exprs = std::collections::HashMap::new();
    let mut next_offset = 1;
    let expr = hir::Expr {
        kind: hir::ExprKind::ProcessCommand {
            cmd: Box::new(str_trim_expr_depth(depth - 3)),
            args: Box::new(hir::Expr {
                kind: hir::ExprKind::Local(0),
                ty: argv_ty,
                span,
            }),
        },
        ty: Ty::Command,
        span: individual_expr_span(&mut next_offset, &mut drop_individual_exprs),
    };
    let mut program = baseline_program();
    program.fns.push(hir::Fn {
        name: "deep_process_command".to_string(),
        origin: hir::FnOrigin::Source { is_entry: false, is_public: false },
        params: vec![0],
        param_modes: vec![align_ast::ParamMode::ByValue],
        // `command` is a body-produced builder, not a source-nameable header type. Keep the
        // deep producer as an expression statement so this synthetic function retains a valid
        // `unit` declaration return while MIR still proves the command owner was lowered.
        ret: Ty::Unit,
        return_borrow: ReturnBorrowSummary::None,
        return_region: ReturnRegionSummary::None,
        return_cleanup: hir::ReturnCleanupAbi::None,
        locals: vec![hir::Local {
            id: 0,
            name: "argv".to_string(),
            ty: argv_ty,
            is_mut: false,
            is_param: true,
            align: None,
        }],
        body: hir::Block {
            stmts: vec![hir::Stmt::Expr(expr)],
            value: None,
        },
        span,
        drop_locals: Vec::new(),
        drop_individual_locals: Vec::new(),
        drop_individual_exprs,
    });
    program
}

fn with_http_body_depth(depth: usize) -> hir::Program {
    assert!(
        depth >= 4,
        "the root Block, expression statement, HTTP Expr, and method need depth four"
    );
    let span = align_span::Span::new(0, 0, 0);
    let mut drop_individual_exprs = std::collections::HashMap::new();
    let mut next_offset = 1;
    let expr = hir::Expr {
        kind: hir::ExprKind::HttpRequest {
            method: Box::new(str_trim_expr_depth(depth - 3)),
            url: Box::new(hir::Expr {
                kind: hir::ExprKind::Str("https://example.invalid".to_string()),
                ty: Ty::Str,
                span,
            }),
        },
        ty: Ty::HttpRequest,
        span: individual_expr_span(&mut next_offset, &mut drop_individual_exprs),
    };
    let mut program = baseline_program();
    program.fns.push(hir::Fn {
        name: "deep_http".to_string(),
        origin: hir::FnOrigin::Source { is_entry: false, is_public: false },
        params: Vec::new(),
        param_modes: Vec::new(),
        // `http_request` is a body-produced builder, not a source-nameable header type. Keep the
        // deep producer as an expression statement so this synthetic function retains a valid
        // `unit` declaration return while MIR still proves the request owner was lowered.
        ret: Ty::Unit,
        return_borrow: ReturnBorrowSummary::None,
        return_region: ReturnRegionSummary::None,
        return_cleanup: hir::ReturnCleanupAbi::None,
        locals: Vec::new(),
        body: hir::Block {
            stmts: vec![hir::Stmt::Expr(expr)],
            value: None,
        },
        span,
        drop_locals: Vec::new(),
        drop_individual_locals: Vec::new(),
        drop_individual_exprs,
    });
    program
}

fn with_block_stmt_body_depth(depth: usize) -> hir::Program {
    assert!(depth >= 2, "the root Block and leaf Expr need depth two");
    let span = align_span::Span::new(0, 0, 0);
    let target_expr_depth = depth - 1;
    let mut stmt_layers = (target_expr_depth - 1) / 3;
    while !(target_expr_depth - 1 - 3 * stmt_layers).is_multiple_of(2) {
        stmt_layers -= 1;
    }
    let mut expr = hir::Expr {
        kind: hir::ExprKind::Call {
            func: "dep$block_stmt_sentinel".to_string(),
            args: Vec::new(),
            type_args: Vec::new(),
        },
        ty: Ty::Unit,
        span,
    };
    let mut expr_depth = 1;
    for _ in 0..stmt_layers {
        expr = hir::Expr {
            kind: hir::ExprKind::Block(hir::Block {
                stmts: vec![hir::Stmt::Expr(expr)],
                value: None,
            }),
            ty: Ty::Unit,
            span,
        };
        expr_depth += 3;
    }
    while expr_depth < target_expr_depth {
        expr = hir::Expr {
            kind: hir::ExprKind::Block(hir::Block {
                stmts: Vec::new(),
                value: Some(Box::new(expr)),
            }),
            ty: Ty::Unit,
            span,
        };
        expr_depth += 2;
    }
    let mut program = baseline_program();
    program
        .imported_fns
        .push(imported_fn("dep$block_stmt_sentinel", Vec::new(), Ty::Unit));
    program.fns.push(hir::Fn {
        name: "deep_block_stmt".to_string(),
        origin: hir::FnOrigin::Source { is_entry: false, is_public: false },
        params: Vec::new(),
        param_modes: Vec::new(),
        ret: Ty::Unit,
        return_borrow: ReturnBorrowSummary::None,
        return_region: ReturnRegionSummary::None,
        return_cleanup: hir::ReturnCleanupAbi::None,
        locals: Vec::new(),
        body: hir::Block {
            stmts: Vec::new(),
            value: Some(Box::new(expr)),
        },
        span,
        drop_locals: Vec::new(),
        drop_individual_locals: Vec::new(),
        drop_individual_exprs: Default::default(),
    });
    program
}

fn with_match_arm_body_depth(depth: usize) -> hir::Program {
    assert!(depth >= 2, "the root Block and leaf Expr need depth two");
    let span = align_span::Span::new(0, 0, 0);
    let target_expr_depth = depth - 1;
    let option_unit = Ty::Option(Scalar::Unit);
    let mut expr = hir::Expr {
        kind: hir::ExprKind::Call {
            func: "dep$wildcard_sentinel".to_string(),
            args: Vec::new(),
            type_args: Vec::new(),
        },
        ty: Ty::Unit,
        span,
    };
    let mut expr_depth = 1;
    if target_expr_depth.is_multiple_of(2) {
        expr = hir::Expr {
            kind: hir::ExprKind::OptionSome(Box::new(expr)),
            ty: option_unit,
            span,
        };
        expr_depth += 1;
    }
    while expr_depth < target_expr_depth {
        let result_ty = expr.ty;
        expr = hir::Expr {
            kind: hir::ExprKind::Match {
                scrutinee: Box::new(hir::Expr {
                    kind: hir::ExprKind::OptionNone,
                    ty: option_unit,
                    span,
                }),
                arms: vec![hir::MatchArm {
                    variants: Vec::new(),
                    bindings: Vec::new(),
                    body: expr,
                }],
            },
            ty: result_ty,
            span,
        };
        expr_depth += 2;
    }
    let mut program = baseline_program();
    program
        .imported_fns
        .push(imported_fn("dep$wildcard_sentinel", Vec::new(), Ty::Unit));
    let mut drop_individual_exprs = std::collections::HashMap::new();
    drop_individual_exprs.insert(span, true);
    program.fns.push(hir::Fn {
        name: "deep_match_arm".to_string(),
        origin: hir::FnOrigin::Source { is_entry: false, is_public: false },
        params: Vec::new(),
        param_modes: Vec::new(),
        ret: expr.ty,
        return_borrow: ReturnBorrowSummary::None,
        return_region: ReturnRegionSummary::None,
        return_cleanup: hir::ReturnCleanupAbi::None,
        locals: Vec::new(),
        body: hir::Block {
            stmts: Vec::new(),
            value: Some(Box::new(expr)),
        },
        span,
        drop_locals: Vec::new(),
        drop_individual_locals: Vec::new(),
        drop_individual_exprs,
    });
    program
}

fn with_if_branch_body_depth(depth: usize) -> hir::Program {
    assert!(
        depth >= 3,
        "the root Block, If, and branch Expr need depth three"
    );
    let span = align_span::Span::new(0, 0, 0);
    let target_expr_depth = depth - 1;
    let option_unit = Ty::Option(Scalar::Unit);
    let mut expr = if target_expr_depth.is_multiple_of(2) {
        hir::Expr {
            kind: hir::ExprKind::OptionSome(Box::new(hir::Expr {
                kind: hir::ExprKind::Unit,
                ty: Ty::Unit,
                span,
            })),
            ty: option_unit,
            span,
        }
    } else {
        hir::Expr {
            kind: hir::ExprKind::Unit,
            ty: Ty::Unit,
            span,
        }
    };
    let mut expr_depth = if target_expr_depth.is_multiple_of(2) {
        2
    } else {
        1
    };
    while expr_depth < target_expr_depth {
        let ty = expr.ty;
        let else_value = hir::Expr {
            kind: if ty == option_unit {
                hir::ExprKind::OptionNone
            } else {
                hir::ExprKind::Unit
            },
            ty,
            span,
        };
        expr = hir::Expr {
            kind: hir::ExprKind::If {
                cond: Box::new(hir::Expr {
                    kind: hir::ExprKind::Bool(true),
                    ty: Ty::Bool,
                    span,
                }),
                then: hir::Block {
                    stmts: Vec::new(),
                    value: Some(Box::new(expr)),
                },
                els: hir::Block {
                    stmts: Vec::new(),
                    value: Some(Box::new(else_value)),
                },
            },
            ty,
            span,
        };
        expr_depth += 2;
    }
    let ret = expr.ty;
    let mut program = baseline_program();
    program.fns.push(hir::Fn {
        name: "deep_if_branch".to_string(),
        origin: hir::FnOrigin::Source { is_entry: false, is_public: false },
        params: Vec::new(),
        param_modes: Vec::new(),
        ret,
        return_borrow: ReturnBorrowSummary::None,
        return_region: ReturnRegionSummary::None,
        return_cleanup: hir::ReturnCleanupAbi::None,
        locals: Vec::new(),
        body: hir::Block {
            stmts: Vec::new(),
            value: Some(Box::new(expr)),
        },
        span,
        drop_locals: Vec::new(),
        drop_individual_locals: Vec::new(),
        drop_individual_exprs: Default::default(),
    });
    program
}

fn with_binary_match_body_depth(depth: usize) -> hir::Program {
    assert!(
        depth >= 3,
        "the root Block, Match, and arm Expr need depth three"
    );
    let span = align_span::Span::new(0, 0, 0);
    let target_expr_depth = depth - 1;
    let option_unit = Ty::Option(Scalar::Unit);
    let mut expr = if target_expr_depth.is_multiple_of(2) {
        hir::Expr {
            kind: hir::ExprKind::OptionSome(Box::new(hir::Expr {
                kind: hir::ExprKind::Unit,
                ty: Ty::Unit,
                span,
            })),
            ty: option_unit,
            span,
        }
    } else {
        hir::Expr {
            kind: hir::ExprKind::Unit,
            ty: Ty::Unit,
            span,
        }
    };
    let mut expr_depth = if target_expr_depth.is_multiple_of(2) {
        2
    } else {
        1
    };
    while expr_depth < target_expr_depth {
        let ty = expr.ty;
        let default_body = hir::Expr {
            kind: if ty == option_unit {
                hir::ExprKind::OptionNone
            } else {
                hir::ExprKind::Unit
            },
            ty,
            span,
        };
        expr = hir::Expr {
            kind: hir::ExprKind::Match {
                scrutinee: Box::new(hir::Expr {
                    kind: hir::ExprKind::OptionNone,
                    ty: option_unit,
                    span,
                }),
                arms: vec![
                    hir::MatchArm {
                        variants: vec![1],
                        bindings: Vec::new(),
                        body: expr,
                    },
                    hir::MatchArm {
                        variants: Vec::new(),
                        bindings: Vec::new(),
                        body: default_body,
                    },
                ],
            },
            ty,
            span,
        };
        expr_depth += 2;
    }
    let ret = expr.ty;
    let mut drop_individual_exprs = std::collections::HashMap::new();
    drop_individual_exprs.insert(span, true);
    let mut program = baseline_program();
    program.fns.push(hir::Fn {
        name: "deep_binary_match".to_string(),
        origin: hir::FnOrigin::Source { is_entry: false, is_public: false },
        params: Vec::new(),
        param_modes: Vec::new(),
        ret,
        return_borrow: ReturnBorrowSummary::None,
        return_region: ReturnRegionSummary::None,
        return_cleanup: hir::ReturnCleanupAbi::None,
        locals: Vec::new(),
        body: hir::Block {
            stmts: Vec::new(),
            value: Some(Box::new(expr)),
        },
        span,
        drop_locals: Vec::new(),
        drop_individual_locals: Vec::new(),
        drop_individual_exprs,
    });
    program
}

fn with_conditional_operand_body_depth(depth: usize) -> hir::Program {
    assert!(depth >= 2, "the root Block and leaf Expr need depth two");
    let span = align_span::Span::new(0, 0, 0);
    let option_bool = Ty::Option(Scalar::Bool);
    let mut expr = hir::Expr {
        kind: hir::ExprKind::Bool(true),
        ty: Ty::Bool,
        span,
    };
    for expression_depth in 2..depth {
        expr = if expression_depth.is_multiple_of(2) {
            hir::Expr {
                kind: hir::ExprKind::Binary {
                    op: align_ast::BinOp::And,
                    lhs: Box::new(hir::Expr {
                        kind: hir::ExprKind::Bool(true),
                        ty: Ty::Bool,
                        span,
                    }),
                    rhs: Box::new(expr),
                },
                ty: Ty::Bool,
                span,
            }
        } else {
            hir::Expr {
                kind: hir::ExprKind::ElseUnwrap {
                    opt: Box::new(hir::Expr {
                        kind: hir::ExprKind::OptionSome(Box::new(hir::Expr {
                            kind: hir::ExprKind::Bool(false),
                            ty: Ty::Bool,
                            span,
                        })),
                        ty: option_bool,
                        span,
                    }),
                    fallback: Box::new(expr),
                },
                ty: Ty::Bool,
                span,
            }
        };
    }
    let mut program = baseline_program();
    program.fns.push(hir::Fn {
        name: "deep_conditional_operand".to_string(),
        origin: hir::FnOrigin::Source { is_entry: false, is_public: false },
        params: Vec::new(),
        param_modes: Vec::new(),
        ret: Ty::Bool,
        return_borrow: ReturnBorrowSummary::None,
        return_region: ReturnRegionSummary::None,
        return_cleanup: hir::ReturnCleanupAbi::None,
        locals: Vec::new(),
        body: hir::Block {
            stmts: Vec::new(),
            value: Some(Box::new(expr)),
        },
        span,
        drop_locals: Vec::new(),
        drop_individual_locals: Vec::new(),
        drop_individual_exprs: Default::default(),
    });
    program
}

fn with_scoped_control_body_depth(depth: usize) -> hir::Program {
    assert!(depth >= 2, "the root Block and leaf Expr need depth two");
    let span = align_span::Span::new(0, 0, 0);
    let target_expr_depth = depth - 1;
    let option_unit = Ty::Option(Scalar::Unit);
    let mut expr = if target_expr_depth.is_multiple_of(2) {
        hir::Expr {
            kind: hir::ExprKind::OptionSome(Box::new(hir::Expr {
                kind: hir::ExprKind::Unit,
                ty: Ty::Unit,
                span,
            })),
            ty: option_unit,
            span,
        }
    } else {
        hir::Expr {
            kind: hir::ExprKind::Unit,
            ty: Ty::Unit,
            span,
        }
    };
    let mut expr_depth = if target_expr_depth.is_multiple_of(2) {
        2
    } else {
        1
    };
    while expr_depth < target_expr_depth {
        let ty = expr.ty;
        let block = hir::Block {
            stmts: Vec::new(),
            value: Some(Box::new(expr)),
        };
        expr = hir::Expr {
            kind: if expr_depth % 4 < 2 {
                hir::ExprKind::Arena(block)
            } else {
                hir::ExprKind::TaskGroup(block)
            },
            ty,
            span,
        };
        expr_depth += 2;
    }
    let ret = expr.ty;
    let mut program = baseline_program();
    program.fns.push(hir::Fn {
        name: "deep_scoped_control".to_string(),
        origin: hir::FnOrigin::Source { is_entry: false, is_public: false },
        params: Vec::new(),
        param_modes: Vec::new(),
        ret,
        return_borrow: ReturnBorrowSummary::None,
        return_region: ReturnRegionSummary::None,
        return_cleanup: hir::ReturnCleanupAbi::None,
        locals: Vec::new(),
        body: hir::Block {
            stmts: Vec::new(),
            value: Some(Box::new(expr)),
        },
        span,
        drop_locals: Vec::new(),
        drop_individual_locals: Vec::new(),
        drop_individual_exprs: Default::default(),
    });
    program
}

fn with_loop_body_depth(depth: usize) -> hir::Program {
    assert!(depth >= 2, "the root Block and leaf Expr need depth two");
    let span = align_span::Span::new(0, 0, 0);
    let target_expr_depth = depth - 1;
    let mut expr = hir::Expr {
        kind: hir::ExprKind::Int(0),
        ty: int(64),
        span,
    };
    let mut expr_depth = 1usize;
    for _ in 0..(target_expr_depth - 1) % 3 {
        expr = hir::Expr {
            kind: hir::ExprKind::Unary {
                op: align_ast::UnOp::Neg,
                expr: Box::new(expr),
            },
            ty: int(64),
            span,
        };
        expr_depth += 1;
    }
    while expr_depth < target_expr_depth {
        expr = hir::Expr {
            kind: hir::ExprKind::Loop {
                body: hir::Block {
                    stmts: vec![hir::Stmt::Break {
                        value: Some(expr),
                        accepted: true,
                    }],
                    value: None,
                },
                diverges: false,
                body_locals: 0..0,
            },
            ty: int(64),
            span,
        };
        expr_depth += 3;
    }
    let mut program = baseline_program();
    program.fns.push(hir::Fn {
        name: "deep_loop".to_string(),
        origin: hir::FnOrigin::Source { is_entry: false, is_public: false },
        params: Vec::new(),
        param_modes: Vec::new(),
        ret: int(64),
        return_borrow: ReturnBorrowSummary::None,
        return_region: ReturnRegionSummary::None,
        return_cleanup: hir::ReturnCleanupAbi::None,
        locals: Vec::new(),
        body: hir::Block {
            stmts: Vec::new(),
            value: Some(Box::new(expr)),
        },
        span,
        drop_locals: Vec::new(),
        drop_individual_locals: Vec::new(),
        drop_individual_exprs: Default::default(),
    });
    program
}

fn with_stage_body_depth(depth: usize) -> hir::Program {
    assert!(
        depth >= 4,
        "the root Block, pipeline Expr, Stage, and capture need depth four"
    );
    let span = align_span::Span::new(0, 0, 0);
    let target_expr_depth = depth - 1;
    let array_ty = Ty::DynArray(Scalar::Int(IntTy {
        bits: 64,
        signed: true,
    }));
    let mut expr = hir::Expr {
        kind: hir::ExprKind::ArraySum {
            source: Box::new(hir::Expr {
                kind: hir::ExprKind::Local(0),
                ty: array_ty,
                span,
            }),
            stages: vec![hir::Stage {
                kind: hir::StageKind::Map {
                    func: "dep$stage_id".to_string(),
                    captures: vec![hir::Expr {
                        kind: hir::ExprKind::Int(1),
                        ty: int(64),
                        span,
                    }],
                },
                out_ty: int(64),
            }],
        },
        ty: int(64),
        span,
    };
    let mut expr_depth = 3;
    while expr_depth < target_expr_depth {
        expr = hir::Expr {
            kind: hir::ExprKind::Unary {
                op: align_ast::UnOp::Neg,
                expr: Box::new(expr),
            },
            ty: int(64),
            span,
        };
        expr_depth += 1;
    }
    let mut program = baseline_program();
    program
        .imported_fns
        .push(imported_fn("dep$stage_id", vec![int(64), int(64)], int(64)));
    program.fns.push(hir::Fn {
        name: "deep_stage".to_string(),
        origin: hir::FnOrigin::Source { is_entry: false, is_public: false },
        params: vec![0],
        param_modes: vec![align_ast::ParamMode::ByValue],
        ret: int(64),
        return_borrow: ReturnBorrowSummary::None,
        return_region: ReturnRegionSummary::None,
        return_cleanup: hir::ReturnCleanupAbi::None,
        locals: vec![hir::Local {
            id: 0,
            name: "xs".to_string(),
            ty: array_ty,
            is_mut: false,
            is_param: true,
            align: None,
        }],
        body: hir::Block {
            stmts: Vec::new(),
            value: Some(Box::new(expr)),
        },
        span,
        drop_locals: vec![0],
        drop_individual_locals: vec![0],
        drop_individual_exprs: {
            let mut facts = std::collections::HashMap::new();
            facts.insert(span, true);
            facts
        },
    });
    program
}

#[derive(Clone, Copy)]
struct DepthFixture {
    name: &'static str,
    make: fn(usize) -> hir::Program,
    owner: MirOwner,
}

fn normalize_test_return_cleanup(program: &mut hir::Program) {
    let structs = &program.structs;
    let tuples = &program.tuples;
    let enums = &program.enums;
    let tagged_types = &program.tagged_types;
    let classify = |ret| {
        if align_sema::needs_drop_flag(
            ret,
            structs,
            tuples,
            enums,
            tagged_types,
        ) {
            hir::ReturnCleanupAbi::DynamicBit
        } else {
            hir::ReturnCleanupAbi::None
        }
    };
    for function in &mut program.fns {
        function.return_cleanup = classify(function.ret);
    }
    for function in &mut program.externs {
        function.return_cleanup = classify(function.ret);
    }
    for function in &mut program.imported_fns {
        function.return_cleanup = classify(function.ret);
    }
    for function in &mut program.fn_types {
        function.return_cleanup = classify(function.ret);
    }
}

fn depth_fixtures() -> Vec<DepthFixture> {
    vec![
        DepthFixture {
            name: "unary",
            make: with_unary_body_depth,
            owner: MirOwner::Unary,
        },
        DepthFixture {
            name: "mixed eager",
            make: with_mixed_eager_body_depth,
            owner: MirOwner::MixedEager,
        },
        DepthFixture {
            name: "string trim",
            make: with_str_trim_body_depth,
            owner: MirOwner::StrTrim,
        },
        DepthFixture {
            name: "path",
            make: with_path_string_body_depth,
            owner: MirOwner::Path,
        },
        DepthFixture {
            name: "reader",
            make: with_reader_buffered_body_depth,
            owner: MirOwner::Reader,
        },
        DepthFixture {
            name: "bytes/string/Try",
            make: with_bytes_str_cycle_body_depth,
            owner: MirOwner::BytesStrTry,
        },
        DepthFixture {
            name: "regex",
            make: with_regex_string_body_depth,
            owner: MirOwner::Regex,
        },
        DepthFixture {
            name: "template",
            make: with_template_body_depth,
            owner: MirOwner::Template,
        },
        DepthFixture {
            name: "file",
            make: with_file_body_depth,
            owner: MirOwner::File,
        },
        DepthFixture {
            name: "array builder",
            make: with_array_builder_body_depth,
            owner: MirOwner::ArrayBuilder,
        },
        DepthFixture {
            name: "process command",
            make: with_process_command_body_depth,
            owner: MirOwner::Command,
        },
        DepthFixture {
            name: "HTTP",
            make: with_http_body_depth,
            owner: MirOwner::Http,
        },
        DepthFixture {
            name: "block/statement",
            make: with_block_stmt_body_depth,
            owner: MirOwner::BlockStatement,
        },
        DepthFixture {
            name: "wildcard match",
            make: with_match_arm_body_depth,
            owner: MirOwner::WildcardMatch,
        },
        DepthFixture {
            name: "if",
            make: with_if_branch_body_depth,
            owner: MirOwner::If,
        },
        DepthFixture {
            name: "binary match",
            make: with_binary_match_body_depth,
            owner: MirOwner::Match,
        },
        DepthFixture {
            name: "conditional operand",
            make: with_conditional_operand_body_depth,
            owner: MirOwner::Conditional,
        },
        DepthFixture {
            name: "arena/task-group",
            make: with_scoped_control_body_depth,
            owner: MirOwner::Scoped,
        },
        DepthFixture {
            name: "loop",
            make: with_loop_body_depth,
            owner: MirOwner::Loop,
        },
        DepthFixture {
            name: "stage",
            make: with_stage_body_depth,
            owner: MirOwner::Stage,
        },
    ]
}

#[test]
fn checked_hir_depth_closure_matrix() {
    std::thread::Builder::new()
        .name("checked-hir-depth-mir-owner".to_string())
        .stack_size(2 * 1024 * 1024)
        .spawn(|| {
            for fixture in depth_fixtures() {
                for depth in [
                    align_sema::MAX_CHECKED_HIR_DEPTH - 1,
                    align_sema::MAX_CHECKED_HIR_DEPTH,
                ] {
                    let mut program = (fixture.make)(depth);
                    normalize_test_return_cleanup(&mut program);
                    assert!(
                        align_sema::checked_hir_body_depth_is_valid(&program),
                        "{}: valid checked-HIR depth {depth} was rejected",
                        fixture.name
                    );
                    assert_owned_accepted(fixture.name, &program, fixture.owner);
                }
            }

            let depth = align_sema::MAX_CHECKED_HIR_DEPTH + 1;
            let source_map = SourceMap::new();
            for fixture in depth_fixtures() {
                let mut program = (fixture.make)(depth);
                normalize_test_return_cleanup(&mut program);
                assert!(
                    !align_sema::checked_hir_body_depth_is_valid(&program),
                    "{}: over-bound checked-HIR depth {depth} was accepted",
                    fixture.name
                );
                for lowered in [
                    lower_program(&program),
                    lower_program_located(&program, &source_map),
                    lower_program_per_unit(&program),
                    lower_program_per_unit_located(&program, &source_map),
                ] {
                    assert!(
                        is_empty(&lowered),
                        "{}: an over-bound body published partial MIR",
                        fixture.name
                    );
                }
            }
        })
        .expect("spawn checked-HIR depth MIR owner")
        .join()
        .expect("checked-HIR depth MIR owner");
}

#[test]
fn deep_type_consumer_closure_matrix() {
    std::thread::Builder::new()
        .name("deep-type-mir-owner".to_string())
        .stack_size(2 * 1024 * 1024)
        .spawn(|| {
            const DEPTH: usize = 4_096;
            let mut program = baseline_program();
            program.structs = (0..DEPTH)
                .map(|index| StructDef {
                    name: format!("Deep{index}"),
                    source_name: format!("Deep{index}"),
                    fields: vec![FieldDef {
                        name: "next".to_string(),
                        ty: if index + 1 == DEPTH {
                            Ty::String
                        } else {
                            Ty::Struct((index + 1) as u32)
                        },
                    }],
                    align: None,
                    c_repr: false,
                })
                .collect();
            program.tagged_types = (0..DEPTH)
                .map(|index| {
                    TaggedType::Option(if index + 1 == DEPTH {
                        Scalar::Struct(0)
                    } else {
                        Scalar::Tagged((index + 1) as u32)
                    })
                })
                .collect();
            program.fn_types[0].ret = Ty::Tagged(0);
            program.fn_types[0].return_cleanup = hir::ReturnCleanupAbi::DynamicBit;
            assert_accepted("deep nominal/tagged type consumer", &program);

            program.structs[DEPTH - 1].fields.push(FieldDef {
                name: "later_missing".to_string(),
                ty: Ty::Struct(DEPTH as u32 + 7),
            });
            assert_rejected("deep later-sibling type error", &program);
        })
        .expect("spawn deep type MIR owner")
        .join()
        .expect("deep type MIR owner");
}

#[test]
fn malformed_hir_global_type_metadata_fails_closed() {
    for (label, ty) in [
        ("param", Ty::Param(0)),
        ("soa-param", Ty::SoaParam(0)),
        ("int-var", Ty::IntVar(0)),
        ("float-var", Ty::FloatVar(0)),
        ("str-finder", Ty::StrFinder),
        ("error", Ty::Error),
        ("scalar-param", Ty::Option(Scalar::Param(0))),
        ("scalar-soa-param", Ty::Option(Scalar::SoaParam(0))),
        ("missing-struct", Ty::Struct(99)),
        ("missing-enum", Ty::Enum(99)),
        ("missing-tuple", Ty::Tuple(99)),
        ("missing-tagged", Ty::Tagged(99)),
        ("missing-function-type", Ty::Fn(99)),
        ("missing-struct-array", Ty::StructArray(99, 1)),
        (
            "missing-dynamic-struct-array",
            Ty::DynStructArray(99, Layout::Aos),
        ),
        ("missing-soa", Ty::Soa(99)),
        ("missing-scanner", Ty::JsonScanner(99)),
        ("missing-dictionary", Ty::DictEncoded(99, 0)),
        ("missing-dictionary-field", Ty::DictEncoded(0, 99)),
        ("non-string-dictionary-field", Ty::DictEncoded(0, 1)),
    ] {
        assert_rejected(label, &with_return(ty));
    }

    for bits in [0, 7, 24, 128] {
        assert_rejected("integer-width", &with_return(int(bits)));
        assert_rejected(
            "scalar-integer-width",
            &with_return(Ty::Option(scalar_int(bits))),
        );
        assert_rejected(
            "primitive-integer-width",
            &with_return(Ty::DynSliceArray(PrimScalar::Int(IntTy {
                bits,
                signed: false,
            }))),
        );
    }
    for bits in [0, 16, 128] {
        let float = FloatTy { bits };
        assert_rejected("float-width", &with_return(Ty::Float(float)));
        assert_rejected(
            "scalar-float-width",
            &with_return(Ty::Option(Scalar::Float(float))),
        );
        assert_rejected(
            "primitive-float-width",
            &with_return(Ty::DynSliceArray(PrimScalar::Float(float))),
        );
    }
    for lanes in [0, 1, 3, 32] {
        assert_rejected("vector-lanes", &with_return(Ty::Vec(scalar_int(32), lanes)));
        assert_rejected("mask-lanes", &with_return(Ty::Mask(scalar_int(32), lanes)));
    }
    assert_rejected("vector-element", &with_return(Ty::Vec(Scalar::Bool, 4)));
    assert_rejected("mask-element", &with_return(Ty::Mask(Scalar::Str, 4)));

    let mut inline_cycle = baseline_program();
    inline_cycle.structs[0].fields[0].ty = Ty::Struct(0);
    assert_rejected("inline-cycle", &inline_cycle);

    let mut concrete_unused_tagged = baseline_program();
    concrete_unused_tagged.tagged_types[0] = TaggedType::Option(Scalar::Struct(99));
    assert_rejected("unused-concrete-tagged", &concrete_unused_tagged);

    let mut concrete_unused_fn = baseline_program();
    concrete_unused_fn.fn_types[0] = fn_type(Ty::Struct(99));
    assert_rejected("unused-concrete-function-type", &concrete_unused_fn);

    let mut broken_template_tagged = baseline_program();
    broken_template_tagged.tagged_types[0] =
        TaggedType::Result(Scalar::Param(0), Scalar::Struct(99));
    assert_rejected("template-tagged-bad-reference", &broken_template_tagged);

    let mut reachable_template_tagged = baseline_program();
    reachable_template_tagged.tagged_types[0] = TaggedType::Option(Scalar::Param(0));
    reachable_template_tagged.imported_fns.push(ImportedFn {
        ret: Ty::Tagged(0),
        ..with_return(Ty::Unit).imported_fns.remove(0)
    });
    assert_rejected("reachable-template-tagged", &reachable_template_tagged);

    let mut reachable_template_fn = baseline_program();
    reachable_template_fn.fn_types[0] = fn_type(Ty::Param(0));
    reachable_template_fn.imported_fns.push(ImportedFn {
        ret: Ty::Fn(0),
        ..with_return(Ty::Unit).imported_fns.remove(0)
    });
    assert_rejected("reachable-template-function-type", &reachable_template_fn);

    for invalid_index in 0..3 {
        let mut program = baseline_program();
        program.structs = (0..3)
            .map(|index| StructDef {
                name: format!("Root{index}"),
                source_name: format!("Root{index}"),
                fields: vec![FieldDef {
                    name: "value".to_string(),
                    ty: int(64),
                }],
                align: None,
                c_repr: false,
            })
            .collect();
        program.structs[invalid_index].fields[0].ty = int(7);
        assert_rejected("first-middle-final-concrete-root", &program);
    }
}

#[test]
fn malformed_hir_type_placement_fails_closed() {
    let mut field_box = baseline_program();
    field_box.structs[0].fields[0].ty = Ty::Box(scalar_int(64));
    assert_placement_rejected("box in struct field", &field_box);

    let mut field_owned_array = baseline_program();
    field_owned_array.structs[0].fields[0].ty = Ty::DynArray(Scalar::String);
    assert_placement_rejected("deep-free array in struct field", &field_owned_array);

    assert_placement_rejected(
        "nested owned array element",
        &with_return(Ty::DynArray(Scalar::DynArray(PrimScalar::Int(IntTy {
            bits: 32,
            signed: true,
        })))),
    );
    for (label, ty) in [
        ("file slice element", Ty::Slice(Scalar::File)),
        ("file array element", Ty::DynArray(Scalar::File)),
    ] {
        assert_placement_rejected(label, &with_return(ty));
    }

    let mut field_soa_array = baseline_program();
    field_soa_array.structs[0].fields[0].ty = Ty::DynStructArray(0, Layout::Soa);
    assert_placement_rejected("future SoA dynamic struct field", &field_soa_array);

    let mut enum_aligned_field = baseline_program();
    enum_aligned_field.structs.push(StructDef {
        name: "AlignedPayload".to_string(),
        source_name: "AlignedPayload".to_string(),
        fields: vec![FieldDef {
            name: "value".to_string(),
            ty: int(64),
        }],
        align: Some(32),
        c_repr: false,
    });
    enum_aligned_field.enums[0].variants[1].payload = vec![Scalar::Struct(1)];
    enum_aligned_field.structs[0].fields[0].ty = Ty::Enum(0);
    assert_placement_rejected(
        "over-aligned struct nested in an enum field",
        &enum_aligned_field,
    );

    let mut enum_aligned_payload = baseline_program();
    enum_aligned_payload.structs.push(StructDef {
        name: "AlignedPayload".to_string(),
        source_name: "AlignedPayload".to_string(),
        fields: vec![FieldDef {
            name: "value".to_string(),
            ty: int(64),
        }],
        align: Some(32),
        c_repr: false,
    });
    enum_aligned_payload.enums[0].variants[1].payload = vec![Scalar::Struct(1)];
    assert_placement_rejected(
        "over-aligned struct in an enum payload",
        &enum_aligned_payload,
    );

    let mut c_field = baseline_program();
    c_field.structs[0].c_repr = true;
    c_field.structs[0].fields[0].ty = Ty::Bool;
    assert_placement_rejected("non-FFI field in layout(C) struct", &c_field);

    let mut aligned_field = baseline_program();
    aligned_field.structs.push(StructDef {
        name: "Aligned".to_string(),
        source_name: "Aligned".to_string(),
        fields: vec![FieldDef {
            name: "value".to_string(),
            ty: int(64),
        }],
        align: Some(32),
        c_repr: false,
    });
    aligned_field.structs[0].fields[0].ty = Ty::Struct(1);
    assert_placement_rejected("over-aligned inline field", &aligned_field);

    let mut enum_buffer = baseline_program();
    enum_buffer.enums[0].variants[1].payload = vec![Scalar::Buffer];
    assert_placement_rejected("buffer enum payload", &enum_buffer);

    let mut enum_soa = baseline_program();
    enum_soa.enums[0].variants[1].payload = vec![Scalar::Soa(0)];
    assert_placement_rejected("SoA enum payload", &enum_soa);

    let mut tuple_fn = baseline_program();
    tuple_fn.tuples[0].elems = vec![Scalar::Fn(0)];
    assert_placement_rejected("function tuple element", &tuple_fn);

    let mut tuple_slice = baseline_program();
    tuple_slice.tuples[0].elems = vec![Scalar::Slice(PrimScalar::Str)];
    assert_placement_rejected("slice tuple element", &tuple_slice);

    let mut tagged_fn = baseline_program();
    tagged_fn.tagged_types[0] = TaggedType::Option(Scalar::Fn(0));
    assert_placement_rejected("function Option payload", &tagged_fn);

    let mut fn_param = baseline_program();
    fn_param.fn_types[0].params = vec![(align_ast::ParamMode::ByValue, Scalar::Fn(0))];
    assert_placement_rejected("function-valued FnTy parameter", &fn_param);

    let mut imported_box = with_return(Ty::Box(scalar_int(64)));
    assert_placement_rejected("box imported parameter return", &imported_box);
    imported_box.imported_fns[0].ret = Ty::Fn(0);
    assert_placement_rejected("function imported return", &imported_box);

    let mut extern_bool = baseline_program();
    extern_bool.externs.push(hir::ExternFn {
        name: "c_bool".to_string(),
        params: vec![Ty::Bool],
        param_modes: vec![align_ast::ParamMode::ByValue],
        ret: Ty::Unit,
        return_borrow: ReturnBorrowSummary::None,
        return_region: ReturnRegionSummary::None,
        return_cleanup: hir::ReturnCleanupAbi::None,
    });
    assert_placement_rejected("bool extern parameter", &extern_bool);

    let mut extern_view_return = baseline_program();
    extern_view_return.externs.push(hir::ExternFn {
        name: "c_str".to_string(),
        params: vec![],
        param_modes: vec![],
        ret: Ty::Str,
        return_borrow: ReturnBorrowSummary::None,
        return_region: ReturnRegionSummary::None,
        return_cleanup: hir::ReturnCleanupAbi::None,
    });
    assert_placement_rejected("view extern return", &extern_view_return);
}

#[test]
fn region_only_array_builder_headers_are_placement_valid() {
    for (label, element) in [
        ("str", ArrayBuilderElem::Scalar(Scalar::Str)),
        (
            "bytes",
            ArrayBuilderElem::Scalar(Scalar::Slice(PrimScalar::Int(IntTy {
                bits: 8,
                signed: false,
            }))),
        ),
        ("struct", ArrayBuilderElem::Scalar(Scalar::Struct(0))),
        ("sum", ArrayBuilderElem::Scalar(Scalar::Enum(0))),
        ("option", ArrayBuilderElem::Scalar(Scalar::Tagged(0))),
        (
            "vector",
            ArrayBuilderElem::Aggregate(AggregateArrayElem::Vec(scalar_int(32), 4)),
        ),
        (
            "mask",
            ArrayBuilderElem::Aggregate(AggregateArrayElem::Mask(scalar_int(32), 8)),
        ),
        (
            "fixed_array",
            ArrayBuilderElem::Aggregate(AggregateArrayElem::FixedArray(Scalar::Str, 3)),
        ),
        (
            "fixed_struct_array",
            ArrayBuilderElem::Aggregate(AggregateArrayElem::FixedStructArray(0, 2)),
        ),
    ] {
        let mut program = baseline_program();
        program.imported_fns.push(ImportedFn {
            name: format!("dep$push_{label}"),
            params: vec![Ty::array_builder(element)],
            param_modes: vec![align_ast::ParamMode::BorrowMut],
            ret: Ty::Unit,
            return_provenance_known: true,
            return_borrow: ReturnBorrowSummary::None,
            return_region: ReturnRegionSummary::None,
            return_cleanup: hir::ReturnCleanupAbi::None,
            effect: FnEffect::Pure,
        });
        assert!(
            validate_hir::type_placement_metadata_is_valid(&program),
            "region-only array_builder<{label}> header was rejected"
        );
    }

    for (label, element) in [
        (
            "non_numeric_vector_lane",
            AggregateArrayElem::Vec(Scalar::Bool, 4),
        ),
        (
            "unsupported_mask_width",
            AggregateArrayElem::Mask(scalar_int(32), 3),
        ),
        (
            "empty_fixed_array",
            AggregateArrayElem::FixedArray(Scalar::Bool, 0),
        ),
        (
            "owned_fixed_array",
            AggregateArrayElem::FixedArray(Scalar::String, 2),
        ),
    ] {
        let mut program = baseline_program();
        program.imported_fns.push(ImportedFn {
            name: format!("dep$invalid_{label}"),
            params: vec![Ty::array_builder(ArrayBuilderElem::Aggregate(element))],
            param_modes: vec![align_ast::ParamMode::BorrowMut],
            ret: Ty::Unit,
            return_provenance_known: true,
            return_borrow: ReturnBorrowSummary::None,
            return_region: ReturnRegionSummary::None,
            return_cleanup: hir::ReturnCleanupAbi::None,
            effect: FnEffect::Pure,
        });
        assert_placement_rejected(label, &program);
    }

    let mut unknown_struct = baseline_program();
    unknown_struct.imported_fns.push(ImportedFn {
        name: "dep$invalid_unknown_fixed_struct_array".to_string(),
        params: vec![Ty::array_builder(ArrayBuilderElem::Aggregate(
            AggregateArrayElem::FixedStructArray(99, 2),
        ))],
        param_modes: vec![align_ast::ParamMode::BorrowMut],
        ret: Ty::Unit,
        return_provenance_known: true,
        return_borrow: ReturnBorrowSummary::None,
        return_region: ReturnRegionSummary::None,
        return_cleanup: hir::ReturnCleanupAbi::None,
        effect: FnEffect::Pure,
    });
    assert_rejected("unknown_fixed_struct_array", &unknown_struct);
}

#[test]
fn body_only_header_types_fail_placement_closed() {
    for (label, ty) in [
        ("cli parsed", Ty::CliParsed),
        ("http request", Ty::HttpRequest),
        ("http response", Ty::HttpResponse),
        ("http client", Ty::HttpClient),
        ("http server", Ty::HttpServer),
        ("command", Ty::Command),
        ("run output", Ty::RunOutput),
    ] {
        let mut program = baseline_program();
        program.fn_types[0].ret = ty;
        program
            .imported_fns
            .push(imported_fn("dep$body_only_header", Vec::new(), ty));
        assert_placement_rejected(label, &program);
    }
}

#[test]
fn abstract_box_param_fails_placement_closed() {
    let mut program = baseline_program();
    program.fn_types[0].ret = Ty::Box(Scalar::Param(0));
    assert_placement_rejected("abstract box parameter", &program);
}

#[test]
fn malformed_hir_nominal_link_metadata_fails_closed() {
    let mut empty_internal_name = baseline_program();
    empty_internal_name.structs[0].name.clear();
    assert_nominal_rejected("empty nominal name", &empty_internal_name);

    let mut empty_source_name = baseline_program();
    empty_source_name.structs[0].source_name.clear();
    assert_nominal_rejected("empty source name", &empty_source_name);

    let mut nul_internal_name = baseline_program();
    nul_internal_name.structs[0].name = "Record\0private".to_string();
    assert_nominal_rejected("NUL nominal name", &nul_internal_name);

    let mut nul_source_name = baseline_program();
    nul_source_name.structs[0].source_name = "Record\0source".to_string();
    assert_nominal_rejected("NUL source name", &nul_source_name);

    let mut duplicate_internal_name = baseline_program();
    duplicate_internal_name.enums[0].name = "Record".to_string();
    assert_nominal_rejected("duplicate internal nominal name", &duplicate_internal_name);

    let mut invalid_field_name = baseline_program();
    invalid_field_name.structs[0].fields[0].name = "bad-name".to_string();
    assert_nominal_rejected("invalid field name", &invalid_field_name);

    let mut duplicate_field_name = baseline_program();
    duplicate_field_name.structs[0].fields[1].name = "key".to_string();
    assert_nominal_rejected("duplicate field name", &duplicate_field_name);

    let mut invalid_variant_name = baseline_program();
    invalid_variant_name.enums[0].variants[1].name = "bad-variant".to_string();
    assert_nominal_rejected("invalid variant name", &invalid_variant_name);

    let mut duplicate_variant_name = baseline_program();
    duplicate_variant_name.enums[0].variants[1].name = "Empty".to_string();
    assert_nominal_rejected("duplicate variant name", &duplicate_variant_name);

    for align in [Some(0), Some(3), Some(1u32 << 30)] {
        let mut invalid_alignment = baseline_program();
        invalid_alignment.structs[0].align = align;
        assert_nominal_rejected("invalid nominal alignment", &invalid_alignment);
    }

    let mut invalid_enum_base = baseline_program();
    invalid_enum_base.enums[0].variants[1].field_base = 2;
    assert_nominal_rejected("invalid enum field base", &invalid_enum_base);

    let mut duplicate_tuple = baseline_program();
    duplicate_tuple.tuples.push(duplicate_tuple.tuples[0].clone());
    assert_nominal_rejected("duplicate tuple element vector", &duplicate_tuple);

    for link_libs in [
        vec![String::new()],
        vec!["-z".to_string()],
        vec!["lib?name".to_string()],
        vec!["z".to_string(), "z".to_string()],
    ] {
        let mut invalid_link = baseline_program();
        invalid_link.link_libs = link_libs;
        assert_nominal_rejected("invalid link library", &invalid_link);
    }

    let mut incompatible_source_shape = baseline_program();
    incompatible_source_shape.structs.push(StructDef {
        name: "Record$other".to_string(),
        source_name: "Record".to_string(),
        fields: vec![FieldDef {
            name: "key".to_string(),
            ty: Ty::String,
        }],
        align: None,
        c_repr: false,
    });
    assert_nominal_rejected("incompatible repeated source shape", &incompatible_source_shape);

    let mut incompatible_enum_shape = baseline_program();
    incompatible_enum_shape.enums.push(EnumDef {
        name: "Choice$other".to_string(),
        source_name: "Choice".to_string(),
        variants: vec![EnumVariant {
            name: "Different".to_string(),
            payload: Vec::new(),
            field_base: 1,
        }],
    });
    assert_nominal_rejected("incompatible repeated enum shape", &incompatible_enum_shape);

    let mut incompatible_callable_shape = baseline_program();
    incompatible_callable_shape.structs[0].fields = vec![FieldDef {
        name: "handler".to_string(),
        ty: Ty::Fn(0),
    }];
    incompatible_callable_shape.fn_types[0].params = vec![
        (align_ast::ParamMode::ByValue, scalar_int(64)),
    ];
    incompatible_callable_shape.fn_types.push(fn_type(Ty::Int(IntTy {
        bits: 32,
        signed: true,
    })));
    incompatible_callable_shape.fn_types[1].params = vec![
        (align_ast::ParamMode::Out, scalar_int(64)),
    ];
    incompatible_callable_shape.fn_types[1].return_borrow = ReturnBorrowSummary::Roots {
        params: vec![0],
        captures: Vec::new(),
    };
    incompatible_callable_shape.structs.push(StructDef {
        name: "Record$callable".to_string(),
        source_name: "Record".to_string(),
        fields: vec![FieldDef {
            name: "handler".to_string(),
            ty: Ty::Fn(1),
        }],
        align: None,
        c_repr: false,
    });
    assert_nominal_rejected("incompatible callable ABI shape", &incompatible_callable_shape);

    let mut cross_kind_source_name = baseline_program();
    cross_kind_source_name.enums[0].source_name = "Record".to_string();
    assert_nominal_rejected("cross-kind source name collision", &cross_kind_source_name);
}

#[test]
fn valid_hir_nominal_link_preflight_is_mir_identity() {
    let mut program = baseline_program();
    program.link_libs = vec!["z".to_string(), "foo-bar_2.1+".to_string()];
    assert!(validate_hir::nominal_link_metadata_is_valid(&program));
    assert_accepted("valid nominal/link metadata", &program);

    let mut equal_source_shape = baseline_program();
    equal_source_shape.structs.push(StructDef {
        name: "Record$origin1".to_string(),
        source_name: "Record".to_string(),
        fields: equal_source_shape.structs[0].fields.clone(),
        align: equal_source_shape.structs[0].align,
        c_repr: equal_source_shape.structs[0].c_repr,
    });
    assert!(validate_hir::nominal_link_metadata_is_valid(&equal_source_shape));
    assert_accepted("equal source shape with private identity", &equal_source_shape);

    let mut effect_origin = baseline_program();
    effect_origin.structs[0].fields = vec![FieldDef {
        name: "handler".to_string(),
        ty: Ty::Fn(0),
    }];
    effect_origin.fn_types.push(fn_type(Ty::Unit));
    effect_origin.fn_types[1].effect.set(FnEffect::Impure);
    effect_origin.structs.push(StructDef {
        name: "Record$impure".to_string(),
        source_name: "Record".to_string(),
        fields: vec![FieldDef {
            name: "handler".to_string(),
            ty: Ty::Fn(1),
        }],
        align: None,
        c_repr: false,
    });
    effect_origin.fns.push(body_test_named_function(
        "effect_target",
        hir::Block {
            stmts: Vec::new(),
            value: Some(Box::new(hir::Expr {
                kind: hir::ExprKind::Call {
                    func: "print".to_string(),
                    args: vec![hir::Expr {
                        kind: hir::ExprKind::Int(0),
                        ty: int(64),
                        span: align_span::Span::new(0, 0, 0),
                    }],
                    type_args: Vec::new(),
                },
                ty: Ty::Unit,
                span: align_span::Span::new(0, 0, 0),
            })),
        },
        Vec::new(),
        Ty::Unit,
    ));
    effect_origin.fns.push(body_test_named_function(
        "effect_value",
        hir::Block {
            stmts: vec![hir::Stmt::Let {
                local: 0,
                init: hir::Expr {
                    kind: hir::ExprKind::FnValue("effect_target".to_string()),
                    ty: Ty::Fn(1),
                    span: align_span::Span::new(0, 0, 0),
                },
            }],
            value: Some(Box::new(hir::Expr {
                kind: hir::ExprKind::Unit,
                ty: Ty::Unit,
                span: align_span::Span::new(0, 0, 0),
            })),
        },
        vec![hir::Local {
            id: 0,
            name: "handler".to_string(),
            ty: Ty::Fn(1),
            is_mut: false,
            is_param: false,
            align: None,
        }],
        Ty::Unit,
    ));
    assert!(validate_hir::nominal_link_metadata_is_valid(&effect_origin));
    assert_accepted("function effect origin excluded from source shape", &effect_origin);
}

#[test]
fn nominal_source_shape_preserves_shared_node_correspondence() {
    let mut program = baseline_program();
    program.structs.extend([
        StructDef {
            name: "C0".to_string(),
            source_name: "C".to_string(),
            fields: vec![FieldDef {
                name: "value".to_string(),
                ty: Ty::String,
            }],
            align: None,
            c_repr: false,
        },
        StructDef {
            name: "C1".to_string(),
            source_name: "C".to_string(),
            fields: vec![FieldDef {
                name: "value".to_string(),
                ty: Ty::String,
            }],
            align: None,
            c_repr: false,
        },
        StructDef {
            name: "C2".to_string(),
            source_name: "C".to_string(),
            fields: vec![FieldDef {
                name: "value".to_string(),
                ty: Ty::String,
            }],
            align: None,
            c_repr: false,
        },
        StructDef {
            name: "A0".to_string(),
            source_name: "A".to_string(),
            fields: vec![FieldDef {
                name: "child".to_string(),
                ty: Ty::Struct(1),
            }],
            align: None,
            c_repr: false,
        },
        StructDef {
            name: "A1".to_string(),
            source_name: "A".to_string(),
            fields: vec![FieldDef {
                name: "child".to_string(),
                ty: Ty::Struct(3),
            }],
            align: None,
            c_repr: false,
        },
        StructDef {
            name: "R0".to_string(),
            source_name: "R".to_string(),
            fields: vec![
                FieldDef {
                    name: "direct".to_string(),
                    ty: Ty::Struct(1),
                },
                FieldDef {
                    name: "nested".to_string(),
                    ty: Ty::Struct(4),
                },
            ],
            align: None,
            c_repr: false,
        },
        StructDef {
            name: "R1".to_string(),
            source_name: "R".to_string(),
            fields: vec![
                FieldDef {
                    name: "direct".to_string(),
                    ty: Ty::Struct(2),
                },
                FieldDef {
                    name: "nested".to_string(),
                    ty: Ty::Struct(5),
                },
            ],
            align: None,
            c_repr: false,
        },
    ]);
    assert_nominal_rejected("shared source-shape correspondence", &program);
}

#[test]
fn deep_hir_source_shape_is_stack_bounded() {
    std::thread::Builder::new()
        .name("deep-source-shape".to_string())
        .stack_size(2 * 1024 * 1024)
        .spawn(|| {
            const DEPTH: usize = 4_096;
            let mut program = baseline_program();
            program.structs = (0..(DEPTH * 2))
                .map(|id| {
                    let branch = id / DEPTH;
                    let index = id % DEPTH;
                    StructDef {
                        name: format!("Deep{branch}_{index}"),
                        source_name: format!("Deep{index}"),
                        fields: vec![FieldDef {
                            name: "next".to_string(),
                            ty: if index + 1 == DEPTH {
                                Ty::String
                            } else {
                                Ty::Struct((id + 1) as u32)
                            },
                        }],
                        align: None,
                        c_repr: false,
                    }
                })
                .collect();
            assert!(validate_hir::nominal_link_metadata_is_valid(&program));
            assert_accepted("deep equal source shape", &program);

            program.structs[DEPTH * 2 - 1].fields[0].ty = Ty::Bool;
            assert_nominal_rejected("deep later source-shape mismatch", &program);
        })
        .expect("spawn deep source-shape validator")
        .join()
        .expect("deep source-shape validator");
}

#[test]
fn valid_hir_type_placement_preflight_is_mir_identity() {
    let mut program = baseline_program();
    program.structs[0].fields = vec![
        FieldDef {
            name: "handler".to_string(),
            ty: Ty::Fn(0),
        },
        FieldDef {
            name: "view".to_string(),
            ty: Ty::Slice(Scalar::Int(IntTy {
                bits: 32,
                signed: true,
            })),
        },
        FieldDef {
            name: "owned".to_string(),
            ty: Ty::File,
        },
        FieldDef {
            name: "nested_owned_array".to_string(),
            ty: Ty::Option(Scalar::DynArray(PrimScalar::String)),
        },
    ];
    program.enums[0].variants[1].payload = vec![Scalar::Fn(0), Scalar::ResponseBuilder];
    program.tagged_types[0] = TaggedType::Result(Scalar::File, Scalar::String);
    program.tuples[0].elems = vec![Scalar::String, Scalar::DynArray(PrimScalar::Str)];
    program.fn_types[0].params = vec![
        (align_ast::ParamMode::ByValue, Scalar::Buffer),
        (
            align_ast::ParamMode::ByValue,
            Scalar::Slice(PrimScalar::Int(IntTy {
                bits: 8,
                signed: false,
            })),
        ),
    ];
    program.fn_types[0].ret = Ty::Result(Scalar::String, Scalar::Enum(0));
    program.fn_types[0].return_cleanup = hir::ReturnCleanupAbi::DynamicBit;
    program
        .imported_fns
        .push(imported_fn("dep$placement", vec![Ty::File], Ty::Unit));
    program.imported_fns[0].ret = Ty::Result(Scalar::File, Scalar::Enum(0));
    program.imported_fns[0].return_cleanup = hir::ReturnCleanupAbi::DynamicBit;
    assert_accepted("body-independent placement matrix", &program);

    let mut externs = program.clone();
    let c_struct = externs.structs.len() as u32;
    externs.structs.push(StructDef {
        name: "CRecord".to_string(),
        source_name: "CRecord".to_string(),
        fields: vec![FieldDef {
            name: "value".to_string(),
            ty: int(32),
        }],
        align: None,
        c_repr: true,
    });
    externs.externs.push(hir::ExternFn {
        name: "c_record".to_string(),
        params: vec![
            Ty::Struct(c_struct),
            Ty::Str,
            Ty::Slice(Scalar::Int(IntTy {
                bits: 8,
                signed: false,
            })),
        ],
        param_modes: vec![align_ast::ParamMode::ByValue; 3],
        ret: Ty::Struct(c_struct),
        return_borrow: ReturnBorrowSummary::None,
        return_region: ReturnRegionSummary::None,
        return_cleanup: hir::ReturnCleanupAbi::None,
    });
    assert_accepted("extern placement matrix", &externs);
}

#[test]
fn deep_hir_type_dag_placement_is_stack_bounded() {
    std::thread::Builder::new()
        .name("deep-type-placement".to_string())
        .stack_size(2 * 1024 * 1024)
        .spawn(|| {
            const DEPTH: usize = 4_096;
            let mut program = baseline_program();
            program.structs = (0..DEPTH)
                .map(|index| StructDef {
                    name: format!("Placement{index}"),
                    source_name: format!("Placement{index}"),
                    fields: vec![FieldDef {
                        name: "next".to_string(),
                        ty: if index + 1 == DEPTH {
                            Ty::String
                        } else {
                            Ty::Struct((index + 1) as u32)
                        },
                    }],
                    align: None,
                    c_repr: false,
                })
                .collect();
            assert!(validate_hir::type_placement_metadata_is_valid(&program));
            assert_accepted("deep valid placement", &program);

            program.structs[DEPTH - 1].fields[0].ty = Ty::Box(scalar_int(64));
            assert_placement_rejected("deep later placement failure", &program);

            // A shared binary DAG makes an un-memoized walker revisit 2^n paths; 256 is already
            // far beyond what an exponential implementation can finish, while keeping the
            // four-entrypoint owner test quick once completed tagged nodes are memoized.
            const TAG_DEPTH: usize = 256;
            let mut tagged_dag = baseline_program();
            tagged_dag.tagged_types = (0..TAG_DEPTH)
                .map(|id| {
                    if id == 0 {
                        TaggedType::Option(scalar_int(64))
                    } else {
                        TaggedType::Result(
                            Scalar::Tagged((id - 1) as u32),
                            Scalar::Tagged((id - 1) as u32),
                        )
                    }
                })
                .collect();
            // Reach the same shared DAG through an inline struct field so the alignment walker
            // cannot silently remain exponential while the payload walker is memoized.
            tagged_dag.structs[0].fields[0].ty = Ty::Tagged((TAG_DEPTH - 1) as u32);
            tagged_dag.imported_fns.push(imported_fn(
                "dep$tagged_dag",
                Vec::new(),
                Ty::Tagged((TAG_DEPTH - 1) as u32),
            ));
            assert!(
                validate_hir::global_type_metadata_is_valid(&tagged_dag),
                "shared tagged placement DAG: global graph validator rejected valid metadata"
            );
            assert!(
                validate_hir::type_placement_metadata_is_valid(&tagged_dag),
                "shared tagged placement DAG: placement validator rejected valid metadata"
            );
        })
        .expect("spawn deep placement validator")
        .join()
        .expect("deep placement validator");
}

fn body_test_expr(kind: hir::ExprKind, ty: Ty) -> hir::Expr {
    hir::Expr {
        kind,
        ty,
        span: align_span::Span::new(0, 0, 0),
    }
}

fn body_test_return_cleanup(ret: Ty) -> hir::ReturnCleanupAbi {
    let dynamic = match ret {
        Ty::String
        | Ty::DynArray(_)
        | Ty::DynStructArray(..)
        | Ty::DynSliceArray(_)
        | Ty::DynResponseArray => true,
        Ty::Option(value) => value.is_move(),
        Ty::Result(ok, err) => ok.is_move() || err.is_move(),
        other => align_sema::is_move_handle(other),
    };
    if dynamic {
        hir::ReturnCleanupAbi::DynamicBit
    } else {
        hir::ReturnCleanupAbi::None
    }
}

fn body_test_named_function(
    name: &str,
    body: hir::Block,
    locals: Vec<hir::Local>,
    ret: Ty,
) -> hir::Fn {
    hir::Fn {
        name: name.to_string(),
        origin: hir::FnOrigin::Source {
            is_entry: false,
            is_public: false,
        },
        params: Vec::new(),
        param_modes: Vec::new(),
        ret,
        return_borrow: ReturnBorrowSummary::None,
        return_region: ReturnRegionSummary::None,
        return_cleanup: body_test_return_cleanup(ret),
        locals,
        body,
        span: align_span::Span::new(0, 0, 0),
        drop_locals: Vec::new(),
        drop_individual_locals: Vec::new(),
        drop_individual_exprs: std::collections::HashMap::new(),
    }
}

fn body_test_function(body: hir::Block, locals: Vec<hir::Local>, ret: Ty) -> hir::Fn {
    body_test_named_function("body_test", body, locals, ret)
}

fn body_test_parameter_function(name: &str, ty: Ty, body: hir::Block, ret: Ty) -> hir::Fn {
    let mut function = body_test_named_function(
        name,
        body,
        vec![hir::Local {
            id: 0,
            name: "arg".to_string(),
            ty,
            is_mut: false,
            is_param: true,
            align: None,
        }],
        ret,
    );
    function.params = vec![0];
    function.param_modes = vec![align_ast::ParamMode::ByValue];
    function
}

fn body_test_function_with_params(
    name: &str,
    locals: Vec<hir::Local>,
    params: Vec<hir::LocalId>,
    body: hir::Block,
    ret: Ty,
) -> hir::Fn {
    let mut function = body_test_named_function(name, body, locals, ret);
    function.param_modes = vec![align_ast::ParamMode::ByValue; params.len()];
    function.params = params;
    function
}

fn body_test_local(
    id: u32,
    name: &str,
    ty: Ty,
    is_mut: bool,
    is_param: bool,
) -> hir::Local {
    hir::Local {
        id,
        name: name.to_string(),
        ty,
        is_mut,
        is_param,
        align: None,
    }
}

fn body_unit_case(name: &str, expression: hir::Expr) -> hir::Fn {
    body_test_named_function(
        name,
        hir::Block {
            stmts: vec![hir::Stmt::Expr(expression)],
            value: Some(Box::new(body_test_expr(hir::ExprKind::Unit, Ty::Unit))),
        },
        Vec::new(),
        Ty::Unit,
    )
}

fn body_tail_case(name: &str, expression: hir::Expr, ret: Ty) -> hir::Fn {
    body_test_named_function(
        name,
        hir::Block {
            stmts: Vec::new(),
            value: Some(Box::new(expression)),
        },
        Vec::new(),
        ret,
    )
}

fn body_native_case(name: &str, expression: hir::Expr, ret: Ty) -> hir::Fn {
    if let Ty::Result(Scalar::Buffer, error) = &ret {
        let function_ret = Ty::Result(Scalar::Unit, *error);
        body_test_named_function(
            name,
            hir::Block {
                stmts: vec![hir::Stmt::Expr(body_test_expr(
                    hir::ExprKind::Try(Box::new(expression)),
                    Ty::Buffer,
                ))],
                value: Some(Box::new(body_test_expr(
                    hir::ExprKind::ResultOk(Box::new(body_test_expr(
                        hir::ExprKind::Unit,
                        Ty::Unit,
                    ))),
                    function_ret,
                ))),
            },
            Vec::new(),
            function_ret,
        )
    } else if matches!(&ret, Ty::Result(..)) {
        body_tail_case(name, expression, ret)
    } else {
        body_unit_case(name, expression)
    }
}

fn body_statement_expression_mut<'a>(
    program: &'a mut hir::Program,
    name: &str,
) -> &'a mut hir::Expr {
    let statement = body_first_statement_mut(program, name);
    let hir::Stmt::Expr(expression) = statement else {
        panic!("statement fixture {name} is not an expression")
    };
    expression
}

fn body_value_expression_mut<'a>(
    program: &'a mut hir::Program,
    name: &str,
) -> &'a mut hir::Expr {
    program
        .fns
        .iter_mut()
        .find(|function| function.name.as_str() == name)
        .unwrap_or_else(|| panic!("missing value fixture {name}"))
        .body
        .value
        .as_deref_mut()
        .unwrap_or_else(|| panic!("value fixture {name} has no value"))
}

fn body_first_statement_mut<'a>(program: &'a mut hir::Program, name: &str) -> &'a mut hir::Stmt {
    program
        .fns
        .iter_mut()
        .find(|function| function.name.as_str() == name)
        .unwrap_or_else(|| panic!("missing statement fixture {name}"))
        .body
        .stmts
        .first_mut()
        .unwrap_or_else(|| panic!("statement fixture {name} has no statement"))
}

fn body_loop_statement_mut<'a>(program: &'a mut hir::Program, name: &str) -> &'a mut hir::Stmt {
    let expression = program
        .fns
        .iter_mut()
        .find(|function| function.name.as_str() == name)
        .unwrap_or_else(|| panic!("missing loop statement fixture {name}"))
        .body
        .value
        .as_mut()
        .unwrap_or_else(|| panic!("loop fixture {name} has no tail"));
    let hir::ExprKind::Loop { body, .. } = &mut expression.kind else {
        panic!("loop fixture {name} lost its loop")
    };
    body.stmts
        .first_mut()
        .unwrap_or_else(|| panic!("loop fixture {name} has no statement"))
}

#[test]
fn hir_body_validator_core() {
    let integer = int(64);
    let mut program = baseline_program();
    program.fns.push(body_test_function(
        hir::Block {
            stmts: vec![hir::Stmt::Let {
                local: 0,
                init: body_test_expr(hir::ExprKind::Int(3), integer),
            }],
            value: Some(Box::new(body_test_expr(hir::ExprKind::Local(0), integer))),
        },
        vec![hir::Local {
            id: 0,
            name: "x".to_string(),
            ty: integer,
            is_mut: false,
            is_param: false,
            align: None,
        }],
        integer,
    ));
    assert!(body_core_metadata_is_valid(&program));

    let hir::Stmt::Let { init, .. } = &mut program.fns[0].body.stmts[0] else {
        panic!("test fixture lost its let");
    };
    init.ty = Ty::Bool;
    assert!(!body_core_metadata_is_valid(&program));
}

#[test]
fn hir_body_validator_accepts_builtin_display_and_hash_calls() {
    let program = checked_source_program(
        "fn main() -> i32 {\n  print(1)\n  print(\"x\")\n  print(hash64(\"x\"))\n  pair := hash128(\"x\")\n  return 0\n}\n",
    );
    assert!(body_core_metadata_is_valid(&program));
}

#[test]
fn hir_body_validator_rejects_unborrowed_builtin_string() {
    let mut program = checked_source_program(
        "fn main() -> i32 {\n  s := \"x\".clone()\n  print(s)\n  return 0\n}\n",
    );
    let statement = program.fns[0]
        .body
        .stmts
        .iter_mut()
        .find_map(|statement| match statement {
            hir::Stmt::Expr(expression) => Some(expression),
            _ => None,
        })
        .expect("builtin display fixture has an expression statement");
    let hir::ExprKind::Call { args, .. } = &mut statement.kind else {
        panic!("builtin display fixture lost its call")
    };
    let argument = args.first_mut().expect("builtin display call has an argument");
    argument.kind = hir::ExprKind::Local(0);
    argument.ty = Ty::String;
    assert!(!body_core_metadata_is_valid(&program));
}

#[test]
fn hir_body_validator_accepts_function_value_local_specialization() {
    let program = checked_source_program(
        "fn noop() {}\nfn main() -> i32 {\n  f := noop\n  f()\n  return 0\n}\n",
    );
    assert!(body_core_metadata_is_valid(&program));
}

#[test]
fn hir_body_validator_accepts_structural_function_value_match_join() {
    let fn_one = 1u32;
    let fn_two = 2u32;
    let mut program = baseline_program();
    program.fn_types.extend([fn_type(Ty::Unit), fn_type(Ty::Unit)]);
    program.fns.push(body_test_named_function(
        "noop",
        hir::Block {
            stmts: Vec::new(),
            value: None,
        },
        Vec::new(),
        Ty::Unit,
    ));
    let match_expr = body_test_expr(
        hir::ExprKind::Match {
            scrutinee: Box::new(body_test_expr(
                hir::ExprKind::EnumValue {
                    enum_id: 0,
                    variant: 0,
                    payload: Vec::new(),
                },
                Ty::Enum(0),
            )),
            arms: vec![
                hir::MatchArm {
                    variants: vec![0],
                    bindings: Vec::new(),
                    body: body_test_expr(
                        hir::ExprKind::FnValue("noop".to_string()),
                        Ty::Fn(fn_one),
                    ),
                },
                hir::MatchArm {
                    variants: vec![1],
                    bindings: vec![1],
                    body: body_test_expr(
                        hir::ExprKind::FnValue("noop".to_string()),
                        Ty::Fn(fn_two),
                    ),
                },
            ],
        },
        Ty::Fn(fn_one),
    );
    program.fns.push(body_test_function(
        hir::Block {
            stmts: vec![
                hir::Stmt::Let {
                    local: 0,
                    init: match_expr,
                },
                hir::Stmt::Expr(body_test_expr(
                    hir::ExprKind::CallFnValue {
                        callee: Box::new(body_test_expr(
                            hir::ExprKind::Local(0),
                            Ty::Fn(fn_one),
                        )),
                        args: Vec::new(),
                    },
                    Ty::Unit,
                )),
            ],
            value: Some(Box::new(body_test_expr(hir::ExprKind::Unit, Ty::Unit))),
        },
        vec![
            body_test_local(0, "selected", Ty::Fn(fn_one), false, false),
            body_test_local(1, "ignored", int(64), false, false),
        ],
        Ty::Unit,
    ));
    assert!(body_core_metadata_is_valid(&program));
}

#[test]
fn hir_body_validator_rejects_out_of_scope_local_use() {
    let integer = int(64);
    let mut program = baseline_program();
    program.fns.push(body_test_function(
        hir::Block {
            stmts: vec![hir::Stmt::Expr(body_test_expr(
                hir::ExprKind::If {
                    cond: Box::new(body_test_expr(hir::ExprKind::Bool(true), Ty::Bool)),
                    then: hir::Block {
                        stmts: vec![hir::Stmt::Let {
                            local: 0,
                            init: body_test_expr(hir::ExprKind::Int(7), integer),
                        }],
                        value: None,
                    },
                    els: hir::Block {
                        stmts: Vec::new(),
                        value: None,
                    },
                },
                Ty::Unit,
            ))],
            value: Some(Box::new(body_test_expr(
                hir::ExprKind::Local(0),
                integer,
            ))),
        },
        vec![body_test_local(0, "branch_value", integer, false, false)],
        integer,
    ));
    assert!(!validate_hir::body_only_metadata_is_valid(&program));

    let mut unbound = baseline_program();
    unbound.fns.push(body_tail_case(
        "unbound_local_use",
        body_test_expr(hir::ExprKind::Local(0), integer),
        integer,
    ));
    unbound.fns[0]
        .locals
        .push(body_test_local(0, "missing", integer, false, false));
    assert!(!validate_hir::body_only_metadata_is_valid(&unbound));
}

#[test]
fn hir_body_validator_accepts_nested_tagged_payload_construction() {
    let program = checked_source_program(
        "Output { text: string, note: Option<string> }\n\
         NativeError { code: Option<string>, message: string }\n\
         DbError { Native(NativeError), Decode(string) }\n\
         fn run(mode: i32) -> Result<Option<Output>, DbError> {\n\
           if mode == 0 { return Ok(None) }\n\
           if mode == 1 { return Ok(Some(Output { text: \"row\".clone(), note: Some(\"note\".clone()) })) }\n\
         if mode == 2 { return Err(DbError.Decode(\"decode\".clone())) }\n\
           return Err(DbError.Native(NativeError { code: Some(\"7\".clone()), message: \"native\".clone() }))\n\
         }\n\
         fn score(result: Result<Option<Output>, DbError>) -> i32 = match result {\n\
           Ok(value) => match value {\n\
             Some(output) => output.text.len() as i32 + match output.note { Some(note) => note.len() as i32, None => 0 },\n\
             None => 2,\n\
           },\n\
           Err(error) => match error {\n\
             Native(value) => value.message.len() as i32 + match value.code { Some(code) => code.len() as i32, None => 0 },\n\
             Decode(message) => message.len() as i32,\n\
           },\n\
         }\n",
    );
    assert!(body_core_metadata_is_valid(&program));
    assert!(
        align_sema::checked_hir_body_facts_are_valid(&program),
        "nested tagged body must satisfy fact replay"
    );
}

#[test]
fn hir_body_validator_rejects_divergent_eager_parent_type_mismatch() {
    let integer = int(64);
    let diverging = body_test_expr(
        hir::ExprKind::Loop {
            body: hir::Block {
                stmts: Vec::new(),
                value: None,
            },
            diverges: true,
            body_locals: 0..0,
        },
        integer,
    );
    let mut program = baseline_program();
    program.fns.push(body_tail_case(
        "divergent_eager_parent",
        body_test_expr(
            hir::ExprKind::Unary {
                op: align_ast::UnOp::Neg,
                expr: Box::new(diverging),
            },
            Ty::Bool,
        ),
        Ty::Bool,
    ));
    assert!(!body_core_metadata_is_valid(&program));
}

#[test]
fn hir_body_validator_integer_width_is_fail_closed_without_panic() {
    let integer = int(64);
    let mut program = baseline_program();
    program.fns.push(body_tail_case(
        "invalid_integer_width",
        body_test_expr(
            hir::ExprKind::Int(0),
            Ty::Int(IntTy {
                bits: 0,
                signed: true,
            }),
        ),
        integer,
    ));
    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        body_core_metadata_is_valid(&program)
    }));
    assert!(result.is_ok(), "invalid integer width must not panic");
    assert!(!result.expect("panic result checked"));
}

#[test]
fn hir_body_validator_local_type_placement_is_fail_closed() {
    let unit = Ty::Unit;
    let local = |ty| hir::Local {
        id: 0,
        name: "value".to_string(),
        ty,
        is_mut: false,
        is_param: false,
        align: None,
    };
    let function = |ty| {
        body_test_function(
            hir::Block {
                stmts: Vec::new(),
                value: Some(Box::new(body_test_expr(hir::ExprKind::Unit, unit))),
            },
            vec![local(ty)],
            unit,
        )
    };

    let mut valid = baseline_program();
    valid.fns.push(function(Ty::DynStructArray(0, Layout::Aos)));
    assert!(body_core_metadata_is_valid(&valid));

    let mut dynamic_array_of_struct = baseline_program();
    dynamic_array_of_struct
        .fns
        .push(function(Ty::DynArray(Scalar::Struct(0))));
    assert!(!body_core_metadata_is_valid(&dynamic_array_of_struct));

    let mut over_aligned_dynamic_struct_array = baseline_program();
    over_aligned_dynamic_struct_array.structs[0].align = Some(16);
    over_aligned_dynamic_struct_array
        .fns
        .push(function(Ty::DynStructArray(0, Layout::Aos)));
    assert!(!body_core_metadata_is_valid(&over_aligned_dynamic_struct_array));

    let mut task_of_struct = baseline_program();
    task_of_struct
        .fns
        .push(function(Ty::Task(Scalar::Struct(0))));
    assert!(!body_core_metadata_is_valid(&task_of_struct));
}

#[test]
fn hir_body_validator_expression_inventory() {
    let integer = int(64);
    let float = Ty::Float(FloatTy { bits: 64 });
    let choice = Ty::Enum(0);
    let result = Ty::Result(scalar_int(64), Scalar::Enum(0));
    let task = Ty::Task(scalar_int(64));
    let boxed = Ty::Box(scalar_int(64));
    let mut program = baseline_program();
    program.fn_types.extend([
        body_fn_type(
            vec![(align_ast::ParamMode::ByValue, scalar_int(64))],
            integer,
        ),
        body_fn_type(Vec::new(), integer),
        body_fn_type(
            vec![(align_ast::ParamMode::ByValue, Scalar::Enum(0))],
            choice,
        ),
        body_fn_type(Vec::new(), integer),
    ]);
    program.fns.extend([
        body_test_named_function(
            "fn_value_target",
            hir::Block {
                stmts: Vec::new(),
                value: Some(Box::new(body_test_expr(hir::ExprKind::Unit, Ty::Unit))),
            },
            Vec::new(),
            Ty::Unit,
        ),
        body_test_parameter_function(
            "indirect_target",
            integer,
            hir::Block {
                stmts: Vec::new(),
                value: Some(Box::new(body_test_expr(hir::ExprKind::Local(0), integer))),
            },
            integer,
        ),
        {
            let mut function = body_test_parameter_function(
                "capture_target",
                integer,
                hir::Block {
                    stmts: Vec::new(),
                    value: Some(Box::new(body_test_expr(hir::ExprKind::Local(0), integer))),
                },
                integer,
            );
            function.origin = hir::FnOrigin::Lifted { capture_count: 1 };
            function.locals[0].is_param = false;
            function
        },
        body_test_parameter_function(
            "map_error_target",
            choice,
            hir::Block {
                stmts: Vec::new(),
                value: Some(Box::new(body_test_expr(hir::ExprKind::Local(0), choice))),
            },
            choice,
        ),
        {
            let mut function = body_tail_case(
                "spawn_target",
                body_test_expr(hir::ExprKind::Int(1), integer),
                integer,
            );
            function.origin = hir::FnOrigin::Lifted { capture_count: 0 };
            function
        },
    ]);

    program.fns.extend([
        body_unit_case(
            "float_literal_case",
            body_test_expr(hir::ExprKind::Float(1.5), float),
        ),
        body_unit_case(
            "char_literal_case",
            body_test_expr(hir::ExprKind::Char('A' as u32), Ty::Char),
        ),
        body_unit_case(
            "unary_neg_case",
            body_test_expr(
                hir::ExprKind::Unary {
                    op: align_ast::UnOp::Neg,
                    expr: Box::new(body_test_expr(hir::ExprKind::Int(1), integer)),
                },
                integer,
            ),
        ),
        body_unit_case(
            "unary_not_case",
            body_test_expr(
                hir::ExprKind::Unary {
                    op: align_ast::UnOp::Not,
                    expr: Box::new(body_test_expr(hir::ExprKind::Bool(true), Ty::Bool)),
                },
                Ty::Bool,
            ),
        ),
        body_unit_case(
            "unary_bit_not_case",
            body_test_expr(
                hir::ExprKind::Unary {
                    op: align_ast::UnOp::BitNot,
                    expr: Box::new(body_test_expr(hir::ExprKind::Int(1), integer)),
                },
                integer,
            ),
        ),
        body_unit_case(
            "cast_case",
            body_test_expr(
                hir::ExprKind::Cast(Box::new(body_test_expr(
                    hir::ExprKind::Int(1),
                    integer,
                ))),
                float,
            ),
        ),
        body_unit_case(
            "binary_add_case",
            body_test_expr(
                hir::ExprKind::Binary {
                    op: align_ast::BinOp::Add,
                    lhs: Box::new(body_test_expr(hir::ExprKind::Int(1), integer)),
                    rhs: Box::new(body_test_expr(hir::ExprKind::Int(2), integer)),
                },
                integer,
            ),
        ),
        body_unit_case(
            "binary_sub_case",
            body_test_expr(
                hir::ExprKind::Binary {
                    op: align_ast::BinOp::Sub,
                    lhs: Box::new(body_test_expr(hir::ExprKind::Int(2), integer)),
                    rhs: Box::new(body_test_expr(hir::ExprKind::Int(1), integer)),
                },
                integer,
            ),
        ),
        body_unit_case(
            "binary_mul_case",
            body_test_expr(
                hir::ExprKind::Binary {
                    op: align_ast::BinOp::Mul,
                    lhs: Box::new(body_test_expr(hir::ExprKind::Int(2), integer)),
                    rhs: Box::new(body_test_expr(hir::ExprKind::Int(3), integer)),
                },
                integer,
            ),
        ),
        body_unit_case(
            "binary_div_case",
            body_test_expr(
                hir::ExprKind::Binary {
                    op: align_ast::BinOp::Div,
                    lhs: Box::new(body_test_expr(hir::ExprKind::Int(4), integer)),
                    rhs: Box::new(body_test_expr(hir::ExprKind::Int(2), integer)),
                },
                integer,
            ),
        ),
        body_unit_case(
            "binary_rem_case",
            body_test_expr(
                hir::ExprKind::Binary {
                    op: align_ast::BinOp::Rem,
                    lhs: Box::new(body_test_expr(hir::ExprKind::Int(5), integer)),
                    rhs: Box::new(body_test_expr(hir::ExprKind::Int(2), integer)),
                },
                integer,
            ),
        ),
        body_unit_case(
            "binary_eq_case",
            body_test_expr(
                hir::ExprKind::Binary {
                    op: align_ast::BinOp::Eq,
                    lhs: Box::new(body_test_expr(hir::ExprKind::Int(1), integer)),
                    rhs: Box::new(body_test_expr(hir::ExprKind::Int(1), integer)),
                },
                Ty::Bool,
            ),
        ),
        body_unit_case(
            "binary_ne_case",
            body_test_expr(
                hir::ExprKind::Binary {
                    op: align_ast::BinOp::Ne,
                    lhs: Box::new(body_test_expr(hir::ExprKind::Int(1), integer)),
                    rhs: Box::new(body_test_expr(hir::ExprKind::Int(2), integer)),
                },
                Ty::Bool,
            ),
        ),
        body_unit_case(
            "binary_lt_case",
            body_test_expr(
                hir::ExprKind::Binary {
                    op: align_ast::BinOp::Lt,
                    lhs: Box::new(body_test_expr(hir::ExprKind::Int(1), integer)),
                    rhs: Box::new(body_test_expr(hir::ExprKind::Int(2), integer)),
                },
                Ty::Bool,
            ),
        ),
        body_unit_case(
            "binary_le_case",
            body_test_expr(
                hir::ExprKind::Binary {
                    op: align_ast::BinOp::Le,
                    lhs: Box::new(body_test_expr(hir::ExprKind::Int(1), integer)),
                    rhs: Box::new(body_test_expr(hir::ExprKind::Int(2), integer)),
                },
                Ty::Bool,
            ),
        ),
        body_unit_case(
            "binary_gt_case",
            body_test_expr(
                hir::ExprKind::Binary {
                    op: align_ast::BinOp::Gt,
                    lhs: Box::new(body_test_expr(hir::ExprKind::Int(2), integer)),
                    rhs: Box::new(body_test_expr(hir::ExprKind::Int(1), integer)),
                },
                Ty::Bool,
            ),
        ),
        body_unit_case(
            "binary_ge_case",
            body_test_expr(
                hir::ExprKind::Binary {
                    op: align_ast::BinOp::Ge,
                    lhs: Box::new(body_test_expr(hir::ExprKind::Int(2), integer)),
                    rhs: Box::new(body_test_expr(hir::ExprKind::Int(1), integer)),
                },
                Ty::Bool,
            ),
        ),
        body_unit_case(
            "binary_and_case",
            body_test_expr(
                hir::ExprKind::Binary {
                    op: align_ast::BinOp::And,
                    lhs: Box::new(body_test_expr(hir::ExprKind::Bool(true), Ty::Bool)),
                    rhs: Box::new(body_test_expr(hir::ExprKind::Bool(false), Ty::Bool)),
                },
                Ty::Bool,
            ),
        ),
        body_unit_case(
            "binary_or_case",
            body_test_expr(
                hir::ExprKind::Binary {
                    op: align_ast::BinOp::Or,
                    lhs: Box::new(body_test_expr(hir::ExprKind::Bool(true), Ty::Bool)),
                    rhs: Box::new(body_test_expr(hir::ExprKind::Bool(false), Ty::Bool)),
                },
                Ty::Bool,
            ),
        ),
        body_unit_case(
            "binary_bit_and_case",
            body_test_expr(
                hir::ExprKind::Binary {
                    op: align_ast::BinOp::BitAnd,
                    lhs: Box::new(body_test_expr(hir::ExprKind::Int(1), integer)),
                    rhs: Box::new(body_test_expr(hir::ExprKind::Int(2), integer)),
                },
                integer,
            ),
        ),
        body_unit_case(
            "binary_bit_or_case",
            body_test_expr(
                hir::ExprKind::Binary {
                    op: align_ast::BinOp::BitOr,
                    lhs: Box::new(body_test_expr(hir::ExprKind::Int(1), integer)),
                    rhs: Box::new(body_test_expr(hir::ExprKind::Int(2), integer)),
                },
                integer,
            ),
        ),
        body_unit_case(
            "binary_bit_xor_case",
            body_test_expr(
                hir::ExprKind::Binary {
                    op: align_ast::BinOp::BitXor,
                    lhs: Box::new(body_test_expr(hir::ExprKind::Int(1), integer)),
                    rhs: Box::new(body_test_expr(hir::ExprKind::Int(2), integer)),
                },
                integer,
            ),
        ),
        body_unit_case(
            "binary_shl_case",
            body_test_expr(
                hir::ExprKind::Binary {
                    op: align_ast::BinOp::Shl,
                    lhs: Box::new(body_test_expr(hir::ExprKind::Int(1), integer)),
                    rhs: Box::new(body_test_expr(hir::ExprKind::Int(2), integer)),
                },
                integer,
            ),
        ),
        body_unit_case(
            "binary_shr_case",
            body_test_expr(
                hir::ExprKind::Binary {
                    op: align_ast::BinOp::Shr,
                    lhs: Box::new(body_test_expr(hir::ExprKind::Int(4), integer)),
                    rhs: Box::new(body_test_expr(hir::ExprKind::Int(1), integer)),
                },
                integer,
            ),
        ),
        body_unit_case(
            "int_arith_saturating_case",
            body_test_expr(
                hir::ExprKind::IntArith {
                    op: align_ast::BinOp::Add,
                    mode: hir::ArithMode::Saturating,
                    lhs: Box::new(body_test_expr(hir::ExprKind::Int(1), integer)),
                    rhs: Box::new(body_test_expr(hir::ExprKind::Int(2), integer)),
                },
                integer,
            ),
        ),
        body_unit_case(
            "int_arith_checked_case",
            body_test_expr(
                hir::ExprKind::IntArith {
                    op: align_ast::BinOp::Mul,
                    mode: hir::ArithMode::Checked,
                    lhs: Box::new(body_test_expr(hir::ExprKind::Int(2), integer)),
                    rhs: Box::new(body_test_expr(hir::ExprKind::Int(3), integer)),
                },
                Ty::Option(scalar_int(64)),
            ),
        ),
        body_unit_case(
            "math_abs_case",
            body_test_expr(
                hir::ExprKind::MathOp {
                    fn_: hir::MathFn::Abs,
                    operands: vec![body_test_expr(hir::ExprKind::Int(1), integer)],
                },
                integer,
            ),
        ),
        body_unit_case(
            "math_min_case",
            body_test_expr(
                hir::ExprKind::MathOp {
                    fn_: hir::MathFn::Min,
                    operands: vec![
                        body_test_expr(hir::ExprKind::Int(1), integer),
                        body_test_expr(hir::ExprKind::Int(2), integer),
                    ],
                },
                integer,
            ),
        ),
        body_unit_case(
            "math_max_case",
            body_test_expr(
                hir::ExprKind::MathOp {
                    fn_: hir::MathFn::Max,
                    operands: vec![
                        body_test_expr(hir::ExprKind::Float(1.0), float),
                        body_test_expr(hir::ExprKind::Float(2.0), float),
                    ],
                },
                float,
            ),
        ),
        body_unit_case(
            "math_sqrt_case",
            body_test_expr(
                hir::ExprKind::MathOp {
                    fn_: hir::MathFn::Sqrt,
                    operands: vec![body_test_expr(hir::ExprKind::Float(1.0), float)],
                },
                float,
            ),
        ),
        body_unit_case(
            "math_floor_case",
            body_test_expr(
                hir::ExprKind::MathOp {
                    fn_: hir::MathFn::Floor,
                    operands: vec![body_test_expr(hir::ExprKind::Float(1.0), float)],
                },
                float,
            ),
        ),
        body_unit_case(
            "math_ceil_case",
            body_test_expr(
                hir::ExprKind::MathOp {
                    fn_: hir::MathFn::Ceil,
                    operands: vec![body_test_expr(hir::ExprKind::Float(1.0), float)],
                },
                float,
            ),
        ),
        body_unit_case(
            "math_round_case",
            body_test_expr(
                hir::ExprKind::MathOp {
                    fn_: hir::MathFn::Round,
                    operands: vec![body_test_expr(hir::ExprKind::Float(1.0), float)],
                },
                float,
            ),
        ),
        body_unit_case(
            "math_trunc_case",
            body_test_expr(
                hir::ExprKind::MathOp {
                    fn_: hir::MathFn::Trunc,
                    operands: vec![body_test_expr(hir::ExprKind::Float(1.0), float)],
                },
                float,
            ),
        ),
        body_unit_case(
            "math_pow_case",
            body_test_expr(
                hir::ExprKind::MathOp {
                    fn_: hir::MathFn::Pow,
                    operands: vec![
                        body_test_expr(hir::ExprKind::Float(2.0), float),
                        body_test_expr(hir::ExprKind::Float(3.0), float),
                    ],
                },
                float,
            ),
        ),
        body_unit_case(
            "math_fma_case",
            body_test_expr(
                hir::ExprKind::MathOp {
                    fn_: hir::MathFn::Fma,
                    operands: vec![
                        body_test_expr(hir::ExprKind::Float(1.0), float),
                        body_test_expr(hir::ExprKind::Float(2.0), float),
                        body_test_expr(hir::ExprKind::Float(3.0), float),
                    ],
                },
                float,
            ),
        ),
    ]);

    let fn_value = body_test_expr(
        hir::ExprKind::FnValue("fn_value_target".to_string()),
        Ty::Fn(0),
    );
    program.fns.push(body_unit_case("fn_value_case", fn_value));

    let closure = body_test_expr(
        hir::ExprKind::Closure {
            lifted: "capture_target".to_string(),
            captures: vec![body_test_expr(hir::ExprKind::Local(0), integer)],
        },
        Ty::Fn(2),
    );
    program.fns.push(body_test_named_function(
        "closure_case",
        hir::Block {
            stmts: vec![
                hir::Stmt::Let {
                    local: 0,
                    init: body_test_expr(hir::ExprKind::Int(1), integer),
                },
                hir::Stmt::Expr(closure),
            ],
            value: Some(Box::new(body_test_expr(hir::ExprKind::Unit, Ty::Unit))),
        },
        vec![hir::Local {
            id: 0,
            name: "capture".to_string(),
            ty: integer,
            is_mut: false,
            is_param: false,
            align: None,
        }],
        Ty::Unit,
    ));

    program.fns.push(body_unit_case(
        "call_fn_value_case",
        body_test_expr(
            hir::ExprKind::CallFnValue {
                callee: Box::new(body_test_expr(
                    hir::ExprKind::FnValue("indirect_target".to_string()),
                    Ty::Fn(1),
                )),
                args: vec![body_test_expr(hir::ExprKind::Int(2), integer)],
            },
            integer,
        ),
    ));

    program.fns.push(body_unit_case(
        "task_group_wait_case",
        body_test_expr(
            hir::ExprKind::TaskGroup(hir::Block {
                stmts: Vec::new(),
                value: Some(Box::new(body_test_expr(hir::ExprKind::Wait, Ty::Unit))),
            }),
            Ty::Unit,
        ),
    ));

    program.fns.push(body_unit_case(
        "enum_value_case",
        body_test_expr(
            hir::ExprKind::EnumValue {
                enum_id: 0,
                variant: 0,
                payload: Vec::new(),
            },
            choice,
        ),
    ));
    program.fns.push(body_unit_case(
        "match_case",
        body_test_expr(
            hir::ExprKind::Match {
                scrutinee: Box::new(body_test_expr(
                    hir::ExprKind::EnumValue {
                        enum_id: 0,
                        variant: 0,
                        payload: Vec::new(),
                    },
                    choice,
                )),
                arms: vec![hir::MatchArm {
                    variants: Vec::new(),
                    bindings: Vec::new(),
                    body: body_test_expr(hir::ExprKind::Unit, Ty::Unit),
                }],
            },
            Ty::Unit,
        ),
    ));

    program.fns.push(body_tail_case(
        "map_err_case",
        body_test_expr(
            hir::ExprKind::ResultMapErr {
                result: Box::new(body_test_expr(
                    hir::ExprKind::ResultOk(Box::new(body_test_expr(
                        hir::ExprKind::Int(1),
                        integer,
                    ))),
                    result,
                )),
                f: Box::new(body_test_expr(
                    hir::ExprKind::FnValue("map_error_target".to_string()),
                    Ty::Fn(3),
                )),
            },
            result,
        ),
        result,
    ));

    let spawn_expr = || {
        body_test_expr(
            hir::ExprKind::Spawn {
                closure: Box::new(body_test_expr(
                    hir::ExprKind::FnValue("spawn_target".to_string()),
                    Ty::Fn(4),
                )),
                fallible: false,
            },
            task,
        )
    };
    program.fns.push(body_unit_case(
        "spawn_case",
        body_test_expr(
            hir::ExprKind::TaskGroup(hir::Block {
                stmts: vec![hir::Stmt::Expr(spawn_expr())],
                value: Some(Box::new(body_test_expr(hir::ExprKind::Unit, Ty::Unit))),
            }),
            Ty::Unit,
        ),
    ));
    program.fns.push(body_test_named_function(
        "task_get_case",
        hir::Block {
            stmts: vec![hir::Stmt::Expr(body_test_expr(
                hir::ExprKind::TaskGroup(hir::Block {
                    stmts: vec![hir::Stmt::Let {
                        local: 0,
                        init: spawn_expr(),
                    }],
                    value: Some(Box::new(body_test_expr(
                        hir::ExprKind::TaskGet(Box::new(body_test_expr(
                            hir::ExprKind::Local(0),
                            task,
                        ))),
                        integer,
                    ))),
                }),
                integer,
            ))],
            value: Some(Box::new(body_test_expr(hir::ExprKind::Unit, Ty::Unit))),
        },
        vec![hir::Local {
            id: 0,
            name: "task".to_string(),
            ty: task,
            is_mut: false,
            is_param: false,
            align: None,
        }],
        Ty::Unit,
    ));

    program.fns.push(body_unit_case(
        "call_case",
        body_test_expr(
            hir::ExprKind::Call {
                func: "indirect_target".to_string(),
                args: vec![body_test_expr(hir::ExprKind::Int(3), integer)],
                type_args: Vec::new(),
            },
            integer,
        ),
    ));
    program.fns.push(body_unit_case(
        "if_case",
        body_test_expr(
            hir::ExprKind::If {
                cond: Box::new(body_test_expr(hir::ExprKind::Bool(true), Ty::Bool)),
                then: hir::Block {
                    stmts: Vec::new(),
                    value: Some(Box::new(body_test_expr(hir::ExprKind::Int(1), integer))),
                },
                els: hir::Block {
                    stmts: Vec::new(),
                    value: Some(Box::new(body_test_expr(hir::ExprKind::Int(2), integer))),
                },
            },
            integer,
        ),
    ));
    let diverging_bool = body_test_expr(
        hir::ExprKind::Loop {
            body: hir::Block {
                stmts: Vec::new(),
                value: None,
            },
            diverges: true,
            body_locals: 0..0,
        },
        Ty::Bool,
    );
    program.fns.push(body_unit_case(
        "if_diverging_condition_case",
        body_test_expr(
            hir::ExprKind::If {
                cond: Box::new(diverging_bool),
                then: hir::Block {
                    stmts: Vec::new(),
                    value: Some(Box::new(body_test_expr(hir::ExprKind::Int(1), integer))),
                },
                els: hir::Block {
                    stmts: Vec::new(),
                    value: Some(Box::new(body_test_expr(
                        hir::ExprKind::Bool(true),
                        Ty::Bool,
                    ))),
                },
            },
            integer,
        ),
    ));
    let diverging_choice = body_test_expr(
        hir::ExprKind::Loop {
            body: hir::Block {
                stmts: Vec::new(),
                value: None,
            },
            diverges: true,
            body_locals: 0..0,
        },
        choice,
    );
    program.fns.push({
        let mut function = body_unit_case(
            "match_diverging_scrutinee_case",
            body_test_expr(
                hir::ExprKind::Match {
                    scrutinee: Box::new(diverging_choice),
                    arms: vec![
                        hir::MatchArm {
                            variants: vec![0],
                            bindings: Vec::new(),
                            body: body_test_expr(hir::ExprKind::Int(1), integer),
                        },
                        hir::MatchArm {
                            variants: vec![1],
                            bindings: vec![0],
                            body: body_test_expr(hir::ExprKind::Bool(true), Ty::Bool),
                        },
                    ],
                },
                integer,
            ),
        );
        function.locals = vec![hir::Local {
            id: 0,
            name: "payload".to_string(),
            ty: integer,
            is_mut: false,
            is_param: false,
            align: None,
        }];
        function
    });
    program.fns.push(body_unit_case(
        "struct_lit_case",
        body_test_expr(
            hir::ExprKind::StructLit {
                struct_id: 0,
                fields: vec![
                    body_test_expr(hir::ExprKind::Str("key".to_string()), Ty::Str),
                    body_test_expr(hir::ExprKind::Int(4), integer),
                ],
            },
            Ty::Struct(0),
        ),
    ));
    program.fns.push(body_test_parameter_function(
        "field_case",
        Ty::Struct(0),
        hir::Block {
            stmts: vec![hir::Stmt::Expr(body_test_expr(
                hir::ExprKind::Field {
                    root: 0,
                    path: vec![1],
                },
                integer,
            ))],
            value: Some(Box::new(body_test_expr(hir::ExprKind::Unit, Ty::Unit))),
        },
        Ty::Unit,
    ));
    program.fns.push(body_test_parameter_function(
        "soa_column_case",
        Ty::Soa(0),
        hir::Block {
            stmts: vec![hir::Stmt::Expr(body_test_expr(
                hir::ExprKind::SoaColumn {
                    base: 0,
                    struct_id: 0,
                    field: 1,
                },
                Ty::Slice(scalar_int(64)),
            ))],
            value: Some(Box::new(body_test_expr(hir::ExprKind::Unit, Ty::Unit))),
        },
        Ty::Unit,
    ));
    program.fns.push(body_unit_case(
        "tuple_case",
        body_test_expr(
            hir::ExprKind::Tuple {
                tuple_id: 0,
                elems: vec![
                    body_test_expr(hir::ExprKind::Int(1), integer),
                    body_test_expr(hir::ExprKind::Bool(true), Ty::Bool),
                ],
            },
            Ty::Tuple(0),
        ),
    ));
    program.fns.push(body_test_parameter_function(
        "tuple_index_case",
        Ty::Tuple(0),
        hir::Block {
            stmts: vec![hir::Stmt::Expr(body_test_expr(
                hir::ExprKind::TupleIndex {
                    recv: Box::new(body_test_expr(hir::ExprKind::Local(0), Ty::Tuple(0))),
                    index: 0,
                },
                integer,
            ))],
            value: Some(Box::new(body_test_expr(hir::ExprKind::Unit, Ty::Unit))),
        },
        Ty::Unit,
    ));
    program.fns.push(body_unit_case(
        "index_field_case",
        body_test_expr(
            hir::ExprKind::IndexField {
                base: 0,
                index: 0,
                path: vec![1],
            },
            integer,
        ),
    ));
    program.fns.last_mut().expect("index field case").locals = vec![hir::Local {
        id: 0,
        name: "rows".to_string(),
        ty: Ty::StructArray(0, 1),
        is_mut: false,
        is_param: true,
        align: None,
    }];

    program.fns.push(body_unit_case(
        "block_case",
        body_test_expr(
            hir::ExprKind::Block(hir::Block {
                stmts: Vec::new(),
                value: Some(Box::new(body_test_expr(hir::ExprKind::Int(5), integer))),
            }),
            integer,
        ),
    ));
    program.fns.push(body_unit_case(
        "option_some_case",
        body_test_expr(
            hir::ExprKind::OptionSome(Box::new(body_test_expr(
                hir::ExprKind::Int(1),
                integer,
            ))),
            Ty::Option(scalar_int(64)),
        ),
    ));
    program.fns.push(body_unit_case(
        "option_none_case",
        body_test_expr(hir::ExprKind::OptionNone, Ty::Option(scalar_int(64))),
    ));
    program.fns.push(body_unit_case(
        "else_unwrap_case",
        body_test_expr(
            hir::ExprKind::ElseUnwrap {
                opt: Box::new(body_test_expr(
                    hir::ExprKind::OptionSome(Box::new(body_test_expr(
                        hir::ExprKind::Int(1),
                        integer,
                    ))),
                    Ty::Option(scalar_int(64)),
                )),
                fallback: Box::new(body_test_expr(hir::ExprKind::Int(0), integer)),
            },
            integer,
        ),
    ));
    program.fns.push(body_tail_case(
        "result_ok_case",
        body_test_expr(
            hir::ExprKind::ResultOk(Box::new(body_test_expr(
                hir::ExprKind::Int(1),
                integer,
            ))),
            result,
        ),
        result,
    ));
    program.fns.push(body_tail_case(
        "result_err_case",
        body_test_expr(
            hir::ExprKind::ResultErr(Box::new(body_test_expr(
                hir::ExprKind::EnumValue {
                    enum_id: 0,
                    variant: 0,
                    payload: Vec::new(),
                },
                choice,
            ))),
            result,
        ),
        result,
    ));
    program.fns.push(body_tail_case(
        "try_case",
        body_test_expr(
            hir::ExprKind::ResultOk(Box::new(body_test_expr(
                hir::ExprKind::Try(Box::new(body_test_expr(
                    hir::ExprKind::ResultOk(Box::new(body_test_expr(
                        hir::ExprKind::Int(1),
                        integer,
                    ))),
                    result,
                ))),
                integer,
            ))),
            result,
        ),
        result,
    ));
    program.fns.push(body_unit_case(
        "loop_case",
        body_test_expr(
            hir::ExprKind::Loop {
                body: hir::Block {
                    stmts: vec![hir::Stmt::Break {
                        value: Some(body_test_expr(hir::ExprKind::Int(1), integer)),
                        accepted: true,
                    }],
                    value: None,
                },
                diverges: false,
                body_locals: 0..0,
            },
            integer,
        ),
    ));
    program.fns.push(body_unit_case(
        "arena_case",
        body_test_expr(
            hir::ExprKind::Arena(hir::Block {
                stmts: Vec::new(),
                value: Some(Box::new(body_test_expr(hir::ExprKind::Int(1), integer))),
            }),
            integer,
        ),
    ));
    program.fns.push(body_unit_case(
        "unsafe_case",
        body_test_expr(
            hir::ExprKind::Unsafe(hir::Block {
                stmts: Vec::new(),
                value: Some(Box::new(body_test_expr(hir::ExprKind::Int(1), integer))),
            }),
            integer,
        ),
    ));
    let raw_local = |name: &str, expression: hir::Expr| {
        let mut function = body_unit_case(name, expression);
        function.locals = vec![hir::Local {
            id: 0,
            name: "ptr".to_string(),
            ty: Ty::Raw,
            is_mut: true,
            is_param: true,
            align: None,
        }];
        function
    };
    program.fns.push(body_unit_case(
        "raw_alloc_case",
        body_test_expr(
            hir::ExprKind::Unsafe(hir::Block {
                stmts: Vec::new(),
                value: Some(Box::new(body_test_expr(
                    hir::ExprKind::RawAlloc(Box::new(body_test_expr(
                        hir::ExprKind::Int(8),
                        integer,
                    ))),
                    Ty::Raw,
                ))),
            }),
            Ty::Raw,
        ),
    ));
    program.fns.push(raw_local(
        "raw_free_case",
        body_test_expr(
            hir::ExprKind::Unsafe(hir::Block {
                stmts: Vec::new(),
                value: Some(Box::new(body_test_expr(
                    hir::ExprKind::RawFree(Box::new(body_test_expr(
                        hir::ExprKind::Local(0),
                        Ty::Raw,
                    ))),
                    Ty::Unit,
                ))),
            }),
            Ty::Unit,
        ),
    ));
    program.fns.push(raw_local(
        "raw_load_case",
        body_test_expr(
            hir::ExprKind::Unsafe(hir::Block {
                stmts: Vec::new(),
                value: Some(Box::new(body_test_expr(
                    hir::ExprKind::RawLoad {
                        ptr: Box::new(body_test_expr(hir::ExprKind::Local(0), Ty::Raw)),
                        offset: Box::new(body_test_expr(hir::ExprKind::Int(0), integer)),
                        scalar: scalar_int(64),
                    },
                    integer,
                ))),
            }),
            integer,
        ),
    ));
    program.fns.push(raw_local(
        "raw_store_case",
        body_test_expr(
            hir::ExprKind::Unsafe(hir::Block {
                stmts: Vec::new(),
                value: Some(Box::new(body_test_expr(
                    hir::ExprKind::RawStore {
                        ptr: Box::new(body_test_expr(hir::ExprKind::Local(0), Ty::Raw)),
                        offset: Box::new(body_test_expr(hir::ExprKind::Int(0), integer)),
                        value: Box::new(body_test_expr(hir::ExprKind::Int(1), integer)),
                    },
                    Ty::Unit,
                ))),
            }),
            Ty::Unit,
        ),
    ));
    program.fns.push(raw_local(
        "raw_offset_case",
        body_test_expr(
            hir::ExprKind::Unsafe(hir::Block {
                stmts: Vec::new(),
                value: Some(Box::new(body_test_expr(
                    hir::ExprKind::RawOffset {
                        ptr: Box::new(body_test_expr(hir::ExprKind::Local(0), Ty::Raw)),
                        offset: Box::new(body_test_expr(hir::ExprKind::Int(1), integer)),
                    },
                    Ty::Raw,
                ))),
            }),
            Ty::Raw,
        ),
    ));
    program.fns.push(body_unit_case(
        "heap_new_case",
        body_test_expr(
            hir::ExprKind::Arena(hir::Block {
                stmts: Vec::new(),
                value: Some(Box::new(body_test_expr(
                    hir::ExprKind::HeapNew(Box::new(body_test_expr(
                        hir::ExprKind::Int(1),
                        integer,
                    ))),
                    boxed,
                ))),
            }),
            boxed,
        ),
    ));
    let box_local = |name: &str, expression: hir::Expr| {
        let mut function = body_unit_case(name, expression);
        function.locals = vec![hir::Local {
            id: 0,
            name: "box".to_string(),
            ty: boxed,
            is_mut: false,
            is_param: true,
            align: None,
        }];
        function
    };
    program.fns.push(box_local(
        "box_get_case",
        body_test_expr(
            hir::ExprKind::BoxGet(Box::new(body_test_expr(
                hir::ExprKind::Local(0),
                boxed,
            ))),
            integer,
        ),
    ));
    program.fns.push(box_local(
        "box_clone_case",
        body_test_expr(
            hir::ExprKind::Arena(hir::Block {
                stmts: Vec::new(),
                value: Some(Box::new(body_test_expr(
                    hir::ExprKind::BoxClone(Box::new(body_test_expr(
                        hir::ExprKind::Local(0),
                        boxed,
                    ))),
                    boxed,
                ))),
            }),
            boxed,
        ),
    ));
    program.fns.push(body_unit_case(
        "str_clone_case",
        body_test_expr(
            hir::ExprKind::StrClone(Box::new(body_test_expr(
                hir::ExprKind::Str("x".to_string()),
                Ty::Str,
            ))),
            Ty::String,
        ),
    ));
    program.fns.push(body_unit_case(
        "str_predicate_case",
        body_test_expr(
            hir::ExprKind::StrPredicate {
                kind: hir::StrPredKind::Contains,
                haystack: Box::new(body_test_expr(
                    hir::ExprKind::Str("a".to_string()),
                    Ty::Str,
                )),
                needle: Box::new(body_test_expr(
                    hir::ExprKind::Str("b".to_string()),
                    Ty::Str,
                )),
            },
            Ty::Bool,
        ),
    ));
    program.fns.push(body_unit_case(
        "str_trim_case",
        body_test_expr(
            hir::ExprKind::StrTrim {
                kind: hir::StrTrimKind::Both,
                recv: Box::new(body_test_expr(
                    hir::ExprKind::Str(" x ".to_string()),
                    Ty::Str,
                )),
            },
            Ty::Str,
        ),
    ));
    program.fns.push({
        let mut function = body_unit_case(
            "str_borrow_case",
            body_test_expr(
                hir::ExprKind::StrBorrow(Box::new(body_test_expr(
                    hir::ExprKind::Local(0),
                    Ty::String,
                ))),
                Ty::Str,
            ),
        );
        function.locals = vec![hir::Local {
            id: 0,
            name: "owned".to_string(),
            ty: Ty::String,
            is_mut: false,
            is_param: true,
            align: None,
        }];
        function
    });
    program.fns.push(body_unit_case(
        "builder_new_case",
        body_test_expr(hir::ExprKind::BuilderNew { capacity: None }, Ty::Builder),
    ));
    program.fns.push({
        let mut function = body_unit_case(
            "builder_write_case",
            body_test_expr(
                hir::ExprKind::BuilderWrite {
                    builder: Box::new(body_test_expr(hir::ExprKind::Local(0), Ty::Builder)),
                    arg: Box::new(body_test_expr(hir::ExprKind::Str("x".to_string()), Ty::Str)),
                    kind: hir::BuilderWriteKind::Str,
                },
                Ty::Unit,
            ),
        );
        function.locals = vec![hir::Local {
            id: 0,
            name: "builder".to_string(),
            ty: Ty::Builder,
            is_mut: true,
            is_param: true,
            align: None,
        }];
        function
    });
    program.fns.push({
        let mut function = body_unit_case(
            "builder_to_string_case",
            body_test_expr(
                hir::ExprKind::BuilderToString(Box::new(body_test_expr(
                    hir::ExprKind::Local(0),
                    Ty::Builder,
                ))),
                Ty::String,
            ),
        );
        function.locals = vec![hir::Local {
            id: 0,
            name: "builder".to_string(),
            ty: Ty::Builder,
            is_mut: false,
            is_param: true,
            align: None,
        }];
        function
    });

    assert!(body_core_metadata_is_valid(&program));

    // Keep the inventory test table-driven at the assertion boundary: every case above enters
    // the same dormant helper, and a representative envelope mutation must fail closed.
    let mut malformed = program.clone();
    let case = malformed
        .fns
        .iter_mut()
        .find(|function| function.name.as_str() == "str_predicate_case")
        .expect("inventory case");
    let hir::Stmt::Expr(expression) = &mut case.body.stmts[0] else {
        panic!("inventory case lost its expression");
    };
    expression.ty = Ty::Int(IntTy { bits: 32, signed: true });
    assert!(!body_core_metadata_is_valid(&malformed));
}

#[test]
fn hir_body_validator_storage_vector_array() {
    let integer = int(64);
    let scalar = scalar_int(64);
    let vector = |start: i128| {
        body_test_expr(
            hir::ExprKind::VecLit {
                elems: (0..4)
                    .map(|offset| body_test_expr(hir::ExprKind::Int(start + offset), integer))
                    .collect(),
                elem: scalar,
            },
            Ty::Vec(scalar, 4),
        )
    };
    let mask = || {
        body_test_expr(
            hir::ExprKind::Binary {
                op: align_ast::BinOp::Eq,
                lhs: Box::new(vector(0)),
                rhs: Box::new(vector(1)),
            },
            Ty::Mask(scalar, 4),
        )
    };
    let record = |value: i128| {
        body_test_expr(
            hir::ExprKind::StructLit {
                struct_id: 0,
                fields: vec![
                    body_test_expr(hir::ExprKind::Str("key".to_string()), Ty::Str),
                    body_test_expr(hir::ExprKind::Int(value), integer),
                ],
            },
            Ty::Struct(0),
        )
    };
    let move_struct = 1;
    let move_record = || {
        body_test_expr(
            hir::ExprKind::StructLit {
                struct_id: move_struct,
                fields: vec![body_test_expr(
                    hir::ExprKind::StrClone(Box::new(body_test_expr(
                        hir::ExprKind::Str("owned".to_string()),
                        Ty::Str,
                    ))),
                    Ty::String,
                )],
            },
            Ty::Struct(move_struct),
        )
    };
    let array_i64 = |values: &[i128]| {
        body_test_expr(
            hir::ExprKind::ArrayLit {
                elems: values
                    .iter()
                    .map(|value| body_test_expr(hir::ExprKind::Int(*value), integer))
                    .collect(),
                elem: integer,
                pooled: false,
            },
            Ty::Array(scalar, values.len() as u32),
        )
    };
    let mut program = baseline_program();
    program.structs.push(StructDef {
        name: "MoveRecord".to_string(),
        source_name: "MoveRecord".to_string(),
        fields: vec![FieldDef {
            name: "owned".to_string(),
            ty: Ty::String,
        }],
        align: None,
        c_repr: false,
    });
    program.fns.extend([
        body_unit_case("array_literal_case", array_i64(&[1, 2])),
        body_unit_case(
            "struct_array_literal_case",
            body_test_expr(
                hir::ExprKind::ArrayLit {
                    elems: vec![record(1), record(2)],
                    elem: Ty::Struct(0),
                    pooled: false,
                },
                Ty::StructArray(0, 2),
            ),
        ),
        body_unit_case(
            "move_struct_array_literal_case",
            body_test_expr(
                hir::ExprKind::ArrayLit {
                    elems: vec![move_record()],
                    elem: Ty::Struct(move_struct),
                    pooled: false,
                },
                Ty::StructArray(move_struct, 1),
            ),
        ),
        body_unit_case(
            "const_array_case",
            body_test_expr(
                hir::ExprKind::ConstArray {
                    elems: vec![
                        body_test_expr(hir::ExprKind::Int(1), integer),
                        body_test_expr(hir::ExprKind::Int(2), integer),
                    ],
                    elem: scalar,
                    len: 2,
                },
                Ty::Slice(scalar),
            ),
        ),
        body_unit_case(
            "array_zip_case",
            body_test_expr(
                hir::ExprKind::ArrayZip {
                    sources: vec![array_i64(&[1, 2]), body_test_expr(
                        hir::ExprKind::ArrayLit {
                            elems: vec![
                                body_test_expr(hir::ExprKind::Bool(true), Ty::Bool),
                                body_test_expr(hir::ExprKind::Bool(false), Ty::Bool),
                            ],
                            elem: Ty::Bool,
                            pooled: false,
                        },
                        Ty::Array(Scalar::Bool, 2),
                    )],
                    tuple_id: 0,
                },
                Ty::Tuple(0),
            ),
        ),
        body_unit_case(
            "select_case",
            body_test_expr(
                hir::ExprKind::Select {
                    mask: Box::new(mask()),
                    a: Box::new(vector(2)),
                    b: Box::new(vector(3)),
                },
                Ty::Vec(scalar, 4),
            ),
        ),
        body_tail_case(
            "vec_sum_where_case",
            body_test_expr(
                hir::ExprKind::VecSumWhere {
                    vec: Box::new(vector(2)),
                    mask: Box::new(mask()),
                },
                integer,
            ),
            integer,
        ),
        body_tail_case(
            "vec_dot_case",
            body_test_expr(
                hir::ExprKind::VecDot {
                    a: Box::new(vector(2)),
                    b: Box::new(vector(3)),
                },
                integer,
            ),
            integer,
        ),
        body_tail_case(
            "vec_min_case",
            body_test_expr(
                hir::ExprKind::VecMinMax {
                    vec: Box::new(vector(2)),
                    max: false,
                },
                integer,
            ),
            integer,
        ),
        body_tail_case(
            "vec_sum_case",
            body_test_expr(
                hir::ExprKind::VecSum {
                    vec: Box::new(vector(2)),
                },
                integer,
            ),
            integer,
        ),
        body_test_parameter_function(
            "vec_load_case",
            Ty::Slice(scalar),
            hir::Block {
                stmts: vec![hir::Stmt::Expr(body_test_expr(
                    hir::ExprKind::VecLoad {
                        src: Box::new(body_test_expr(hir::ExprKind::Local(0), Ty::Slice(scalar))),
                        index: Box::new(body_test_expr(hir::ExprKind::Int(0), integer)),
                        elem: scalar,
                        n: 4,
                    },
                    Ty::Vec(scalar, 4),
                ))],
                value: Some(Box::new(body_test_expr(
                    hir::ExprKind::Unit,
                    Ty::Unit,
                ))),
            },
            Ty::Unit,
        ),
        body_test_named_function(
            "vec_store_case",
            hir::Block {
                stmts: Vec::new(),
                value: Some(Box::new(body_test_expr(
                    hir::ExprKind::VecStore {
                        dst: Box::new(body_test_expr(hir::ExprKind::Local(0), Ty::Slice(scalar))),
                        index: Box::new(body_test_expr(hir::ExprKind::Int(0), integer)),
                        value: Box::new(vector(2)),
                        elem: scalar,
                        n: 4,
                    },
                    Ty::Unit,
                ))),
            },
            vec![hir::Local {
                id: 0,
                name: "dst".to_string(),
                ty: Ty::Slice(scalar),
                is_mut: true,
                is_param: false,
                align: None,
            }],
            Ty::Unit,
        ),
        body_unit_case("vec_literal_case", vector(2)),
    ]);
    let pooled_elements = (0..32)
        .map(|value| body_test_expr(hir::ExprKind::Int(value), integer))
        .collect::<Vec<_>>();
    program.fns.push(body_test_function(
        hir::Block {
            stmts: vec![hir::Stmt::Let {
                local: 0,
                init: body_test_expr(
                    hir::ExprKind::ArrayLit {
                        elems: pooled_elements,
                        elem: integer,
                        pooled: true,
                    },
                    Ty::Array(scalar, 32),
                ),
            }],
            value: Some(Box::new(body_test_expr(hir::ExprKind::Unit, Ty::Unit))),
        },
        vec![hir::Local {
            id: 0,
            name: "table".to_string(),
            ty: Ty::Array(scalar, 32),
            is_mut: false,
            is_param: false,
            align: None,
        }],
        Ty::Unit,
    ));

    assert!(body_core_metadata_is_valid(&program));

    // Sema admits a Move struct only when every element is a direct struct literal, which lets MIR
    // construct owned fields in their final slots. A typed wrapper around the same value would
    // require a whole-value move/null path, so handcrafted HIR must not widen that contract.
    let mut wrapped_move_struct = program.clone();
    let expression = body_statement_expression_mut(
        &mut wrapped_move_struct,
        "move_struct_array_literal_case",
    );
    let mut wrapped = false;
    if let hir::ExprKind::ArrayLit { elems, .. } = &mut expression.kind
        && let Some(direct) = elems.pop()
    {
        elems.push(body_test_expr(
            hir::ExprKind::Block(hir::Block {
                stmts: Vec::new(),
                value: Some(Box::new(direct)),
            }),
            Ty::Struct(move_struct),
        ));
        wrapped = true;
    }
    assert!(wrapped, "Move-struct fixture must contain one array element");
    assert!(
        !body_core_metadata_is_valid(&wrapped_move_struct),
        "a wrapped Move-struct fixed-array element must fail before MIR lowering",
    );

    // No scalar Move value has a fixed-array element Drop path. This handcrafted HIR keeps the
    // validator-side string rejection independent from sema, whose source gate rejects it first.
    let owned_string = || {
        body_test_expr(
            hir::ExprKind::StrClone(Box::new(body_test_expr(
                hir::ExprKind::Str("owned".to_string()),
                Ty::Str,
            ))),
            Ty::String,
        )
    };
    let mut owned_string_program = program.clone();
    owned_string_program.fns.push(body_unit_case(
        "owned_string_array_rejected",
        body_test_expr(
            hir::ExprKind::ArrayLit {
                elems: vec![owned_string()],
                elem: Ty::String,
                pooled: false,
            },
            Ty::Array(Scalar::String, 1),
        ),
    ));
    assert!(
        !body_core_metadata_is_valid(&owned_string_program),
        "an owned-string fixed array must fail closed at the HIR boundary",
    );

    // Resource owners and checked refs are excluded from fixed arrays recursively. Keep this
    // validator-side negative independent from sema so handcrafted HIR cannot bypass the source
    // admission rule and copy one generation-bearing value into multiple element slots.
    let mut resource_program = baseline_program();
    resource_program.resources.push(ResourceDef {
        name: "pkg.test$owner".to_string(),
        source_name: "pkg.test$owner".to_string(),
        declaring_module: "pkg.test".to_string(),
        generic_arity: 0,
        drop_hook: "pkg.test$drop_owner".to_string(),
        drop_thunk: "__align_resource_drop$pkg.test$owner".to_string(),
        representation_version: 1,
        drop_abi_fingerprint: *b"align-res-drop-1",
    });
    let reference = Ty::ResourceRef(0);
    resource_program.fns.push(body_test_parameter_function(
        "resource_ref_control",
        reference,
        hir::Block {
            stmts: Vec::new(),
            value: Some(Box::new(body_test_expr(hir::ExprKind::Unit, Ty::Unit))),
        },
        Ty::Unit,
    ));
    assert!(
        body_core_metadata_is_valid(&resource_program),
        "a standalone resource_ref parameter must remain producer-valid",
    );
    let locals = vec![
        body_test_local(0, "first", reference, false, true),
        body_test_local(1, "second", reference, false, true),
    ];
    resource_program.fns.push(body_test_function_with_params(
        "resource_ref_array_rejected",
        locals,
        vec![0, 1],
        hir::Block {
            stmts: vec![hir::Stmt::Expr(body_test_expr(
                hir::ExprKind::ArrayLit {
                    elems: vec![
                        body_test_expr(hir::ExprKind::Local(0), reference),
                        body_test_expr(hir::ExprKind::Local(1), reference),
                    ],
                    elem: reference,
                    pooled: false,
                },
                Ty::Array(Scalar::ResourceRef(0), 2),
            ))],
            value: Some(Box::new(body_test_expr(hir::ExprKind::Unit, Ty::Unit))),
        },
        Ty::Unit,
    ));
    assert!(
        !body_core_metadata_is_valid(&resource_program),
        "a resource_ref fixed array must fail closed at the HIR boundary",
    );

    let mut reject = program.clone();
    match &mut body_statement_expression_mut(&mut reject, "array_literal_case").kind {
        hir::ExprKind::ArrayLit { elem, .. } => *elem = Ty::Bool,
        _ => unreachable!(),
    }
    assert!(!body_core_metadata_is_valid(&reject));

    let mut reject = program.clone();
    let expression = body_statement_expression_mut(&mut reject, "array_literal_case");
    match &mut expression.kind {
        hir::ExprKind::ArrayLit { elem, .. } => *elem = Ty::Slice(scalar),
        _ => unreachable!(),
    }
    expression.ty = Ty::Array(Scalar::Slice(PrimScalar::Int(IntTy { bits: 64, signed: true })), 2);
    assert!(!body_core_metadata_is_valid(&reject));

    let mut reject = program.clone();
    match &mut body_statement_expression_mut(&mut reject, "const_array_case").kind {
        hir::ExprKind::ConstArray { len, .. } => *len = 3,
        _ => unreachable!(),
    }
    assert!(!body_core_metadata_is_valid(&reject));

    let mut reject = program.clone();
    match &mut body_statement_expression_mut(&mut reject, "array_zip_case").kind {
        hir::ExprKind::ArrayZip { tuple_id, .. } => *tuple_id = 99,
        _ => unreachable!(),
    }
    assert!(!body_core_metadata_is_valid(&reject));

    let mut reject = program.clone();
    match &mut body_statement_expression_mut(&mut reject, "select_case").kind {
        hir::ExprKind::Select { mask, .. } => mask.ty = Ty::Bool,
        _ => unreachable!(),
    }
    assert!(!body_core_metadata_is_valid(&reject));

    let mut reject = program.clone();
    match &mut body_statement_expression_mut(&mut reject, "vec_load_case").kind {
        hir::ExprKind::VecLoad { elem, .. } => *elem = Scalar::Float(FloatTy { bits: 64 }),
        _ => unreachable!(),
    }
    assert!(!body_core_metadata_is_valid(&reject));

    let mut reject = program.clone();
    reject
        .fns
        .iter_mut()
        .find(|function| function.name.as_str() == "vec_store_case")
        .expect("vec-store fixture")
        .locals[0]
        .is_mut = false;
    assert!(!body_core_metadata_is_valid(&reject));

    let mut reject = program.clone();
    match &mut body_statement_expression_mut(&mut reject, "vec_literal_case").kind {
        hir::ExprKind::VecLit { elems, .. } => {
            elems.pop();
        }
        _ => unreachable!(),
    }
    assert!(!body_core_metadata_is_valid(&reject));

    let mut reject = program.clone();
    reject
        .fns
        .iter_mut()
        .find(|function| function.name.as_str() == "body_test")
        .expect("pooled fixture")
        .locals[0]
        .is_mut = true;
    assert!(!body_core_metadata_is_valid(&reject));
}

#[test]
fn hir_body_validator_storage_vector_array_control_flow() {
    let integer = int(64);
    let scalar = scalar_int(64);
    let array = |value: i128| {
        body_test_expr(
            hir::ExprKind::ArrayLit {
                elems: vec![body_test_expr(hir::ExprKind::Int(value), integer)],
                elem: integer,
                pooled: false,
            },
            Ty::Array(scalar, 1),
        )
    };
    let mut valid = baseline_program();
    valid.fns.push(body_unit_case(
        "storage_branch_join",
        body_test_expr(
            hir::ExprKind::If {
                cond: Box::new(body_test_expr(hir::ExprKind::Bool(true), Ty::Bool)),
                then: hir::Block {
                    stmts: Vec::new(),
                    value: Some(Box::new(array(1))),
                },
                els: hir::Block {
                    stmts: Vec::new(),
                    value: Some(Box::new(array(2))),
                },
            },
            Ty::Array(scalar, 1),
        ),
    ));
    valid.fns.push(body_unit_case(
        "storage_loop_join",
        body_test_expr(
            hir::ExprKind::Loop {
                body: hir::Block {
                    stmts: vec![hir::Stmt::Break {
                        value: Some(array(3)),
                        accepted: true,
                    }],
                    value: None,
                },
                diverges: false,
                body_locals: 0..0,
            },
            Ty::Array(scalar, 1),
        ),
    ));
    assert!(body_core_metadata_is_valid(&valid));

    let diverging = body_test_expr(
        hir::ExprKind::Loop {
            body: hir::Block {
                stmts: Vec::new(),
                value: None,
            },
            diverges: true,
            body_locals: 0..0,
        },
        integer,
    );
    let malformed_retained = body_test_expr(
        hir::ExprKind::Int(0),
        Ty::Int(IntTy {
            bits: 0,
            signed: true,
        }),
    );
    let mut program = baseline_program();
    program.fns.push(body_unit_case(
        "storage_control_flow",
        body_test_expr(
            hir::ExprKind::ArrayLit {
                elems: vec![diverging, malformed_retained],
                elem: integer,
                pooled: false,
            },
            Ty::Array(scalar, 2),
        ),
    ));
    assert!(!body_core_metadata_is_valid(&program));
}

#[test]
fn hir_body_validator_pipeline_stage_records() {
    let integer = int(64);
    let scalar = scalar_int(64);
    let dyn_int = Ty::DynArray(scalar);
    let dyn_str = Ty::DynArray(Scalar::Str);
    let dyn_flags = Ty::DynStructArray(1, Layout::Aos);
    let mut program = baseline_program();
    program.structs.push(StructDef {
        name: "Flags".to_string(),
        source_name: "Flags".to_string(),
        fields: vec![
            FieldDef {
                name: "active".to_string(),
                ty: Ty::Bool,
            },
            FieldDef {
                name: "value".to_string(),
                ty: integer,
            },
        ],
        align: None,
        c_repr: false,
    });
    program.imported_fns.extend([
        imported_fn("dep$captured_map", vec![integer, integer], integer),
        imported_fn("dep$predicate", vec![integer], Ty::Bool),
    ]);

    let source_int = body_test_expr(hir::ExprKind::Local(0), dyn_int);
    program.fns.push(body_test_parameter_function(
        "pipeline_stage_scalar",
        dyn_int,
        hir::Block {
            stmts: Vec::new(),
            value: Some(Box::new(body_test_expr(
                hir::ExprKind::ArraySum {
                    source: Box::new(source_int),
                    stages: vec![
                        hir::Stage {
                            kind: hir::StageKind::Map {
                                func: "dep$captured_map".to_string(),
                                captures: vec![body_test_expr(
                                    hir::ExprKind::Int(2),
                                    integer,
                                )],
                            },
                            out_ty: integer,
                        },
                        hir::Stage {
                            kind: hir::StageKind::Where {
                                func: "dep$predicate".to_string(),
                                captures: Vec::new(),
                            },
                            out_ty: integer,
                        },
                    ],
                },
                integer,
            ))),
        },
        integer,
    ));

    program.fns.push(body_test_parameter_function(
        "pipeline_stage_string",
        dyn_str,
        hir::Block {
            stmts: Vec::new(),
            value: Some(Box::new(body_test_expr(
                hir::ExprKind::ArrayCount {
                    source: Box::new(body_test_expr(hir::ExprKind::Local(0), dyn_str)),
                    stages: vec![hir::Stage {
                        kind: hir::StageKind::WhereStrContains {
                            needle: body_test_expr(
                                hir::ExprKind::Str("needle".to_string()),
                                Ty::Str,
                            ),
                        },
                        out_ty: Ty::Str,
                    }],
                },
                integer,
            ))),
        },
        integer,
    ));

    program.fns.push(body_test_parameter_function(
        "pipeline_stage_fields",
        dyn_flags,
        hir::Block {
            stmts: Vec::new(),
            value: Some(Box::new(body_test_expr(
                hir::ExprKind::ArraySum {
                    source: Box::new(body_test_expr(hir::ExprKind::Local(0), dyn_flags)),
                    stages: vec![
                        hir::Stage {
                            kind: hir::StageKind::WhereField { field: 0 },
                            out_ty: Ty::Struct(1),
                        },
                        hir::Stage {
                            kind: hir::StageKind::Project { field: 1 },
                            out_ty: integer,
                        },
                    ],
                },
                integer,
            ))),
        },
        integer,
    ));
    assert!(body_core_metadata_is_valid(&program));

    let mut reject = program.clone();
    let expression = body_value_expression_mut(&mut reject, "pipeline_stage_scalar");
    let hir::ExprKind::ArraySum { stages, .. } = &mut expression.kind else {
        panic!("scalar pipeline fixture lost its terminal")
    };
    stages[0].out_ty = Ty::Bool;
    assert!(!body_core_metadata_is_valid(&reject));

    let mut reject = program.clone();
    let expression = body_value_expression_mut(&mut reject, "pipeline_stage_scalar");
    let hir::ExprKind::ArraySum { stages, .. } = &mut expression.kind else {
        panic!("scalar pipeline fixture lost its terminal")
    };
    stages[0].kind = hir::StageKind::Map {
        func: "".to_string(),
        captures: Vec::new(),
    };
    assert!(!body_core_metadata_is_valid(&reject));

    let mut reject = program.clone();
    let expression = body_value_expression_mut(&mut reject, "pipeline_stage_fields");
    let hir::ExprKind::ArraySum { stages, .. } = &mut expression.kind else {
        panic!("field pipeline fixture lost its terminal")
    };
    stages[0].kind = hir::StageKind::WhereField { field: 1 };
    assert!(!body_core_metadata_is_valid(&reject));

    let mut reject = program.clone();
    let expression = body_value_expression_mut(&mut reject, "pipeline_stage_string");
    let hir::ExprKind::ArrayCount { stages, .. } = &mut expression.kind else {
        panic!("string pipeline fixture lost its terminal")
    };
    stages[0].kind = hir::StageKind::WhereStrContains {
        needle: body_test_expr(hir::ExprKind::Int(1), integer),
    };
    assert!(!body_core_metadata_is_valid(&reject));
}

#[test]
fn hir_body_validator_pipeline_terminals() {
    let integer = int(64);
    let scalar = scalar_int(64);
    let dyn_int = Ty::DynArray(scalar);
    let mut program = baseline_program();
    program.imported_fns.extend([
        imported_fn("dep$terminal_pred", vec![integer], Ty::Bool),
        imported_fn("dep$terminal_fold", vec![integer, integer], integer),
        imported_fn("dep$terminal_key", vec![integer], integer),
        imported_fn("dep$terminal_map", vec![integer], integer),
    ]);
    program.tuples.push(TupleDef {
        elems: vec![Scalar::DynArray(PrimScalar::Int(align_sema::IntTy {
            bits: 64,
            signed: true,
        })); 2],
    });
    let partition_tuple = Ty::Tuple(1);
    let local_source = || body_test_expr(hir::ExprKind::Local(0), dyn_int);
    let add_pipeline = |program: &mut hir::Program,
                        name: &str,
                        expression: hir::Expr,
                        ret: Ty| {
        program.fns.push(body_test_parameter_function(
            name,
            dyn_int,
            hir::Block {
                stmts: Vec::new(),
                value: Some(Box::new(expression)),
            },
            ret,
        ));
    };
    add_pipeline(
        &mut program,
        "pipeline_sum",
        body_test_expr(
            hir::ExprKind::ArraySum {
                source: Box::new(local_source()),
                stages: Vec::new(),
            },
            integer,
        ),
        integer,
    );
    add_pipeline(
        &mut program,
        "pipeline_count",
        body_test_expr(
            hir::ExprKind::ArrayCount {
                source: Box::new(local_source()),
                stages: Vec::new(),
            },
            integer,
        ),
        integer,
    );
    add_pipeline(
        &mut program,
        "pipeline_any",
        body_test_expr(
            hir::ExprKind::ArrayAnyAll {
                source: Box::new(local_source()),
                stages: Vec::new(),
                func: "dep$terminal_pred".to_string(),
                captures: Vec::new(),
                all: false,
            },
            Ty::Bool,
        ),
        Ty::Bool,
    );
    add_pipeline(
        &mut program,
        "pipeline_minmax",
        body_test_expr(
            hir::ExprKind::ArrayMinMax {
                source: Box::new(local_source()),
                stages: Vec::new(),
                is_max: true,
            },
            integer,
        ),
        integer,
    );
    add_pipeline(
        &mut program,
        "pipeline_reduce",
        body_test_expr(
            hir::ExprKind::ArrayReduce {
                source: Box::new(local_source()),
                stages: Vec::new(),
                func: "dep$terminal_fold".to_string(),
                captures: Vec::new(),
                init: Box::new(body_test_expr(hir::ExprKind::Int(0), integer)),
            },
            integer,
        ),
        integer,
    );
    add_pipeline(
        &mut program,
        "pipeline_scan",
        body_test_expr(
            hir::ExprKind::ArrayScan {
                source: Box::new(local_source()),
                stages: Vec::new(),
                func: "dep$terminal_fold".to_string(),
                captures: Vec::new(),
                init: Box::new(body_test_expr(hir::ExprKind::Int(0), integer)),
                elem: integer,
            },
            Ty::DynArray(scalar),
        ),
        Ty::DynArray(scalar),
    );
    add_pipeline(
        &mut program,
        "pipeline_sort",
        body_test_expr(
            hir::ExprKind::ArraySort {
                source: Box::new(local_source()),
                stages: Vec::new(),
                elem: integer,
            },
            dyn_int,
        ),
        dyn_int,
    );
    add_pipeline(
        &mut program,
        "pipeline_sort_by",
        body_test_expr(
            hir::ExprKind::ArraySortBy {
                source: Box::new(local_source()),
                stages: Vec::new(),
                key_func: "dep$terminal_key".to_string(),
                captures: Vec::new(),
                key_ty: integer,
                elem: integer,
            },
            dyn_int,
        ),
        dyn_int,
    );
    add_pipeline(
        &mut program,
        "pipeline_to_array",
        body_test_expr(
            hir::ExprKind::ArrayToArray {
                source: Box::new(local_source()),
                stages: Vec::new(),
                elem: integer,
            },
            dyn_int,
        ),
        dyn_int,
    );
    add_pipeline(
        &mut program,
        "pipeline_partition",
        body_test_expr(
            hir::ExprKind::ArrayPartition {
                source: Box::new(local_source()),
                stages: Vec::new(),
                func: "dep$terminal_pred".to_string(),
                captures: Vec::new(),
                elem: integer,
            },
            partition_tuple,
        ),
        partition_tuple,
    );
    add_pipeline(
        &mut program,
        "pipeline_par_map",
        body_test_expr(
            hir::ExprKind::ArrayParMap {
                source: Box::new(local_source()),
                stages: Vec::new(),
                func: "dep$terminal_map".to_string(),
                captures: Vec::new(),
                elem: integer,
            },
            dyn_int,
        ),
        dyn_int,
    );

    let array = |value: i128| {
        body_test_expr(
            hir::ExprKind::ArrayLit {
                elems: vec![
                    body_test_expr(hir::ExprKind::Int(value), integer),
                    body_test_expr(hir::ExprKind::Int(value + 1), integer),
                ],
                elem: integer,
                pooled: false,
            },
            Ty::Array(scalar, 2),
        )
    };
    program.fns.push(body_tail_case(
        "pipeline_dot",
        body_test_expr(
            hir::ExprKind::ArrayDot {
                a: Box::new(array(1)),
                b: Box::new(array(3)),
                elem: integer,
            },
            integer,
        ),
        integer,
    ));
    let bool_array = body_test_expr(
        hir::ExprKind::ArrayLit {
            elems: vec![
                body_test_expr(hir::ExprKind::Bool(true), Ty::Bool),
                body_test_expr(hir::ExprKind::Bool(false), Ty::Bool),
            ],
            elem: Ty::Bool,
            pooled: false,
        },
        Ty::Array(Scalar::Bool, 2),
    );
    program.fns.push(body_tail_case(
        "pipeline_zip_count",
        body_test_expr(
            hir::ExprKind::ArrayCount {
                source: Box::new(body_test_expr(
                    hir::ExprKind::ArrayZip {
                        sources: vec![array(1), bool_array],
                        tuple_id: 0,
                    },
                    Ty::Tuple(0),
                )),
                stages: Vec::new(),
            },
            integer,
        ),
        integer,
    ));

    let map_into_source = body_test_expr(hir::ExprKind::Local(0), dyn_int);
    program.fns.push(body_test_function_with_params(
        "pipeline_map_into",
        vec![
            body_test_local(0, "source", dyn_int, false, true),
            body_test_local(1, "destination", Ty::Slice(scalar), true, false),
        ],
        vec![0],
        hir::Block {
            stmts: Vec::new(),
            value: Some(Box::new(body_test_expr(
                hir::ExprKind::ArrayMapInto {
                    source: Box::new(map_into_source),
                    stages: vec![hir::Stage {
                        kind: hir::StageKind::Map {
                            func: "dep$terminal_map".to_string(),
                            captures: Vec::new(),
                        },
                        out_ty: integer,
                    }],
                    dst: Box::new(body_test_expr(
                        hir::ExprKind::Local(1),
                        Ty::Slice(scalar),
                    )),
                    elem: integer,
                },
                Ty::Unit,
            ))),
        },
        Ty::Unit,
    ));

    program.fns.push(body_test_parameter_function(
        "pipeline_to_soa",
        Ty::DynStructArray(0, Layout::Aos),
        hir::Block {
            stmts: Vec::new(),
            value: Some(Box::new(body_test_expr(
                hir::ExprKind::Arena(hir::Block {
                    stmts: Vec::new(),
                    value: Some(Box::new(body_test_expr(
                        hir::ExprKind::ArrayToSoa {
                            source: Box::new(body_test_expr(
                                hir::ExprKind::Local(0),
                                Ty::DynStructArray(0, Layout::Aos),
                            )),
                            struct_id: 0,
                        },
                        Ty::Soa(0),
                    ))),
                }),
                Ty::Soa(0),
            ))),
        },
        Ty::Soa(0),
    ));
    assert!(body_core_metadata_is_valid(&program));

    let mut reject = program.clone();
    let expression = body_value_expression_mut(&mut reject, "pipeline_any");
    let hir::ExprKind::ArrayAnyAll { func, .. } = &mut expression.kind else {
        panic!("any pipeline fixture lost its terminal")
    };
    *func = "".to_string();
    assert!(!body_core_metadata_is_valid(&reject));

    let mut reject = program.clone();
    let expression = body_value_expression_mut(&mut reject, "pipeline_reduce");
    let hir::ExprKind::ArrayReduce { init, .. } = &mut expression.kind else {
        panic!("reduce pipeline fixture lost its terminal")
    };
    init.ty = Ty::Bool;
    assert!(!body_core_metadata_is_valid(&reject));

    let mut reject = program.clone();
    let expression = body_value_expression_mut(&mut reject, "pipeline_sort_by");
    let hir::ExprKind::ArraySortBy { key_ty, .. } = &mut expression.kind else {
        panic!("sort-by pipeline fixture lost its terminal")
    };
    *key_ty = Ty::Bool;
    assert!(!body_core_metadata_is_valid(&reject));

    let mut reject = program.clone();
    reject
        .fns
        .iter_mut()
        .find(|function| function.name.as_str() == "pipeline_map_into")
        .expect("map-into fixture")
        .locals[1]
        .is_mut = false;
    assert!(!body_core_metadata_is_valid(&reject));

    let mut reject = program.clone();
    let expression = body_value_expression_mut(&mut reject, "pipeline_to_soa");
    let hir::ExprKind::Arena(block) = &mut expression.kind else {
        panic!("to-soa fixture lost its arena")
    };
    let value = block.value.as_deref_mut().expect("to-soa value");
    let hir::ExprKind::ArrayToSoa { source, .. } = &mut value.kind else {
        panic!("to-soa fixture lost its transpose")
    };
    source.kind = hir::ExprKind::Local(0);
    source.ty = Ty::Soa(0);
    assert!(!body_core_metadata_is_valid(&reject));

    let mut reject = program.clone();
    let expression = body_value_expression_mut(&mut reject, "pipeline_to_soa");
    let hir::ExprKind::Arena(block) = &mut expression.kind else {
        panic!("to-soa fixture lost its arena")
    };
    let value = block.value.as_deref_mut().expect("to-soa value");
    let hir::ExprKind::ArrayToSoa { source, .. } = &mut value.kind else {
        panic!("to-soa fixture lost its transpose")
    };
    **source = body_test_expr(
        hir::ExprKind::If {
            cond: Box::new(body_test_expr(hir::ExprKind::Bool(true), Ty::Bool)),
            then: hir::Block {
                stmts: Vec::new(),
                value: Some(Box::new(body_test_expr(
                    hir::ExprKind::Local(0),
                    Ty::DynStructArray(0, Layout::Aos),
                ))),
            },
            els: hir::Block {
                stmts: Vec::new(),
                value: Some(Box::new(body_test_expr(
                    hir::ExprKind::Local(0),
                    Ty::DynStructArray(0, Layout::Aos),
                ))),
            },
        },
        Ty::DynStructArray(0, Layout::Aos),
    );
    assert!(!body_core_metadata_is_valid(&reject));

    let mut impure = program.clone();
    impure
        .imported_fns
        .iter_mut()
        .find(|function| function.name.as_str() == "dep$terminal_map")
        .expect("par-map callable")
        .effect = FnEffect::Impure;
    assert!(body_core_metadata_is_valid(&impure));

    let mut scanner = baseline_program();
    let scanner_ty = Ty::JsonScanner(0);
    scanner.fns.push(body_test_parameter_function(
        "pipeline_scanner_rejected",
        scanner_ty,
        hir::Block {
            stmts: Vec::new(),
            value: Some(Box::new(body_test_expr(
                hir::ExprKind::ArrayCount {
                    source: Box::new(body_test_expr(
                        hir::ExprKind::Local(0),
                        scanner_ty,
                    )),
                    stages: Vec::new(),
                },
                integer,
            ))),
        },
        integer,
    ));
    assert!(!body_core_metadata_is_valid(&scanner));
}

#[test]
fn hir_body_validator_pipeline_array_views() {
    let integer = int(64);
    let scalar = scalar_int(64);
    let dyn_int = Ty::DynArray(scalar);
    let dyn_record = Ty::DynStructArray(0, Layout::Aos);
    let mut program = baseline_program();
    let move_struct = program.structs.len() as u32;
    program.structs.push(StructDef {
        name: "MoveRecord".to_string(),
        source_name: "MoveRecord".to_string(),
        fields: vec![FieldDef {
            name: "owned".to_string(),
            ty: Ty::String,
        }],
        align: None,
        c_repr: false,
    });
    let add = |program: &mut hir::Program,
               name: &str,
               expression: hir::Expr,
               ret: Ty,
               source_ty: Ty| {
        program.fns.push(body_test_parameter_function(
            name,
            source_ty,
            hir::Block {
                stmts: Vec::new(),
                value: Some(Box::new(expression)),
            },
            ret,
        ));
    };
    add(
        &mut program,
        "array_view_to_slice",
        body_test_expr(
            hir::ExprKind::ArrayToSlice(Box::new(body_test_expr(
                hir::ExprKind::Local(0),
                dyn_int,
            ))),
            Ty::Slice(scalar),
        ),
        Ty::Slice(scalar),
        dyn_int,
    );
    program.fns.push(body_test_function(
        hir::Block {
            stmts: Vec::new(),
            value: Some(Box::new(body_test_expr(
                hir::ExprKind::ArrayToSlice(Box::new(body_test_expr(
                    hir::ExprKind::Local(0),
                    Ty::Array(scalar, 2),
                ))),
                Ty::Slice(scalar),
            ))),
        },
        vec![body_test_local(0, "array", Ty::Array(scalar, 2), false, false)],
        Ty::Slice(scalar),
    ));
    add(
        &mut program,
        "array_view_slice_len",
        body_test_expr(
            hir::ExprKind::Len(Box::new(body_test_expr(
                hir::ExprKind::Local(0),
                Ty::Slice(scalar),
            ))),
            integer,
        ),
        integer,
        Ty::Slice(scalar),
    );
    add(
        &mut program,
        "array_view_soa_len",
        body_test_expr(
            hir::ExprKind::Len(Box::new(body_test_expr(
                hir::ExprKind::Local(0),
                Ty::Soa(0),
            ))),
            integer,
        ),
        integer,
        Ty::Soa(0),
    );
    add(
        &mut program,
        "array_view_len",
        body_test_expr(
            hir::ExprKind::Len(Box::new(body_test_expr(
                hir::ExprKind::Local(0),
                dyn_int,
            ))),
            integer,
        ),
        integer,
        dyn_int,
    );
    add(
        &mut program,
        "array_view_index",
        body_test_expr(
            hir::ExprKind::Index {
                recv: Box::new(body_test_expr(hir::ExprKind::Local(0), dyn_int)),
                index: Box::new(body_test_expr(hir::ExprKind::Int(0), integer)),
            },
            integer,
        ),
        integer,
        dyn_int,
    );
    add(
        &mut program,
        "array_view_range",
        body_test_expr(
            hir::ExprKind::SliceRange {
                recv: Box::new(body_test_expr(hir::ExprKind::Local(0), dyn_int)),
                start: Some(Box::new(body_test_expr(hir::ExprKind::Int(0), integer))),
                end: Some(Box::new(body_test_expr(hir::ExprKind::Int(1), integer))),
            },
            Ty::Slice(scalar),
        ),
        Ty::Slice(scalar),
        dyn_int,
    );
    add(
        &mut program,
        "array_view_elem_field",
        body_test_expr(
            hir::ExprKind::ElemField {
                recv: Box::new(body_test_expr(hir::ExprKind::Local(0), dyn_record)),
                index: Box::new(body_test_expr(hir::ExprKind::Int(0), integer)),
                path: vec![1],
                struct_id: 0,
            },
            integer,
        ),
        integer,
        dyn_record,
    );
    let chunks = body_test_expr(
        hir::ExprKind::ArrayChunks {
            source: Box::new(body_test_expr(hir::ExprKind::Local(0), dyn_int)),
            n: Box::new(body_test_expr(hir::ExprKind::Int(2), integer)),
            elem: integer,
        },
        Ty::DynSliceArray(PrimScalar::Int(align_sema::IntTy {
            bits: 64,
            signed: true,
        })),
    );
    program.fns.push(body_test_parameter_function(
        "array_view_chunks",
        dyn_int,
        hir::Block {
            stmts: vec![hir::Stmt::Expr(chunks)],
            value: Some(Box::new(body_test_expr(hir::ExprKind::Unit, Ty::Unit))),
        },
        Ty::Unit,
    ));
    assert!(body_core_metadata_is_valid(&program));

    for (name, source_ty, element) in [
        (
            "array_view_fixed_move_string",
            Ty::Array(Scalar::String, 2),
            Scalar::String,
        ),
        (
            "array_view_dynamic_move_string",
            Ty::DynArray(Scalar::String),
            Scalar::String,
        ),
        (
            "array_view_fixed_move_struct",
            Ty::StructArray(move_struct, 2),
            Scalar::Struct(move_struct),
        ),
        (
            "array_view_dynamic_move_struct",
            Ty::DynStructArray(move_struct, Layout::Aos),
            Scalar::Struct(move_struct),
        ),
    ] {
        let view = body_test_expr(
            hir::ExprKind::ArrayToSlice(Box::new(body_test_expr(
                hir::ExprKind::Local(0),
                source_ty,
            ))),
            Ty::Slice(element),
        );
        let mut reject = program.clone();
        reject.fns.push(body_test_parameter_function(
            name,
            source_ty,
            hir::Block {
                stmts: vec![hir::Stmt::Expr(view)],
                value: Some(Box::new(body_test_expr(hir::ExprKind::Unit, Ty::Unit))),
            },
            Ty::Unit,
        ));
        assert!(
            !body_core_metadata_is_valid(&reject),
            "{name}: array-to-slice must reject Move-element arrays"
        );
    }

    let mut reject = program.clone();
    let expression = body_value_expression_mut(&mut reject, "array_view_index");
    let hir::ExprKind::Index { index, .. } = &mut expression.kind else {
        panic!("index fixture lost its node")
    };
    index.ty = Ty::Bool;
    assert!(!body_core_metadata_is_valid(&reject));

    let mut reject = program.clone();
    let expression = body_value_expression_mut(&mut reject, "array_view_range");
    let hir::ExprKind::SliceRange { end, .. } = &mut expression.kind else {
        panic!("range fixture lost its node")
    };
    end.as_deref_mut().expect("range end").ty = Ty::Bool;
    assert!(!body_core_metadata_is_valid(&reject));

    let mut reject = program.clone();
    let expression = body_value_expression_mut(&mut reject, "array_view_elem_field");
    let hir::ExprKind::ElemField { path, .. } = &mut expression.kind else {
        panic!("element-field fixture lost its node")
    };
    *path = vec![9];
    assert!(!body_core_metadata_is_valid(&reject));

    let mut reject = program.clone();
    let expression = body_statement_expression_mut(&mut reject, "array_view_chunks");
    let hir::ExprKind::ArrayChunks { n, .. } = &mut expression.kind else {
        panic!("chunks fixture lost its node")
    };
    n.ty = Ty::Bool;
    assert!(!body_core_metadata_is_valid(&reject));
}

#[test]
fn hir_body_validator_pipeline_control_flow() {
    let integer = int(64);
    let dyn_int = Ty::DynArray(scalar_int(64));
    let mut program = baseline_program();
    program.imported_fns.push(imported_fn(
        "dep$control_map",
        vec![integer, integer],
        integer,
    ));
    let source = body_test_expr(
        hir::ExprKind::If {
            cond: Box::new(body_test_expr(hir::ExprKind::Bool(true), Ty::Bool)),
            then: hir::Block {
                stmts: Vec::new(),
                value: Some(Box::new(body_test_expr(
                    hir::ExprKind::Local(0),
                    dyn_int,
                ))),
            },
            els: hir::Block {
                stmts: Vec::new(),
                value: Some(Box::new(body_test_expr(
                    hir::ExprKind::Local(0),
                    dyn_int,
                ))),
            },
        },
        dyn_int,
    );
    program.fns.push(body_test_parameter_function(
        "pipeline_if_source",
        dyn_int,
        hir::Block {
            stmts: Vec::new(),
            value: Some(Box::new(body_test_expr(
                hir::ExprKind::ArrayCount {
                    source: Box::new(source),
                    stages: Vec::new(),
                },
                integer,
            ))),
        },
        integer,
    ));
    assert!(body_core_metadata_is_valid(&program));

    let malformed_capture = body_test_expr(
        hir::ExprKind::Int(0),
        Ty::Int(IntTy {
            bits: 0,
            signed: true,
        }),
    );
    let diverging_source = body_test_expr(
        hir::ExprKind::Loop {
            body: hir::Block {
                stmts: Vec::new(),
                value: None,
            },
            diverges: true,
            body_locals: 0..0,
        },
        dyn_int,
    );
    let mut reject = baseline_program();
    reject.imported_fns.push(imported_fn(
        "dep$control_map",
        vec![integer, integer],
        integer,
    ));
    reject.fns.push(body_test_parameter_function(
        "pipeline_retained_child",
        dyn_int,
        hir::Block {
            stmts: Vec::new(),
            value: Some(Box::new(body_test_expr(
                hir::ExprKind::ArraySum {
                    source: Box::new(diverging_source),
                    stages: vec![hir::Stage {
                        kind: hir::StageKind::Map {
                            func: "dep$control_map".to_string(),
                            captures: vec![malformed_capture],
                        },
                        out_ty: integer,
                    }],
                },
                integer,
            ))),
        },
        integer,
    ));
    assert!(!body_core_metadata_is_valid(&reject));
}

#[test]
fn deep_hir_body_pipeline_type_dag_is_stack_bounded() {
    let program = with_stage_body_depth(align_sema::MAX_CHECKED_HIR_DEPTH);
    let handle = std::thread::Builder::new()
        .stack_size(2 * 1024 * 1024)
        .spawn(move || assert!(body_core_metadata_is_valid(&program)))
        .expect("spawn deep pipeline body validator");
    handle.join().expect("join deep pipeline body validator");
}

#[test]
fn hir_body_validator_pipeline_deferred_b2b2() {
    let integer = int(64);
    let scalar = scalar_int(64);
    let string = body_test_expr(hir::ExprKind::Str("x".to_string()), Ty::Str);
    let number = body_test_expr(hir::ExprKind::Int(1), integer);
    let unit = body_test_expr(hir::ExprKind::Unit, Ty::Unit);
    let template_parts = vec![
        hir::TemplatePart::Text("x".to_string()),
        hir::TemplatePart::Hole(number.clone()),
        hir::TemplatePart::JsonStr(string.clone()),
        hir::TemplatePart::OptionField {
            access: unit.clone(),
            name: "x".to_string(),
        },
        hir::TemplatePart::OptionStructField {
            access: unit.clone(),
            name: "record".to_string(),
            struct_id: 0,
        },
        hir::TemplatePart::PopComma,
        hir::TemplatePart::StructArrayField {
            access: unit.clone(),
            struct_id: 0,
        },
        hir::TemplatePart::ScalarArrayField {
            access: unit.clone(),
            elem: scalar,
        },
        hir::TemplatePart::UnionValue {
            access: unit.clone(),
            enum_id: 0,
        },
    ];
    let mut cases = vec![hir::ExprKind::Template(template_parts)];
    cases.extend([
        hir::ExprKind::JsonDecode {
            struct_id: 0,
            input: Box::new(string.clone()),
        },
        hir::ExprKind::JsonDecodeArray {
            elem: integer,
            input: Box::new(string.clone()),
        },
        hir::ExprKind::JsonDecodeScalar {
            scalar: integer,
            input: Box::new(string.clone()),
        },
        hir::ExprKind::JsonDecodeStructArray {
            struct_id: 0,
            input: Box::new(string.clone()),
        },
        hir::ExprKind::JsonDecodeSoa {
            struct_id: 0,
            input: Box::new(string.clone()),
        },
        hir::ExprKind::JsonDecodeUnion {
            enum_id: 0,
            input: Box::new(string.clone()),
        },
        hir::ExprKind::JsonDoc {
            input: Box::new(string.clone()),
        },
        hir::ExprKind::JsonDocKind {
            doc: Box::new(unit.clone()),
        },
        hir::ExprKind::JsonDocGet {
            doc: Box::new(unit.clone()),
            key: Box::new(string.clone()),
        },
        hir::ExprKind::JsonDocAt {
            doc: Box::new(unit.clone()),
            index: Box::new(number.clone()),
        },
        hir::ExprKind::JsonDocAsStr {
            doc: Box::new(unit.clone()),
        },
        hir::ExprKind::JsonDocAsScalar {
            doc: Box::new(unit.clone()),
            scalar: integer,
        },
        hir::ExprKind::JsonDocLen {
            doc: Box::new(unit.clone()),
        },
        hir::ExprKind::JsonDocKey {
            doc: Box::new(unit.clone()),
            index: Box::new(number.clone()),
        },
        hir::ExprKind::JsonDocElems {
            doc: Box::new(unit.clone()),
        },
        hir::ExprKind::JsonScan {
            struct_id: 0,
            input: Box::new(string.clone()),
        },
    ]);
    for source in [
        hir::GroupSource::SoaI64,
        hir::GroupSource::SoaStr,
        hir::GroupSource::AosStr,
        hir::GroupSource::Encoded,
    ] {
        for value_field in [None, Some(1)] {
            for op in [
                hir::GroupOp::Sum,
                hir::GroupOp::Min,
                hir::GroupOp::Max,
                hir::GroupOp::Count,
            ] {
                cases.push(hir::ExprKind::ArrayGroupAgg {
                    base: 0,
                    struct_id: 0,
                    key_field: 0,
                    value_field,
                    op,
                    source,
                });
                cases.push(hir::ExprKind::ArrayGroupAggMulti {
                    base: 0,
                    struct_id: 0,
                    key_field: 0,
                    aggs: vec![hir::GroupAgg1 { op, value_field }],
                    source,
                });
            }
        }
    }
    cases.push(hir::ExprKind::ArrayDictEncode {
        base: 0,
        struct_id: 0,
        key_field: 0,
    });

    for (index, kind) in cases.into_iter().enumerate() {
        let mut program = baseline_program();
        program.fns.push(body_tail_case(
            &format!("deferred_b2b2_{index}"),
            body_test_expr(kind, Ty::Unit),
            Ty::Unit,
        ));
        assert!(
            !body_core_metadata_is_valid(&program),
            "deferred b2b2 discriminator {index} was accepted"
        );
    }
}

#[test]
fn hir_body_validator_pipeline_template_json_group() {
    let integer = int(64);
    let scalar_integer = scalar_int(64);
    let mut program = baseline_program();
    let error_id = push_builtin_error(&mut program);
    let kind_id = push_builtin_json_kind(&mut program);
    let union_id = program.enums.len() as u32;
    program.enums.push(EnumDef {
        name: "JsonValue".to_string(),
        source_name: "JsonValue".to_string(),
        variants: vec![
            EnumVariant {
                name: "Text".to_string(),
                payload: vec![Scalar::Str],
                field_base: 1,
            },
            EnumVariant {
                name: "Number".to_string(),
                payload: vec![scalar_integer],
                field_base: 2,
            },
            EnumVariant {
                name: "Flag".to_string(),
                payload: vec![Scalar::Bool],
                field_base: 3,
            },
            EnumVariant {
                name: "Object".to_string(),
                payload: vec![Scalar::Struct(0)],
                field_base: 4,
            },
            EnumVariant {
                name: "Array".to_string(),
                payload: vec![Scalar::DynStructArray(0)],
                field_base: 5,
            },
        ],
    });
    let array_i64 = align_sema::ty_to_scalar(Ty::DynArray(scalar_integer)).unwrap();
    let array_str = align_sema::ty_to_scalar(Ty::DynArray(Scalar::Str)).unwrap();
    let tuple_i64_id = program.tuples.len() as u32;
    program.tuples.push(TupleDef {
        elems: vec![array_i64, array_i64],
    });
    let tuple_str_id = program.tuples.len() as u32;
    program.tuples.push(TupleDef {
        elems: vec![array_str, array_i64],
    });
    let tuple_multi_id = program.tuples.len() as u32;
    program.tuples.push(TupleDef {
        elems: vec![array_str, array_i64, array_i64],
    });
    program
        .imported_fns
        .push(imported_fn("scan$predicate", vec![integer], Ty::Bool));
    program
        .imported_fns
        .push(imported_fn("scan$reduce", vec![integer, integer], integer));

    let local = |id: u32, name: &str, ty: Ty| body_test_local(id, name, ty, false, false);
    let add_tail = |program: &mut hir::Program,
                    name: &str,
                    locals: Vec<hir::Local>,
                    expression: hir::Expr,
                    ret: Ty| {
        program.fns.push(body_test_named_function(
            name,
            hir::Block {
                stmts: Vec::new(),
                value: Some(Box::new(expression)),
            },
            locals,
            ret,
        ));
    };
    let arena = |value: hir::Expr, ty: Ty| {
        body_test_expr(
            hir::ExprKind::Arena(hir::Block {
                stmts: Vec::new(),
                value: Some(Box::new(value)),
            }),
            ty,
        )
    };
    let result_i64 = Ty::Result(scalar_integer, Scalar::Enum(error_id));
    let result_record = Ty::Result(Scalar::Struct(0), Scalar::Enum(error_id));
    let result_array = Ty::Result(Scalar::DynArray(PrimScalar::Int(IntTy { bits: 64, signed: true })), Scalar::Enum(error_id));
    let result_record_array = Ty::Result(Scalar::DynStructArray(0), Scalar::Enum(error_id));
    let result_union = Ty::Result(Scalar::Enum(union_id), Scalar::Enum(error_id));

    add_tail(
        &mut program,
        "b2b2_template",
        vec![
            local(0, "optional_value", Ty::Option(scalar_integer)),
            local(1, "optional_record", Ty::Option(Scalar::Struct(0))),
            local(2, "records", Ty::DynStructArray(0, Layout::Aos)),
            local(3, "values", Ty::DynArray(scalar_integer)),
            local(4, "union_value", Ty::Enum(union_id)),
            local(5, "hole", integer),
        ],
        body_test_expr(
            hir::ExprKind::Template(vec![
                hir::TemplatePart::Text("{".to_string()),
                hir::TemplatePart::OptionField {
                    access: body_test_expr(hir::ExprKind::Local(0), Ty::Option(scalar_integer)),
                    name: "value".to_string(),
                },
                hir::TemplatePart::Text("{".to_string()),
                hir::TemplatePart::OptionStructField {
                    access: body_test_expr(
                        hir::ExprKind::Local(1),
                        Ty::Option(Scalar::Struct(0)),
                    ),
                    name: "record".to_string(),
                    struct_id: 0,
                },
                hir::TemplatePart::PopComma,
                hir::TemplatePart::Text("}".to_string()),
                hir::TemplatePart::PopComma,
                hir::TemplatePart::Text("}".to_string()),
                hir::TemplatePart::StructArrayField {
                    access: body_test_expr(
                        hir::ExprKind::Local(2),
                        Ty::DynStructArray(0, Layout::Aos),
                    ),
                    struct_id: 0,
                },
                hir::TemplatePart::ScalarArrayField {
                    access: body_test_expr(hir::ExprKind::Local(3), Ty::DynArray(scalar_integer)),
                    elem: scalar_integer,
                },
                hir::TemplatePart::UnionValue {
                    access: body_test_expr(hir::ExprKind::Local(4), Ty::Enum(union_id)),
                    enum_id: union_id,
                },
                hir::TemplatePart::Hole(body_test_expr(hir::ExprKind::Local(5), integer)),
            ]),
            Ty::Str,
        ),
        Ty::Str,
    );
    add_tail(
        &mut program,
        "b2b2_decode_struct",
        Vec::new(),
        body_test_expr(
            hir::ExprKind::JsonDecode {
                struct_id: 0,
                input: Box::new(body_test_expr(hir::ExprKind::Str("{}".to_string()), Ty::Str)),
            },
            result_record,
        ),
        result_record,
    );
    add_tail(
        &mut program,
        "b2b2_decode_array",
        Vec::new(),
        body_test_expr(
            hir::ExprKind::JsonDecodeArray {
                elem: integer,
                input: Box::new(body_test_expr(hir::ExprKind::Str("[]".to_string()), Ty::Str)),
            },
            result_array,
        ),
        result_array,
    );
    add_tail(
        &mut program,
        "b2b2_decode_scalar",
        Vec::new(),
        body_test_expr(
            hir::ExprKind::JsonDecodeScalar {
                scalar: integer,
                input: Box::new(body_test_expr(hir::ExprKind::Str("1".to_string()), Ty::Str)),
            },
            result_i64,
        ),
        result_i64,
    );
    add_tail(
        &mut program,
        "b2b2_decode_struct_array",
        Vec::new(),
        body_test_expr(
            hir::ExprKind::JsonDecodeStructArray {
                struct_id: 0,
                input: Box::new(body_test_expr(hir::ExprKind::Str("[]".to_string()), Ty::Str)),
            },
            result_record_array,
        ),
        result_record_array,
    );
    add_tail(
        &mut program,
        "b2b2_decode_soa",
        Vec::new(),
        arena(
            body_test_expr(
                hir::ExprKind::JsonDecodeSoa {
                    struct_id: 0,
                    input: Box::new(body_test_expr(hir::ExprKind::Str("[]".to_string()), Ty::Str)),
                },
                Ty::Result(Scalar::Soa(0), Scalar::Enum(error_id)),
            ),
            Ty::Result(Scalar::Soa(0), Scalar::Enum(error_id)),
        ),
        Ty::Result(Scalar::Soa(0), Scalar::Enum(error_id)),
    );
    add_tail(
        &mut program,
        "b2b2_decode_union",
        Vec::new(),
        body_test_expr(
            hir::ExprKind::JsonDecodeUnion {
                enum_id: union_id,
                input: Box::new(body_test_expr(hir::ExprKind::Str("1".to_string()), Ty::Str)),
            },
            result_union,
        ),
        result_union,
    );
    add_tail(
        &mut program,
        "b2b2_json_doc",
        Vec::new(),
        arena(
            body_test_expr(
                hir::ExprKind::JsonDoc {
                    input: Box::new(body_test_expr(hir::ExprKind::Str("{}".to_string()), Ty::Str)),
                },
                Ty::Result(Scalar::JsonDoc, Scalar::Enum(error_id)),
            ),
            Ty::Result(Scalar::JsonDoc, Scalar::Enum(error_id)),
        ),
        Ty::Result(Scalar::JsonDoc, Scalar::Enum(error_id)),
    );
    let doc_local = || body_test_expr(hir::ExprKind::Local(0), Ty::JsonDoc);
    add_tail(
        &mut program,
        "b2b2_json_doc_kind",
        vec![local(0, "doc", Ty::JsonDoc)],
        body_test_expr(hir::ExprKind::JsonDocKind { doc: Box::new(doc_local()) }, Ty::Enum(kind_id)),
        Ty::Enum(kind_id),
    );
    add_tail(
        &mut program,
        "b2b2_json_doc_get",
        vec![local(0, "doc", Ty::JsonDoc)],
        body_test_expr(
            hir::ExprKind::JsonDocGet {
                doc: Box::new(doc_local()),
                key: Box::new(body_test_expr(hir::ExprKind::Str("key".to_string()), Ty::Str)),
            },
            Ty::JsonDoc,
        ),
        Ty::JsonDoc,
    );
    add_tail(
        &mut program,
        "b2b2_json_doc_at",
        vec![local(0, "doc", Ty::JsonDoc)],
        body_test_expr(
            hir::ExprKind::JsonDocAt {
                doc: Box::new(doc_local()),
                index: Box::new(body_test_expr(hir::ExprKind::Int(0), integer)),
            },
            Ty::JsonDoc,
        ),
        Ty::JsonDoc,
    );
    add_tail(
        &mut program,
        "b2b2_json_doc_as_str",
        vec![local(0, "doc", Ty::JsonDoc)],
        body_test_expr(
            hir::ExprKind::JsonDocAsStr { doc: Box::new(doc_local()) },
            Ty::Option(Scalar::Str),
        ),
        Ty::Option(Scalar::Str),
    );
    add_tail(
        &mut program,
        "b2b2_json_doc_as_scalar",
        vec![local(0, "doc", Ty::JsonDoc)],
        body_test_expr(
            hir::ExprKind::JsonDocAsScalar {
                doc: Box::new(doc_local()),
                scalar: integer,
            },
            Ty::Option(scalar_integer),
        ),
        Ty::Option(scalar_integer),
    );
    add_tail(
        &mut program,
        "b2b2_json_doc_len",
        vec![local(0, "doc", Ty::JsonDoc)],
        body_test_expr(
            hir::ExprKind::JsonDocLen { doc: Box::new(doc_local()) },
            integer,
        ),
        integer,
    );
    add_tail(
        &mut program,
        "b2b2_json_doc_key",
        vec![local(0, "doc", Ty::JsonDoc)],
        body_test_expr(
            hir::ExprKind::JsonDocKey {
                doc: Box::new(doc_local()),
                index: Box::new(body_test_expr(hir::ExprKind::Int(0), integer)),
            },
            Ty::Option(Scalar::Str),
        ),
        Ty::Option(Scalar::Str),
    );
    add_tail(
        &mut program,
        "b2b2_json_doc_elems",
        vec![local(0, "doc", Ty::JsonDoc)],
        arena(
            body_test_expr(
                hir::ExprKind::JsonDocElems { doc: Box::new(doc_local()) },
                Ty::Slice(Scalar::JsonDoc),
            ),
            Ty::Slice(Scalar::JsonDoc),
        ),
        Ty::Slice(Scalar::JsonDoc),
    );

    let scanner_local = || local(0, "scanner", Ty::JsonScanner(0));
    let scanner_source = || body_test_expr(hir::ExprKind::Local(0), Ty::JsonScanner(0));
    let project_value = || hir::Stage {
        kind: hir::StageKind::Project { field: 1 },
        out_ty: integer,
    };
    add_tail(
        &mut program,
        "b2b2_scan_sum",
        vec![scanner_local()],
        body_test_expr(
            hir::ExprKind::ArraySum {
                source: Box::new(scanner_source()),
                stages: vec![project_value()],
            },
            result_i64,
        ),
        result_i64,
    );
    add_tail(
        &mut program,
        "b2b2_scan_count",
        vec![scanner_local()],
        body_test_expr(
            hir::ExprKind::ArrayCount {
                source: Box::new(scanner_source()),
                stages: Vec::new(),
            },
            result_i64,
        ),
        result_i64,
    );
    add_tail(
        &mut program,
        "b2b2_scan_any",
        vec![scanner_local()],
        body_test_expr(
            hir::ExprKind::ArrayAnyAll {
                source: Box::new(scanner_source()),
                stages: vec![project_value()],
                func: "scan$predicate".to_string(),
                captures: Vec::new(),
                all: false,
            },
            Ty::Result(Scalar::Bool, Scalar::Enum(error_id)),
        ),
        Ty::Result(Scalar::Bool, Scalar::Enum(error_id)),
    );
    add_tail(
        &mut program,
        "b2b2_scan_min",
        vec![scanner_local()],
        body_test_expr(
            hir::ExprKind::ArrayMinMax {
                source: Box::new(scanner_source()),
                stages: vec![project_value()],
                is_max: false,
            },
            result_i64,
        ),
        result_i64,
    );
    add_tail(
        &mut program,
        "b2b2_scan_reduce",
        vec![scanner_local()],
        body_test_expr(
            hir::ExprKind::ArrayReduce {
                source: Box::new(scanner_source()),
                stages: vec![project_value()],
                func: "scan$reduce".to_string(),
                captures: Vec::new(),
                init: Box::new(body_test_expr(hir::ExprKind::Int(0), integer)),
            },
            result_i64,
        ),
        result_i64,
    );

    let add_group = |program: &mut hir::Program,
                     name: &str,
                     base_ty: Ty,
                     source: hir::GroupSource,
                     key_field: u32,
                     result: Ty| {
        add_tail(
            program,
            name,
            vec![local(0, "base", base_ty)],
            body_test_expr(
                hir::ExprKind::ArrayGroupAgg {
                    base: 0,
                    struct_id: 0,
                    key_field,
                    value_field: Some(1),
                    op: hir::GroupOp::Sum,
                    source,
                },
                result,
            ),
            result,
        );
    };
    add_group(
        &mut program,
        "b2b2_group_soa_i64",
        Ty::Soa(0),
        hir::GroupSource::SoaI64,
        1,
        Ty::Tuple(tuple_i64_id),
    );
    add_group(
        &mut program,
        "b2b2_group_soa_str",
        Ty::Soa(0),
        hir::GroupSource::SoaStr,
        0,
        Ty::Tuple(tuple_str_id),
    );
    add_group(
        &mut program,
        "b2b2_group_aos_str",
        Ty::DynStructArray(0, Layout::Aos),
        hir::GroupSource::AosStr,
        0,
        Ty::Tuple(tuple_str_id),
    );
    add_group(
        &mut program,
        "b2b2_group_encoded",
        Ty::DictEncoded(0, 0),
        hir::GroupSource::Encoded,
        0,
        Ty::Tuple(tuple_str_id),
    );
    add_tail(
        &mut program,
        "b2b2_group_multi",
        vec![local(0, "base", Ty::DynStructArray(0, Layout::Aos))],
        body_test_expr(
            hir::ExprKind::ArrayGroupAggMulti {
                base: 0,
                struct_id: 0,
                key_field: 0,
                aggs: vec![
                    hir::GroupAgg1 {
                        op: hir::GroupOp::Sum,
                        value_field: Some(1),
                    },
                    hir::GroupAgg1 {
                        op: hir::GroupOp::Count,
                        value_field: None,
                    },
                ],
                source: hir::GroupSource::AosStr,
            },
            Ty::Tuple(tuple_multi_id),
        ),
        Ty::Tuple(tuple_multi_id),
    );
    program.fns.push(body_test_named_function(
        "b2b2_dict_encode",
        hir::Block {
            stmts: vec![hir::Stmt::Expr(body_test_expr(
                hir::ExprKind::ArrayDictEncode {
                    base: 0,
                    struct_id: 0,
                    key_field: 0,
                },
                Ty::DictEncoded(0, 0),
            ))],
            value: Some(Box::new(body_test_expr(hir::ExprKind::Unit, Ty::Unit))),
        },
        vec![local(0, "base", Ty::DynStructArray(0, Layout::Aos))],
        Ty::Unit,
    ));

    assert!(body_core_metadata_is_valid(&program));

    let mut reject = program.clone();
    let expression = body_value_expression_mut(&mut reject, "b2b2_template");
    let hir::ExprKind::Template(parts) = &mut expression.kind else {
        panic!("template fixture lost its template")
    };
    parts.retain(|part| !matches!(part, hir::TemplatePart::PopComma));
    assert!(!body_core_metadata_is_valid(&reject));

    let mut reject = program.clone();
    let expression = body_value_expression_mut(&mut reject, "b2b2_scan_count");
    expression.ty = integer;
    assert!(!body_core_metadata_is_valid(&reject));

    let mut reject = program.clone();
    let expression = body_value_expression_mut(&mut reject, "b2b2_group_soa_i64");
    let hir::ExprKind::ArrayGroupAgg { source, .. } = &mut expression.kind else {
        panic!("group fixture lost its aggregate")
    };
    *source = hir::GroupSource::AosStr;
    assert!(!body_core_metadata_is_valid(&reject));
}

#[test]
fn hir_body_validator_pipeline_template_json_group_control_flow() {
    let integer = int(64);
    let mut program = baseline_program();
    let error_id = push_builtin_error(&mut program);
    let result_doc = Ty::Result(Scalar::JsonDoc, Scalar::Enum(error_id));
    let divergent = body_test_expr(
        hir::ExprKind::Loop {
            body: hir::Block {
                stmts: Vec::new(),
                value: None,
            },
            diverges: true,
            body_locals: 0..0,
        },
        integer,
    );
    program.fns.push(body_tail_case(
        "b2b2_control_template_diverges",
        body_test_expr(
            hir::ExprKind::Template(vec![hir::TemplatePart::Hole(divergent)]),
            Ty::Str,
        ),
        Ty::Str,
    ));
    let doc = || body_test_expr(
        hir::ExprKind::JsonDoc {
            input: Box::new(body_test_expr(hir::ExprKind::Str("{}".to_string()), Ty::Str)),
        },
        result_doc,
    );
    let branch = body_test_expr(
        hir::ExprKind::If {
            cond: Box::new(body_test_expr(hir::ExprKind::Bool(true), Ty::Bool)),
            then: hir::Block {
                stmts: Vec::new(),
                value: Some(Box::new(doc())),
            },
            els: hir::Block {
                stmts: Vec::new(),
                value: Some(Box::new(doc())),
            },
        },
        result_doc,
    );
    program.fns.push(body_tail_case(
        "b2b2_control_arena_branches",
        body_test_expr(
            hir::ExprKind::Arena(hir::Block {
                stmts: Vec::new(),
                value: Some(Box::new(branch)),
            }),
            result_doc,
        ),
        result_doc,
    ));
    assert!(body_core_metadata_is_valid(&program));

    let mut reject = program.clone();
    let expression = body_value_expression_mut(&mut reject, "b2b2_control_template_diverges");
    let hir::ExprKind::Template(parts) = &mut expression.kind else {
        panic!("diverging template fixture lost its template")
    };
    parts.push(hir::TemplatePart::JsonStr(body_test_expr(
        hir::ExprKind::Unit,
        Ty::Unit,
    )));
    assert!(!body_core_metadata_is_valid(&reject));

    let mut reject = program.clone();
    reject.fns.push(body_tail_case(
        "b2b2_control_doc_without_arena",
        doc(),
        result_doc,
    ));
    assert!(!body_core_metadata_is_valid(&reject));
}

#[test]
fn hir_body_validator_native() {
    let mut program = baseline_program();
    let error = push_builtin_error(&mut program);
    let regex_match = push_builtin_regex_match(&mut program);
    let argon2_params = push_builtin_argon2_params(&mut program);
    let i64_ty = int(64);
    let i32_ty = int(32);
    let u8_scalar = Scalar::Int(IntTy { bits: 8, signed: false });
    let bytes = Ty::Slice(u8_scalar);
    let result_i64 = native_result(i64_ty, error);
    let result_unit = native_result(Ty::Unit, error);
    let result_buffer = native_result(Ty::Buffer, error);
    let result_response = native_result(Ty::HttpResponse, error);
    let result_u8_array = Ty::DynArray(u8_scalar);

    macro_rules! add {
        ($name:expr, $expression:expr, $locals:expr, $ret:expr) => {
            program.fns.push(body_native_case($name, $expression, $ret));
            let locals: Vec<hir::Local> = $locals;
            if !locals.is_empty() {
                let function = program.fns.last_mut().expect("native function just added");
                function.locals = locals;
            }
        };
    }

    add!(
        "native_fs_read_file",
        body_test_expr(
            hir::ExprKind::FsReadFile {
                path: Box::new(native_str()),
            },
            native_result(Ty::String, error),
        ),
        Vec::new(),
        native_result(Ty::String, error)
    );
    add!(
        "native_reader_stdin",
        body_test_expr(hir::ExprKind::ReaderStdin, Ty::Reader),
        Vec::new(),
        Ty::Reader
    );
    add!(
        "native_reader_open",
        body_test_expr(
            hir::ExprKind::ReaderOpen {
                path: Box::new(native_str()),
            },
            native_result(Ty::Reader, error),
        ),
        Vec::new(),
        native_result(Ty::Reader, error)
    );
    add!(
        "native_writer_std",
        body_test_expr(
            hir::ExprKind::WriterStd {
                fd: 1,
                buffered: false,
            },
            Ty::Writer,
        ),
        Vec::new(),
        Ty::Writer
    );
    program.fns.push(body_test_named_function(
        "native_writer_std_buffered_local",
        hir::Block {
            stmts: vec![hir::Stmt::Let {
                local: 0,
                init: body_test_expr(
                    hir::ExprKind::WriterStd {
                        fd: 1,
                        buffered: true,
                    },
                    Ty::Writer,
                ),
            }],
            value: Some(Box::new(body_test_expr(
                hir::ExprKind::WriterFlush {
                    writer: Box::new(native_local(0, Ty::Writer)),
                },
                result_unit,
            ))),
        },
        vec![body_test_local(0, "writer", Ty::Writer, false, false)],
        result_unit,
    ));
    add!(
        "native_writer_create",
        body_test_expr(
            hir::ExprKind::WriterCreate {
                path: Box::new(native_str()),
            },
            native_result(Ty::Writer, error),
        ),
        Vec::new(),
        native_result(Ty::Writer, error)
    );
    add!(
        "native_reader_read",
        body_test_expr(
            hir::ExprKind::ReaderRead {
                reader: Box::new(body_test_expr(hir::ExprKind::ReaderStdin, Ty::Reader)),
                buffer: Box::new(native_local(0, Ty::Buffer)),
            },
            result_i64,
        ),
        vec![body_test_local(0, "buffer", Ty::Buffer, true, false)],
        result_i64
    );
    add!(
        "native_reader_buffered",
        body_test_expr(
            hir::ExprKind::ReaderBuffered {
                reader: Box::new(body_test_expr(hir::ExprKind::ReaderStdin, Ty::Reader)),
            },
            Ty::Reader,
        ),
        Vec::new(),
        Ty::Reader
    );
    program.fns.push(body_test_named_function(
        "native_reader_read_line",
        hir::Block {
            stmts: vec![hir::Stmt::Let {
                local: 2,
                init: body_test_expr(
                    hir::ExprKind::ReaderBuffered {
                        reader: Box::new(native_local(0, Ty::Reader)),
                    },
                    Ty::Reader,
                ),
            }],
            value: Some(Box::new(body_test_expr(
                hir::ExprKind::ReaderReadLine {
                    reader: Box::new(native_local(2, Ty::Reader)),
                    buffer: Box::new(native_local(1, Ty::Buffer)),
                },
                result_i64,
            ))),
        },
        vec![
            body_test_local(0, "reader", Ty::Reader, false, false),
            body_test_local(1, "buffer", Ty::Buffer, true, false),
            body_test_local(2, "buffered", Ty::Reader, false, false),
        ],
        result_i64,
    ));
    add!(
        "native_bytes_as_str",
        body_test_expr(
            hir::ExprKind::BytesAsStr {
                bytes: Box::new(native_local(0, bytes)),
            },
            native_result(Ty::Str, error),
        ),
        vec![body_test_local(0, "bytes", bytes, false, false)],
        native_result(Ty::Str, error)
    );
    add!(
        "native_writer_write",
        body_test_expr(
            hir::ExprKind::WriterWrite {
                writer: Box::new(body_test_expr(
                    hir::ExprKind::WriterStd {
                        fd: 1,
                        buffered: false,
                    },
                    Ty::Writer,
                )),
                arg: Box::new(native_str()),
                builder: false,
            },
            result_unit,
        ),
        Vec::new(),
        result_unit
    );
    add!(
        "native_writer_flush",
        body_test_expr(
            hir::ExprKind::WriterFlush {
                writer: Box::new(body_test_expr(
                    hir::ExprKind::WriterStd {
                        fd: 2,
                        buffered: false,
                    },
                    Ty::Writer,
                )),
            },
            result_unit,
        ),
        Vec::new(),
        result_unit
    );
    add!(
        "native_io_copy",
        body_test_expr(
            hir::ExprKind::IoCopy {
                reader: Box::new(body_test_expr(hir::ExprKind::ReaderStdin, Ty::Reader)),
                writer: Box::new(body_test_expr(
                    hir::ExprKind::WriterStd {
                        fd: 1,
                        buffered: false,
                    },
                    Ty::Writer,
                )),
            },
            result_i64,
        ),
        Vec::new(),
        result_i64
    );
    add!(
        "native_file_create_rw",
        body_test_expr(
            hir::ExprKind::FileCreateRw {
                path: Box::new(native_str()),
            },
            native_result(Ty::File, error),
        ),
        Vec::new(),
        native_result(Ty::File, error)
    );
    add!(
        "native_file_open_rw",
        body_test_expr(
            hir::ExprKind::FileOpenRw {
                path: Box::new(native_str()),
            },
            native_result(Ty::File, error),
        ),
        Vec::new(),
        native_result(Ty::File, error)
    );
    add!(
        "native_file_pread",
        body_test_expr(
            hir::ExprKind::FilePread {
                file: Box::new(native_local(0, Ty::File)),
                buffer: Box::new(native_local(1, Ty::Buffer)),
                offset: Box::new(native_i64()),
            },
            result_i64,
        ),
        vec![
            body_test_local(0, "file", Ty::File, false, false),
            body_test_local(1, "buffer", Ty::Buffer, true, false),
        ],
        result_i64
    );
    add!(
        "native_file_pwrite",
        body_test_expr(
            hir::ExprKind::FilePwrite {
                file: Box::new(native_local(0, Ty::File)),
                data: Box::new(native_str()),
                offset: Box::new(native_i64()),
            },
            result_i64,
        ),
        vec![body_test_local(0, "file", Ty::File, false, false)],
        result_i64
    );
    add!(
        "native_file_len",
        body_test_expr(
            hir::ExprKind::FileLen {
                file: Box::new(native_local(0, Ty::File)),
            },
            result_i64,
        ),
        vec![body_test_local(0, "file", Ty::File, false, false)],
        result_i64
    );
    add!(
        "native_buffer_new",
        body_test_expr(
            hir::ExprKind::BufferNew {
                capacity: Box::new(native_i64()),
            },
            Ty::Buffer,
        ),
        Vec::new(),
        Ty::Buffer
    );
    add!(
        "native_buffer_bytes",
        body_test_expr(
            hir::ExprKind::BufferBytes {
                buffer: Box::new(native_local(0, Ty::Buffer)),
            },
            bytes,
        ),
        vec![body_test_local(0, "buffer", Ty::Buffer, false, false)],
        bytes
    );
    add!(
        "native_str_bytes",
        body_test_expr(
            hir::ExprKind::StrBytes {
                inner: Box::new(native_str()),
            },
            bytes,
        ),
        Vec::new(),
        bytes
    );
    add!(
        "native_buffer_len",
        body_test_expr(
            hir::ExprKind::BufferLen {
                buffer: Box::new(native_local(0, Ty::Buffer)),
            },
            i64_ty,
        ),
        vec![body_test_local(0, "buffer", Ty::Buffer, false, false)],
        i64_ty
    );
    add!(
        "native_bytes_read",
        body_test_expr(
            hir::ExprKind::BytesRead {
                bytes: Box::new(native_local(0, bytes)),
                offset: Box::new(native_i64()),
                be: true,
            },
            i32_ty,
        ),
        vec![body_test_local(0, "bytes", bytes, false, false)],
        i32_ty
    );
    add!(
        "native_buffer_put",
        body_test_expr(
            hir::ExprKind::BufferPut {
                buffer: Box::new(native_local(0, Ty::Buffer)),
                value: Box::new(body_test_expr(hir::ExprKind::Int(1), i32_ty)),
                be: true,
            },
            Ty::Unit,
        ),
        vec![body_test_local(0, "buffer", Ty::Buffer, true, false)],
        Ty::Unit
    );
    add!(
        "native_buffer_append",
        body_test_expr(
            hir::ExprKind::BufferAppend {
                buffer: Box::new(native_local(0, Ty::Buffer)),
                data: Box::new(native_str()),
            },
            Ty::Unit,
        ),
        vec![body_test_local(0, "buffer", Ty::Buffer, true, false)],
        Ty::Unit
    );
    add!(
        "native_array_builder_new",
        body_test_expr(
            hir::ExprKind::ArrayBuilderNew {
                elem: ArrayBuilderElem::Scalar(Scalar::String),
                region: None,
            },
            Ty::ArrayBuilder(Scalar::String),
        ),
        Vec::new(),
        Ty::ArrayBuilder(Scalar::String)
    );
    program.fns.push(body_test_named_function(
        "native_named_region_materialization",
        hir::Block {
            stmts: Vec::new(),
            value: Some(Box::new(body_test_expr(
                hir::ExprKind::NamedArena {
                    local: 0,
                    block: hir::Block {
                        stmts: vec![
                            hir::Stmt::Expr(body_test_expr(
                                hir::ExprKind::CloneIn {
                                    value: Box::new(native_str()),
                                    region: Box::new(native_local(0, Ty::ArenaHandle)),
                                },
                                Ty::Str,
                            )),
                            hir::Stmt::Expr(body_test_expr(
                                hir::ExprKind::ArrayBuilderNew {
                                    elem: ArrayBuilderElem::Scalar(scalar_int(64)),
                                    region: Some(Box::new(native_local(
                                        0,
                                        Ty::ArenaHandle,
                                    ))),
                                },
                                Ty::ArrayBuilder(scalar_int(64)),
                            )),
                        ],
                        value: Some(Box::new(body_test_expr(
                            hir::ExprKind::Unit,
                            Ty::Unit,
                        ))),
                    },
                },
                Ty::Unit,
            ))),
        },
        vec![body_test_local(
            0,
            "out",
            Ty::ArenaHandle,
            false,
            false,
        )],
        Ty::Unit,
    ));
    add!(
        "native_array_builder_push",
        body_test_expr(
            hir::ExprKind::ArrayBuilderPush {
                builder: Box::new(native_local(
                    0,
                    Ty::ArrayBuilder(Scalar::String),
                )),
                value: Box::new(body_test_expr(
                    hir::ExprKind::StrClone(Box::new(native_str())),
                    Ty::String,
                )),
                moves_value: true,
            },
            Ty::Unit,
        ),
        vec![body_test_local(
            0,
            "builder",
            Ty::ArrayBuilder(Scalar::String),
            true,
            false,
        )],
        Ty::Unit
    );
    add!(
        "native_array_builder_append",
        body_test_expr(
            hir::ExprKind::ArrayBuilderAppend {
                builder: Box::new(native_local(
                    0,
                    Ty::ArrayBuilder(scalar_int(64)),
                )),
                data: Box::new(native_local(1, Ty::Slice(scalar_int(64)))),
            },
            Ty::Unit,
        ),
        vec![
            body_test_local(
                0,
                "builder",
                Ty::ArrayBuilder(scalar_int(64)),
                true,
                false,
            ),
            body_test_local(1, "data", Ty::Slice(scalar_int(64)), false, false),
        ],
        Ty::Unit
    );
    add!(
        "native_array_builder_build",
        body_test_expr(
            hir::ExprKind::ArrayBuilderBuild(Box::new(native_local(
                0,
                Ty::ArrayBuilder(scalar_int(64)),
            ))),
            Ty::DynArray(scalar_int(64)),
        ),
        vec![body_test_local(
            0,
            "builder",
            Ty::ArrayBuilder(scalar_int(64)),
            false,
            false,
        )],
        Ty::DynArray(scalar_int(64))
    );
    let vector_element = AggregateArrayElem::Vec(scalar_int(32), 4);
    let vector_builder = Ty::array_builder(ArrayBuilderElem::Aggregate(vector_element));
    add!(
        "native_aggregate_array_builder_push",
        body_test_expr(
            hir::ExprKind::ArrayBuilderPush {
                builder: Box::new(native_local(0, vector_builder)),
                value: Box::new(body_test_expr(
                    hir::ExprKind::VecLit {
                        elems: (0..4)
                            .map(|value| {
                                body_test_expr(hir::ExprKind::Int(value), i32_ty)
                            })
                            .collect(),
                        elem: scalar_int(32),
                    },
                    vector_element.ty(),
                )),
                moves_value: false,
            },
            Ty::Unit,
        ),
        vec![body_test_local(0, "builder", vector_builder, true, false)],
        Ty::Unit
    );
    add!(
        "native_aggregate_array_builder_build",
        body_test_expr(
            hir::ExprKind::ArrayBuilderBuild(Box::new(native_local(0, vector_builder))),
            Ty::dyn_aggregate_array(vector_element),
        ),
        vec![body_test_local(0, "builder", vector_builder, false, false)],
        Ty::dyn_aggregate_array(vector_element)
    );
    add!(
        "native_fs_write_file",
        body_test_expr(
            hir::ExprKind::FsWriteFile {
                path: Box::new(native_str()),
                data: Box::new(native_str()),
                builder: false,
            },
            result_unit,
        ),
        Vec::new(),
        result_unit
    );
    add!(
        "native_fs_exists",
        body_test_expr(
            hir::ExprKind::FsExists {
                path: Box::new(native_str()),
            },
            Ty::Bool,
        ),
        Vec::new(),
        Ty::Bool
    );
    add!(
        "native_fs_remove",
        body_test_expr(
            hir::ExprKind::FsRemove {
                path: Box::new(native_str()),
            },
            result_unit,
        ),
        Vec::new(),
        result_unit
    );
    add!(
        "native_fs_read_dir",
        body_test_expr(
            hir::ExprKind::FsReadDir {
                path: Box::new(native_str()),
            },
            native_result(Ty::DynArray(Scalar::String), error),
        ),
        Vec::new(),
        native_result(Ty::DynArray(Scalar::String), error)
    );
    add!(
        "native_dns_resolve",
        body_test_expr(
            hir::ExprKind::DnsResolve {
                host: Box::new(native_str()),
            },
            native_result(Ty::DynArray(Scalar::String), error),
        ),
        Vec::new(),
        native_result(Ty::DynArray(Scalar::String), error)
    );
    add!(
        "native_tcp_connect",
        body_test_expr(
            hir::ExprKind::TcpConnect {
                host: Box::new(native_str()),
                port: Box::new(native_i64()),
            },
            native_result(Ty::TcpConn, error),
        ),
        Vec::new(),
        native_result(Ty::TcpConn, error)
    );
    add!(
        "native_conn_reader",
        body_test_expr(
            hir::ExprKind::ConnReader {
                conn: Box::new(native_local(0, Ty::TcpConn)),
            },
            Ty::Reader,
        ),
        vec![body_test_local(0, "conn", Ty::TcpConn, false, false)],
        Ty::Reader
    );
    add!(
        "native_conn_writer",
        body_test_expr(
            hir::ExprKind::ConnWriter {
                conn: Box::new(native_local(0, Ty::TcpConn)),
            },
            Ty::Writer,
        ),
        vec![body_test_local(0, "conn", Ty::TcpConn, false, false)],
        Ty::Writer
    );
    add!(
        "native_tcp_read_timeout",
        body_test_expr(
            hir::ExprKind::TcpReadTimeout {
                conn: Box::new(native_local(0, Ty::TcpConn)),
                ns: Box::new(native_i64()),
            },
            Ty::Unit,
        ),
        vec![body_test_local(0, "conn", Ty::TcpConn, false, false)],
        Ty::Unit
    );
    add!(
        "native_tcp_write_timeout",
        body_test_expr(
            hir::ExprKind::TcpWriteTimeout {
                conn: Box::new(native_local(0, Ty::TcpConn)),
                ns: Box::new(native_i64()),
            },
            Ty::Unit,
        ),
        vec![body_test_local(0, "conn", Ty::TcpConn, false, false)],
        Ty::Unit
    );
    add!(
        "native_tcp_listen",
        body_test_expr(
            hir::ExprKind::TcpListen {
                host: Box::new(native_str()),
                port: Box::new(native_i64()),
            },
            native_result(Ty::TcpListener, error),
        ),
        Vec::new(),
        native_result(Ty::TcpListener, error)
    );
    add!(
        "native_tcp_accept",
        body_test_expr(
            hir::ExprKind::TcpAccept {
                listener: Box::new(native_local(0, Ty::TcpListener)),
            },
            native_result(Ty::TcpConn, error),
        ),
        vec![body_test_local(0, "listener", Ty::TcpListener, false, false)],
        native_result(Ty::TcpConn, error)
    );
    add!(
        "native_udp_bind",
        body_test_expr(
            hir::ExprKind::UdpBind {
                host: Box::new(native_str()),
                port: Box::new(native_i64()),
            },
            native_result(Ty::UdpSocket, error),
        ),
        Vec::new(),
        native_result(Ty::UdpSocket, error)
    );
    add!(
        "native_udp_send_to",
        body_test_expr(
            hir::ExprKind::UdpSendTo {
                sock: Box::new(native_local(0, Ty::UdpSocket)),
                data: Box::new(native_str()),
                host: Box::new(native_str()),
                port: Box::new(native_i64()),
            },
            result_i64,
        ),
        vec![body_test_local(0, "socket", Ty::UdpSocket, false, false)],
        result_i64
    );
    add!(
        "native_udp_recv_from",
        body_test_expr(
            hir::ExprKind::UdpRecvFrom {
                sock: Box::new(native_local(0, Ty::UdpSocket)),
                buffer: Box::new(native_local(1, Ty::Buffer)),
            },
            result_i64,
        ),
        vec![
            body_test_local(0, "socket", Ty::UdpSocket, false, false),
            body_test_local(1, "buffer", Ty::Buffer, true, false),
        ],
        result_i64
    );
    let view_result = native_result(Ty::Str, error);
    add!(
        "native_file_read_view",
        body_test_expr(
            hir::ExprKind::Arena(hir::Block {
                stmts: Vec::new(),
                value: Some(Box::new(body_test_expr(
                    hir::ExprKind::FsReadFileView {
                        path: Box::new(native_str()),
                    },
                    view_result,
                ))),
            }),
            view_result,
        ),
        Vec::new(),
        view_result
    );
    add!(
        "native_file_read_bytes_view",
        body_test_expr(
            hir::ExprKind::Arena(hir::Block {
                stmts: Vec::new(),
                value: Some(Box::new(body_test_expr(
                    hir::ExprKind::FsReadBytesView {
                        path: Box::new(native_str()),
                    },
                    native_result(bytes, error),
                ))),
            }),
            native_result(bytes, error),
        ),
        Vec::new(),
        native_result(bytes, error)
    );
    add!(
        "native_path_join",
        body_test_expr(
            hir::ExprKind::PathJoin {
                a: Box::new(native_str()),
                b: Box::new(native_str()),
            },
            Ty::String,
        ),
        Vec::new(),
        Ty::String
    );
    add!(
        "native_path_component",
        body_test_expr(
            hir::ExprKind::PathComponent {
                kind: hir::PathComponentKind::Base,
                path: Box::new(native_str()),
            },
            Ty::Str,
        ),
        Vec::new(),
        Ty::Str
    );
    add!(
        "native_path_normalize",
        body_test_expr(
            hir::ExprKind::PathNormalize {
                path: Box::new(native_str()),
            },
            Ty::String,
        ),
        Vec::new(),
        Ty::String
    );
    add!(
        "native_env_get",
        body_test_expr(
            hir::ExprKind::EnvGet {
                name: Box::new(native_str()),
            },
            Ty::Option(Scalar::String),
        ),
        Vec::new(),
        Ty::Option(Scalar::String)
    );
    add!(
        "native_env_set",
        body_test_expr(
            hir::ExprKind::EnvSet {
                name: Box::new(native_str()),
                value: Box::new(native_str()),
            },
            result_unit,
        ),
        Vec::new(),
        result_unit
    );
    for (name, kind, ret) in [
        ("native_time_now", hir::ExprKind::TimeNow, i64_ty),
        ("native_time_instant", hir::ExprKind::TimeInstant, i64_ty),
        ("native_process_cpu_count", hir::ExprKind::ProcessCpuCount, i64_ty),
    ] {
        add!(name, body_test_expr(kind, ret), Vec::new(), ret);
    }
    add!(
        "native_time_sleep",
        body_test_expr(
            hir::ExprKind::TimeSleep {
                ns: Box::new(native_i64()),
            },
            Ty::Unit,
        ),
        Vec::new(),
        Ty::Unit
    );
    add!(
        "native_process_spawn",
        body_test_expr(
            hir::ExprKind::ProcessSpawn {
                cmd: Box::new(native_str()),
                args: Box::new(native_local(0, Ty::DynArray(Scalar::Str))),
            },
            native_result(Ty::Child, error),
        ),
        vec![body_test_local(0, "args", Ty::DynArray(Scalar::Str), false, false)],
        native_result(Ty::Child, error)
    );
    add!(
        "native_child_wait",
        body_test_expr(
            hir::ExprKind::ChildWait {
                child: Box::new(native_local(0, Ty::Child)),
            },
            result_i64,
        ),
        vec![body_test_local(0, "child", Ty::Child, false, false)],
        result_i64
    );
    add!(
        "native_child_kill",
        body_test_expr(
            hir::ExprKind::ChildKill {
                child: Box::new(native_local(0, Ty::Child)),
                sig: Box::new(native_i64()),
            },
            result_unit,
        ),
        vec![body_test_local(0, "child", Ty::Child, false, false)],
        result_unit
    );
    add!(
        "native_process_exec",
        body_test_expr(
            hir::ExprKind::ProcessExec {
                cmd: Box::new(native_str()),
                args: Box::new(native_local(0, Ty::Slice(Scalar::Str))),
            },
            result_unit,
        ),
        vec![body_test_local(0, "args", Ty::Slice(Scalar::Str), false, false)],
        result_unit
    );
    add!(
        "native_process_command",
        body_test_expr(
            hir::ExprKind::ProcessCommand {
                cmd: Box::new(native_str()),
                args: Box::new(native_local(0, Ty::DynArray(Scalar::Str))),
            },
            Ty::Command,
        ),
        vec![body_test_local(0, "args", Ty::DynArray(Scalar::Str), false, false)],
        Ty::Command
    );
    add!(
        "native_command_cwd",
        body_test_expr(
            hir::ExprKind::CommandCwd {
                command: Box::new(native_local(0, Ty::Command)),
                dir: Box::new(native_str()),
            },
            Ty::Unit,
        ),
        vec![body_test_local(0, "command", Ty::Command, false, false)],
        Ty::Unit
    );
    add!(
        "native_command_timeout",
        body_test_expr(
            hir::ExprKind::CommandTimeout {
                command: Box::new(native_local(0, Ty::Command)),
                ns: Box::new(native_i64()),
            },
            Ty::Unit,
        ),
        vec![body_test_local(0, "command", Ty::Command, false, false)],
        Ty::Unit
    );
    add!(
        "native_command_env",
        body_test_expr(
            hir::ExprKind::CommandEnv {
                command: Box::new(native_local(0, Ty::Command)),
                name: Box::new(native_str()),
                value: Box::new(native_str()),
            },
            Ty::Unit,
        ),
        vec![body_test_local(0, "command", Ty::Command, false, false)],
        Ty::Unit
    );
    add!(
        "native_command_env_clear",
        body_test_expr(
            hir::ExprKind::CommandEnvClear {
                command: Box::new(native_local(0, Ty::Command)),
            },
            Ty::Unit,
        ),
        vec![body_test_local(0, "command", Ty::Command, false, false)],
        Ty::Unit
    );
    add!(
        "native_command_run",
        body_test_expr(
            hir::ExprKind::CommandRun {
                command: Box::new(native_local(0, Ty::Command)),
            },
            native_result(Ty::RunOutput, error),
        ),
        vec![body_test_local(0, "command", Ty::Command, false, false)],
        native_result(Ty::RunOutput, error)
    );
    add!(
        "native_run_output_code",
        body_test_expr(
            hir::ExprKind::RunOutputCode {
                out: Box::new(native_local(0, Ty::RunOutput)),
            },
            i64_ty,
        ),
        vec![body_test_local(0, "out", Ty::RunOutput, false, false)],
        i64_ty
    );
    add!(
        "native_run_output_stdout",
        body_test_expr(
            hir::ExprKind::RunOutputStdout {
                out: Box::new(native_local(0, Ty::RunOutput)),
            },
            Ty::Str,
        ),
        vec![body_test_local(0, "out", Ty::RunOutput, false, false)],
        Ty::Str
    );
    add!(
        "native_run_output_stderr",
        body_test_expr(
            hir::ExprKind::RunOutputStderr {
                out: Box::new(native_local(0, Ty::RunOutput)),
            },
            Ty::Str,
        ),
        vec![body_test_local(0, "out", Ty::RunOutput, false, false)],
        Ty::Str
    );
    add!(
        "native_encoding_encode",
        body_test_expr(
            hir::ExprKind::EncodingEncode {
                kind: hir::EncodingKind::Base64,
                data: Box::new(native_str()),
            },
            Ty::String,
        ),
        Vec::new(),
        Ty::String
    );
    add!(
        "native_encoding_decode",
        body_test_expr(
            hir::ExprKind::EncodingDecode {
                kind: hir::EncodingKind::Hex,
                input: Box::new(native_str()),
            },
            result_buffer,
        ),
        Vec::new(),
        result_buffer
    );
    add!(
        "native_utf8_valid",
        body_test_expr(
            hir::ExprKind::Utf8Valid {
                data: Box::new(native_local(0, bytes)),
            },
            Ty::Bool,
        ),
        vec![body_test_local(0, "bytes", bytes, false, false)],
        Ty::Bool
    );
    add!(
        "native_compress",
        body_test_expr(
            hir::ExprKind::Compress {
                kind: hir::CompressKind::Gzip,
                data: Box::new(native_str()),
                level: Box::new(native_i64()),
            },
            result_buffer,
        ),
        Vec::new(),
        result_buffer
    );
    add!(
        "native_decompress",
        body_test_expr(
            hir::ExprKind::Decompress {
                kind: hir::CompressKind::Zstd,
                data: Box::new(native_str()),
            },
            result_buffer,
        ),
        Vec::new(),
        result_buffer
    );
    add!(
        "native_rand_seed",
        body_test_expr(hir::ExprKind::RandSeed, Ty::Rng),
        Vec::new(),
        Ty::Rng
    );
    add!(
        "native_rand_seed_with",
        body_test_expr(
            hir::ExprKind::RandSeedWith {
                seed: Box::new(native_i64()),
            },
            Ty::Rng,
        ),
        Vec::new(),
        Ty::Rng
    );
    add!(
        "native_rand_next",
        body_test_expr(
            hir::ExprKind::RandNext {
                rng: Box::new(native_local(0, Ty::Rng)),
            },
            i64_ty,
        ),
        vec![body_test_local(0, "rng", Ty::Rng, true, false)],
        i64_ty
    );
    add!(
        "native_rand_range",
        body_test_expr(
            hir::ExprKind::RandRange {
                rng: Box::new(native_local(0, Ty::Rng)),
                lo: Box::new(native_i64()),
                hi: Box::new(native_i64()),
            },
            i64_ty,
        ),
        vec![body_test_local(0, "rng", Ty::Rng, true, false)],
        i64_ty
    );
    add!(
        "native_rand_shuffle",
        body_test_expr(
            hir::ExprKind::RandShuffle {
                rng: Box::new(native_local(0, Ty::Rng)),
                xs: Box::new(native_local(1, Ty::Slice(scalar_int(64)))),
                elem: i64_ty,
            },
            Ty::Unit,
        ),
        vec![
            body_test_local(0, "rng", Ty::Rng, true, false),
            body_test_local(1, "xs", Ty::Slice(scalar_int(64)), true, false),
        ],
        Ty::Unit
    );
    add!(
        "native_rand_sample",
        body_test_expr(
            hir::ExprKind::RandSample {
                rng: Box::new(native_local(0, Ty::Rng)),
                xs: Box::new(native_local(1, Ty::Slice(scalar_int(64)))),
                k: Box::new(native_i64()),
                elem: i64_ty,
            },
            Ty::DynArray(scalar_int(64)),
        ),
        vec![
            body_test_local(0, "rng", Ty::Rng, true, false),
            body_test_local(1, "xs", Ty::Slice(scalar_int(64)), false, false),
        ],
        Ty::DynArray(scalar_int(64))
    );
    add!(
        "native_regex_compile",
        body_test_expr(
            hir::ExprKind::RegexCompile {
                pattern: Box::new(native_str()),
            },
            native_result(Ty::Regex, error),
        ),
        Vec::new(),
        native_result(Ty::Regex, error)
    );
    add!(
        "native_regex_is_match",
        body_test_expr(
            hir::ExprKind::RegexIsMatch {
                regex: Box::new(native_local(0, Ty::Regex)),
                text: Box::new(native_str()),
            },
            Ty::Bool,
        ),
        vec![body_test_local(0, "regex", Ty::Regex, false, false)],
        Ty::Bool
    );
    add!(
        "native_regex_find",
        body_test_expr(
            hir::ExprKind::RegexFind {
                regex: Box::new(native_local(0, Ty::Regex)),
                text: Box::new(native_str()),
                start: Some(Box::new(native_i64())),
            },
            Ty::Option(Scalar::Struct(regex_match)),
        ),
        vec![body_test_local(0, "regex", Ty::Regex, false, false)],
        Ty::Option(Scalar::Struct(regex_match))
    );
    for (name, kind) in [
        (
            "native_regex_find_all",
            hir::ExprKind::RegexFindAll {
                regex: Box::new(native_local(0, Ty::Regex)),
                text: Box::new(native_str()),
            },
        ),
        (
            "native_regex_split",
            hir::ExprKind::RegexSplit {
                regex: Box::new(native_local(0, Ty::Regex)),
                text: Box::new(native_str()),
            },
        ),
    ] {
        add!(
            name,
            body_test_expr(
                kind,
                Ty::DynStructArray(regex_match, Layout::Aos),
            ),
            vec![body_test_local(0, "regex", Ty::Regex, false, false)],
            Ty::DynStructArray(regex_match, Layout::Aos)
        );
    }
    add!(
        "native_regex_replace",
        body_test_expr(
            hir::ExprKind::RegexReplace {
                regex: Box::new(native_local(0, Ty::Regex)),
                text: Box::new(native_str()),
                repl: Box::new(native_str()),
                all: true,
            },
            Ty::String,
        ),
        vec![body_test_local(0, "regex", Ty::Regex, false, false)],
        Ty::String
    );
    add!(
        "native_regex_captures",
        body_test_expr(
            hir::ExprKind::RegexCaptures {
                regex: Box::new(native_local(0, Ty::Regex)),
                text: Box::new(native_str()),
            },
            Ty::Option(Scalar::Captures),
        ),
        vec![body_test_local(0, "regex", Ty::Regex, false, false)],
        Ty::Option(Scalar::Captures)
    );
    add!(
        "native_regex_group_count",
        body_test_expr(
            hir::ExprKind::RegexGroupCount {
                regex: Box::new(native_local(0, Ty::Regex)),
            },
            i64_ty,
        ),
        vec![body_test_local(0, "regex", Ty::Regex, false, false)],
        i64_ty
    );
    add!(
        "native_regex_group_index",
        body_test_expr(
            hir::ExprKind::RegexGroupIndex {
                regex: Box::new(native_local(0, Ty::Regex)),
                name: Box::new(native_str()),
            },
            Ty::Option(Scalar::Int(IntTy { bits: 64, signed: true })),
        ),
        vec![body_test_local(0, "regex", Ty::Regex, false, false)],
        Ty::Option(Scalar::Int(IntTy { bits: 64, signed: true }))
    );
    add!(
        "native_captures_group",
        body_test_expr(
            hir::ExprKind::CapturesGroup {
                caps: Box::new(native_local(0, Ty::Captures)),
                index: Box::new(native_i64()),
            },
            Ty::Option(Scalar::Struct(regex_match)),
        ),
        vec![body_test_local(0, "caps", Ty::Captures, false, false)],
        Ty::Option(Scalar::Struct(regex_match))
    );
    add!(
        "native_cli_command",
        body_test_expr(
            hir::ExprKind::CliCommand {
                name: Box::new(native_str()),
            },
            Ty::CliCommand,
        ),
        Vec::new(),
        Ty::CliCommand
    );
    add!(
        "native_cli_flag_bool",
        body_test_expr(
            hir::ExprKind::CliFlag {
                cmd: Box::new(native_local(0, Ty::CliCommand)),
                kind: hir::CliFlagKind::Bool,
                name: Box::new(native_str()),
                default: None,
            },
            Ty::Unit,
        ),
        vec![body_test_local(0, "cmd", Ty::CliCommand, false, false)],
        Ty::Unit
    );
    add!(
        "native_cli_flag_i64",
        body_test_expr(
            hir::ExprKind::CliFlag {
                cmd: Box::new(native_local(0, Ty::CliCommand)),
                kind: hir::CliFlagKind::I64,
                name: Box::new(native_str()),
                default: Some(Box::new(native_i64())),
            },
            Ty::Unit,
        ),
        vec![body_test_local(0, "cmd", Ty::CliCommand, false, false)],
        Ty::Unit
    );
    add!(
        "native_cli_flag_str",
        body_test_expr(
            hir::ExprKind::CliFlag {
                cmd: Box::new(native_local(0, Ty::CliCommand)),
                kind: hir::CliFlagKind::Str,
                name: Box::new(native_str()),
                default: Some(Box::new(native_str())),
            },
            Ty::Unit,
        ),
        vec![body_test_local(0, "cmd", Ty::CliCommand, false, false)],
        Ty::Unit
    );
    add!(
        "native_cli_parse",
        body_test_expr(
            hir::ExprKind::CliParse {
                cmd: Box::new(native_local(0, Ty::CliCommand)),
                args: Box::new(native_local(1, Ty::DynArray(Scalar::Str))),
            },
            native_result(Ty::CliParsed, error),
        ),
        vec![
            body_test_local(0, "cmd", Ty::CliCommand, false, false),
            body_test_local(1, "args", Ty::DynArray(Scalar::Str), false, false),
        ],
        native_result(Ty::CliParsed, error)
    );
    add!(
        "native_cli_get_bool",
        body_test_expr(
            hir::ExprKind::CliGetBool {
                parsed: Box::new(native_local(0, Ty::CliParsed)),
                name: Box::new(native_str()),
            },
            Ty::Bool,
        ),
        vec![body_test_local(0, "parsed", Ty::CliParsed, false, false)],
        Ty::Bool
    );
    add!(
        "native_cli_get_i64",
        body_test_expr(
            hir::ExprKind::CliGetI64 {
                parsed: Box::new(native_local(0, Ty::CliParsed)),
                name: Box::new(native_str()),
            },
            i64_ty,
        ),
        vec![body_test_local(0, "parsed", Ty::CliParsed, false, false)],
        i64_ty
    );
    add!(
        "native_cli_get_str",
        body_test_expr(
            hir::ExprKind::CliGetStr {
                parsed: Box::new(native_local(0, Ty::CliParsed)),
                name: Box::new(native_str()),
            },
            Ty::Str,
        ),
        vec![body_test_local(0, "parsed", Ty::CliParsed, false, false)],
        Ty::Str
    );
    add!(
        "native_cli_usage",
        body_test_expr(
            hir::ExprKind::CliUsage {
                cmd: Box::new(native_local(0, Ty::CliCommand)),
            },
            Ty::String,
        ),
        vec![body_test_local(0, "cmd", Ty::CliCommand, false, false)],
        Ty::String
    );

    add!(
        "native_http_request",
        body_test_expr(
            hir::ExprKind::HttpRequest {
                method: Box::new(native_str()),
                url: Box::new(native_str()),
            },
            Ty::HttpRequest,
        ),
        Vec::new(),
        Ty::HttpRequest
    );
    add!(
        "native_http_header",
        body_test_expr(
            hir::ExprKind::HttpHeader {
                req: Box::new(native_local(0, Ty::HttpRequest)),
                name: Box::new(native_str()),
                value: Box::new(native_str()),
            },
            Ty::Unit,
        ),
        vec![body_test_local(0, "request", Ty::HttpRequest, false, false)],
        Ty::Unit
    );
    add!(
        "native_http_body",
        body_test_expr(
            hir::ExprKind::HttpBody {
                req: Box::new(native_local(0, Ty::HttpRequest)),
                data: Box::new(native_str()),
            },
            Ty::Unit,
        ),
        vec![body_test_local(0, "request", Ty::HttpRequest, false, false)],
        Ty::Unit
    );
    add!(
        "native_http_request_timeout",
        body_test_expr(
            hir::ExprKind::HttpRequestTimeout {
                req: Box::new(native_local(0, Ty::HttpRequest)),
                ns: Box::new(native_i64()),
            },
            Ty::Unit,
        ),
        vec![body_test_local(0, "request", Ty::HttpRequest, false, false)],
        Ty::Unit
    );
    add!(
        "native_http_parse",
        body_test_expr(
            hir::ExprKind::HttpParse {
                data: Box::new(native_str()),
            },
            result_response,
        ),
        Vec::new(),
        result_response
    );
    add!(
        "native_http_resp_status",
        body_test_expr(
            hir::ExprKind::HttpRespStatus {
                resp: Box::new(native_local(0, Ty::HttpResponse)),
            },
            i64_ty,
        ),
        vec![body_test_local(0, "response", Ty::HttpResponse, false, false)],
        i64_ty
    );
    add!(
        "native_http_resp_header",
        body_test_expr(
            hir::ExprKind::HttpRespHeader {
                resp: Box::new(native_local(0, Ty::HttpResponse)),
                name: Box::new(native_str()),
            },
            Ty::Option(Scalar::Str),
        ),
        vec![body_test_local(0, "response", Ty::HttpResponse, false, false)],
        Ty::Option(Scalar::Str)
    );
    add!(
        "native_http_resp_body",
        body_test_expr(
            hir::ExprKind::HttpRespBody {
                resp: Box::new(native_local(0, Ty::HttpResponse)),
            },
            bytes,
        ),
        vec![body_test_local(0, "response", Ty::HttpResponse, false, false)],
        bytes
    );
    add!(
        "native_http_client",
        body_test_expr(hir::ExprKind::HttpClient, Ty::HttpClient),
        Vec::new(),
        Ty::HttpClient
    );
    add!(
        "native_http_client_timeout",
        body_test_expr(
            hir::ExprKind::HttpClientTimeout {
                client: Box::new(native_local(0, Ty::HttpClient)),
                ns: Box::new(native_i64()),
            },
            Ty::Unit,
        ),
        vec![body_test_local(0, "client", Ty::HttpClient, false, false)],
        Ty::Unit
    );
    add!(
        "native_http_client_get",
        body_test_expr(
            hir::ExprKind::HttpClientGet {
                client: Box::new(native_local(0, Ty::HttpClient)),
                url: Box::new(native_str()),
            },
            result_response,
        ),
        vec![body_test_local(0, "client", Ty::HttpClient, false, false)],
        result_response
    );
    add!(
        "native_http_client_post",
        body_test_expr(
            hir::ExprKind::HttpClientPost {
                client: Box::new(native_local(0, Ty::HttpClient)),
                url: Box::new(native_str()),
                body: Box::new(native_str()),
            },
            result_response,
        ),
        vec![body_test_local(0, "client", Ty::HttpClient, false, false)],
        result_response
    );
    add!(
        "native_http_client_request",
        body_test_expr(
            hir::ExprKind::HttpClientRequest {
                client: Box::new(native_local(0, Ty::HttpClient)),
                req: Box::new(native_local(1, Ty::HttpRequest)),
            },
            result_response,
        ),
        vec![
            body_test_local(0, "client", Ty::HttpClient, false, false),
            body_test_local(1, "request", Ty::HttpRequest, false, false),
        ],
        result_response
    );
    add!(
        "native_http_get_many",
        body_test_expr(
            hir::ExprKind::HttpGetMany {
                client: Box::new(native_local(0, Ty::HttpClient)),
                urls: Box::new(native_local(1, Ty::Slice(Scalar::Str))),
                max_concurrency: Box::new(native_i64()),
            },
            native_result(Ty::DynResponseArray, error),
        ),
        vec![
            body_test_local(0, "client", Ty::HttpClient, false, false),
            body_test_local(1, "urls", Ty::Slice(Scalar::Str), false, false),
        ],
        native_result(Ty::DynResponseArray, error)
    );
    add!(
        "native_http_serve",
        body_test_expr(
            hir::ExprKind::HttpServe {
                host: Box::new(native_str()),
                port: Box::new(native_i64()),
                shared: true,
            },
            native_result(Ty::HttpServer, error),
        ),
        Vec::new(),
        native_result(Ty::HttpServer, error)
    );
    add!(
        "native_http_accept",
        body_test_expr(
            hir::ExprKind::HttpAccept {
                server: Box::new(native_local(0, Ty::HttpServer)),
            },
            native_result(Ty::HttpRequestCtx, error),
        ),
        vec![body_test_local(0, "server", Ty::HttpServer, false, false)],
        native_result(Ty::HttpRequestCtx, error)
    );
    add!(
        "native_http_response_builder",
        body_test_expr(
            hir::ExprKind::HttpResponseBuilder {
                status: Box::new(native_i64()),
            },
            Ty::ResponseBuilder,
        ),
        Vec::new(),
        Ty::ResponseBuilder
    );
    add!(
        "native_http_rb_header",
        body_test_expr(
            hir::ExprKind::HttpRbHeader {
                rb: Box::new(native_local(0, Ty::ResponseBuilder)),
                name: Box::new(native_str()),
                value: Box::new(native_str()),
            },
            Ty::Unit,
        ),
        vec![body_test_local(0, "builder", Ty::ResponseBuilder, false, false)],
        Ty::Unit
    );
    add!(
        "native_http_rb_body",
        body_test_expr(
            hir::ExprKind::HttpRbBody {
                rb: Box::new(native_local(0, Ty::ResponseBuilder)),
                data: Box::new(native_str()),
            },
            Ty::Unit,
        ),
        vec![body_test_local(0, "builder", Ty::ResponseBuilder, false, false)],
        Ty::Unit
    );
    let ctx_struct = program.structs.len() as u32;
    program.structs.push(StructDef {
        name: "CtxHolder".to_string(),
        source_name: "CtxHolder".to_string(),
        fields: vec![FieldDef {
            name: "ctx".to_string(),
            ty: Ty::HttpRequestCtx,
        }],
        align: None,
        c_repr: false,
    });
    let ctx_field = || {
        body_test_expr(
            hir::ExprKind::Field {
                root: 0,
                path: vec![0],
            },
            Ty::HttpRequestCtx,
        )
    };
    add!(
        "native_http_ctx_method",
        body_test_expr(
            hir::ExprKind::HttpCtxMethod {
                ctx: Box::new(ctx_field()),
            },
            Ty::Str,
        ),
        vec![body_test_local(0, "holder", Ty::Struct(ctx_struct), false, false)],
        Ty::Str
    );
    add!(
        "native_http_ctx_path",
        body_test_expr(
            hir::ExprKind::HttpCtxPath {
                ctx: Box::new(ctx_field()),
            },
            Ty::Str,
        ),
        vec![body_test_local(0, "holder", Ty::Struct(ctx_struct), false, false)],
        Ty::Str
    );
    add!(
        "native_http_ctx_headers",
        body_test_expr(
            hir::ExprKind::HttpCtxHeaders {
                ctx: Box::new(ctx_field()),
            },
            Ty::HttpHeaders,
        ),
        vec![body_test_local(0, "holder", Ty::Struct(ctx_struct), false, false)],
        Ty::HttpHeaders
    );
    add!(
        "native_http_ctx_header",
        body_test_expr(
            hir::ExprKind::HttpCtxHeader {
                headers: Box::new(body_test_expr(
                    hir::ExprKind::HttpCtxHeaders {
                        ctx: Box::new(ctx_field()),
                    },
                    Ty::HttpHeaders,
                )),
                name: Box::new(native_str()),
            },
            Ty::Option(Scalar::Str),
        ),
        vec![body_test_local(0, "holder", Ty::Struct(ctx_struct), false, false)],
        Ty::Option(Scalar::Str)
    );
    add!(
        "native_http_ctx_body",
        body_test_expr(
            hir::ExprKind::HttpCtxBody {
                ctx: Box::new(ctx_field()),
            },
            bytes,
        ),
        vec![body_test_local(0, "holder", Ty::Struct(ctx_struct), false, false)],
        bytes
    );
    add!(
        "native_http_respond",
        body_test_expr(
            hir::ExprKind::HttpRespond {
                ctx: Box::new(ctx_field()),
                rb: Box::new(native_local(1, Ty::ResponseBuilder)),
            },
            result_unit,
        ),
        vec![
            body_test_local(0, "holder", Ty::Struct(ctx_struct), false, false),
            body_test_local(1, "builder", Ty::ResponseBuilder, false, false),
        ],
        result_unit
    );
    add!(
        "native_http_respond_stream",
        body_test_expr(
            hir::ExprKind::HttpRespondStream {
                ctx: Box::new(ctx_field()),
                rb: Box::new(native_local(1, Ty::ResponseBuilder)),
            },
            native_result(Ty::HttpStream, error),
        ),
        vec![
            body_test_local(0, "holder", Ty::Struct(ctx_struct), false, false),
            body_test_local(1, "builder", Ty::ResponseBuilder, false, false),
        ],
        native_result(Ty::HttpStream, error)
    );
    add!(
        "native_http_stream_send",
        body_test_expr(
            hir::ExprKind::HttpStreamSend {
                stream: Box::new(native_local(0, Ty::HttpStream)),
                chunk: Box::new(native_str()),
                event: true,
            },
            result_unit,
        ),
        vec![body_test_local(0, "stream", Ty::HttpStream, false, false)],
        result_unit
    );
    add!(
        "native_http_stream_finish",
        body_test_expr(
            hir::ExprKind::HttpStreamFinish {
                stream: Box::new(native_local(0, Ty::HttpStream)),
            },
            result_unit,
        ),
        vec![body_test_local(0, "stream", Ty::HttpStream, false, false)],
        result_unit
    );
    add!(
        "native_http_stream_reject",
        body_test_expr(
            hir::ExprKind::HttpStreamReject {
                stream: Box::new(native_local(0, Ty::HttpStream)),
                rb: Box::new(native_local(1, Ty::ResponseBuilder)),
            },
            result_unit,
        ),
        vec![
            body_test_local(0, "stream", Ty::HttpStream, false, false),
            body_test_local(1, "builder", Ty::ResponseBuilder, false, false),
        ],
        result_unit
    );
    add!(
        "native_crypto_ct_equal",
        body_test_expr(
            hir::ExprKind::CryptoCtEqual {
                a: Box::new(native_str()),
                b: Box::new(native_local(0, bytes)),
            },
            Ty::Bool,
        ),
        vec![body_test_local(0, "bytes", bytes, false, false)],
        Ty::Bool
    );
    add!(
        "native_crypto_random",
        body_test_expr(
            hir::ExprKind::CryptoRandom {
                out: Box::new(native_local(0, Ty::Buffer)),
            },
            Ty::Unit,
        ),
        vec![body_test_local(0, "out", Ty::Buffer, true, false)],
        Ty::Unit
    );
    add!(
        "native_crypto_hash",
        body_test_expr(
            hir::ExprKind::CryptoHash {
                algo: hir::HashAlgo::Sha256,
                data: Box::new(native_str()),
            },
            result_u8_array,
        ),
        Vec::new(),
        result_u8_array
    );
    add!(
        "native_crypto_hmac",
        body_test_expr(
            hir::ExprKind::CryptoHmac {
                key: Box::new(native_str()),
                data: Box::new(native_str()),
            },
            result_u8_array,
        ),
        Vec::new(),
        result_u8_array
    );
    add!(
        "native_crypto_hkdf",
        body_test_expr(
            hir::ExprKind::CryptoHkdf {
                salt: Box::new(native_str()),
                ikm: Box::new(native_str()),
                info: Box::new(native_str()),
                len: Box::new(native_i64()),
            },
            result_buffer,
        ),
        Vec::new(),
        result_buffer
    );
    add!(
        "native_crypto_aead",
        body_test_expr(
            hir::ExprKind::CryptoAead {
                cipher: hir::AeadCipher::Aes256Gcm,
                dir: hir::AeadDir::Open,
                key: Box::new(native_str()),
                nonce: Box::new(native_str()),
                input: Box::new(native_str()),
                aad: Box::new(native_str()),
            },
            result_buffer,
        ),
        Vec::new(),
        result_buffer
    );
    add!(
        "native_crypto_argon2",
        body_test_expr(
            hir::ExprKind::CryptoArgon2 {
                password: Box::new(native_str()),
                salt: Box::new(native_str()),
                params: Box::new(native_local(0, Ty::Struct(argon2_params))),
            },
            result_buffer,
        ),
        vec![body_test_local(
            0,
            "params",
            Ty::Struct(argon2_params),
            false,
            false,
        )],
        result_buffer
    );

    assert!(
        validate_hir::global_type_metadata_is_valid(&program),
        "native global metadata"
    );
    assert!(
        validate_hir::type_placement_metadata_is_valid(&program),
        "native type placement metadata"
    );
    assert!(
        validate_hir::nominal_link_metadata_is_valid(&program),
        "native nominal metadata"
    );
    assert!(body_core_metadata_is_valid(&program), "native body metadata");

    let mut reject = program.clone();
    let function = reject
        .fns
        .iter_mut()
        .find(|function| function.name == "native_named_region_materialization")
        .expect("named region fixture is present");
    function.locals.push(body_test_local(
        1,
        "alias",
        Ty::ArenaHandle,
        false,
        false,
    ));
    let expression = function
        .body
        .value
        .as_deref_mut()
        .expect("named region fixture has a value");
    let hir::ExprKind::NamedArena { block, .. } = &mut expression.kind else {
        panic!("named region fixture lost its arena")
    };
    block.stmts.push(hir::Stmt::Let {
        local: 1,
        init: native_local(0, Ty::ArenaHandle),
    });
    assert!(
        !body_core_metadata_is_valid(&reject),
        "an ordinary local must not store a region capability"
    );

    let mut reject = program.clone();
    let expression = body_value_expression_mut(
        &mut reject,
        "native_named_region_materialization",
    );
    let hir::ExprKind::NamedArena { local, .. } = &mut expression.kind else {
        panic!("named region fixture lost its arena")
    };
    *local = 99;
    assert!(!body_core_metadata_is_valid(&reject));

    let mut reject = program.clone();
    let expression = body_value_expression_mut(
        &mut reject,
        "native_named_region_materialization",
    );
    let hir::ExprKind::NamedArena { block, .. } = &mut expression.kind else {
        panic!("named region fixture lost its arena")
    };
    let hir::Stmt::Expr(clone) = &mut block.stmts[0] else {
        panic!("named region fixture lost clone_in")
    };
    let hir::ExprKind::CloneIn { region, .. } = &mut clone.kind else {
        panic!("named region fixture lost clone_in")
    };
    region.ty = Ty::Bool;
    assert!(!body_core_metadata_is_valid(&reject));

    let mut reject = program.clone();
    let expression = body_value_expression_mut(
        &mut reject,
        "native_named_region_materialization",
    );
    let hir::ExprKind::NamedArena { block, .. } = &mut expression.kind else {
        panic!("named region fixture lost its arena")
    };
    let hir::Stmt::Expr(builder) = &mut block.stmts[1] else {
        panic!("named region fixture lost its builder")
    };
    let hir::ExprKind::ArrayBuilderNew { elem, .. } = &mut builder.kind else {
        panic!("named region fixture lost its builder")
    };
    *elem = ArrayBuilderElem::Scalar(Scalar::String);
    builder.ty = Ty::ArrayBuilder(Scalar::String);
    assert!(!body_core_metadata_is_valid(&reject));

    let mut reject = program.clone();
    let expression = body_statement_expression_mut(
        &mut reject,
        "native_aggregate_array_builder_build",
    );
    expression.ty = Ty::dyn_aggregate_array(AggregateArrayElem::Mask(scalar_int(32), 4));
    assert!(!body_core_metadata_is_valid(&reject));

    let mut reject = program.clone();
    reject.fns.push(body_test_named_function(
        "native_rand_shuffle_readonly_slice",
        hir::Block {
            stmts: vec![hir::Stmt::Let {
                local: 1,
                init: body_test_expr(
                    hir::ExprKind::StrBytes {
                        inner: Box::new(native_str()),
                    },
                    Ty::Slice(Scalar::Int(IntTy { bits: 8, signed: false })),
                ),
            }],
            value: Some(Box::new(body_test_expr(
                hir::ExprKind::RandShuffle {
                    rng: Box::new(native_local(0, Ty::Rng)),
                    xs: Box::new(native_local(
                        1,
                        Ty::Slice(Scalar::Int(IntTy { bits: 8, signed: false })),
                    )),
                    elem: Ty::Int(IntTy { bits: 8, signed: false }),
                },
                Ty::Unit,
            ))),
        },
        vec![
            body_test_local(0, "rng", Ty::Rng, true, false),
            body_test_local(
                1,
                "xs",
                Ty::Slice(Scalar::Int(IntTy { bits: 8, signed: false })),
                true,
                false,
            ),
        ],
        Ty::Unit,
    ));
    assert!(!body_core_metadata_is_valid(&reject));

    let mut reject = program.clone();
    reject.fns.push(body_tail_case(
        "native_writer_buffered_temporary_write",
        body_test_expr(
            hir::ExprKind::WriterWrite {
                writer: Box::new(body_test_expr(
                    hir::ExprKind::WriterStd {
                        fd: 1,
                        buffered: true,
                    },
                    Ty::Writer,
                )),
                arg: Box::new(native_str()),
                builder: false,
            },
            result_unit,
        ),
        result_unit,
    ));
    assert!(!body_core_metadata_is_valid(&reject));

    let mut deferred = program.clone();
    let native = deferred
        .fns
        .iter_mut()
        .find(|function| function.name.as_str() == "native_buffer_new")
        .expect("native buffer fixture");
    native.drop_locals = vec![u32::MAX];
    native.drop_individual_locals = vec![u32::MAX];
    native
        .drop_individual_exprs
        .insert(align_span::Span::new(999, 4, 5), true);
    deferred.fn_types[0].effect.set(FnEffect::Impure);
    assert!(body_core_metadata_is_valid(&deferred));
}

#[test]
fn hir_body_validator_generated_callables() {
    let integer = int(64);
    let mut program = baseline_program();
    let error = push_builtin_error(&mut program);
    let imported_fid = program.fn_types.len() as u32;
    program
        .fn_types
        .push(body_fn_type(vec![(align_ast::ParamMode::ByValue, scalar_int(64))], integer));
    let closure_fid = program.fn_types.len() as u32;
    program.fn_types.push(body_fn_type(Vec::new(), integer));
    let map_err_fid = program.fn_types.len() as u32;
    program.fn_types.push(body_fn_type(
        vec![(align_ast::ParamMode::ByValue, Scalar::Enum(error))],
        Ty::Str,
    ));
    let extern_fid = program.fn_types.len() as u32;
    program.fn_types.push(fn_type(integer));

    program.imported_fns.push(imported_fn("dep$generated", vec![integer], integer));
    program.externs.push(hir::ExternFn {
        name: "c$generated".to_string(),
        params: vec![integer],
        param_modes: vec![align_ast::ParamMode::ByValue],
        ret: integer,
        return_borrow: ReturnBorrowSummary::None,
        return_region: ReturnRegionSummary::None,
        return_cleanup: hir::ReturnCleanupAbi::None,
    });
    program.fns.push(body_unit_case("generated_source", body_test_expr(hir::ExprKind::Unit, Ty::Unit)));
    let mut lifted = body_test_parameter_function(
        "generated_lifted",
        integer,
        hir::Block {
            stmts: Vec::new(),
            value: Some(Box::new(body_test_expr(hir::ExprKind::Local(0), integer))),
        },
        integer,
    );
    lifted.origin = hir::FnOrigin::Lifted { capture_count: 1 };
    lifted.locals[0].is_param = false;
    program.fns.push(lifted);
    program.fns.push(body_test_parameter_function(
        "generated_map_err",
        Ty::Enum(error),
        hir::Block {
            stmts: Vec::new(),
            value: Some(Box::new(body_test_expr(hir::ExprKind::Str("mapped".to_string()), Ty::Str))),
        },
        Ty::Str,
    ));
    let mut monomorph = body_tail_case(
        "generated$pick$i64",
        body_test_expr(hir::ExprKind::Int(1), integer),
        integer,
    );
    monomorph.origin = hir::FnOrigin::Monomorph;
    program.fns.push(monomorph);

    program.fns.push(body_unit_case(
        "generated_fn_value_source",
        body_test_expr(
            hir::ExprKind::FnValue("generated_source".to_string()),
            Ty::Fn(0),
        ),
    ));
    program.fns.push(body_unit_case(
        "generated_fn_value_imported",
        body_test_expr(
            hir::ExprKind::FnValue("dep$generated".to_string()),
            Ty::Fn(imported_fid),
        ),
    ));
    program.fns.push(body_unit_case(
        "generated_closure",
        body_test_expr(
            hir::ExprKind::Closure {
                lifted: "generated_lifted".to_string(),
                captures: vec![native_i64()],
            },
            Ty::Fn(closure_fid),
        ),
    ));
    program.fns.push(body_tail_case(
        "generated_call_fn_value",
        body_test_expr(
            hir::ExprKind::CallFnValue {
                callee: Box::new(body_test_expr(
                    hir::ExprKind::FnValue("dep$generated".to_string()),
                    Ty::Fn(imported_fid),
                )),
                args: vec![native_i64()],
            },
            integer,
        ),
        integer,
    ));
    let input_result = Ty::Result(scalar_int(64), Scalar::Enum(error));
    let mapped_result = Ty::Result(scalar_int(64), Scalar::Str);
    program.fns.push(body_tail_case(
        "generated_map_err_call",
        body_test_expr(
            hir::ExprKind::ResultMapErr {
                result: Box::new(body_test_expr(
                    hir::ExprKind::ResultOk(Box::new(native_i64())),
                    input_result,
                )),
                f: Box::new(body_test_expr(
                    hir::ExprKind::FnValue("generated_map_err".to_string()),
                    Ty::Fn(map_err_fid),
                )),
            },
            mapped_result,
        ),
        mapped_result,
    ));
    program.fns.push(body_tail_case(
        "generated_direct_source_call",
        body_test_expr(
            hir::ExprKind::Call {
                func: "generated_source".to_string(),
                args: Vec::new(),
                type_args: Vec::new(),
            },
            Ty::Unit,
        ),
        Ty::Unit,
    ));
    program.fns.push(body_tail_case(
        "generated_direct_imported_call",
        body_test_expr(
            hir::ExprKind::Call {
                func: "dep$generated".to_string(),
                args: vec![native_i64()],
                type_args: Vec::new(),
            },
            integer,
        ),
        integer,
    ));
    program.fns.push(body_tail_case(
        "generated_direct_monomorph_call",
        body_test_expr(
            hir::ExprKind::Call {
                func: "generated$pick$i64".to_string(),
                args: Vec::new(),
                type_args: vec![integer],
            },
            integer,
        ),
        integer,
    ));
    program.fns.push(body_tail_case(
        "generated_direct_extern_call",
        body_test_expr(
            hir::ExprKind::Unsafe(hir::Block {
                stmts: Vec::new(),
                value: Some(Box::new(body_test_expr(
                    hir::ExprKind::Call {
                        func: "c$generated".to_string(),
                        args: vec![native_i64()],
                        type_args: Vec::new(),
                    },
                    integer,
                ))),
            }),
            integer,
        ),
        integer,
    ));
    let dyn_int = Ty::DynArray(scalar_int(64));
    program.imported_fns.push(imported_fn("dep$generated_map", vec![integer], integer));
    program.fns.push(body_test_parameter_function(
        "generated_pipeline_imported",
        dyn_int,
        hir::Block {
            stmts: Vec::new(),
            value: Some(Box::new(body_test_expr(
                hir::ExprKind::ArraySum {
                    source: Box::new(native_local(0, dyn_int)),
                    stages: vec![hir::Stage {
                        kind: hir::StageKind::Map {
                            func: "dep$generated_map".to_string(),
                            captures: Vec::new(),
                        },
                        out_ty: integer,
                    }],
                },
                integer,
            ))),
        },
        integer,
    ));
    program.externs.push(hir::ExternFn {
        name: "c$generated_map".to_string(),
        params: vec![integer],
        param_modes: vec![align_ast::ParamMode::ByValue],
        ret: integer,
        return_borrow: ReturnBorrowSummary::None,
        return_region: ReturnRegionSummary::None,
        return_cleanup: hir::ReturnCleanupAbi::None,
    });
    program.fns.push(body_test_parameter_function(
        "generated_pipeline_extern",
        dyn_int,
        hir::Block {
            stmts: Vec::new(),
            value: Some(Box::new(body_test_expr(
                hir::ExprKind::Unsafe(hir::Block {
                    stmts: Vec::new(),
                    value: Some(Box::new(body_test_expr(
                        hir::ExprKind::ArraySum {
                            source: Box::new(native_local(0, dyn_int)),
                            stages: vec![hir::Stage {
                                kind: hir::StageKind::Map {
                                    func: "c$generated_map".to_string(),
                                    captures: Vec::new(),
                                },
                                out_ty: integer,
                            }],
                        },
                        integer,
                    ))),
                }),
                integer,
            ))),
        },
        integer,
    ));

    assert!(body_core_metadata_is_valid(&program));

    let mut reject = program.clone();
    reject.fns.push(body_unit_case(
        "generated_extern_fn_value_rejected",
        body_test_expr(
            hir::ExprKind::Unsafe(hir::Block {
                stmts: Vec::new(),
                value: Some(Box::new(body_test_expr(
                    hir::ExprKind::FnValue("c$generated".to_string()),
                    Ty::Fn(extern_fid),
                ))),
            }),
            Ty::Fn(extern_fid),
        ),
    ));
    assert!(!body_core_metadata_is_valid(&reject));

    let mut reject = program.clone();
    reject.fns.push(body_tail_case(
        "generated_extern_direct_without_unsafe",
        body_test_expr(
            hir::ExprKind::Call {
                func: "c$generated".to_string(),
                args: vec![native_i64()],
                type_args: Vec::new(),
            },
            integer,
        ),
        integer,
    ));
    assert!(!body_core_metadata_is_valid(&reject));

    let mut reject = program.clone();
    let expression = body_statement_expression_mut(&mut reject, "generated_closure");
    let hir::ExprKind::Closure { lifted, .. } = &mut expression.kind else {
        panic!("generated closure fixture lost its closure")
    };
    *lifted = "c$generated".to_string();
    assert!(!body_core_metadata_is_valid(&reject));

    let mut reject = program.clone();
    reject.fn_types[imported_fid as usize].ret = Ty::Bool;
    assert!(!body_core_metadata_is_valid(&reject));

    let mut reject = program.clone();
    let target = reject
        .fns
        .iter_mut()
        .find(|function| function.name.as_str() == "generated_source")
        .expect("generated source target");
    target.origin = hir::FnOrigin::Monomorph;
    assert!(!body_core_metadata_is_valid(&reject));

    let mut reject = program.clone();
    let expression = body_value_expression_mut(&mut reject, "generated_direct_monomorph_call");
    let hir::ExprKind::Call { type_args, .. } = &mut expression.kind else {
        panic!("generated monomorph fixture lost its call")
    };
    type_args[0] = Ty::Bool;
    assert!(!body_core_metadata_is_valid(&reject));
}

#[test]
fn hir_body_validator_native_control_flow() {
    let integer = int(64);
    let mut program = baseline_program();
    let error = push_builtin_error(&mut program);
    program.externs.push(hir::ExternFn {
        name: "c$control".to_string(),
        params: Vec::new(),
        param_modes: Vec::new(),
        ret: Ty::Unit,
        return_borrow: ReturnBorrowSummary::None,
        return_region: ReturnRegionSummary::None,
        return_cleanup: hir::ReturnCleanupAbi::None,
    });
    program.fns.push(body_tail_case(
        "native_control_exit",
        body_test_expr(
            hir::ExprKind::ProcessExit {
                code: Box::new(native_i64()),
            },
            Ty::Unit,
        ),
        integer,
    ));
    program.fns.push(body_tail_case(
        "native_control_abort",
        body_test_expr(hir::ExprKind::ProcessAbort, Ty::Unit),
        integer,
    ));
    program.fns.push(body_tail_case(
        "native_control_branch",
        body_test_expr(
            hir::ExprKind::If {
                cond: Box::new(body_test_expr(hir::ExprKind::Bool(true), Ty::Bool)),
                then: hir::Block {
                    stmts: Vec::new(),
                    value: Some(Box::new(body_test_expr(
                        hir::ExprKind::ProcessExit {
                            code: Box::new(native_i64()),
                        },
                        Ty::Unit,
                    ))),
                },
                els: hir::Block {
                    stmts: Vec::new(),
                    value: Some(Box::new(native_i64())),
                },
            },
            integer,
        ),
        integer,
    ));
    program.fns.push(body_tail_case(
        "native_control_loop",
        body_test_expr(
            hir::ExprKind::Loop {
                body: hir::Block {
                    stmts: vec![hir::Stmt::Expr(body_test_expr(
                        hir::ExprKind::ProcessExit {
                            code: Box::new(native_i64()),
                        },
                        Ty::Unit,
                    ))],
                    value: None,
                },
                diverges: true,
                body_locals: 0..0,
            },
            integer,
        ),
        integer,
    ));
    program.fns.push(body_tail_case(
        "native_control_arena_view",
        body_test_expr(
            hir::ExprKind::Arena(hir::Block {
                stmts: Vec::new(),
                value: Some(Box::new(body_test_expr(
                    hir::ExprKind::FsReadFileView {
                        path: Box::new(native_str()),
                    },
                    Ty::Result(Scalar::Str, Scalar::Enum(error)),
                ))),
            }),
            Ty::Result(Scalar::Str, Scalar::Enum(error)),
        ),
        Ty::Result(Scalar::Str, Scalar::Enum(error)),
    ));
    program.fns.push(body_tail_case(
        "native_control_unsafe_extern",
        body_test_expr(
            hir::ExprKind::Unsafe(hir::Block {
                stmts: Vec::new(),
                value: Some(Box::new(body_test_expr(
                    hir::ExprKind::Call {
                        func: "c$control".to_string(),
                        args: Vec::new(),
                        type_args: Vec::new(),
                    },
                    Ty::Unit,
                ))),
            }),
            Ty::Unit,
        ),
        Ty::Unit,
    ));
    assert!(body_core_metadata_is_valid(&program));

    let mut reject = program.clone();
    let expression = body_value_expression_mut(&mut reject, "native_control_arena_view");
    let hir::ExprKind::Arena(block) = &mut expression.kind else {
        panic!("native arena fixture lost its arena")
    };
    let block = std::mem::replace(block, hir::Block {
        stmts: Vec::new(),
        value: None,
    });
    expression.kind = hir::ExprKind::Block(block);
    assert!(!body_core_metadata_is_valid(&reject));

    let mut reject = program.clone();
    let expression = body_value_expression_mut(&mut reject, "native_control_unsafe_extern");
    let hir::ExprKind::Unsafe(block) = &mut expression.kind else {
        panic!("native unsafe fixture lost its unsafe block")
    };
    let block = std::mem::replace(block, hir::Block {
        stmts: Vec::new(),
        value: None,
    });
    expression.kind = hir::ExprKind::Block(block);
    assert!(!body_core_metadata_is_valid(&reject));

    let mut reject = program.clone();
    let expression = body_value_expression_mut(&mut reject, "native_control_exit");
    let hir::ExprKind::ProcessExit { code } = &mut expression.kind else {
        panic!("native exit fixture lost its exit")
    };
    code.ty = Ty::Bool;
    assert!(!body_core_metadata_is_valid(&reject));

    let mut reject = program.clone();
    reject.fns.push(body_test_named_function(
        "native_control_retained_malformed_child",
        hir::Block {
            stmts: vec![
                hir::Stmt::Expr(body_test_expr(
                    hir::ExprKind::ProcessExit {
                        code: Box::new(native_i64()),
                    },
                    Ty::Unit,
                )),
                hir::Stmt::Expr(body_test_expr(hir::ExprKind::Int(1), Ty::Bool)),
            ],
            value: Some(Box::new(body_test_expr(hir::ExprKind::Unit, Ty::Unit))),
        },
        Vec::new(),
        Ty::Unit,
    ));
    assert!(!body_core_metadata_is_valid(&reject));

}

#[test]
fn deep_hir_body_native_type_dag_is_stack_bounded() {
    const DEPTH: usize = 4_096;
    let mut program = baseline_program();
    let base = program.structs.len() as u32;
    program.structs.extend((0..DEPTH).map(|index| StructDef {
        name: format!("NativeNode{index}"),
        source_name: format!("NativeNode{index}"),
        fields: vec![FieldDef {
            name: if index + 1 == DEPTH {
                "ctx".to_string()
            } else {
                "next".to_string()
            },
            ty: if index + 1 == DEPTH {
                Ty::HttpRequestCtx
            } else {
                Ty::Struct(base + index as u32 + 1)
            },
        }],
        align: None,
        c_repr: false,
    }));
    let path = vec![0; DEPTH];
    program.fns.push(body_test_named_function(
        "deep_native_ctx",
        hir::Block {
            stmts: Vec::new(),
            value: Some(Box::new(body_test_expr(
                hir::ExprKind::HttpCtxMethod {
                    ctx: Box::new(body_test_expr(
                        hir::ExprKind::Field { root: 0, path },
                        Ty::HttpRequestCtx,
                    )),
                },
                Ty::Str,
            ))),
        },
        vec![body_test_local(0, "root", Ty::Struct(base), false, false)],
        Ty::Str,
    ));
    let handle = std::thread::Builder::new()
        .name("deep-native-body".to_string())
        .stack_size(2 * 1024 * 1024)
        .spawn(move || assert!(body_core_metadata_is_valid(&program)))
        .expect("spawn deep native validator");
    handle.join().expect("join deep native validator");

    let mut malformed = baseline_program();
    let base = malformed.structs.len() as u32;
    malformed.structs.extend((0..DEPTH).map(|index| StructDef {
        name: format!("NativeMalformedNode{index}"),
        source_name: format!("NativeMalformedNode{index}"),
        fields: vec![FieldDef {
            name: if index + 1 == DEPTH {
                "ctx".to_string()
            } else {
                "next".to_string()
            },
            ty: if index + 1 == DEPTH {
                Ty::HttpRequestCtx
            } else {
                Ty::Struct(base + index as u32 + 1)
            },
        }],
        align: None,
        c_repr: false,
    }));
    malformed.fns.push(body_test_named_function(
        "deep_native_valid_first",
        hir::Block {
            stmts: Vec::new(),
            value: Some(Box::new(body_test_expr(
                hir::ExprKind::HttpCtxMethod {
                    ctx: Box::new(body_test_expr(
                        hir::ExprKind::Field {
                            root: 0,
                            path: vec![0; DEPTH],
                        },
                        Ty::HttpRequestCtx,
                    )),
                },
                Ty::Str,
            ))),
        },
        vec![body_test_local(0, "root", Ty::Struct(base), false, false)],
        Ty::Str,
    ));
    malformed.fns.push(body_test_named_function(
        "deep_native_malformed_later",
        hir::Block {
            stmts: Vec::new(),
            value: Some(Box::new(body_test_expr(
                hir::ExprKind::HttpCtxMethod {
                    ctx: Box::new(body_test_expr(
                        hir::ExprKind::Field {
                            root: 0,
                            path: {
                                let mut path = vec![0; DEPTH];
                                path.push(0);
                                path
                            },
                        },
                        Ty::HttpRequestCtx,
                    )),
                },
                Ty::Str,
            ))),
        },
        vec![body_test_local(0, "root", Ty::Struct(base), false, false)],
        Ty::Str,
    ));
    let handle = std::thread::Builder::new()
        .name("deep-native-malformed-body".to_string())
        .stack_size(2 * 1024 * 1024)
        .spawn(move || assert!(!body_core_metadata_is_valid(&malformed)))
        .expect("spawn deep malformed native validator");
    handle.join().expect("join deep malformed native validator");
}

#[test]
fn deep_hir_body_pipeline_b2b2_type_dag_is_stack_bounded() {
    let depth = 512usize;
    let mut program = baseline_program();
    let error_id = push_builtin_error(&mut program);
    program.structs = (0..depth)
        .map(|id| StructDef {
            name: format!("JsonNode{id}"),
            source_name: format!("JsonNode{id}"),
            fields: vec![FieldDef {
                name: if id + 1 == depth {
                    "value".to_string()
                } else {
                    "next".to_string()
                },
                ty: if id + 1 == depth {
                    Ty::Str
                } else {
                    Ty::Struct((id + 1) as u32)
                },
            }],
            align: None,
            c_repr: false,
        })
        .collect();
    let result = Ty::Result(Scalar::Struct(0), Scalar::Enum(error_id));
    program.fns.push(body_tail_case(
        "b2b2_deep_json_decode",
        body_test_expr(
            hir::ExprKind::JsonDecode {
                struct_id: 0,
                input: Box::new(body_test_expr(hir::ExprKind::Str("{}".to_string()), Ty::Str)),
            },
            result,
        ),
        result,
    ));
    let handle = std::thread::Builder::new()
        .stack_size(2 * 1024 * 1024)
        .spawn(move || assert!(body_core_metadata_is_valid(&program)))
        .expect("spawn deep b2b2 descriptor validator");
    handle.join().expect("join deep b2b2 descriptor validator");
}

#[test]
fn hir_body_validator_pipeline_deferred_facts_are_not_consumed() {
    let integer = int(64);
    let scalar = scalar_int(64);
    let mut program = baseline_program();
    program.fns.push(body_unit_case(
        "storage_deferred_facts",
        body_test_expr(
            hir::ExprKind::ArrayLit {
                elems: vec![
                    body_test_expr(hir::ExprKind::Int(1), integer),
                    body_test_expr(hir::ExprKind::Int(2), integer),
                ],
                elem: integer,
                pooled: false,
            },
            Ty::Array(scalar, 2),
        ),
    ));
    let template_index = program.fns.len();
    program.fns.push(body_tail_case(
        "template_deferred_facts",
        body_test_expr(
            hir::ExprKind::Template(vec![hir::TemplatePart::Text("x".to_string())]),
            Ty::Str,
        ),
        Ty::Str,
    ));
    assert!(body_core_metadata_is_valid(&program));
    program.fns[0].drop_locals = vec![0, 0, 1];
    program.fns[0].drop_individual_locals = vec![0];
    program.fns[0]
        .drop_individual_exprs
        .insert(align_span::Span::new(0, 4, 5), true);
    program.fns[template_index].drop_locals = vec![1, 0, 1];
    program.fns[template_index].drop_individual_locals = vec![1];
    program.fns[template_index]
        .drop_individual_exprs
        .insert(align_span::Span::new(0, 8, 9), false);
    assert!(body_core_metadata_is_valid(&program));
}

#[test]
fn deep_hir_body_storage_type_dag_is_stack_bounded() {
    let integer = int(64);
    const TYPE_DEPTH: usize = 4_096;
    let mut program = baseline_program();
    program.structs = (0..TYPE_DEPTH)
        .map(|id| StructDef {
            name: format!("StorageNode{id}"),
            source_name: format!("StorageNode{id}"),
            fields: vec![FieldDef {
                name: "next".to_string(),
                ty: if id + 1 == TYPE_DEPTH {
                    integer
                } else {
                    Ty::Struct((id + 1) as u32)
                },
            }],
            align: None,
            c_repr: false,
        })
        .collect();
    program.fns.push(body_tail_case(
        "storage_type_depth",
        body_test_expr(
            hir::ExprKind::VecLit {
                elems: vec![
                    body_test_expr(hir::ExprKind::Int(1), integer),
                    body_test_expr(hir::ExprKind::Int(2), integer),
                ],
                elem: scalar_int(64),
            },
            Ty::Vec(scalar_int(64), 2),
        ),
        Ty::Vec(scalar_int(64), 2),
    ));
    let handle = std::thread::Builder::new()
        .stack_size(2 * 1024 * 1024)
        .spawn(move || {
            assert!(body_core_metadata_is_valid(&program));
            program.structs[TYPE_DEPTH - 1].fields[0].ty = Ty::Struct(TYPE_DEPTH as u32);
            assert!(!body_core_metadata_is_valid(&program));
        })
        .expect("spawn deep storage body validator");
    handle.join().expect("join deep storage body validator");
}

#[test]
fn hir_body_validator_statements() {
    let integer = int(64);
    let mut program = baseline_program();
    let drop_old = Cell::new(false);
    let drop_new = Cell::new(false);
    program.fns.push(body_test_function(
        hir::Block {
            stmts: vec![
                hir::Stmt::Let {
                    local: 0,
                    init: body_test_expr(hir::ExprKind::Int(1), integer),
                },
                hir::Stmt::Assign {
                    local: 0,
                    value: body_test_expr(hir::ExprKind::Int(2), integer),
                    drop_old,
                    drop_new,
                },
            ],
            value: Some(Box::new(body_test_expr(hir::ExprKind::Local(0), integer))),
        },
        vec![hir::Local {
            id: 0,
            name: "x".to_string(),
            ty: integer,
            is_mut: true,
            is_param: false,
            align: None,
        }],
        integer,
    ));
    assert!(body_core_metadata_is_valid(&program));
}

#[test]
fn hir_body_validator_statement_inventory() {
    let integer = int(64);
    let unit = Ty::Unit;
    let span = align_span::Span::new(0, 0, 0);
    let mut program = baseline_program();
    let no_tail = |name: &str, statement: hir::Stmt, locals: Vec<hir::Local>, ret: Ty| {
        body_test_named_function(
            name,
            hir::Block {
                stmts: vec![statement],
                value: None,
            },
            locals,
            ret,
        )
    };
    let local = |id: u32, name: &str, ty: Ty, is_mut: bool| hir::Local {
        id,
        name: name.to_string(),
        ty,
        is_mut,
        is_param: false,
        align: None,
    };
    let expr = |kind: hir::ExprKind, ty: Ty| hir::Expr { kind, ty, span };

    program.fns.push(no_tail(
        "stmt_let",
        hir::Stmt::Let {
            local: 0,
            init: expr(hir::ExprKind::Int(1), integer),
        },
        vec![local(0, "value", integer, false)],
        unit,
    ));
    program.fns.push(no_tail(
        "stmt_let_tuple",
        hir::Stmt::LetTuple {
            locals: vec![Some(0), None],
            tuple_id: 0,
            init: expr(
                hir::ExprKind::Tuple {
                    tuple_id: 0,
                    elems: vec![
                        expr(hir::ExprKind::Int(1), integer),
                        expr(hir::ExprKind::Bool(true), Ty::Bool),
                    ],
                },
                Ty::Tuple(0),
            ),
        },
        vec![local(0, "first", integer, false), local(1, "second", Ty::Bool, false)],
        unit,
    ));
    program.fns.push(no_tail(
        "stmt_assign",
        hir::Stmt::Assign {
            local: 0,
            value: expr(hir::ExprKind::Int(2), integer),
            drop_old: std::cell::Cell::new(false),
            drop_new: std::cell::Cell::new(false),
        },
        vec![local(0, "value", integer, true)],
        unit,
    ));
    program.fns.push(no_tail(
        "stmt_assign_index",
        hir::Stmt::AssignIndex {
            base: 0,
            index: expr(hir::ExprKind::Int(0), integer),
            value: expr(hir::ExprKind::Int(3), integer),
        },
        vec![local(0, "values", Ty::Array(scalar_int(64), 3), true)],
        unit,
    ));
    program.fns.push(no_tail(
        "stmt_assign_vec_lane",
        hir::Stmt::AssignVecLane {
            local: 0,
            lane: 2,
            value: expr(hir::ExprKind::Int(4), integer),
        },
        vec![local(0, "lanes", Ty::Vec(scalar_int(64), 4), true)],
        unit,
    ));
    program.fns.push(no_tail(
        "stmt_assign_field",
        hir::Stmt::AssignField {
            root: 0,
            path: vec![1],
            value: expr(hir::ExprKind::Int(5), integer),
        },
        vec![local(0, "record", Ty::Struct(0), true)],
        unit,
    ));
    program.fns.push(no_tail(
        "stmt_assign_elem_field",
        hir::Stmt::AssignElemField {
            base: 0,
            index: expr(hir::ExprKind::Int(0), integer),
            path: vec![1],
            struct_id: 0,
            soa: false,
            value: expr(hir::ExprKind::Int(6), integer),
        },
        vec![local(0, "records", Ty::StructArray(0, 2), true)],
        unit,
    ));
    program.fns.push(no_tail(
        "stmt_assign_elem",
        hir::Stmt::AssignElem {
            base: 0,
            index: expr(hir::ExprKind::Int(0), integer),
            struct_id: 0,
            soa: true,
            value: expr(
                hir::ExprKind::StructLit {
                    struct_id: 0,
                    fields: vec![
                        expr(hir::ExprKind::Str("key".to_string()), Ty::Str),
                        expr(hir::ExprKind::Int(7), integer),
                    ],
                },
                Ty::Struct(0),
            ),
        },
        vec![local(0, "columns", Ty::Soa(0), true)],
        unit,
    ));
    program.fns.push(no_tail(
        "stmt_return_none",
        hir::Stmt::Return(None),
        Vec::new(),
        unit,
    ));
    program.fns.push(no_tail(
        "stmt_return_some",
        hir::Stmt::Return(Some(expr(hir::ExprKind::Int(8), integer))),
        Vec::new(),
        integer,
    ));
    program.fns.push(body_tail_case(
        "stmt_break_value",
        expr(
            hir::ExprKind::Loop {
                body: hir::Block {
                    stmts: vec![hir::Stmt::Break {
                        value: Some(expr(hir::ExprKind::Int(9), integer)),
                        accepted: true,
                    }],
                    value: None,
                },
                diverges: false,
                body_locals: 0..0,
            },
            integer,
        ),
        integer,
    ));
    program.fns.push(no_tail(
        "stmt_break_rejected",
        hir::Stmt::Break {
            value: None,
            accepted: false,
        },
        Vec::new(),
        integer,
    ));
    program.fns.push(no_tail(
        "stmt_expr",
        hir::Stmt::Expr(expr(hir::ExprKind::Int(10), integer)),
        Vec::new(),
        unit,
    ));

    assert!(body_core_metadata_is_valid(&program));

    let mut reject = program.clone();
    match body_first_statement_mut(&mut reject, "stmt_let") {
        hir::Stmt::Let { local, .. } => *local = 99,
        _ => unreachable!(),
    }
    assert!(!body_core_metadata_is_valid(&reject));
    let mut reject = program.clone();
    match body_first_statement_mut(&mut reject, "stmt_let") {
        hir::Stmt::Let { init, .. } => init.ty = Ty::Bool,
        _ => unreachable!(),
    }
    assert!(!body_core_metadata_is_valid(&reject));

    let mut reject = program.clone();
    match body_first_statement_mut(&mut reject, "stmt_let_tuple") {
        hir::Stmt::LetTuple { locals, .. } => *locals = vec![Some(0)],
        _ => unreachable!(),
    }
    assert!(!body_core_metadata_is_valid(&reject));
    let mut reject = program.clone();
    match body_first_statement_mut(&mut reject, "stmt_let_tuple") {
        hir::Stmt::LetTuple { tuple_id, .. } => *tuple_id = 99,
        _ => unreachable!(),
    }
    assert!(!body_core_metadata_is_valid(&reject));
    let mut reject = program.clone();
    match body_first_statement_mut(&mut reject, "stmt_let_tuple") {
        hir::Stmt::LetTuple { init, .. } => init.ty = Ty::Bool,
        _ => unreachable!(),
    }
    assert!(!body_core_metadata_is_valid(&reject));

    let mut reject = program.clone();
    match body_first_statement_mut(&mut reject, "stmt_assign") {
        hir::Stmt::Assign { local, .. } => *local = 99,
        _ => unreachable!(),
    }
    assert!(!body_core_metadata_is_valid(&reject));
    let mut reject = program.clone();
    match body_first_statement_mut(&mut reject, "stmt_assign") {
        hir::Stmt::Assign { value, .. } => value.ty = Ty::Bool,
        _ => unreachable!(),
    }
    assert!(!body_core_metadata_is_valid(&reject));

    let mut reject = program.clone();
    match body_first_statement_mut(&mut reject, "stmt_assign_index") {
        hir::Stmt::AssignIndex { base, .. } => *base = 99,
        _ => unreachable!(),
    }
    assert!(!body_core_metadata_is_valid(&reject));
    let mut reject = program.clone();
    match body_first_statement_mut(&mut reject, "stmt_assign_index") {
        hir::Stmt::AssignIndex { index, .. } => index.ty = Ty::Bool,
        _ => unreachable!(),
    }
    assert!(!body_core_metadata_is_valid(&reject));
    let mut reject = program.clone();
    match body_first_statement_mut(&mut reject, "stmt_assign_index") {
        hir::Stmt::AssignIndex { value, .. } => value.ty = Ty::Bool,
        _ => unreachable!(),
    }
    assert!(!body_core_metadata_is_valid(&reject));

    let mut reject = program.clone();
    match body_first_statement_mut(&mut reject, "stmt_assign_vec_lane") {
        hir::Stmt::AssignVecLane { local, .. } => *local = 99,
        _ => unreachable!(),
    }
    assert!(!body_core_metadata_is_valid(&reject));
    let mut reject = program.clone();
    match body_first_statement_mut(&mut reject, "stmt_assign_vec_lane") {
        hir::Stmt::AssignVecLane { lane, .. } => *lane = 4,
        _ => unreachable!(),
    }
    assert!(!body_core_metadata_is_valid(&reject));
    let mut reject = program.clone();
    match body_first_statement_mut(&mut reject, "stmt_assign_vec_lane") {
        hir::Stmt::AssignVecLane { value, .. } => value.ty = Ty::Bool,
        _ => unreachable!(),
    }
    assert!(!body_core_metadata_is_valid(&reject));

    let mut reject = program.clone();
    match body_first_statement_mut(&mut reject, "stmt_assign_field") {
        hir::Stmt::AssignField { root, .. } => *root = 99,
        _ => unreachable!(),
    }
    assert!(!body_core_metadata_is_valid(&reject));
    let mut reject = program.clone();
    match body_first_statement_mut(&mut reject, "stmt_assign_field") {
        hir::Stmt::AssignField { path, .. } => path.clear(),
        _ => unreachable!(),
    }
    assert!(!body_core_metadata_is_valid(&reject));
    let mut reject = program.clone();
    match body_first_statement_mut(&mut reject, "stmt_assign_field") {
        hir::Stmt::AssignField { value, .. } => value.ty = Ty::Bool,
        _ => unreachable!(),
    }
    assert!(!body_core_metadata_is_valid(&reject));

    let mut reject = program.clone();
    match body_first_statement_mut(&mut reject, "stmt_assign_elem_field") {
        hir::Stmt::AssignElemField { base, .. } => *base = 99,
        _ => unreachable!(),
    }
    assert!(!body_core_metadata_is_valid(&reject));
    let mut reject = program.clone();
    match body_first_statement_mut(&mut reject, "stmt_assign_elem_field") {
        hir::Stmt::AssignElemField { path, .. } => path.clear(),
        _ => unreachable!(),
    }
    assert!(!body_core_metadata_is_valid(&reject));
    let mut reject = program.clone();
    match body_first_statement_mut(&mut reject, "stmt_assign_elem_field") {
        hir::Stmt::AssignElemField { struct_id, .. } => *struct_id = 99,
        _ => unreachable!(),
    }
    assert!(!body_core_metadata_is_valid(&reject));
    let mut reject = program.clone();
    match body_first_statement_mut(&mut reject, "stmt_assign_elem_field") {
        hir::Stmt::AssignElemField { soa, .. } => *soa = true,
        _ => unreachable!(),
    }
    assert!(!body_core_metadata_is_valid(&reject));
    let mut reject = program.clone();
    match body_first_statement_mut(&mut reject, "stmt_assign_elem_field") {
        hir::Stmt::AssignElemField { index, .. } => index.ty = Ty::Bool,
        _ => unreachable!(),
    }
    assert!(!body_core_metadata_is_valid(&reject));
    let mut reject = program.clone();
    match body_first_statement_mut(&mut reject, "stmt_assign_elem_field") {
        hir::Stmt::AssignElemField { value, .. } => value.ty = Ty::Bool,
        _ => unreachable!(),
    }
    assert!(!body_core_metadata_is_valid(&reject));

    let mut reject = program.clone();
    match body_first_statement_mut(&mut reject, "stmt_assign_elem") {
        hir::Stmt::AssignElem { base, .. } => *base = 99,
        _ => unreachable!(),
    }
    assert!(!body_core_metadata_is_valid(&reject));
    let mut reject = program.clone();
    match body_first_statement_mut(&mut reject, "stmt_assign_elem") {
        hir::Stmt::AssignElem { struct_id, .. } => *struct_id = 99,
        _ => unreachable!(),
    }
    assert!(!body_core_metadata_is_valid(&reject));
    let mut reject = program.clone();
    match body_first_statement_mut(&mut reject, "stmt_assign_elem") {
        hir::Stmt::AssignElem { soa, .. } => *soa = false,
        _ => unreachable!(),
    }
    assert!(!body_core_metadata_is_valid(&reject));
    let mut reject = program.clone();
    match body_first_statement_mut(&mut reject, "stmt_assign_elem") {
        hir::Stmt::AssignElem { index, .. } => index.ty = Ty::Bool,
        _ => unreachable!(),
    }
    assert!(!body_core_metadata_is_valid(&reject));
    let mut reject = program.clone();
    match body_first_statement_mut(&mut reject, "stmt_assign_elem") {
        hir::Stmt::AssignElem { value, .. } => value.ty = Ty::Bool,
        _ => unreachable!(),
    }
    assert!(!body_core_metadata_is_valid(&reject));

    let mut reject = program.clone();
    *body_first_statement_mut(&mut reject, "stmt_return_none") = hir::Stmt::Return(Some(expr(
        hir::ExprKind::Int(11),
        integer,
    )));
    assert!(!body_core_metadata_is_valid(&reject));
    let mut reject = program.clone();
    *body_first_statement_mut(&mut reject, "stmt_return_some") = hir::Stmt::Return(None);
    assert!(!body_core_metadata_is_valid(&reject));
    let mut reject = program.clone();
    match body_first_statement_mut(&mut reject, "stmt_return_some") {
        hir::Stmt::Return(Some(value)) => value.ty = Ty::Bool,
        _ => unreachable!(),
    }
    assert!(!body_core_metadata_is_valid(&reject));

    let mut reject = program.clone();
    match body_loop_statement_mut(&mut reject, "stmt_break_value") {
        hir::Stmt::Break { value, .. } => *value = None,
        _ => unreachable!(),
    }
    assert!(!body_core_metadata_is_valid(&reject));
    let mut reject = program.clone();
    match body_first_statement_mut(&mut reject, "stmt_break_rejected") {
        hir::Stmt::Break { accepted, .. } => *accepted = true,
        _ => unreachable!(),
    }
    assert!(!body_core_metadata_is_valid(&reject));

    let mut reject = program.clone();
    match body_first_statement_mut(&mut reject, "stmt_expr") {
        hir::Stmt::Expr(expression) => {
            *expression = expr(
                hir::ExprKind::ResultOk(Box::new(expr(hir::ExprKind::Int(12), integer))),
                Ty::Result(scalar_int(64), Scalar::Enum(0)),
            )
        }
        _ => unreachable!(),
    }
    assert!(!body_core_metadata_is_valid(&reject));
}

#[test]
fn hir_body_type_mangle_golden_vectors() {
    let mut program = baseline_program();
    program.resources.push(ResourceDef {
        name: "pkg.db$rows$Row".to_string(),
        source_name: "pkg.db$rows$Row".to_string(),
        declaring_module: "pkg.db".to_string(),
        generic_arity: 1,
        drop_hook: "pkg.db.internal.resource$drop_rows".to_string(),
        drop_thunk: "__align_resource_drop$pkg.db$rows".to_string(),
        representation_version: 1,
        drop_abi_fingerprint: [0; 16],
    });
    let vectors = [
        (Ty::Int(IntTy { bits: 64, signed: true }), "i64"),
        (Ty::Struct(0), "S6_Record"),
        (Ty::Enum(0), "E6_Choice"),
        (Ty::Resource(0), "W15_pkg_db_rows_Row"),
        (Ty::ResourceRef(0), "J_W15_pkg_db_rows_Row"),
        (Ty::Tagged(0), "O_i64"),
        (Ty::Option(scalar_int(64)), "O_i64"),
        (Ty::Result(scalar_int(64), Scalar::Enum(0)), "R_i64_E6_Choice"),
        (Ty::Box(scalar_int(64)), "B_i64"),
        (Ty::Array(scalar_int(64), 3), "A3_i64"),
        (Ty::StructArray(0, 2), "A2_S6_Record"),
        (Ty::Slice(Scalar::Str), "V_str"),
        (Ty::DynArray(scalar_int(32)), "D_i32"),
        (Ty::DynStructArray(0, Layout::Aos), "D_S6_Record"),
        (Ty::Soa(0), "Q_S6_Record"),
        (Ty::Tuple(0), "U2_i64_bool"),
        // Types the producer spells through its sanitized display form; the validator's own
        // copy of this scheme spelled them without the trailing separator and turned valid
        // programs into empty units, so pin the producer's exact output.
        (Ty::Vec(scalar_int(32), 4), "vec4_i32_"),
        (Ty::Mask(scalar_int(32), 4), "mask4_i32_"),
        (Ty::JsonScanner(0), "json_scanner_struct_0_"),
        (Ty::ArrayBuilder(scalar_int(64)), "array_builder_i64_"),
        (Ty::Param(2), "_type_param_2_"),
        (Ty::Task(scalar_int(64)), "K_i64"),
        (Ty::Fn(0), "F0_____bn_rn"),
    ];
    for (ty, expected) in vectors {
        assert_eq!(body_ty_mangle(ty, &program), expected, "mangle for {ty:?}");
        // The validator must never hold a second model of this scheme: every name it checks was
        // minted by the producer, so the two must be the same function, not merely agree today.
        assert_eq!(
            body_ty_mangle(ty, &program),
            align_sema::ty_mangle(
                ty,
                &program.tagged_types,
                &program.structs,
                &program.enums,
                &program.tuples,
                &program.fn_types,
                &program.resources,
            ),
            "the validator must ask the producer for {ty:?}"
        );
    }

    let mut provenance_program = baseline_program();
    provenance_program.fn_types[0] = FnTy {
        params: vec![(align_ast::ParamMode::ByValue, Scalar::Str)],
        ret: Ty::Str,
        return_borrow: ReturnBorrowSummary::Roots {
            params: vec![0],
            captures: Vec::new(),
        },
        return_region: ReturnRegionSummary::Roots {
            params: vec![0],
            captures: Vec::new(),
        },
        return_cleanup: hir::ReturnCleanupAbi::None,
        effect: Cell::new(FnEffect::Pure),
    };
    assert_eq!(
        body_ty_mangle(Ty::Fn(0), &provenance_program),
        "F0_vstr_str_bp0_c_rp0_c"
    );
}

/// Compile-time tripwire for the delegated-predicate sweep below: a new [`Scalar`] variant fails
/// this match, which is the signal to add it to `delegation_scalar_samples`. A variant that only
/// ever reaches one of the two models (sema's rule and the checked-HIR gate that must ask it) is
/// how a checked program silently lowered to an empty unit three times.
#[allow(dead_code)]
const fn delegation_scalar_sweep_tripwire(scalar: &Scalar) {
    match *scalar {
        Scalar::Int { .. }
        | Scalar::Float { .. }
        | Scalar::Bool
        | Scalar::Char
        | Scalar::Unit
        | Scalar::Struct { .. }
        | Scalar::String
        | Scalar::DynArray { .. }
        | Scalar::DynStructArray { .. }
        | Scalar::DynResponseArray
        | Scalar::Str
        | Scalar::Slice { .. }
        | Scalar::Enum { .. }
        | Scalar::Tagged { .. }
        | Scalar::Soa { .. }
        | Scalar::SoaParam { .. }
        | Scalar::JsonDoc
        | Scalar::Param { .. }
        | Scalar::Reader
        | Scalar::Writer
        | Scalar::Buffer
        | Scalar::Regex
        | Scalar::Captures
        | Scalar::CliParsed
        | Scalar::TcpConn
        | Scalar::TcpListener
        | Scalar::UdpSocket
        | Scalar::Child
        | Scalar::File
        | Scalar::HttpResponse
        | Scalar::HttpServer
        | Scalar::HttpRequestCtx
        | Scalar::ResponseBuilder
        | Scalar::HttpStream
        | Scalar::RunOutput
        | Scalar::Fn { .. }
        | Scalar::Resource { .. }
        | Scalar::ResourceRef { .. } => {}
    }
}

/// One sample per [`Scalar`] variant, plus a Move and a Copy instance of every nominal variant,
/// against `delegation_program`'s tables.
fn delegation_scalar_samples() -> Vec<Scalar> {
    vec![
        scalar_int(8),
        scalar_int(64),
        Scalar::Int(IntTy {
            bits: 32,
            signed: false,
        }),
        Scalar::Float(FloatTy { bits: 32 }),
        Scalar::Float(FloatTy { bits: 64 }),
        Scalar::Bool,
        Scalar::Char,
        Scalar::Unit,
        Scalar::Struct(0),
        Scalar::Struct(1),
        Scalar::String,
        Scalar::DynArray(PrimScalar::Int(IntTy {
            bits: 64,
            signed: true,
        })),
        Scalar::DynStructArray(0),
        Scalar::DynResponseArray,
        Scalar::Str,
        Scalar::Slice(PrimScalar::Int(IntTy {
            bits: 64,
            signed: true,
        })),
        Scalar::Enum(0),
        Scalar::Enum(1),
        Scalar::Tagged(0),
        Scalar::Tagged(1),
        Scalar::Soa(0),
        Scalar::SoaParam(0),
        Scalar::JsonDoc,
        Scalar::Param(0),
        Scalar::Reader,
        Scalar::Writer,
        Scalar::Buffer,
        Scalar::Regex,
        Scalar::Captures,
        Scalar::CliParsed,
        Scalar::TcpConn,
        Scalar::TcpListener,
        Scalar::UdpSocket,
        Scalar::Child,
        Scalar::File,
        Scalar::HttpResponse,
        Scalar::HttpServer,
        Scalar::HttpRequestCtx,
        Scalar::ResponseBuilder,
        Scalar::HttpStream,
        Scalar::RunOutput,
        Scalar::Fn(0),
        Scalar::Resource(0),
        Scalar::ResourceRef(0),
    ]
}

/// `baseline_program` plus one Move instance of every nominal shape (a `string`-bearing struct,
/// sum type, tagged payload, and tuple), a package resource, and the struct shapes the `soa` gates
/// discriminate: `struct#0` Copy + `SoaPlain`, `struct#1` Move, `struct#2` non-plain field,
/// `struct#3` field-less, `struct#4` a `SoaPlain` field class with an impossible store width.
fn delegation_program() -> hir::Program {
    let mut program = baseline_program();
    program.structs.push(StructDef {
        name: "Owned".to_string(),
        source_name: "Owned".to_string(),
        fields: vec![FieldDef {
            name: "text".to_string(),
            ty: Ty::String,
        }],
        align: None,
        c_repr: false,
    });
    program.structs.push(StructDef {
        name: "Nested".to_string(),
        source_name: "Nested".to_string(),
        fields: vec![FieldDef {
            name: "inner".to_string(),
            ty: Ty::Struct(0),
        }],
        align: None,
        c_repr: false,
    });
    program.structs.push(StructDef {
        name: "Empty".to_string(),
        source_name: "Empty".to_string(),
        fields: Vec::new(),
        align: None,
        c_repr: false,
    });
    program.structs.push(StructDef {
        name: "OddWidth".to_string(),
        source_name: "OddWidth".to_string(),
        fields: vec![FieldDef {
            name: "value".to_string(),
            ty: Ty::Int(IntTy {
                bits: 7,
                signed: true,
            }),
        }],
        align: None,
        c_repr: false,
    });
    program.enums.push(EnumDef {
        name: "Owner".to_string(),
        source_name: "Owner".to_string(),
        variants: vec![
            EnumVariant {
                name: "Text".to_string(),
                payload: vec![Scalar::String],
                field_base: 1,
            },
            EnumVariant {
                name: "None".to_string(),
                payload: Vec::new(),
                field_base: 1,
            },
        ],
    });
    program.tagged_types.push(TaggedType::Option(Scalar::String));
    program.tuples.push(TupleDef {
        elems: vec![Scalar::String, scalar_int(64)],
    });
    program.resources.push(ResourceDef {
        name: "pkg.db$conn".to_string(),
        source_name: "pkg.db$conn".to_string(),
        declaring_module: "pkg.db".to_string(),
        generic_arity: 0,
        drop_hook: "pkg.db.internal.resource$drop_conn".to_string(),
        drop_thunk: "__align_resource_drop$pkg.db$conn".to_string(),
        representation_version: 1,
        drop_abi_fingerprint: [0; 16],
    });
    program
}

/// Every checked-HIR gate that re-derived a producer rule must now *ask* that rule. Sweep the
/// constructible type space and assert the two sides are the same function, not merely two lists
/// that agree today: a `Scalar`/`Ty` variant or a sema rule change that moves them apart fails
/// here instead of turning a checked program into an empty unit at the MIR boundary.
#[test]
fn delegated_ownership_and_shape_gates_agree_with_sema() {
    let program = delegation_program();
    let gates = crate::validate_hir::DelegatedGates::new(&program);
    let is_move = |scalar: Scalar| {
        align_sema::scalar_is_move(
            scalar,
            &program.structs,
            &program.enums,
            &program.tagged_types,
        )
    };
    let element_read_ok = |ty: Ty| {
        align_sema::collection_element_read_ok(
            ty,
            &program.structs,
            &program.tuples,
            &program.enums,
            &program.tagged_types,
        )
    };

    for scalar in delegation_scalar_samples() {
        assert_eq!(
            gates.scalar_copy_ok(scalar),
            !is_move(scalar),
            "the copy gate must be sema's Move rule for {scalar:?}"
        );
        // The element rule is asked at `Ty` granularity by `check_index` and at `Scalar`
        // granularity by `check_slice_range`; a gap between those two spellings is exactly the
        // Move-sum-type / resource element that sema accepted and this validator refused.
        let element = align_sema::scalar_to_ty(scalar);
        assert_eq!(
            element_read_ok(element),
            !is_move(scalar),
            "the element read rule must match the scalar Move rule for {scalar:?}"
        );
        assert_eq!(
            gates.collection_element_read_ok(element),
            gates.body_ty_ok(element) && element_read_ok(element),
            "the validator's element gate must be structural validity plus sema's rule for {scalar:?}"
        );
    }

    // A handle is never readable out of a collection: the copy would close the same fd twice.
    for &handle in align_sema::MOVE_HANDLE_TYPES {
        assert!(
            !element_read_ok(handle),
            "{handle:?} must never be readable as a collection element"
        );
    }

    // Aggregate elements own their contents element-wise, so the rule sees through them.
    for (element, expected) in [
        (Ty::Array(scalar_int(64), 2), true),
        (Ty::Array(Scalar::String, 2), false),
        (Ty::StructArray(0, 2), true),
        (Ty::StructArray(1, 2), false),
        (Ty::Tuple(0), true),
        (Ty::Tuple(1), false),
        (Ty::Box(scalar_int(64)), false),
        (
            Ty::ArrayBuilder(scalar_int(64)),
            false,
        ),
        (Ty::Option(scalar_int(64)), true),
        (Ty::Option(Scalar::String), false),
        (Ty::Result(scalar_int(64), Scalar::Enum(0)), true),
        (Ty::Result(scalar_int(64), Scalar::Enum(1)), false),
        (Ty::Str, true),
        (Ty::Soa(0), true),
    ] {
        assert_eq!(
            element_read_ok(element),
            expected,
            "element read rule for {element:?}"
        );
    }

    // The four soa gates are one rule (`soa_plain_ok`) plus, for the two store-bearing gates, the
    // validator's own field-width check. `struct#4` is the only shape where they may differ.
    for id in 0..program.structs.len() as u32 {
        let plain = align_sema::soa_plain_ok(id, &program.structs);
        let [placement, store, json, struct_array] = gates.soa_gates(id);
        assert_eq!(placement, plain, "soa placement gate for struct#{id}");
        assert_eq!(struct_array, plain, "soa struct-array gate for struct#{id}");
        assert_eq!(store, json, "the two store-bearing soa gates for struct#{id}");
        let widths_ok = program.structs[id as usize]
            .fields
            .iter()
            .all(|field| !matches!(field.ty, Ty::Int(integer) if integer.bits == 7));
        assert_eq!(
            store,
            plain && widths_ok,
            "the store-bearing soa gate is sema's shape plus a width check for struct#{id}"
        );
    }
    assert!(align_sema::soa_plain_ok(0, &program.structs));
    assert!(!align_sema::soa_plain_ok(1, &program.structs));
    assert!(!align_sema::soa_plain_ok(2, &program.structs));
    assert!(!align_sema::soa_plain_ok(3, &program.structs));
    assert!(align_sema::soa_plain_ok(4, &program.structs));
    assert!(!align_sema::soa_plain_ok(
        program.structs.len() as u32,
        &program.structs
    ));
}

#[test]
fn hir_body_validator_prepared_binder_bridge_fails_closed() {
    let mut program = baseline_program();
    let params = Ty::Struct(0);
    let statement_id = program.resources.len() as u32;
    program.resources.push(ResourceDef {
        name: "pkg.db$stmt$S6_Record$S6_Record".to_string(),
        source_name: "pkg.db$stmt$S6_Record$S6_Record".to_string(),
        declaring_module: "pkg.db".to_string(),
        generic_arity: 2,
        drop_hook: "pkg.db.internal.resource$drop_stmt".to_string(),
        drop_thunk: "__align_resource_drop$pkg.db$stmt$S6_Record$S6_Record".to_string(),
        representation_version: 1,
        drop_abi_fingerprint: *b"align-res-drop-1",
    });
    let statement = Ty::Resource(statement_id);
    let statement_ref = Ty::ResourceRef(statement_id);
    let i32_ty = Ty::Int(IntTy {
        bits: 32,
        signed: true,
    });
    let reference = body_test_expr(
        hir::ExprKind::ResourceBorrow {
            owner: Box::new(body_test_expr(hir::ExprKind::Local(0), statement)),
            resource: statement_id,
        },
        statement_ref,
    );
    let wrapper = body_test_expr(
        hir::ExprKind::ResourceRaw {
            reference: Box::new(reference),
            resource: statement_id,
        },
        Ty::Raw,
    );
    let callee = body_test_expr(
        hir::ExprKind::RawPointerLoad {
            ptr: Box::new(wrapper),
            offset: Box::new(body_test_expr(
                hir::ExprKind::Int(24),
                Ty::Int(IntTy {
                    bits: 64,
                    signed: true,
                }),
            )),
        },
        Ty::Raw,
    );
    let call = body_test_expr(
        hir::ExprKind::RawCall {
            guard: None,
            callee: Box::new(callee),
            args: vec![
                body_test_expr(hir::ExprKind::Local(1), Ty::Raw),
                body_test_expr(hir::ExprKind::Local(2), params),
            ],
            param_tys: vec![Ty::Raw, params],
            param_modes: vec![
                align_ast::ParamMode::ByValue,
                align_ast::ParamMode::Borrow,
            ],
            return_borrow: ReturnBorrowSummary::None,
            return_region: ReturnRegionSummary::None,
            return_cleanup: hir::ReturnCleanupAbi::None,
        },
        i32_ty,
    );
    let mut function = body_test_named_function(
        "pkg.db.internal.sqlite$prepared_binder_owner",
        hir::Block {
            stmts: Vec::new(),
            value: Some(Box::new(body_test_expr(
                hir::ExprKind::Unsafe(hir::Block {
                    stmts: Vec::new(),
                    value: Some(Box::new(call)),
                }),
                i32_ty,
            ))),
        },
        vec![
            body_test_local(0, "statement", statement, true, true),
            body_test_local(1, "context", Ty::Raw, false, true),
            body_test_local(2, "params", params, false, true),
        ],
        i32_ty,
    );
    function.params = vec![0, 1, 2];
    function.param_modes = vec![
        align_ast::ParamMode::BorrowMut,
        align_ast::ParamMode::ByValue,
        align_ast::ParamMode::ByValue,
    ];
    program.fns.push(function);
    let global = validate_hir::global_type_metadata_is_valid(&program);
    let placement = validate_hir::type_placement_metadata_is_valid(&program);
    let nominal = validate_hir::nominal_link_metadata_is_valid(&program);
    let body = validate_hir::body_only_metadata_is_valid(&program);
    assert!(
        global && placement && nominal && body,
        "the exact prepared binder bridge must survive HIR validation: global={global} placement={placement} nominal={nominal} body={body}"
    );

    fn raw_call(candidate: &mut hir::Program) -> &mut hir::Expr {
        let unsafe_expression = candidate
            .fns
            .last_mut()
            .and_then(|function| function.body.value.as_deref_mut())
            .expect("prepared binder unsafe expression");
        let hir::ExprKind::Unsafe(block) = &mut unsafe_expression.kind else {
            panic!("prepared binder unsafe block")
        };
        block
            .value
            .as_deref_mut()
            .expect("prepared binder expression")
    }

    let mut wrong_offset = program.clone();
    {
        let expression = raw_call(&mut wrong_offset);
        let hir::ExprKind::RawCall { callee, .. } = &mut expression.kind else {
            panic!("prepared binder raw call")
        };
        let hir::ExprKind::RawPointerLoad { offset, .. } = &mut callee.kind else {
            panic!("prepared binder pointer load")
        };
        offset.kind = hir::ExprKind::Int(32);
    }
    assert!(!body_core_metadata_is_valid(&wrong_offset));

    let mut wrong_params_mode = program.clone();
    {
        let expression = raw_call(&mut wrong_params_mode);
        let hir::ExprKind::RawCall { param_modes, .. } = &mut expression.kind else {
            unreachable!()
        };
        param_modes[1] = align_ast::ParamMode::ByValue;
    }
    assert!(!body_core_metadata_is_valid(&wrong_params_mode));

    let mut wrong_params_type = program.clone();
    {
        let expression = raw_call(&mut wrong_params_type);
        let hir::ExprKind::RawCall { param_tys, .. } = &mut expression.kind else {
            unreachable!()
        };
        param_tys[1] = Ty::Bool;
    }
    assert!(!body_core_metadata_is_valid(&wrong_params_type));

    let mut wrong_resource_identity = program;
    wrong_resource_identity.resources[statement_id as usize].name =
        "pkg.db$stmt$S5_Other$S6_Record".to_string();
    assert!(!body_core_metadata_is_valid(&wrong_resource_identity));
}

#[test]
fn hir_body_validator_batch_plan_guard_fails_closed() {
    let mut program = baseline_program();
    program.fns.push(body_test_parameter_function(
        "pkg.db.internal.resource$batch_plan_valid",
        Ty::Raw,
        hir::Block {
            stmts: Vec::new(),
            value: Some(Box::new(body_test_expr(
                hir::ExprKind::Bool(true),
                Ty::Bool,
            ))),
        },
        Ty::Bool,
    ));
    let i64_ty = Ty::Int(IntTy {
        bits: 64,
        signed: true,
    });
    let plan = || body_test_expr(hir::ExprKind::Local(0), Ty::Raw);
    let guard = body_test_expr(
        hir::ExprKind::Call {
            func: "pkg.db.internal.resource$batch_plan_valid".to_string(),
            args: vec![plan()],
            type_args: Vec::new(),
        },
        Ty::Bool,
    );
    let callee = body_test_expr(
        hir::ExprKind::RawPointerLoad {
            ptr: Box::new(plan()),
            offset: Box::new(body_test_expr(hir::ExprKind::Int(16), i64_ty)),
        },
        Ty::Raw,
    );
    let raw_call = body_test_expr(
        hir::ExprKind::RawCall {
            guard: Some(Box::new(guard)),
            callee: Box::new(callee),
            args: vec![body_test_expr(hir::ExprKind::Int(64), i64_ty)],
            param_tys: vec![i64_ty],
            param_modes: vec![align_ast::ParamMode::ByValue],
            return_borrow: ReturnBorrowSummary::None,
            return_region: ReturnRegionSummary::None,
            return_cleanup: hir::ReturnCleanupAbi::None,
        },
        Ty::Raw,
    );
    let mut owner = body_test_named_function(
        "pkg.db.internal.resource$batch_plan_owner",
        hir::Block {
            stmts: Vec::new(),
            value: Some(Box::new(body_test_expr(
                hir::ExprKind::Unsafe(hir::Block {
                    stmts: Vec::new(),
                    value: Some(Box::new(raw_call)),
                }),
                Ty::Raw,
            ))),
        },
        vec![
            body_test_local(0, "plan", Ty::Raw, false, true),
            body_test_local(1, "other", Ty::Raw, false, true),
        ],
        Ty::Raw,
    );
    owner.params = vec![0, 1];
    owner.param_modes = vec![align_ast::ParamMode::ByValue, align_ast::ParamMode::ByValue];
    program.fns.push(owner);
    assert!(body_core_metadata_is_valid(&program));

    fn batch_raw_call(candidate: &mut hir::Program) -> &mut hir::Expr {
        let unsafe_expression = candidate
            .fns
            .last_mut()
            .and_then(|function| function.body.value.as_deref_mut())
            .unwrap_or_else(|| panic!("batch plan unsafe expression"));
        let hir::ExprKind::Unsafe(block) = &mut unsafe_expression.kind else {
            panic!("batch plan unsafe block")
        };
        block
            .value
            .as_deref_mut()
            .unwrap_or_else(|| panic!("batch plan raw call"))
    }

    let mut missing_guard = program.clone();
    let hir::ExprKind::RawCall { guard, .. } = &mut batch_raw_call(&mut missing_guard).kind else {
        panic!("test batch plan raw call")
    };
    *guard = None;
    assert!(!body_core_metadata_is_valid(&missing_guard));

    let mut mismatched_plan = program.clone();
    let hir::ExprKind::RawCall {
        guard: Some(guard), ..
    } = &mut batch_raw_call(&mut mismatched_plan).kind
    else {
        panic!("test guarded batch plan raw call")
    };
    let hir::ExprKind::Call { args, .. } = &mut guard.kind else {
        panic!("test batch plan guard call")
    };
    args[0].kind = hir::ExprKind::Local(1);
    assert!(!body_core_metadata_is_valid(&mismatched_plan));

    let mut forged_predicate = program;
    let hir::ExprKind::RawCall { guard, .. } = &mut batch_raw_call(&mut forged_predicate).kind else {
        panic!("test forged batch plan raw call")
    };
    *guard = Some(Box::new(body_test_expr(
        hir::ExprKind::Bool(true),
        Ty::Bool,
    )));
    assert!(!body_core_metadata_is_valid(&forged_predicate));
}

#[test]
fn hir_body_validator_accepts_module_monomorph_call_name() {
    let integer = int(64);
    let mut program = baseline_program();
    let mut target = body_tail_case(
        "math$pick$i64",
        body_test_expr(hir::ExprKind::Int(1), integer),
        integer,
    );
    target.origin = hir::FnOrigin::Monomorph;
    program.fns.push(target);
    program.fns.push(body_tail_case(
        "caller",
        body_test_expr(
            hir::ExprKind::Call {
                func: "math$pick$i64".to_string(),
                args: Vec::new(),
                type_args: vec![integer],
            },
            integer,
        ),
        integer,
    ));
    assert!(body_core_metadata_is_valid(&program));
}

#[test]
fn deep_hir_body_core_type_dag_is_stack_bounded() {
    let integer = int(64);
    let mut expression = body_test_expr(hir::ExprKind::Int(0), integer);
    for _ in 0..512 {
        expression = body_test_expr(
            hir::ExprKind::Unary {
                op: align_ast::UnOp::Neg,
                expr: Box::new(expression),
            },
            integer,
        );
    }
    let mut program = baseline_program();
    program.fns.push(body_test_function(
        hir::Block {
            stmts: Vec::new(),
            value: Some(Box::new(expression)),
        },
        Vec::new(),
        integer,
    ));

    const TYPE_DEPTH: usize = 4_096;
    program.structs = (0..TYPE_DEPTH)
        .map(|id| StructDef {
            name: format!("BodyNode{id}"),
            source_name: format!("BodyNode{id}"),
            fields: vec![FieldDef {
                name: "next".to_string(),
                ty: if id + 1 == TYPE_DEPTH {
                    integer
                } else {
                    Ty::Struct((id + 1) as u32)
                },
            }],
            align: None,
            c_repr: false,
        })
        .collect();
    program.fns.push(body_test_parameter_function(
        "deep_body_type",
        Ty::Struct(0),
        hir::Block {
            stmts: Vec::new(),
            value: Some(Box::new(body_test_expr(
                hir::ExprKind::Local(0),
                Ty::Struct(0),
            ))),
        },
        Ty::Struct(0),
    ));
    let handle = std::thread::Builder::new()
        .stack_size(2 * 1024 * 1024)
        .spawn(move || {
            assert!(body_core_metadata_is_valid(&program));

            program.structs[TYPE_DEPTH - 1].fields[0].ty = Ty::Struct(TYPE_DEPTH as u32);
            assert!(!body_core_metadata_is_valid(&program));
        })
        .expect("spawn deep body type validator");
    handle.join().expect("join deep body type validator");
}

#[test]
fn hir_body_validator_loop_and_break() {
    let mut program = with_loop_body_depth(5);
    assert!(body_core_metadata_is_valid(&program));
    let hir::ExprKind::Loop { body_locals, .. } = &mut program.fns[0]
        .body
        .value
        .as_mut()
        .expect("loop fixture has a value")
        .kind
    else {
        panic!("loop fixture lost its loop");
    };
    *body_locals = 1..1;
    assert!(!body_core_metadata_is_valid(&program));
}

#[test]
fn hir_body_validator_loop_break_reachability_matches_diverges() {
    let unit = Ty::Unit;
    let unreachable_loop = body_test_expr(
        hir::ExprKind::Loop {
            body: hir::Block {
                stmts: vec![
                    hir::Stmt::Return(None),
                    hir::Stmt::Break {
                        value: None,
                        accepted: true,
                    },
                ],
                value: None,
            },
            diverges: false,
            body_locals: 0..0,
        },
        unit,
    );
    let mut unreachable = baseline_program();
    unreachable
        .fns
        .push(body_tail_case("unreachable_break", unreachable_loop, unit));
    assert!(!body_core_metadata_is_valid(&unreachable));

    let nested_break = body_test_expr(
        hir::ExprKind::If {
            cond: Box::new(body_test_expr(hir::ExprKind::Bool(true), Ty::Bool)),
            then: hir::Block {
                stmts: vec![hir::Stmt::Break {
                    value: None,
                    accepted: true,
                }],
                value: None,
            },
            els: hir::Block {
                stmts: Vec::new(),
                value: Some(Box::new(body_test_expr(hir::ExprKind::Unit, unit))),
            },
        },
        unit,
    );
    let reachable_loop = body_test_expr(
        hir::ExprKind::Loop {
            body: hir::Block {
                stmts: vec![hir::Stmt::Expr(nested_break)],
                value: None,
            },
            diverges: false,
            body_locals: 0..0,
        },
        unit,
    );
    let mut reachable = baseline_program();
    reachable
        .fns
        .push(body_tail_case("nested_break", reachable_loop, unit));
    assert!(body_core_metadata_is_valid(&reachable));

    let mut forged_rejection = reachable.clone();
    let hir::ExprKind::Loop { body, diverges, .. } = &mut forged_rejection
        .fns
        .last_mut()
        .expect("reachable loop function")
        .body
        .value
        .as_mut()
        .expect("reachable loop value")
        .kind
    else {
        panic!("reachable loop fixture lost its loop");
    };
    let hir::Stmt::Expr(hir::Expr {
        kind: hir::ExprKind::If { then, .. },
        ..
    }) = &mut body.stmts[0]
    else {
        panic!("reachable loop fixture lost its conditional");
    };
    let hir::Stmt::Break { accepted, .. } = &mut then.stmts[0] else {
        panic!("reachable loop fixture lost its break");
    };
    *accepted = false;
    *diverges = true;
    assert!(!body_core_metadata_is_valid(&forged_rejection));
}

#[test]
fn hir_body_validator_rejects_break_across_arena_and_task_group() {
    let loop_with = |scope, diverges| {
        body_tail_case(
            "nested_region_break",
            body_test_expr(
                hir::ExprKind::Loop {
                    body: hir::Block {
                        stmts: vec![hir::Stmt::Expr(scope)],
                        value: None,
                    },
                    diverges,
                    body_locals: 0..0,
                },
                Ty::Unit,
            ),
            Ty::Unit,
        )
    };
    let arena_scope = |accepted| {
        body_test_expr(
            hir::ExprKind::Arena(hir::Block {
                stmts: vec![hir::Stmt::Break {
                    value: None,
                    accepted,
                }],
                value: None,
            }),
            Ty::Unit,
        )
    };
    let task_group_scope = |accepted| {
        body_test_expr(
            hir::ExprKind::TaskGroup(hir::Block {
                stmts: vec![hir::Stmt::Break {
                    value: None,
                    accepted,
                }],
                value: None,
            }),
            Ty::Unit,
        )
    };

    let mut arena = baseline_program();
    arena.fns.push(loop_with(arena_scope(true), false));
    assert!(!body_core_metadata_is_valid(&arena));

    let mut rejected_arena = baseline_program();
    rejected_arena.fns.push(loop_with(arena_scope(false), true));
    assert!(body_core_metadata_is_valid(&rejected_arena));

    let mut task_group = baseline_program();
    task_group
        .fns
        .push(loop_with(task_group_scope(true), false));
    assert!(!body_core_metadata_is_valid(&task_group));

    let mut rejected_task_group = baseline_program();
    rejected_task_group
        .fns
        .push(loop_with(task_group_scope(false), true));
    assert!(body_core_metadata_is_valid(&rejected_task_group));
}

#[test]
fn hir_body_validator_deferred_facts_are_not_consumed() {
    let integer = int(64);
    let mut program = baseline_program();
    program.fns.push(body_test_function(
        hir::Block {
            stmts: vec![
                hir::Stmt::Let {
                    local: 0,
                    init: body_test_expr(hir::ExprKind::Int(1), integer),
                },
                hir::Stmt::Assign {
                    local: 0,
                    value: body_test_expr(hir::ExprKind::Int(2), integer),
                    drop_old: Cell::new(false),
                    drop_new: Cell::new(false),
                },
            ],
            value: Some(Box::new(body_test_expr(hir::ExprKind::Local(0), integer))),
        },
        vec![hir::Local {
            id: 0,
            name: "x".to_string(),
            ty: integer,
            is_mut: true,
            is_param: false,
            align: None,
        }],
        integer,
    ));
    assert!(body_core_metadata_is_valid(&program));
    program.fns[0].drop_locals = vec![0, 0, 1];
    program.fns[0].drop_individual_locals = vec![0];
    program.fns[0]
        .drop_individual_exprs
        .insert(align_span::Span::new(0, 4, 5), true);
    let hir::Stmt::Assign {
        drop_old,
        drop_new,
        ..
    } = &program.fns[0].body.stmts[1]
    else {
        panic!("test fixture lost its assignment");
    };
    drop_old.set(true);
    drop_new.set(true);
    assert!(body_core_metadata_is_valid(&program));

    // `Local::is_param` is an am-h declaration/header fact, not a body-core binding source.
    // Mutating it here must not make the dormant b1 helper silently accept or reject the body;
    // the activated preflight will reject the malformed header separately.
    program.fns[0].locals[0].is_param = true;
    assert!(body_core_metadata_is_valid(&program));
}

#[test]
fn valid_hir_global_type_preflight_is_mir_identity() {
    for bits in [8, 16, 32, 64] {
        assert_accepted("integer-width", &with_return(int(bits)));
        assert_accepted(
            "scalar-integer-width",
            &with_return(Ty::Option(scalar_int(bits))),
        );
    }
    for bits in [32, 64] {
        assert_accepted("float-width", &with_return(Ty::Float(FloatTy { bits })));
    }
    for lanes in [2, 4, 8, 16] {
        assert_accepted("vector-lanes", &with_return(Ty::Vec(scalar_int(32), lanes)));
        assert_accepted("mask-lanes", &with_return(Ty::Mask(scalar_int(32), lanes)));
    }

    for (label, ty) in [
        ("struct", Ty::Struct(0)),
        ("enum", Ty::Enum(0)),
        ("tuple", Ty::Tuple(0)),
        ("tagged", Ty::Tagged(0)),
        ("function-type", Ty::Fn(0)),
        ("struct-array", Ty::StructArray(0, 0)),
        (
            "dynamic-struct-array-aos",
            Ty::DynStructArray(0, Layout::Aos),
        ),
        (
            "dynamic-struct-array-soa",
            Ty::DynStructArray(0, Layout::Soa),
        ),
        ("soa", Ty::Soa(0)),
        ("scanner", Ty::JsonScanner(0)),
        ("dictionary", Ty::DictEncoded(0, 0)),
    ] {
        assert_graph_accepted(label, &with_return(ty));
    }

    let valid_leaf_types = [
        Ty::Bool,
        Ty::Char,
        Ty::DynResponseArray,
        Ty::Str,
        Ty::String,
        Ty::ArenaHandle,
        Ty::Raw,
        Ty::Builder,
        Ty::Writer,
        Ty::Reader,
        Ty::Buffer,
        Ty::StrFinder,
        Ty::File,
        Ty::Rng,
        Ty::Regex,
        Ty::Captures,
        Ty::CliCommand,
        Ty::CliParsed,
        Ty::TcpConn,
        Ty::TcpListener,
        Ty::UdpSocket,
        Ty::Child,
        Ty::Command,
        Ty::RunOutput,
        Ty::HttpRequest,
        Ty::HttpResponse,
        Ty::HttpClient,
        Ty::HttpServer,
        Ty::HttpRequestCtx,
        Ty::ResponseBuilder,
        Ty::HttpStream,
        Ty::HttpHeaders,
        Ty::JsonDoc,
        Ty::Unit,
    ];
    for ty in valid_leaf_types {
        if ty != Ty::StrFinder {
            assert_graph_accepted("leaf-type", &with_return(ty));
        }
    }

    let valid_scalars = [
        scalar_int(32),
        Scalar::Float(FloatTy { bits: 64 }),
        Scalar::Bool,
        Scalar::Char,
        Scalar::Unit,
        Scalar::Struct(0),
        Scalar::String,
        Scalar::DynArray(PrimScalar::Int(IntTy {
            bits: 32,
            signed: true,
        })),
        Scalar::DynStructArray(0),
        Scalar::DynResponseArray,
        Scalar::Str,
        Scalar::Slice(PrimScalar::Str),
        Scalar::Enum(0),
        Scalar::Tagged(0),
        Scalar::Soa(0),
        Scalar::JsonDoc,
        Scalar::Reader,
        Scalar::Writer,
        Scalar::Buffer,
        Scalar::Regex,
        Scalar::Captures,
        Scalar::CliParsed,
        Scalar::TcpConn,
        Scalar::TcpListener,
        Scalar::UdpSocket,
        Scalar::Child,
        Scalar::File,
        Scalar::HttpResponse,
        Scalar::HttpServer,
        Scalar::HttpRequestCtx,
        Scalar::ResponseBuilder,
        Scalar::HttpStream,
        Scalar::RunOutput,
        Scalar::Fn(0),
    ];
    for scalar in valid_scalars {
        assert_graph_accepted("scalar-discriminator", &with_return(Ty::Option(scalar)));
    }

    for primitive in [
        PrimScalar::Int(IntTy {
            bits: 8,
            signed: false,
        }),
        PrimScalar::Float(FloatTy { bits: 32 }),
        PrimScalar::Bool,
        PrimScalar::Char,
        PrimScalar::Str,
        PrimScalar::String,
    ] {
        assert_graph_accepted(
            "primitive-discriminator",
            &with_return(Ty::DynSliceArray(primitive)),
        );
    }

    for ty in [
        Ty::Option(scalar_int(32)),
        Ty::Result(Scalar::Bool, Scalar::String),
        Ty::Box(Scalar::Struct(0)),
        Ty::Array(Scalar::Bool, 0),
        Ty::Slice(Scalar::Str),
        Ty::DynArray(Scalar::String),
        Ty::ArrayBuilder(Scalar::String),
        Ty::Task(Scalar::Struct(0)),
    ] {
        assert_graph_accepted("wrapper-type", &with_return(ty));
    }

    for (label, ty) in [
        ("box-header-cycle", Ty::Box(Scalar::Struct(0))),
        (
            "dynamic-array-header-cycle",
            Ty::DynArray(Scalar::Struct(0)),
        ),
        ("task-header-cycle", Ty::Task(Scalar::Struct(0))),
    ] {
        let mut wrapper_cycle = baseline_program();
        wrapper_cycle.structs[0].fields[0].ty = ty;
        assert_graph_accepted(label, &wrapper_cycle);
    }

    let mut function_header_cycle = baseline_program();
    function_header_cycle.structs[0].fields[0].ty = Ty::Fn(0);
    function_header_cycle.fn_types[0] = fn_type(Ty::Struct(0));
    assert_accepted("function-header-cycle", &function_header_cycle);

    let mut abstract_tagged = baseline_program();
    abstract_tagged.tagged_types[0] = TaggedType::Option(Scalar::Param(0));
    assert_accepted("unreachable-template-tagged", &abstract_tagged);
    let mut abstract_fn = baseline_program();
    abstract_fn.fn_types[0] = fn_type(Ty::Param(0));
    assert_accepted("unreachable-template-function-type", &abstract_fn);
    let mut abstract_nominals = baseline_program();
    abstract_nominals.structs.push(StructDef {
        name: "TemplateStruct".to_string(),
        source_name: "TemplateStruct".to_string(),
        fields: vec![FieldDef {
            name: "value".to_string(),
            ty: Ty::Param(0),
        }],
        align: None,
        c_repr: false,
    });
    abstract_nominals.enums.push(EnumDef {
        name: "TemplateEnum".to_string(),
        source_name: "TemplateEnum".to_string(),
        variants: vec![hir::EnumVariant {
            name: "Value".to_string(),
            payload: vec![Scalar::Param(0)],
            field_base: 1,
        }],
    });
    assert_accepted("unreachable-template-nominals", &abstract_nominals);

    let mut deep_graph = baseline_program();
    deep_graph.structs = (0..4_096)
        .map(|index| StructDef {
            name: format!("Deep{index}"),
            source_name: format!("Deep{index}"),
            fields: vec![FieldDef {
                name: "next".to_string(),
                ty: if index == 4_095 {
                    int(64)
                } else {
                    Ty::Struct(index + 1)
                },
            }],
            align: None,
            c_repr: false,
        })
        .collect();
    assert_accepted("iterative-deep-type-graph", &deep_graph);
    deep_graph.structs[4_095].fields[0].ty = Ty::Struct(0);
    assert_rejected("iterative-deep-type-cycle", &deep_graph);

    let program = baseline_program();
    let source_map = SourceMap::new();
    for (checked, unchecked) in [
        (
            lower_program(&program),
            lower_program_unchecked(&program, None, false),
        ),
        (
            lower_program_located(&program, &source_map),
            lower_program_unchecked(
                &program,
                Some(Rc::new(SourceLines::from_map(&source_map))),
                false,
            ),
        ),
        (
            lower_program_per_unit(&program),
            lower_program_unchecked(&program, None, true),
        ),
        (
            lower_program_per_unit_located(&program, &source_map),
            lower_program_unchecked(
                &program,
                Some(Rc::new(SourceLines::from_map(&source_map))),
                true,
            ),
        ),
    ] {
        assert_eq!(format!("{checked:#?}"), format!("{unchecked:#?}"));
    }
}
