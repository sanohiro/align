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

fn imported_fn(name: &str, params: Vec<Ty>, ret: Ty) -> ImportedFn {
    ImportedFn {
        name: name.to_string(),
        param_modes: vec![align_ast::ParamMode::ByValue; params.len()],
        params,
        ret,
        return_borrow: ReturnBorrowSummary::None,
        return_region: ReturnRegionSummary::None,
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
    Structural,
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

fn is_result_ty(program: &Program, ty: Ty) -> bool {
    match ty {
        Ty::Result(..) => true,
        Ty::Tagged(id) => matches!(
            program.tagged_types.get(id as usize),
            Some(TaggedType::Result(..))
        ),
        _ => false,
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
                    Rvalue::ResultOk(_) | Rvalue::ResultErr(_) => assert!(
                        is_result_ty(program, value_ty),
                        "{label}: Result construction has non-Result MIR type {value_ty:?}"
                    ),
                    Rvalue::ResultIsOk(result)
                    | Rvalue::ResultUnwrapOk(result)
                    | Rvalue::ResultUnwrapErr(result) => {
                        let operand_ty = function.operand_ty(result);
                        assert!(
                            is_result_ty(program, operand_ty),
                            "{label}: Result inspection received non-Result MIR operand {operand_ty:?}"
                        );
                    }
                    _ => {}
                }
            }
        }
    }
}

