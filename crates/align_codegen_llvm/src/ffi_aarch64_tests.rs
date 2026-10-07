use super::*;
use inkwell::attributes::{Attribute, AttributeLoc};
use inkwell::targets::{TargetData, TargetTriple};

#[test]
fn arm64_target_domain_is_closed() {
    let little = TargetData::create("e-p:64:64");
    let big = TargetData::create("E-p:64:64");
    let narrow = TargetData::create("e-p:32:32");
    for triple in [
        "aarch64-unknown-linux-gnu",
        "aarch64-unknown-linux-musl",
        "arm64-apple-macosx26.0",
        "aarch64-apple-darwin27.0.0",
    ] {
        assert!(
            ffi_aarch64::Abi::for_target(triple, &little).is_some(),
            "{triple}"
        );
        assert!(ffi_aarch64::Abi::for_target(triple, &big).is_none());
        assert!(ffi_aarch64::Abi::for_target(triple, &narrow).is_none());
    }
    for triple in [
        "aarch64_be-unknown-linux-gnu",
        "aarch64-unknown-linux-gnu_ilp32",
        "aarch64-unknown-linux-android",
        "aarch64-pc-windows-msvc",
        "arm64-apple-ios17.0",
        "arm64-apple-darwin27.0-simulator",
        "aarch64-unknown-freebsd",
        "aarch64-unknown-linux-magic",
        "arm64_32-apple-watchos",
        "arm64-apple-macosxgarbage",
    ] {
        assert!(
            ffi_aarch64::Abi::for_target(triple, &little).is_none(),
            "{triple}"
        );
    }
}

