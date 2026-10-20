use super::*;
use inkwell::{
    targets::FileType,
    values::{AnyValue, CallSiteValue},
};
#[path = "../../align_driver/tests/helpers/trig_cases.rs"]
mod cases;

fn verify(artifacts: Option<&std::path::Path>) -> Result<(), String> {
    Target::initialize_x86(&InitializationConfig::default());
    Target::initialize_aarch64(&InitializationConfig::default());
    let (oracle, source, observations) = cases::sources();
    assert_eq!(observations, 1541);
    let source = format!("{source}\nfn main()->i32=exercise()\n");
    let program = tests::mir(&source);
    if let Some(path) = artifacts {
        std::fs::write(path.join("oracle.c"), oracle).map_err(|e| e.to_string())?;
        std::fs::write(path.join("fixture.align"), source).map_err(|e| e.to_string())?;
    }
    for triple in [
        "x86_64-unknown-linux-gnu",
        "aarch64-unknown-linux-gnu",
        "arm64-apple-macosx26.0",
    ] {
        let target = TargetTriple::create(triple);
        let machine = Target::from_triple(&target)
            .map_err(|e| e.to_string())?
            .create_target_machine(
                &target,
                if triple.starts_with("x86") {
                    "x86-64-v2"
                } else {
                    "generic"
                },
                "",
                OptimizationLevel::Default,
                RelocMode::PIC,
                CodeModel::Default,
            )
            .ok_or("target machine")?;
        let context = Context::create();
        let module = context.create_module("trigonometry");
        module.set_triple(&target);
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
        .map_err(|e| e.to_string())?;
        module.verify().map_err(|e| e.to_string())?;
        let mut calls = [0; 3];
        let mut types = std::collections::BTreeSet::new();
        for function in module.get_functions() {
            for block in function.get_basic_blocks() {
                for instruction in block.get_instructions() {
                    let Ok(call) = CallSiteValue::try_from(instruction) else {
                        continue;
                    };
                    let Some(callee) = call.get_called_fn_value() else {
                        continue;
                    };
                    let name = callee.get_name().to_string_lossy();
                    assert!(
                        !name.contains("alloc"),
                        "numeric fixture adds allocation: {name}"
                    );
                    let operation = if name.starts_with("llvm.sin.") {
                        0
                    } else if name.starts_with("llvm.cos.") {
                        1
                    } else if name.starts_with("llvm.atan2.") {
                        2
                    } else {
                        continue;
                    };
                    calls[operation] += 1;
                    types.insert(name.into_owned());
                    let ret = callee.get_type().get_return_type().ok_or("math return")?;
                    assert_eq!(call.count_arguments(), if operation == 2 { 2 } else { 1 });
                    assert!(
                        callee
                            .get_type()
                            .get_param_types()
                            .iter()
                            .all(|arg| *arg == ret.into())
                    );
                    assert_eq!(
                        call.try_as_basic_value()
                            .basic()
                            .ok_or("call result")?
                            .get_type(),
                        ret
                    );
                    let line = instruction.print_to_string().to_string();
                    for flag in [
                        " fast ",
                        " afn ",
                        " nnan ",
                        " ninf ",
                        " nsz ",
                        " reassoc ",
                        " arcp ",
                        " contract ",
                    ] {
                        assert!(!line.contains(flag), "{line}");
                    }
                }
            }
        }
        assert_eq!(calls, [20, 20, 10], "{triple}");
        assert_eq!(types.len(), 30);
        for operation in ["sin", "cos", "atan2"] {
            for bits in [32, 64] {
                for lanes in [1, 2, 4, 8, 16] {
                    let suffix = if lanes == 1 {
                        format!("f{bits}")
                    } else {
                        format!("v{lanes}f{bits}")
                    };
                    assert!(types.contains(&format!("llvm.{operation}.{suffix}")));
                }
            }
        }
        let qualify =
            |declaration: &str, exports: &[String], accepted: bool| -> Result<(), String> {
                let candidate = tests::mir(&format!("{declaration}\nfn main()->i32=0\n"));
                let output = context.create_module("identity");
                output.set_triple(&target);
                output.set_data_layout(&machine.get_target_data().get_data_layout());
                let result = build_module(
                    &context,
                    &output,
                    &candidate,
                    &machine,
                    None,
                    exports,
                    false,
                    ModuleScope::Whole,
                );
                if accepted {
                    result.map_err(|e| format!("{triple}: {declaration}: {e}"))?;
                    output.verify().map_err(|e| e.to_string())?;
                } else {
                    let error = result
                        .err()
                        .ok_or_else(|| format!("accepted {triple}: {declaration}"))?
                        .to_string();
                    assert!(
                        error.contains("native extern ABI mismatch")
                            || error.contains("callable external identity collision"),
                        "{error}"
                    );
                    assert_eq!(
                        output.get_functions().count(),
                        0,
                        "rejection must precede declarations"
                    );
                }
                Ok(())
            };
        for (symbol, bits, arity) in [
            ("sinf", 32, 1),
            ("sin", 64, 1),
            ("cosf", 32, 1),
            ("cos", 64, 1),
            ("atan2f", 32, 2),
            ("atan2", 64, 2),
        ] {
            let parameters = (0..arity)
                .map(|i| format!("x{i}:f{bits}"))
                .collect::<Vec<_>>()
                .join(",");
            let good = format!("extern \"C\" fn {symbol}({parameters})->f{bits}");
            qualify(&good, &[], true)?;
            for wrong in [
                good.replace("x0:f", "x0:i"),
                good.replace("->f", "->i"),
                format!("extern \"C\" fn {symbol}()->f{bits}"),
            ] {
                qualify(&wrong, &[], false)?;
            }
            if arity == 2 {
                qualify(
                    &good.replace(
                        &format!("x1:f{bits}"),
                        &format!("x1:f{}", if bits == 32 { 64 } else { 32 }),
                    ),
                    &[],
                    false,
                )?;
            }
            let definition = format!("pub fn {symbol}({parameters})->f{bits}=x0");
            qualify(&definition, &[], true)?;
            qualify(&definition, &[symbol.to_owned()], false)?;
        }
        let darwin = triple.starts_with("arm64-apple");
        for (symbol, bits) in [("sincosf", 32), ("sincos", 64)] {
            qualify(
                &format!("extern \"C\" fn {symbol}(x:f{bits},a:raw,b:raw)"),
                &[],
                !darwin,
            )?;
            for signature in [
                format!("(x:i64,a:raw,b:raw)"),
                format!("(x:f{bits},a:raw)"),
                format!("(x:f{bits},a:raw,b:raw)->f{bits}"),
            ] {
                qualify(&format!("extern \"C\" fn {symbol}{signature}"), &[], false)?;
            }
        }
        for (symbol, bits) in [("__sincosf_stret", 32), ("__sincos_stret", 64)] {
            for fields in [
                format!("a:f{bits},b:f{bits}"),
                format!("values:[f{bits};2]"),
            ] {
                qualify(
                    &format!(
                        "layout(C) Pair {{{fields}}}\nextern \"C\" fn {symbol}(x:f{bits})->Pair"
                    ),
                    &[],
                    darwin,
                )?;
            }
            for (attributes, fields) in [
                ("", format!("a:f{bits}")),
                ("", format!("a:f{bits},b:f{bits},c:f{bits}")),
                ("", format!("a:f{bits},b:i64")),
                ("align(32) ", format!("a:f{bits},b:f{bits}")),
            ] {
                qualify(
                    &format!(
                        "{attributes}layout(C) Pair {{{fields}}}\nextern \"C\" fn {symbol}(x:f{bits})->Pair"
                    ),
                    &[],
                    false,
                )?;
            }
            qualify(
                &format!(
                    "layout(C) Pair {{a:f{bits},b:f{bits}}}\nextern \"C\" fn {symbol}(x:i64)->Pair"
                ),
                &[],
                false,
            )?;
        }
        for symbol in ["sincosf", "sincos", "__sincosf_stret", "__sincos_stret"] {
            let definition = format!("pub fn {symbol}()->i64=7");
            qualify(&definition, &[], true)?;
            qualify(&definition, &[symbol.to_owned()], false)?;
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
fn trigonometry_intrinsics_and_native_identities() -> Result<(), String> {
    verify(None)
}

#[test]
#[ignore = "explicit cross-target objects for local native trig qualification"]
fn export_trigonometry_objects() -> Result<(), String> {
    let path = std::path::PathBuf::from(
        std::env::var_os("ALIGN_TRIG_ABI_ARTIFACTS")
            .ok_or("set ALIGN_TRIG_ABI_ARTIFACTS to a fresh path")?,
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

#[test]
fn trigonometry_visibility_across_profiles() -> Result<(), CodegenError> {
    let program = tests::mir(
        r#"
pub fn mixed(v:vec4<f32>, w:vec4<f32>, x:f32)->vec4<f32> {
    a:=v.sin()
    b:=v.cos()
    c:=v.atan2(w)
    scalar:=x.sin()
    old:=v.exp()
    fallback:=v.expm1()
    dead:=v.atan2(v)
    return a+b+c+old+fallback+scalar
}
fn unused(v:vec4<f32>) -> vec4<f32> = v.cos()
fn main()->i32=0
"#,
    );
    let debug = DebugInfo {
        file: "trig.align".into(),
        directory: ".".into(),
    };
    let inventory = math_visibility_inventory(&program);
    assert_eq!(
        inventory.iter().map(|r| r.operation).collect::<Vec<_>>(),
        vec![
            hir::MathFn::Sin,
            hir::MathFn::Cos,
            hir::MathFn::Atan2,
            hir::MathFn::Exp,
            hir::MathFn::Expm1,
            hir::MathFn::Atan2,
            hir::MathFn::Cos
        ]
    );
    assert_eq!(
        inventory
            .iter()
            .map(|r| r.operation_ordinal)
            .collect::<Vec<_>>(),
        vec![1, 2, 3, 4, 5, 6, 1]
    );
    for profile in [
        Profile::Dev,
        Profile::Release,
        Profile::Fast,
        Profile::Small,
        Profile::Tiny,
    ] {
        let before = emit_llvm_ir(
            &program,
            &BuildTarget::Baseline,
            profile,
            true,
            &["mixed".into()],
            None,
        )?;
        let inspected = collect_opt_inspection(
            &program,
            &BuildTarget::Baseline,
            profile,
            &debug,
            &["mixed".into()],
        )?;
        assert_eq!(inspected.math.len(), inventory.len());
        for (index, (row, original)) in inspected.math.iter().zip(&inventory).enumerate() {
            assert_eq!(
                (
                    &row.function,
                    row.operation_ordinal,
                    row.operation,
                    row.ty,
                    row.source
                ),
                (
                    &original.function,
                    original.operation_ordinal,
                    original.operation,
                    original.ty,
                    original.source
                )
            );
            assert_eq!(row.provider, MathProvider::None);
            if index < 5 {
                assert_ne!(
                    row.state,
                    MathVisibilityState::EliminatedOrMerged,
                    "{profile:?}/{row:?}"
                );
            }
            if row.operation == hir::MathFn::Expm1 {
                assert_eq!(row.state, MathVisibilityState::Scalarized);
                assert_eq!(row.llvm_reason.as_deref(), Some(math_libm::LOWERING_REASON));
            } else {
                assert_ne!(row.llvm_reason.as_deref(), Some(math_libm::LOWERING_REASON));
            }
            if row.state == MathVisibilityState::EliminatedOrMerged {
                assert_eq!(row.llvm_reason, None);
            }
        }
        let after = emit_llvm_ir(
            &program,
            &BuildTarget::Baseline,
            profile,
            true,
            &["mixed".into()],
            None,
        )?;
        assert_eq!(before, after);
        assert!(!after.contains("align.math.visibility"));
        for line in after.lines().filter(|line| {
            line.contains("call")
                && ["@llvm.sin.", "@llvm.cos.", "@llvm.atan2."]
                    .iter()
                    .any(|op| line.contains(op))
        }) {
            for flag in [
                " fast ",
                " afn ",
                " nnan ",
                " ninf ",
                " nsz ",
                " reassoc ",
                " arcp ",
                " contract ",
            ] {
                assert!(!line.contains(flag), "{line}");
            }
        }
    }
    Ok(())
}
