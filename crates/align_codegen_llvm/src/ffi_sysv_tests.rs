use super::*;
use inkwell::attributes::{Attribute, AttributeLoc};
use inkwell::targets::{TargetData, TargetTriple};
use inkwell::values::{CallSiteValue, InstructionOpcode};

#[path = "../../align_driver/tests/helpers/ffi_sysv_cases.rs"]
mod cases;

#[test]
fn sysv_record_target_domain_is_closed() {
    for triple in ["x86_64-unknown-linux-gnu", "x86_64-pc-linux-musl"] {
        assert!(sysv_record_target(triple, &TargetData::create("e-p:64:64")));
        assert!(!sysv_record_target(
            triple,
            &TargetData::create("e-p:32:32")
        ));
        assert!(!sysv_record_target(
            triple,
            &TargetData::create("E-p:64:64")
        ));
    }
    for triple in [
        "x86_64-unknown-linux-gnux32",
        "x86_64-unknown-linux-android",
        "x86_64-unknown-linux-magic",
        "x86_64-apple-macosx26.0",
        "x86_64-pc-windows-msvc",
        "i686-unknown-linux-gnu",
        "aarch64-unknown-linux-gnu",
        "x86_64-unknown-freebsd",
        "x86_64other-unknown-linux-gnu",
        "x86_64-unknown-linux-gnu-extra",
    ] {
        assert!(
            !sysv_record_target(triple, &TargetData::create("e-p:64:64")),
            "{triple}"
        );
    }
}

