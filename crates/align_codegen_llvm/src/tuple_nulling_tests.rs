use super::*;

type TestResult<T = ()> = Result<T, Box<dyn std::error::Error>>;

fn fixture(owned: Scalar) -> TestResult<Program> {
    let i32_ty = Ty::Int(IntTy {
        bits: 32,
        signed: true,
    });
    let mut program = Program::default();
    program.tuples.push(TupleDef {
        elems: vec![
            owned,
            Scalar::Int(IntTy {
                bits: 64,
                signed: true,
            }),
            Scalar::String,
        ],
    });
    program.fns.push(Function {
        name: ProgramCall::try_from_logical("main").map_err(|error| format!("{error:?}"))?,
        params: vec![],
        param_modes: vec![],
        borrow_mut_cleanup_slots: vec![],
        return_borrow: hir::ReturnBorrowSummary::None,
        return_region: hir::ReturnRegionSummary::None,
        ret: i32_ty,
        return_cleanup: hir::ReturnCleanupAbi::None,
        slots: vec![Ty::Tuple(0)],
        slot_align: vec![None],
        value_tys: vec![],
        blocks: vec![Block {
            id: 0,
            stmts: vec![
                Stmt::DropFlagInit(0),
                Stmt::NullTupleField(0, 0),
                Stmt::NullTupleField(0, 2),
            ],
            stmt_lines: vec![(0, 0); 3],
            term: Term::Return(Some(Operand::Const(Const::Int(0, i32_ty)))),
        }],
        entry: 0,
        exceptional_edges: vec![],
        cold: false,
        exportable: false,
        available_externally: false,
    });
    Ok(program)
}

#[test]
fn tuple_partial_move_nulls_exact_element_layout() -> TestResult {
    // A pointer owner followed by i64 exposes the old 16-byte header store:
    // opaque LLVM pointers accept that store while the adjacent scalar is lost.
    // Keep a header owner in the same tuple to pin both physical categories.
    let mut owners = Vec::new();
    for &ty in align_sema::MOVE_HANDLE_TYPES {
        // cli.command has no Scalar form and cannot be a tuple element.
        if ty == Ty::CliCommand {
            assert!(ty_to_scalar(ty).is_none());
            continue;
        }
        let scalar =
            ty_to_scalar(ty).ok_or_else(|| format!("Move handle {ty:?} lost its Scalar form"))?;
        assert!(
            scalar.is_move(),
            "Move handle {ty:?} lost tuple Move classification"
        );
        owners.push(scalar);
    }
    owners.extend([
        Scalar::String,
        Scalar::DynArray(align_sema::PrimScalar::Int(IntTy {
            bits: 64,
            signed: true,
        })),
    ]);
    let mut pointer_cases = 0;
    for owner in owners {
        let program = fixture(owner)?;
        validate_mir_producers(&program)?;
        let ir = emit_llvm_ir(
            &program,
            &BuildTarget::Baseline,
            Profile::Release,
            false,
            &[],
            None,
        )
        .map_err(|error| format!("{error:?}"))?;
        let stores: Vec<_> = ir
            .lines()
            .filter(|line| line.trim_start().starts_with("store ") && line.contains("%nulltupfld"))
            .collect();
        assert_eq!(stores.len(), 2, "{owner:?}: {ir}");
        let pointer = align_sema::is_move_handle(scalar_to_ty(owner));
        if pointer {
            pointer_cases += 1;
            assert!(
                stores[0].trim_start().starts_with("store ptr null,"),
                "{owner:?} partial move overwrites a pointer field with a header: {}",
                stores[0]
            );
        } else {
            assert!(
                stores[0].contains("{ ptr, i64 } zeroinitializer"),
                "{owner:?}: {}",
                stores[0]
            );
        }
        assert!(
            stores[1].contains("{ ptr, i64 } zeroinitializer"),
            "{owner:?}: {}",
            stores[1]
        );
    }
    assert!(
        pointer_cases >= 3,
        "pointer-owner inventory was not exercised"
    );
    Ok(())
}

