use super::*;
use inkwell::{
    targets::FileType,
    values::{CallSiteValue, InstructionOpcode},
};

#[path = "../../align_driver/tests/helpers/ffi_fixed_array_cases.rs"]
mod cases;

fn verify(artifacts: Option<&std::path::Path>) -> Result<(), String> {
    Target::initialize_x86(&InitializationConfig::default());
    Target::initialize_aarch64(&InitializationConfig::default());
    let (c, source, count) = cases::sources();
    assert_eq!(count, 392);
    let source = format!("{source}\nfn main() -> i32 = exercise()\n");
    let program = tests::mir(&source);
    if let Some(path) = artifacts {
        std::fs::write(path.join("oracle.c"), c).map_err(|e| e.to_string())?;
        std::fs::write(path.join("fixture.align"), source).map_err(|e| e.to_string())?;
    }
    for triple in [
        "x86_64-unknown-linux-gnu",
        "aarch64-unknown-linux-gnu",
        "arm64-apple-macosx26.0",
    ] {
        let sysv = triple.starts_with("x86");
        let darwin = triple.contains("apple");
        let target = TargetTriple::create(triple);
        let machine = Target::from_triple(&target)
            .map_err(|e| e.to_string())?
            .create_target_machine(
                &target,
                if sysv { "x86-64-v2" } else { "generic" },
                "",
                OptimizationLevel::Default,
                RelocMode::PIC,
                CodeModel::Default,
            )
            .ok_or("target machine")?;
        let context = Context::create();
        let module = context.create_module("fixed_c_fields");
        module.set_triple(&target);
        let data = machine.get_target_data();
        module.set_data_layout(&data.get_data_layout());
        build_module(
            &context,
            &module,
            &program,
            &machine,
            None,
            &[],
            false,
            ModuleScope::Whole,
        )
        .map_err(|e| e.to_string())?;
        module.verify().map_err(|e| e.to_string())?;
        let records = cases::records();
        for (id, definition) in program.structs.iter().enumerate() {
            if !definition.c_repr {
                continue;
            }
            let native = context
                .get_struct_type(&definition.source_name)
                .ok_or("C record LLVM type")?;
            assert_eq!(
                align_sema::struct_abi_layout(
                    id as u32,
                    &program.structs,
                    &program.tuples,
                    &program.enums,
                    &program.tagged_types
                ),
                (
                    data.get_abi_size(&native),
                    u64::from(data.get_abi_alignment(&native))
                ),
                "{triple}: {}",
                definition.name
            );
        }
        // Independent physical expectations; SSE eightbytes may use a wider coerce than Clang's
        // scalar/vector spelling, but must occupy exactly the same native register class.
        for (name, sysv_args, sysv_result, arm_argument, arm_result) in [
            ("Cfai8", "i64", "i64", "i64", "i24"),
            (
                "Cfai32",
                "i64,i64",
                "{ i64, i64 }",
                "[2 x i64]",
                "[2 x i64]",
            ),
            ("Cfai64", "ptr", "void", "ptr", "void"),
            (
                "Cfaf32",
                "double,double",
                "{ double, double }",
                "[3 x float]",
                "record",
            ),
            ("Cfaf64", "ptr", "void", "[3 x double]", "record"),
            ("Cfaf32n1", "double", "double", "[1 x float]", "record"),
            (
                "Cfaf32n4",
                "double,double",
                "{ double, double }",
                "[4 x float]",
                "record",
            ),
            ("Cfaf32n5", "ptr", "void", "ptr", "void"),
            ("Cfaf64n4", "ptr", "void", "[4 x double]", "record"),
            (
                "CfaMixedIS",
                "i64,double",
                "{ i64, double }",
                "[2 x i64]",
                "[2 x i64]",
            ),
            (
                "CfaMixedSI",
                "double,i64",
                "{ double, i64 }",
                "[2 x i64]",
                "[2 x i64]",
            ),
            ("CfaMerged", "i64", "i64", "i64", "i64"),
            (
                "CfaHfaJoined",
                "double,double",
                "{ double, double }",
                "[3 x float]",
                "record",
            ),
            (
                "CfaAlignedHfa",
                "double,double",
                "{ double, double }",
                "[4 x float]",
                "record",
            ),
            (
                "CfaPadded",
                "double,double",
                "{ double, double }",
                if darwin { "i128" } else { "[2 x i64]" },
                "i128",
            ),
            (
                "CfaPointer",
                "i64,i64",
                "{ i64, i64 }",
                "[2 x i64]",
                "[2 x i64]",
            ),
            ("CfaWide", "ptr", "void", "ptr", "void"),
            ("CfaShape", "ptr", "void", "ptr", "void"),
            ("CfaKey", "ptr", "void", "ptr", "void"),
        ] {
            let ordinal = records
                .iter()
                .position(|row| row.name == name)
                .ok_or("fixture record")?;
            let function = module
                .get_function(&format!("cfa_echo_{ordinal}_0_0"))
                .ok_or("extern")?;
            let result = function.get_type().get_return_type();
            let expected_result = if sysv { sysv_result } else { arm_result };
            if expected_result == "record" {
                assert_eq!(
                    result,
                    Some(context.get_struct_type(name).ok_or("HFA type")?.into())
                );
            } else {
                assert_eq!(
                    result
                        .map(|ty| ty.print_to_string().to_string())
                        .unwrap_or_else(|| "void".into()),
                    expected_result,
                    "{triple}/{name}"
                );
            }
            let expected_arguments = if sysv { sysv_args } else { arm_argument }
                .split(',')
                .collect::<Vec<_>>();
            let parameters = function.get_type().get_param_types();
            let offset = usize::from(expected_result == "void");
            for (i, expected) in expected_arguments.iter().enumerate() {
                assert_eq!(
                    parameters[offset + i].print_to_string().to_string(),
                    *expected,
                    "{triple}/{name}/{i}"
                );
            }
            if offset == 1 {
                let kind = inkwell::attributes::Attribute::get_named_enum_kind_id("sret");
                assert!(
                    function
                        .get_enum_attribute(inkwell::attributes::AttributeLoc::Param(0), kind)
                        .is_some(),
                    "{name} sret"
                );
            }
        }
        let mut observed = 0;
        for function in module.get_functions() {
            for block in function.get_basic_blocks() {
                for instruction in block.get_instructions() {
                    if instruction.get_opcode() == InstructionOpcode::Alloca {
                        assert_eq!(
                            Some(block),
                            function.get_first_basic_block(),
                            "dynamic ABI scratch"
                        );
                    }
                    if let Ok(call) = CallSiteValue::try_from(instruction)
                        && let Some(callee) = call.get_called_fn_value()
                        && callee.get_name().to_string_lossy().starts_with("cfa_echo_")
                    {
                        observed += 1;
                        for ordinal in 0..callee.count_params() {
                            let at = inkwell::attributes::AttributeLoc::Param(ordinal);
                            for attribute in
                                ["sret", "byval", "align", "alignstack", "signext", "zeroext"]
                            {
                                let kind = inkwell::attributes::Attribute::get_named_enum_kind_id(
                                    attribute,
                                );
                                assert_eq!(
                                    call.get_enum_attribute(at, kind),
                                    callee.get_enum_attribute(at, kind),
                                    "{triple}/{attribute}/{ordinal}"
                                );
                            }
                        }
                    }
                }
            }
        }
        assert_eq!(observed, count, "{triple} actual emitted calls");
        for field in ["[i64; 3]", "[f32; 3]", "[i8; 3]"] {
            for signature in ["(value: Bad)", "() -> Bad"] {
                let forged = tests::mir(&format!(
                    "layout(C) Bad {{ value:{field} }}\nextern \"C\" fn align_rt_free{signature}\nfn main() {{}}\n"
                ));
                let rejected = context.create_module("fixed_native_collision");
                let failure = build_module(
                    &context,
                    &rejected,
                    &forged,
                    &machine,
                    None,
                    &[],
                    false,
                    ModuleScope::Whole,
                )
                .err()
                .ok_or("fixed-array native collision accepted")?;
                assert!(
                    failure.to_string().contains("native extern ABI mismatch"),
                    "{failure}"
                );
                assert_eq!(rejected.get_functions().count(), 0);
            }
        }
        if let Some(path) = artifacts {
            module
                .print_to_file(path.join(format!("{triple}.ll")))
                .map_err(|e| e.to_string())?;
            machine
                .write_to_file(&module, FileType::Object, &path.join(format!("{triple}.o")))
                .map_err(|e| e.to_string())?;
        }
    }
    Ok(())
}

#[test]
fn fixed_numeric_c_record_abis() -> Result<(), String> {
    verify(None)
}

#[test]
#[ignore = "explicit retained cross-target objects for the local C execution oracle"]
fn export_fixed_numeric_c_record_abis() -> Result<(), String> {
    let path = std::path::PathBuf::from(
        std::env::var_os("ALIGN_C_FIXED_ABI_ARTIFACTS").ok_or("set a new artifact directory")?,
    );
    std::fs::create_dir(&path).map_err(|e| e.to_string())?;
    struct OwnedExport(std::path::PathBuf, bool);
    impl Drop for OwnedExport {
        fn drop(&mut self) {
            if !self.1 {
                let _ = std::fs::remove_dir_all(&self.0);
            }
        }
    }
    let mut owned = OwnedExport(path, false);
    verify(Some(&owned.0))?;
    owned.1 = true;
    Ok(())
}