#[test]
fn sysv_memory_types_attributes_storage_and_native_preflight() -> Result<(), String> {
    Target::initialize_x86(&InitializationConfig::default());
    let triple = TargetTriple::create("x86_64-unknown-linux-gnu");
    let tm = Target::from_triple(&triple)
        .map_err(|e| e.to_string())?
        .create_target_machine(
            &triple,
            "x86-64-v2",
            "",
            OptimizationLevel::Default,
            RelocMode::PIC,
            CodeModel::Default,
        )
        .ok_or("missing x86 target")?;
    let ctx = Context::create();
    let module = ctx.create_module("sysv_memory_owner");
    let (_, factory, count) = cases::sources();
    assert_eq!(count, 4336, "native case inventory changed");
    let program = tests::mir(&format!("{factory}\nfn main() -> i32 = exercise()\n"));
    build_module(
        &ctx,
        &module,
        &program,
        &tm,
        None,
        &[],
        false,
        ModuleScope::Whole,
    )
    .map_err(|e| e.to_string())?;
    module.verify().map_err(|e| e.to_string())?;
    let data = tm.get_target_data();
    let mut calls = std::collections::HashMap::new();
    let mut scratch = std::collections::HashSet::new();
    let mut total_record_calls = 0;
    for function in module.get_functions() {
        let mut record_calls = 0;
        for block in function.get_basic_blocks() {
            for instruction in block.get_instructions() {
                if instruction.get_opcode() == InstructionOpcode::Alloca {
                    assert!(
                        Some(block) == function.get_first_basic_block(),
                        "dynamic loop alloca"
                    );
                }
                if let Ok(call) = CallSiteValue::try_from(instruction)
                    && let Some(callee) = call.get_called_fn_value()
                {
                    let name = callee.get_name().to_string_lossy();
                    if name.starts_with("sysv_")
                        && !matches!(name.as_ref(), "sysv_data" | "sysv_pointer" | "sysv_seen")
                    {
                        record_calls += 1;
                    }
                    for ordinal in 0..call.count_arguments() {
                        // Align's internal return-transport pass has its own storage contract.
                        if callee.get_name().to_string_lossy().starts_with("sysv_")
                            && ["byval", "sret"].iter().any(|kind| {
                                call.get_enum_attribute(
                                    AttributeLoc::Param(ordinal),
                                    Attribute::get_named_enum_kind_id(kind),
                                )
                                .is_some()
                            })
                        {
                            let value = instruction
                                .get_operand(ordinal)
                                .and_then(|operand| operand.value())
                                .ok_or("missing native memory operand")?;
                            let BasicValueEnum::PointerValue(pointer) = value else {
                                return Err("non-pointer native memory operand".into());
                            };
                            assert_eq!(
                                pointer.as_instruction_value().map(|slot| slot.get_opcode()),
                                Some(InstructionOpcode::Alloca),
                                "native scratch must have its own allocation"
                            );
                            assert!(
                                scratch.insert(pointer),
                                "argument/result scratch was reused across static operands"
                            );
                        }
                    }
                    calls
                        .entry(callee.get_name().to_string_lossy().into_owned())
                        .or_insert_with(Vec::new)
                        .push(call);
                }
            }
        }
        // Keep the ABI matrix out of one giant frontend/LLVM function. This bound
        // includes the seven cross-record/loop probes in the coordinating function.
        assert!(
            record_calls <= 8,
            "{} bundles {record_calls} native record calls",
            function.get_name().to_string_lossy()
        );
        total_record_calls += record_calls;
    }
    assert_eq!(
        total_record_calls, 241,
        "native call sites must all survive"
    );
    let attrs = |function: FunctionValue<'_>, ordinal, kind| {
        function.get_enum_attribute(
            AttributeLoc::Param(ordinal),
            Attribute::get_named_enum_kind_id(kind),
        )
    };
    let repeat = |ty: &str, n| std::iter::repeat_n(ty, n).collect::<Vec<_>>().join(",");
    let tail = "i8,i16,float,i64";
    // Golden physical signatures are independent of the classifier and source generator.
    // (name, physical types, result, sret alignment, byval ordinal/size/alignment).
    let golden = [
        (
            "sysv_First_0_0_false",
            format!("ptr,ptr,ptr,{tail}"),
            "void",
            Some(8),
            vec![(1, 24, 8), (2, 24, 8)],
        ),
        (
            "sysv_Bytes17_0_0_false",
            format!("ptr,ptr,ptr,{tail}"),
            "void",
            Some(1),
            vec![(1, 17, 8), (2, 17, 8)],
        ),
        (
            "sysv_P32_0_0_false",
            format!("ptr,ptr,ptr,{tail}"),
            "void",
            Some(32),
            vec![(1, 32, 32), (2, 32, 32)],
        ),
        (
            "sysv_Page_0_0_false",
            format!("ptr,ptr,ptr,{tail}"),
            "void",
            Some(4096),
            vec![(1, 4096, 4096), (2, 4096, 4096)],
        ),
        (
            "sysv_Pair_4_0_false",
            format!("{},ptr,{tail}", repeat("i64", 6)),
            "{ i64, i64 }",
            None,
            vec![(6, 16, 8)],
        ),
        (
            "sysv_Pair_5_0_false",
            format!("{},ptr,ptr,{tail}", repeat("i64", 5)),
            "{ i64, i64 }",
            None,
            vec![(5, 16, 8), (6, 16, 8)],
        ),
        (
            "sysv_Pair_3_0_true",
            format!("ptr,{},ptr,{tail}", repeat("i64", 5)),
            "void",
            Some(8),
            vec![(6, 16, 8)],
        ),
        (
            "sysv_Pair_4_0_true",
            format!("ptr,{},ptr,ptr,{tail}", repeat("i64", 4)),
            "void",
            Some(8),
            vec![(5, 16, 8), (6, 16, 8)],
        ),
        (
            "sysv_F8N2_0_6_false",
            format!("{},ptr,{tail}", repeat("double", 8)),
            "{ double, double }",
            None,
            vec![(8, 16, 8)],
        ),
        (
            "sysv_F8N2_0_7_false",
            format!("{},ptr,ptr,{tail}", repeat("double", 7)),
            "{ double, double }",
            None,
            vec![(7, 16, 8), (8, 16, 8)],
        ),
        (
            "sysv_F8N1_0_8_false",
            format!("{},ptr,ptr,{tail}", repeat("double", 8)),
            "double",
            None,
            vec![(8, 8, 8), (9, 8, 8)],
        ),
        (
            "sysv_Byte_6_0_false",
            format!("{},ptr,ptr,{tail}", repeat("i64", 6)),
            "i64",
            None,
            vec![(6, 1, 8), (7, 1, 8)],
        ),
        (
            "sysv_P16_5_0_false",
            format!("{},ptr,{tail}", repeat("i64", 6)),
            "i64",
            None,
            vec![(6, 16, 16)],
        ),
        (
            "sysv_rollback_gp",
            format!("{},ptr,i64,i64", repeat("i64", 5)),
            "i32",
            None,
            vec![(5, 16, 8)],
        ),
        (
            "sysv_rollback_fp",
            format!("{},ptr,double,double", repeat("double", 7)),
            "i32",
            None,
            vec![(7, 16, 8)],
        ),
        (
            "sysv_rollback_mixed",
            format!("{},ptr,i64,i64,i64", repeat("double", 8)),
            "i32",
            None,
            vec![(8, 16, 8)],
        ),
        (
            "sysv_rollback_reverse",
            format!("{},ptr,double,double,double", repeat("i64", 6)),
            "i32",
            None,
            vec![(6, 16, 8)],
        ),
        (
            "sysv_view_pressure",
            "ptr,i64,i64,i64,ptr,ptr,i64".into(),
            "void",
            Some(8),
            vec![(5, 16, 8)],
        ),
        (
            "sysv_before_memory",
            "ptr,i64,i64,i64,i64,i64,i64".into(),
            "i32",
            None,
            vec![(0, 24, 8)],
        ),
    ];
    for (name, parameters, result, sret, byval) in golden {
        let function = module
            .get_function(name)
            .ok_or_else(|| format!("missing {name}"))?;
        let actual = function
            .get_type()
            .get_param_types()
            .iter()
            .map(|t| t.print_to_string().to_string())
            .collect::<Vec<_>>()
            .join(",");
        assert_eq!(actual, parameters, "{name}");
        assert_eq!(
            function
                .get_type()
                .get_return_type()
                .map(|t| t.print_to_string().to_string())
                .unwrap_or_else(|| "void".into()),
            result,
            "{name}"
        );
        let actual_calls = calls
            .get(name)
            .ok_or_else(|| format!("no actual call to {name}"))?;
        for ordinal in 0..function.count_params() {
            let expected = byval.iter().find(|(i, _, _)| *i == ordinal);
            let attribute = attrs(function, ordinal, "byval");
            assert_eq!(attribute.is_some(), expected.is_some(), "{name}/{ordinal}");
            if let Some((_, size, alignment)) = expected {
                let attribute = attribute.ok_or("missing byval")?;
                assert!(attribute.is_type());
                assert_eq!(
                    data.get_abi_size(&attribute.get_type_value()),
                    *size,
                    "{name}/{ordinal}"
                );
                assert_eq!(
                    attrs(function, ordinal, "align").map(|a| a.get_enum_value()),
                    Some(*alignment)
                );
            }
            assert_eq!(
                attrs(function, ordinal, "sret").is_some(),
                ordinal == 0 && sret.is_some(),
                "{name}/{ordinal}"
            );
            if ordinal == 0
                && let Some(alignment) = sret
            {
                assert!(attrs(function, 0, "sret").ok_or("missing sret")?.is_type());
                assert_eq!(
                    attrs(function, 0, "align").map(|a| a.get_enum_value()),
                    Some(alignment)
                );
            }
            for call in actual_calls {
                for kind in ["byval", "sret", "align"] {
                    assert_eq!(
                        call.get_enum_attribute(
                            AttributeLoc::Param(ordinal),
                            Attribute::get_named_enum_kind_id(kind)
                        ),
                        attrs(function, ordinal, kind),
                        "{name}/{ordinal}/{kind}"
                    );
                }
            }
        }
    }
    let ir = module.print_to_string().to_string();
    for expected in [
        "alloca [512 x i64], align 4096",
        "alloca [3 x i64], align 8",
        "alloca [4 x i64], align 32",
        "alloca [2 x i64], align 16",
    ] {
        assert!(ir.contains(expected), "missing full storage {expected}");
    }
    // Metadata checks stay total even when bypassing the source formation owner.
    let def = program
        .structs
        .iter()
        .find(|s| s.name.as_str() == "Sret")
        .ok_or("missing Sret")?;
    let st = ctx.struct_type(&[ctx.i64_type().into(); 3], false);
    for alignment in [0, 3] {
        let mut malformed = def.clone();
        malformed.align = Some(alignment);
        assert!(SysvMemory::new(0, st, &malformed, &data).is_err());
    }
    let oversized = ctx.struct_type(
        &[
            ctx.i64_type().array_type(u32::MAX).into(),
            ctx.i64_type().into(),
        ],
        false,
    );
    assert!(SysvMemory::new(0, oversized, def, &data).is_err());
    for declaration in [
        "layout(C) Big {a:i64,b:i64,c:i64}\nextern \"C\" fn align_rt_free(value:Big)",
        "layout(C) Big {a:i64,b:i64,c:i64}\nextern \"C\" fn align_rt_free() -> Big",
        "layout(C) One {a:i64}\nextern \"C\" fn align_rt_buffer_capacity(value:One) -> i64",
    ] {
        let program = tests::mir(&format!("{declaration}\nfn main() {{}}"));
        let rejected = ctx.create_module("fixed_native");
        let error = build_module(
            &ctx,
            &rejected,
            &program,
            &tm,
            None,
            &[],
            false,
            ModuleScope::Whole,
        )
        .err()
        .ok_or("accepted fixed native record")?;
        assert!(
            error.to_string().contains("native extern ABI mismatch"),
            "{error}"
        );
        assert!(rejected.get_first_function().is_none());
    }
    for unsupported in [
        "x86_64-unknown-linux-gnux32",
        "x86_64-pc-windows-msvc",
        "x86_64-apple-macosx26.0",
    ] {
        let triple = TargetTriple::create(unsupported);
        let machine = Target::from_triple(&triple)
            .map_err(|e| e.to_string())?
            .create_target_machine(
                &triple,
                "x86-64",
                "",
                OptimizationLevel::Default,
                RelocMode::PIC,
                CodeModel::Default,
            )
            .ok_or("missing refused target")?;
        let rejected = ctx.create_module("refused_target");
        assert!(
            build_module(
                &ctx,
                &rejected,
                &program,
                &machine,
                None,
                &[],
                false,
                ModuleScope::Whole
            )
            .is_err(),
            "{unsupported}"
        );
        assert!(rejected.get_first_function().is_none());
    }
    Ok(())
}