#[test]
fn struct_partial_move_nulls_each_existing_owned_layout() -> TestResult {
    let mut program = fixture(Scalar::String)?;
    program.enums.push(EnumDef {
        name: "Content".into(),
        source_name: "Content".into(),
        variants: vec![hir::EnumVariant {
            name: "Text".into(),
            payload: vec![Scalar::String],
            field_base: 1,
        }],
    });
    program
        .tagged_types
        .push(hir::TaggedType::Option(Scalar::String));
    let mut types = align_sema::MOVE_HANDLE_TYPES.to_vec();
    let pointer_count = types.len();
    types.extend([
        Ty::String,
        Ty::Enum(0),
        Ty::Option(Scalar::String),
        Ty::Result(Scalar::String, Scalar::Bool),
        Ty::Tagged(0),
    ]);
    program.structs.push(StructDef {
        name: "Owners".into(),
        source_name: "Owners".into(),
        align: None,
        c_repr: false,
        fields: types
            .iter()
            .enumerate()
            .map(|(index, ty)| hir::FieldDef {
                name: format!("field{index}"),
                ty: *ty,
            })
            .collect(),
    });
    let function = program.fns.first_mut().ok_or("missing fixture function")?;
    function.slots = vec![Ty::Struct(0)];
    let block = function.blocks.first_mut().ok_or("missing fixture block")?;
    block.stmts = vec![Stmt::DropFlagInit(0)];
    for index in 0..types.len() {
        block
            .stmts
            .push(Stmt::NullStructField(0, u32::try_from(index)?));
    }
    block.stmt_lines = vec![(0, 0); block.stmts.len()];
    validate_mir_producers(&program)?;
    validate_thin_partition_program(&program, &[]).map_err(|error| format!("{error:?}"))?;
    let ir = emit_llvm_ir(
        &program,
        &BuildTarget::Baseline,
        Profile::Release,
        false,
        &[],
        None,
    )
    .map_err(|error| format!("{error:?}"))?;
    let stores: Vec<_> = ir
        .lines()
        .filter(|line| line.trim_start().starts_with("store ") && line.contains("%drop.field.ptr"))
        .collect();
    assert_eq!(stores.len(), types.len(), "{ir}");
    assert!(
        stores
            .iter()
            .take(pointer_count)
            .all(|line| line.trim_start().starts_with("store ptr null,")),
        "{ir}"
    );
    assert!(
        stores
            .get(pointer_count)
            .is_some_and(|line| line.contains("{ ptr, i64 } zeroinitializer")),
        "{ir}"
    );
    assert!(
        stores
            .iter()
            .skip(pointer_count + 1)
            .all(|line| line.contains("zeroinitializer")),
        "{ir}"
    );
    Ok(())
}

#[test]
fn partial_field_nulling_rejects_malformed_metadata_before_emission() -> TestResult {
    let original = fixture(Scalar::String)?;
    let mut malformed = Vec::new();
    for (ty, statement) in [
        (Ty::Tuple(0), Stmt::NullTupleField(99, 0)),
        (Ty::Tuple(99), Stmt::NullTupleField(0, 0)),
        (Ty::String, Stmt::NullTupleField(0, 0)),
        (Ty::Tuple(0), Stmt::NullTupleField(0, 99)),
        (Ty::Tuple(0), Stmt::NullTupleField(0, 1)),
        (Ty::Tuple(0), Stmt::NullStructField(0, 0)),
        (Ty::Struct(99), Stmt::NullStructField(0, 0)),
    ] {
        let mut program = original.clone();
        let function = program.fns.first_mut().ok_or("missing fixture function")?;
        function.slots = vec![ty];
        function
            .blocks
            .first_mut()
            .ok_or("missing fixture block")?
            .stmts = vec![statement];
        function
            .blocks
            .first_mut()
            .ok_or("missing fixture block")?
            .stmt_lines = vec![(0, 0)];
        malformed.push(program);
    }
    let mut record = original.clone();
    record.structs.push(StructDef {
        name: "Outer".into(),
        source_name: "Outer".into(),
        align: None,
        c_repr: false,
        fields: vec![
            hir::FieldDef {
                name: "copy".into(),
                ty: Ty::Bool,
            },
            hir::FieldDef {
                name: "nested".into(),
                ty: Ty::Struct(1),
            },
        ],
    });
    record.structs.push(StructDef {
        name: "Inner".into(),
        source_name: "Inner".into(),
        align: None,
        c_repr: false,
        fields: vec![hir::FieldDef {
            name: "owned".into(),
            ty: Ty::String,
        }],
    });
    for field in [0, 1, 99] {
        let mut program = record.clone();
        let function = program.fns.first_mut().ok_or("missing fixture function")?;
        function.slots = vec![Ty::Struct(0)];
        function
            .blocks
            .first_mut()
            .ok_or("missing fixture block")?
            .stmts = vec![Stmt::NullStructField(0, field)];
        function
            .blocks
            .first_mut()
            .ok_or("missing fixture block")?
            .stmt_lines = vec![(0, 0)];
        malformed.push(program);
    }
    let mut unreachable = original.clone();
    let function = unreachable
        .fns
        .first_mut()
        .ok_or("missing fixture function")?;
    function.blocks.push(Block {
        id: 1,
        stmts: vec![Stmt::NullTupleField(0, 1)],
        stmt_lines: vec![(0, 0)],
        term: Term::Unreachable,
    });
    malformed.push(unreachable);
    for program in malformed {
        assert!(align_mir::producer::validate_partial_field_nulling(&program).is_err());
        assert!(validate_mir_producers(&program).is_err());
        assert!(validate_thin_partition_program(&program, &[]).is_err());
        assert!(
            emit_llvm_ir(
                &program,
                &BuildTarget::Baseline,
                Profile::Release,
                false,
                &[],
                None
            )
            .is_err()
        );
    }
    Ok(())
}