fn assert_mir_owner(label: &str, program: &Program, owner: MirOwner) {
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
            conversions > 0 && result_tests > 0 && result_unwraps > 0
        }
        MirOwner::Regex => has(|rv| matches!(rv, Rvalue::RegexReplace { .. })),
        MirOwner::Template => has(|rv| matches!(rv, Rvalue::Template(..))),
        MirOwner::File => has(|rv| matches!(rv, Rvalue::FileCreateRw { .. })),
        MirOwner::ArrayBuilder => has(|rv| matches!(rv, Rvalue::ArrayBuilderPush { .. })),
        MirOwner::Command => has(|rv| matches!(rv, Rvalue::Command { .. })),
        MirOwner::Http => has(|rv| matches!(rv, Rvalue::HttpRequest { .. })),
        MirOwner::Match => has(|rv| matches!(rv, Rvalue::OptionIsSome(..))),
        MirOwner::Conditional => {
            has(|rv| matches!(rv, Rvalue::OptionIsSome(..)))
                && program.fns.iter().any(|function| {
                    function
                        .blocks
                        .iter()
                        .any(|block| matches!(block.term, Term::Branch(..)))
                })
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
        MirOwner::Stage => has(|rv| matches!(rv, Rvalue::Call(name, _) if name == "dep$stage_id")),
        // Block/statement wrappers can be semantically transparent and therefore emit no MIR
        // instruction of their own. Their exact record depth is owned by the HIR preflight above;
        // successful function construction proves the structural lowering path completed.
        MirOwner::Structural => !program.fns.is_empty(),
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

fn assert_accepted_impl(label: &str, program: &hir::Program, owner: Option<MirOwner>) {
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
        assert_result_rvalue_contract(label, &lowered);
        if let Some(owner) = owner {
            assert_mir_owner(label, &lowered, owner);
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
        lifted_capture_count: None,
        params: Vec::new(),
        param_modes: Vec::new(),
        ret,
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

fn with_str_trim_body_depth(depth: usize) -> hir::Program {
    assert!(depth >= 2, "the root Block and leaf Expr need depth two");
    let span = align_span::Span::new(0, 0, 0);
    let expr = str_trim_expr_depth(depth - 1);
    let mut program = baseline_program();
    program.fns.push(hir::Fn {
        name: "deep_str_trim".to_string(),
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

fn with_path_string_body_depth(depth: usize) -> hir::Program {
    assert!(depth >= 2, "the root Block and leaf Expr need depth two");
    let span = align_span::Span::new(0, 0, 0);
    let mut expr = hir::Expr {
        kind: hir::ExprKind::Str("x".to_string()),
        ty: Ty::Str,
        span,
    };
    for expression_depth in 2..depth {
        expr = if expr.ty == Ty::String {
            hir::Expr {
                kind: hir::ExprKind::StrBorrow(Box::new(expr)),
                ty: Ty::Str,
                span,
            }
        } else {
            match expression_depth % 4 {
                0 | 2 => hir::Expr {
                    kind: hir::ExprKind::PathComponent {
                        kind: hir::PathComponentKind::Base,
                        path: Box::new(expr),
                    },
                    ty: Ty::Str,
                    span,
                },
                1 => hir::Expr {
                    kind: hir::ExprKind::PathNormalize {
                        path: Box::new(expr),
                    },
                    ty: Ty::String,
                    span,
                },
                _ => hir::Expr {
                    kind: hir::ExprKind::PathJoin {
                        a: Box::new(expr),
                        b: Box::new(hir::Expr {
                            kind: hir::ExprKind::Str("y".to_string()),
                            ty: Ty::Str,
                            span,
                        }),
                    },
                    ty: Ty::String,
                    span,
                },
            }
        };
    }
    let ret = expr.ty;
    let mut program = baseline_program();
    program.fns.push(hir::Fn {
        name: "deep_path_string".to_string(),
        lifted_capture_count: None,
        params: Vec::new(),
        param_modes: Vec::new(),
        ret,
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

fn with_reader_buffered_body_depth(depth: usize) -> hir::Program {
    assert!(depth >= 2, "the root Block and leaf Expr need depth two");
    let span = align_span::Span::new(0, 0, 0);
    let mut expr = hir::Expr {
        kind: hir::ExprKind::ReaderStdin,
        ty: Ty::Reader,
        span,
    };
    for _ in 2..depth {
        expr = hir::Expr {
            kind: hir::ExprKind::ReaderBuffered {
                reader: Box::new(expr),
            },
            ty: Ty::Reader,
            span,
        };
    }
    let mut program = baseline_program();
    program.fns.push(hir::Fn {
        name: "deep_reader_buffered".to_string(),
        lifted_capture_count: None,
        params: Vec::new(),
        param_modes: Vec::new(),
        ret: Ty::Reader,
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
        lifted_capture_count: None,
        params: Vec::new(),
        param_modes: Vec::new(),
        ret: result_ty,
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

fn with_regex_string_body_depth(depth: usize) -> hir::Program {
    assert!(depth >= 2, "the root Block and leaf Expr need depth two");
    let span = align_span::Span::new(0, 0, 0);
    let mut expr = hir::Expr {
        kind: hir::ExprKind::Str("x".to_string()),
        ty: Ty::Str,
        span,
    };
    for _ in 2..depth {
        expr = if expr.ty == Ty::String {
            hir::Expr {
                kind: hir::ExprKind::StrBorrow(Box::new(expr)),
                ty: Ty::Str,
                span,
            }
        } else {
            hir::Expr {
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
                span,
            }
        };
    }
    let ret = expr.ty;
    let mut program = baseline_program();
    program.fns.push(hir::Fn {
        name: "deep_regex_string".to_string(),
        lifted_capture_count: None,
        params: vec![0],
        param_modes: vec![align_ast::ParamMode::ByValue],
        ret,
        return_borrow: ReturnBorrowSummary::None,
        return_region: ReturnRegionSummary::None,
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

fn with_file_body_depth(depth: usize) -> hir::Program {
    assert!(
        depth >= 3,
        "the root Block, file Expr, and path need depth three"
    );
    let span = align_span::Span::new(0, 0, 0);
    let mut program = baseline_program();
    let error_id = push_builtin_error(&mut program);
    let result_ty = Ty::Result(Scalar::File, Scalar::Enum(error_id));
    let expr = hir::Expr {
        kind: hir::ExprKind::FileCreateRw {
            path: Box::new(str_trim_expr_depth(depth - 2)),
        },
        ty: result_ty,
        span,
    };
    program.fns.push(hir::Fn {
        name: "deep_file".to_string(),
        lifted_capture_count: None,
        params: Vec::new(),
        param_modes: Vec::new(),
        ret: result_ty,
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

fn with_array_builder_body_depth(depth: usize) -> hir::Program {
    assert!(
        depth >= 3,
        "the root Block, array-builder Expr, and value need depth three"
    );
    let span = align_span::Span::new(0, 0, 0);
    let elem = scalar_int(64);
    let builder_ty = Ty::ArrayBuilder(elem);
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
        lifted_capture_count: None,
        params: vec![0],
        param_modes: vec![align_ast::ParamMode::ByValue],
        ret: Ty::Unit,
        return_borrow: ReturnBorrowSummary::None,
        return_region: ReturnRegionSummary::None,
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
        drop_individual_exprs: Default::default(),
        exportable: false,
    });
    program
}

fn with_process_command_body_depth(depth: usize) -> hir::Program {
    assert!(
        depth >= 3,
        "the root Block, process-command Expr, and command need depth three"
    );
    let span = align_span::Span::new(0, 0, 0);
    let argv_ty = Ty::Slice(Scalar::Str);
    let expr = hir::Expr {
        kind: hir::ExprKind::ProcessCommand {
            cmd: Box::new(str_trim_expr_depth(depth - 2)),
            args: Box::new(hir::Expr {
                kind: hir::ExprKind::Local(0),
                ty: argv_ty,
                span,
            }),
        },
        ty: Ty::Command,
        span,
    };
    let mut program = baseline_program();
    program.fns.push(hir::Fn {
        name: "deep_process_command".to_string(),
        lifted_capture_count: None,
        params: vec![0],
        param_modes: vec![align_ast::ParamMode::ByValue],
        ret: Ty::Command,
        return_borrow: ReturnBorrowSummary::None,
        return_region: ReturnRegionSummary::None,
        locals: vec![hir::Local {
            id: 0,
            name: "argv".to_string(),
            ty: argv_ty,
            is_mut: false,
            is_param: true,
            align: None,
        }],
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

fn with_http_body_depth(depth: usize) -> hir::Program {
    assert!(
        depth >= 3,
        "the root Block, HTTP Expr, and method need depth three"
    );
    let span = align_span::Span::new(0, 0, 0);
    let expr = hir::Expr {
        kind: hir::ExprKind::HttpRequest {
            method: Box::new(str_trim_expr_depth(depth - 2)),
            url: Box::new(hir::Expr {
                kind: hir::ExprKind::Str("https://example.invalid".to_string()),
                ty: Ty::Str,
                span,
            }),
        },
        ty: Ty::HttpRequest,
        span,
    };
    let mut program = baseline_program();
    program.fns.push(hir::Fn {
        name: "deep_http".to_string(),
        lifted_capture_count: None,
        params: Vec::new(),
        param_modes: Vec::new(),
        ret: Ty::HttpRequest,
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
        lifted_capture_count: None,
        params: Vec::new(),
        param_modes: Vec::new(),
        ret,
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
                        variants: vec![0],
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
    let mut program = baseline_program();
    program.fns.push(hir::Fn {
        name: "deep_binary_match".to_string(),
        lifted_capture_count: None,
        params: Vec::new(),
        param_modes: Vec::new(),
        ret,
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
        lifted_capture_count: None,
        params: Vec::new(),
        param_modes: Vec::new(),
        ret: Ty::Bool,
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
        lifted_capture_count: None,
        params: Vec::new(),
        param_modes: Vec::new(),
        ret,
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

fn with_stage_body_depth(depth: usize) -> hir::Program {
    assert!(
        depth >= 4,
        "the root Block, pipeline Expr, Stage, and capture need depth four"
    );
    let span = align_span::Span::new(0, 0, 0);
    let target_expr_depth = depth - 1;
    let array_ty = Ty::Array(scalar_int(64), 1);
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
        lifted_capture_count: None,
        params: vec![0],
        param_modes: vec![align_ast::ParamMode::ByValue],
        ret: int(64),
        return_borrow: ReturnBorrowSummary::None,
        return_region: ReturnRegionSummary::None,
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
        drop_locals: Vec::new(),
        drop_individual_locals: Vec::new(),
        drop_individual_exprs: Default::default(),
        exportable: false,
    });
    program
}

#[derive(Clone, Copy)]
struct DepthFixture {
    name: &'static str,
    make: fn(usize) -> hir::Program,
    owner: MirOwner,
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
            owner: MirOwner::Structural,
        },
        DepthFixture {
            name: "wildcard match",
            make: with_match_arm_body_depth,
            owner: MirOwner::Structural,
        },
        DepthFixture {
            name: "if",
            make: with_if_branch_body_depth,
            owner: MirOwner::Structural,
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
                    let program = (fixture.make)(depth);
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
                let program = (fixture.make)(depth);
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