#[test]
fn arm64_record_types_attributes_storage_and_native_preflight() -> Result<(), String> {
    Target::initialize_aarch64(&InitializationConfig::default());
    let _example = tests::mir(
        "layout(C) NativeParams { length:u64,data:raw,flags:u8 }\nextern \"C\" fn probe_params(value:NativeParams,after:i64) -> NativeParams\nfn main() { unsafe { result := probe_params(NativeParams { length:42,data:raw.null(),flags:1 },77) } }\n",
    );
    // Name, fields, values, alignment, Linux argument, Darwin argument, return (H = native HFA).
    let cases = [
        ("B", "a:u8", "a:250", 0u32, "i64", "i64", "i8"),
        ("T", "a:u8,b:u8,c:u8", "a:1,b:2,c:3", 0, "i64", "i64", "i24"),
        ("I", "a:i64", "a:-42", 0, "i64", "i64", "i64"),
        (
            "W",
            "a:i8,b:u16,c:i32,d:u64",
            "a:-2,b:60000,c:-42,d:77",
            0,
            "[2 x i64]",
            "[2 x i64]",
            "[2 x i64]",
        ),
        (
            "Twelve",
            "a:i32,b:i32,c:i32",
            "a:1,b:2,c:3",
            0,
            "[2 x i64]",
            "[2 x i64]",
            "[2 x i64]",
        ),
        (
            "Mix",
            "a:i64,b:f64",
            "a:42,b:3.5",
            0,
            "[2 x i64]",
            "[2 x i64]",
            "[2 x i64]",
        ),
        ("P", "a:raw", "a:raw.null()", 0, "ptr", "ptr", "i64"),
        (
            "PP",
            "a:raw,b:raw",
            "a:raw.null(),b:raw.null()",
            0,
            "[2 x ptr]",
            "[2 x ptr]",
            "[2 x i64]",
        ),
        (
            "P16",
            "a:raw",
            "a:raw.null()",
            16,
            "[2 x ptr]",
            "i128",
            "i128",
        ),
        ("I16", "a:i64", "a:42", 16, "[2 x i64]", "i128", "i128"),
        ("F16", "a:f64", "a:3.5", 16, "[2 x i64]", "i128", "i128"),
        (
            "FF16",
            "a:f32,b:f32",
            "a:1.5,b:2.5",
            16,
            "[2 x i64]",
            "i128",
            "i128",
        ),
        ("H1", "a:f32", "a:1.5", 0, "[1 x float]", "[1 x float]", "H"),
        (
            "H2",
            "a:f32,b:f32",
            "a:1.5,b:2.5",
            0,
            "[2 x float]",
            "[2 x float]",
            "H",
        ),
        (
            "H3",
            "a:f32,b:f32,c:f32",
            "a:1.5,b:2.5,c:3.5",
            0,
            "[3 x float]",
            "[3 x float]",
            "H",
        ),
        (
            "H4",
            "a:f32,b:f32,c:f32,d:f32",
            "a:1.5,b:2.5,c:3.5,d:4.5",
            16,
            "[4 x float]",
            "[4 x float]",
            "H",
        ),
        (
            "D1",
            "a:f64",
            "a:1.5",
            0,
            "[1 x double]",
            "[1 x double]",
            "H",
        ),
        (
            "D2",
            "a:f64,b:f64",
            "a:1.5,b:2.5",
            16,
            "[2 x double]",
            "[2 x double]",
            "H",
        ),
        (
            "D3",
            "a:f64,b:f64,c:f64",
            "a:1.5,b:2.5,c:3.5",
            0,
            "[3 x double]",
            "[3 x double]",
            "H",
        ),
        (
            "D4",
            "a:f64,b:f64,c:f64,d:f64",
            "a:1.5,b:2.5,c:3.5,d:4.5",
            0,
            "[4 x double]",
            "[4 x double]",
            "H",
        ),
        (
            "D5",
            "a:f64,b:f64,c:f64,d:f64,e:f64",
            "a:1.5,b:2.5,c:3.5,d:4.5,e:5.5",
            0,
            "ptr",
            "ptr",
            "void",
        ),
        (
            "Params",
            "a:u64,b:raw,c:u8",
            "a:42,b:raw.null(),c:1",
            0,
            "ptr",
            "ptr",
            "void",
        ),
        ("A32", "a:raw", "a:raw.null()", 32, "ptr", "ptr", "void"),
        (
            "D32",
            "a:f64,b:f64",
            "a:1.5,b:2.5",
            32,
            "ptr",
            "ptr",
            "void",
        ),
    ];
    let mut source = String::new();
    let mut calls = String::new();
    for (name, fields, values, alignment, ..) in cases {
        if alignment > 0 {
            source.push_str(&format!("align({alignment}) "));
        }
        source.push_str(&format!("layout(C) {name} {{ {fields} }}\nextern \"C\" fn echo_{name}(value:{name},signed:i8,unsigned:u16,tail:i64) -> {name}\n"));
        calls.push_str(&format!(
            "    x{name} := echo_{name}({name} {{ {values} }},-2,60000,77)\n"
        ));
    }
    source.push_str("extern \"C\" fn narrow_signed(value:i16) -> i8\nextern \"C\" fn narrow_unsigned(value:u8) -> u16\nfn main() -> i32 {\n unsafe {\n");
    source.push_str(&calls);
    source.push_str("mut iteration := 0\nloop { if iteration == 100 { break }; loop_value := echo_Params(Params { a:42,b:raw.null(),c:1 },-2,60000,77); iteration = iteration + 1 }\n s := narrow_signed(-42)\n u := narrow_unsigned(250)\n return 0\n }\n}\n");
    let program = tests::mir(&source);
    for triple in ["aarch64-unknown-linux-gnu", "arm64-apple-macosx26.0"] {
        let triple = TargetTriple::create(triple);
        let machine = Target::from_triple(&triple)
            .map_err(|error| error.to_string())?
            .create_target_machine(
                &triple,
                "generic",
                "",
                OptimizationLevel::Default,
                RelocMode::PIC,
                CodeModel::Default,
            )
            .ok_or("ARM target machine unavailable")?;
        let darwin = triple.as_str().to_string_lossy().contains("apple");
        let context = Context::create();
        let module = context.create_module("arm_records");
        module.set_triple(&triple);
        module.set_data_layout(&machine.get_target_data().get_data_layout());
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
        .map_err(|error| error.to_string())?;
        module.verify().map_err(|error| error.to_string())?;
        let ir = module.print_to_string().to_string();
        for (name, _, _, alignment, linux_arg, darwin_arg, result) in cases {
            let function = module
                .get_function(&format!("echo_{name}"))
                .ok_or("missing record extern")?;
            let offset = u32::from(result == "void");
            assert_eq!(function.count_params(), 4 + offset, "{name}");
            let params = function.get_type().get_param_types();
            assert_eq!(
                params
                    .get(offset as usize)
                    .ok_or("missing record parameter")?
                    .print_to_string()
                    .to_string(),
                if darwin { darwin_arg } else { linux_arg },
                "{name}"
            );
            let returned = function.get_type().get_return_type();
            match result {
                "void" => assert!(returned.is_none(), "{name}"),
                "H" => assert!(
                    matches!(returned, Some(BasicTypeEnum::StructType(_))),
                    "{name}"
                ),
                scalar => assert_eq!(
                    returned
                        .ok_or("missing return")?
                        .print_to_string()
                        .to_string(),
                    scalar,
                    "{name}"
                ),
            }
            let call = ir
                .lines()
                .find(|line| line.contains("call ") && line.contains(&format!("@echo_{name}(")))
                .ok_or("missing record call")?;
            for (index, kind) in [(offset + 1, "signext"), (offset + 2, "zeroext")] {
                assert_eq!(
                    function
                        .get_enum_attribute(
                            AttributeLoc::Param(index),
                            Attribute::get_named_enum_kind_id(kind)
                        )
                        .is_some(),
                    darwin,
                    "{name}/{kind}"
                );
                assert_eq!(call.contains(kind), darwin, "{name}/{call}");
            }
            assert_eq!(
                function
                    .get_enum_attribute(
                        AttributeLoc::Param(offset),
                        Attribute::get_named_enum_kind_id("alignstack")
                    )
                    .map(|attribute| attribute.get_enum_value()),
                (!darwin && result == "H").then_some(8),
                "{name}"
            );
            assert_eq!(
                call.contains("alignstack(8)"),
                !darwin && result == "H",
                "{name}/{call}"
            );
            assert!(!call.contains("byval"), "{call}");
            if result == "void" {
                assert!(call.contains("sret("), "{call}");
                assert_eq!(
                    function
                        .get_enum_attribute(
                            AttributeLoc::Param(0),
                            Attribute::get_named_enum_kind_id("align")
                        )
                        .map(|attribute| attribute.get_enum_value()),
                    Some(u64::from(alignment.max(8))),
                    "{name}"
                );
            }
        }
        for (name, extension) in [("narrow_signed", "signext"), ("narrow_unsigned", "zeroext")] {
            let function = module.get_function(name).ok_or("missing scalar extern")?;
            let call = ir
                .lines()
                .find(|line| line.contains("call ") && line.contains(&format!("@{name}(")))
                .ok_or("missing scalar call")?;
            assert_eq!(call.matches(extension).count(), if darwin { 2 } else { 0 });
            for location in [AttributeLoc::Return, AttributeLoc::Param(0)] {
                assert_eq!(
                    function
                        .get_enum_attribute(location, Attribute::get_named_enum_kind_id(extension))
                        .is_some(),
                    darwin
                );
            }
        }
        // Each ARM record call owns two distinct full slots, including indirect argument/result.
        let slots = ir
            .lines()
            .filter(|line| line.contains("arm_record") && line.contains(" = alloca "))
            .collect::<Vec<_>>();
        assert_eq!(slots.len(), 2 * (cases.len() + 1));
        for function in module.get_functions() {
            for block in function.get_basic_blocks() {
                for instruction in block.get_instructions() {
                    if instruction.get_opcode() == inkwell::values::InstructionOpcode::Alloca {
                        assert!(
                            Some(block) == function.get_first_basic_block(),
                            "dynamic loop alloca"
                        );
                    }
                }
            }
        }
        assert!(
            slots
                .iter()
                .any(|line| line.contains("[5 x i64]") && line.contains("align 8"))
        );
        assert!(
            slots
                .iter()
                .any(|line| line.contains("[4 x i64]") && line.contains("align 32"))
        );
        assert!(
            slots
                .iter()
                .any(|line| line.contains("[2 x i64]") && line.contains("align 16"))
        );
        for signature in [
            "fn align_rt_free(value: Big)",
            "fn align_rt_free() -> Big",
            "fn align_rt_free(value: Ptr)",
        ] {
            let forged = tests::mir(&format!(
                "layout(C) Big {{ a:i64,b:i64,c:i64 }}\nlayout(C) Ptr {{ a:raw }}\nextern \"C\" {signature}\nfn main() {{}}\n"
            ));
            let rejected = context.create_module("native_collision");
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
            .ok_or("native record collision accepted")?;
            assert!(
                failure.to_string().contains("native extern ABI mismatch"),
                "{failure}"
            );
            assert_eq!(rejected.get_functions().count(), 0);
        }
    }
    for unsupported in [
        "aarch64-unknown-freebsd",
        "aarch64-pc-windows-msvc",
        "arm64-apple-ios17.0",
    ] {
        let triple = TargetTriple::create(unsupported);
        let machine = Target::from_triple(&triple)
            .map_err(|error| error.to_string())?
            .create_target_machine(
                &triple,
                "generic",
                "",
                OptimizationLevel::Default,
                RelocMode::PIC,
                CodeModel::Default,
            )
            .ok_or("unsupported-target machine unavailable")?;
        let context = Context::create();
        let module = context.create_module("unsupported_record_abi");
        let error = build_module(
            &context,
            &module,
            &program,
            &machine,
            None,
            &[],
            false,
            ModuleScope::Whole,
        )
        .err()
        .ok_or("unsupported record ABI was accepted")?;
        assert!(error.to_string().contains("little-endian ARM64"), "{error}");
        assert_eq!(module.get_functions().count(), 0);
    }
    Ok(())
}
