use super::*;
use align_sema::{
    FloatTy, FnEffect, IntTy, Layout, PrimScalar, Scalar, Ty,
    hir::{
        self, EnumDef, EnumVariant, FieldDef, FnTy, ImportedFn, ReturnBorrowSummary,
        ReturnRegionSummary, StructDef, TaggedType, TupleDef,
    },
};
use std::cell::Cell;

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
        effect: Cell::new(FnEffect::Pure),
    }
}

fn baseline_program() -> hir::Program {
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
        tagged_types: vec![TaggedType::Option(scalar_int(64))],
        tuples: vec![TupleDef {
            elems: vec![scalar_int(64), Scalar::Bool],
        }],
        fn_types: vec![fn_type(Ty::Unit)],
        imported_fns: Vec::new(),
    }
}

fn with_return(ty: Ty) -> hir::Program {
    let mut program = baseline_program();
    program.imported_fns.push(ImportedFn {
        name: "dep$value".to_string(),
        params: Vec::new(),
        param_modes: Vec::new(),
        ret: ty,
        return_borrow: ReturnBorrowSummary::None,
        return_region: ReturnRegionSummary::None,
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
        !validate_hir::global_type_metadata_is_valid(program),
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

fn assert_accepted(label: &str, program: &hir::Program) {
    assert!(
        validate_hir::global_type_metadata_is_valid(program),
        "{label}: validator rejected valid metadata"
    );
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
    }
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
        lifted_capture_count: None,
        params: Vec::new(),
        param_modes: Vec::new(),
        ret: int(64),
        return_borrow: ReturnBorrowSummary::None,
        return_region: ReturnRegionSummary::None,
        locals: Vec::new(),
        body: hir::Block {
            stmts: Vec::new(),
            value: Some(Box::new(expr)),
        },
        span,
        drop_locals: Vec::new(),
        drop_individual_locals: Vec::new(),
        drop_individual_exprs: Default::default(),
        exportable: false,
    });
    program
}

fn with_template_body_depth(depth: usize) -> hir::Program {
    assert!(depth >= 2, "the root Block and leaf Expr need depth two");
    let span = align_span::Span::new(0, 0, 0);
    let target_expr_depth = depth - 1;
    let mut expr = hir::Expr {
        kind: hir::ExprKind::Str("x".to_string()),
        ty: Ty::Str,
        span,
    };
    let mut expr_depth = 1;
    if target_expr_depth.is_multiple_of(2) {
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
    while expr_depth < target_expr_depth {
        expr = hir::Expr {
            kind: hir::ExprKind::Template(vec![hir::TemplatePart::Hole(expr)]),
            ty: Ty::Str,
            span,
        };
        expr_depth += 2;
    }
    let mut program = baseline_program();
    program.fns.push(hir::Fn {
        name: "deep_template".to_string(),
        lifted_capture_count: None,
        params: Vec::new(),
        param_modes: Vec::new(),
        ret: Ty::Str,
        return_borrow: ReturnBorrowSummary::None,
        return_region: ReturnRegionSummary::None,
        locals: Vec::new(),
        body: hir::Block {
            stmts: Vec::new(),
            value: Some(Box::new(expr)),
        },
        span,
        drop_locals: Vec::new(),
        drop_individual_locals: Vec::new(),
        drop_individual_exprs: Default::default(),
        exportable: false,
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
        kind: hir::ExprKind::Unit,
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
    program.fns.push(hir::Fn {
        name: "deep_block_stmt".to_string(),
        lifted_capture_count: None,
        params: Vec::new(),
        param_modes: Vec::new(),
        ret: Ty::Unit,
        return_borrow: ReturnBorrowSummary::None,
        return_region: ReturnRegionSummary::None,
        locals: Vec::new(),
        body: hir::Block {
            stmts: Vec::new(),
            value: Some(Box::new(expr)),
        },
        span,
        drop_locals: Vec::new(),
        drop_individual_locals: Vec::new(),
        drop_individual_exprs: Default::default(),
        exportable: false,
    });
    program
}

fn with_match_arm_body_depth(depth: usize) -> hir::Program {
    assert!(depth >= 2, "the root Block and leaf Expr need depth two");
    let span = align_span::Span::new(0, 0, 0);
    let target_expr_depth = depth - 1;
    let option_unit = Ty::Option(Scalar::Unit);
    let mut expr = hir::Expr {
        kind: hir::ExprKind::Unit,
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
    program.fns.push(hir::Fn {
        name: "deep_match_arm".to_string(),
        lifted_capture_count: None,
        params: Vec::new(),
        param_modes: Vec::new(),
        ret: expr.ty,
        return_borrow: ReturnBorrowSummary::None,
        return_region: ReturnRegionSummary::None,
        locals: Vec::new(),
        body: hir::Block {
            stmts: Vec::new(),
            value: Some(Box::new(expr)),
        },
        span,
        drop_locals: Vec::new(),
        drop_individual_locals: Vec::new(),
        drop_individual_exprs: Default::default(),
        exportable: false,
    });
    program
}

#[test]
fn checked_hir_depth_closure_matrix() {
    for make_program in [
        with_unary_body_depth as fn(usize) -> hir::Program,
        with_template_body_depth,
        with_block_stmt_body_depth,
        with_match_arm_body_depth,
    ] {
        for depth in [
            align_sema::MAX_CHECKED_HIR_DEPTH - 1,
            align_sema::MAX_CHECKED_HIR_DEPTH,
        ] {
            let program = make_program(depth);
            assert!(
                align_sema::checked_hir_body_depth_is_valid(&program),
                "valid checked-HIR depth {depth} was rejected"
            );
            assert_accepted("checked-HIR depth boundary", &program);
        }
    }

    let depth = align_sema::MAX_CHECKED_HIR_DEPTH + 1;
    let source_map = SourceMap::new();
    for make_program in [
        with_unary_body_depth as fn(usize) -> hir::Program,
        with_template_body_depth,
        with_block_stmt_body_depth,
        with_match_arm_body_depth,
    ] {
        let program = make_program(depth);
        assert!(
            !align_sema::checked_hir_body_depth_is_valid(&program),
            "over-bound checked-HIR depth {depth} was accepted"
        );
        for lowered in [
            lower_program(&program),
            lower_program_located(&program, &source_map),
            lower_program_per_unit(&program),
            lower_program_per_unit_located(&program, &source_map),
        ] {
            assert!(
                is_empty(&lowered),
                "an over-bound body published partial MIR"
            );
        }
    }
}

#[test]
fn malformed_hir_global_type_metadata_fails_closed() {
    for (label, ty) in [
        ("param", Ty::Param(0)),
        ("int-var", Ty::IntVar(0)),
        ("float-var", Ty::FloatVar(0)),
        ("str-finder", Ty::StrFinder),
        ("error", Ty::Error),
        ("scalar-param", Ty::Option(Scalar::Param(0))),
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
        assert_accepted(label, &with_return(ty));
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
            assert_accepted("leaf-type", &with_return(ty));
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
        assert_accepted("scalar-discriminator", &with_return(Ty::Option(scalar)));
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
        assert_accepted(
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
        assert_accepted("wrapper-type", &with_return(ty));
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
        assert_accepted(label, &wrapper_cycle);
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
