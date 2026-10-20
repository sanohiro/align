use super::*;
use inkwell::{
    attributes::{Attribute, AttributeLoc},
    targets::FileType,
    values::CallSiteValue,
};
#[path = "../../align_driver/tests/helpers/math_libm_cases.rs"]
mod cases;

fn verify(artifacts: Option<&std::path::Path>) -> Result<(), String> {
    Target::initialize_x86(&InitializationConfig::default());
    Target::initialize_aarch64(&InitializationConfig::default());
    let (c, source, observations) = cases::sources();
    assert_eq!(observations, 582);
    let source = format!("{source}\nfn main()->i32=exercise()\n");
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
        let module = context.create_module("cancellation_math");
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
        let mut calls = HashMap::new();
        for &(symbol, operation, bits) in &math_libm::ROWS {
            let function = module
                .get_function(symbol)
                .ok_or("missing libm declaration")?;
            assert_eq!(
                function.get_type(),
                math_libm::function_type(&context, symbol).ok_or("fixed ABI")?
            );
            assert_eq!(function.get_call_conventions(), 0);
            assert_eq!(math_libm::symbol(operation, bits), Some(symbol));
            assert!(
                function
                    .get_enum_attribute(
                        AttributeLoc::Function,
                        Attribute::get_named_enum_kind_id("memory")
                    )
                    .is_none()
            );
            calls.insert(symbol.to_owned(), 0);
        }
        for function in module.get_functions() {
            for block in function.get_basic_blocks() {
                for instruction in block.get_instructions() {
                    if let Ok(call) = CallSiteValue::try_from(instruction)
                        && let Some(callee) = call.get_called_fn_value()
                        && let Some(count) =
                            calls.get_mut(callee.get_name().to_string_lossy().as_ref())
                    {
                        *count += 1;
                        assert_eq!(call.count_arguments(), 1);
                        assert_eq!(call.get_call_convention(), 0);
                        assert_eq!(
                            call.try_as_basic_value()
                                .basic()
                                .ok_or("math result")?
                                .get_type(),
                            callee
                                .get_type()
                                .get_return_type()
                                .ok_or("math return type")?
                        );
                    }
                }
            }
        }
        assert_eq!(calls.len(), 4);
        assert!(
            calls.values().all(|count| *count == 31),
            "{triple}: {calls:?}"
        );
        for &(symbol, _, bits) in &math_libm::ROWS {
            for signature in [
                format!("(x:i64)->f{bits}"),
                format!("(x:f{bits})->i64"),
                format!("()->f{bits}"),
            ] {
                let forged = tests::mir(&format!(
                    "extern \"C\" fn {symbol}{signature}\nfn main() {{}}\n"
                ));
                let rejected = context.create_module("bad_math_native");
                let error = build_module(
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
                .ok_or("mismatched native math accepted")?;
                assert!(
                    error.to_string().contains("native extern ABI mismatch"),
                    "{error}"
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
fn scalar_libm_math_abi_and_vector_fallback() -> Result<(), String> {
    verify(None)
}

#[test]
fn scalar_libm_visibility_keeps_ordinals_and_compiler_provenance() -> Result<(), CodegenError> {
    let program = tests::mir(
        r#"
pub fn mixed(v:vec4<f32>, x:f32)->vec4<f32> {
    a := v.expm1()
    scalar := x.log1p()
    b := v.exp()
    c := v.log1p()
    return a + b + c + scalar
}
fn unused(v:vec4<f32>)->vec4<f32> = v.log1p()
fn main()->i32 = 0
"#,
    );
    let debug = DebugInfo {
        file: "math.align".into(),
        directory: ".".into(),
    };
    let inventory = math_visibility_inventory(&program);
    assert_eq!(
        inventory
            .iter()
            .map(|row| row.operation)
            .collect::<Vec<_>>(),
        vec![
            hir::MathFn::Expm1,
            hir::MathFn::Exp,
            hir::MathFn::Log1p,
            hir::MathFn::Log1p
        ]
    );
    assert_eq!(
        inventory
            .iter()
            .map(|row| row.operation_ordinal)
            .collect::<Vec<_>>(),
        vec![1, 2, 3, 1]
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
        let mut result = collect_opt_inspection(
            &program,
            &BuildTarget::Baseline,
            profile,
            &debug,
            &["mixed".into()],
        )?;
        assert_eq!(result.math.len(), inventory.len());
        for (index, (row, original)) in result.math.iter().zip(&inventory).enumerate() {
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
            if row.operation.uses_scalar_libm() {
                assert_ne!(row.state, MathVisibilityState::RetainedVectorIr);
                if index < 3 {
                    assert_eq!(row.state, MathVisibilityState::Scalarized);
                }
                assert_eq!(
                    row.llvm_reason.as_deref(),
                    (row.state == MathVisibilityState::Scalarized)
                        .then_some(math_libm::LOWERING_REASON)
                );
            }
        }
        // Matching optimizer remarks must not overwrite the compiler's lowering provenance.
        let remarks =
            result
                .math
                .iter()
                .filter_map(|row| {
                    row.source.map(|(line,column)|
            format!("math.align:{line}:{column}: Scalarized for an unrelated optimizer reason"))
                })
                .collect::<Vec<_>>();
        attach_math_scalarization_reasons(&mut result.math, &remarks, "math.align");
        assert_eq!(
            result.math[0].llvm_reason.as_deref(),
            Some(math_libm::LOWERING_REASON)
        );
        result.math[0].state = MathVisibilityState::EliminatedOrMerged;
        result.math[0].llvm_reason = None;
        attach_math_scalarization_reasons(&mut result.math, &remarks, "math.align");
        assert_eq!(result.math[0].llvm_reason, None);
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
    }
    Ok(())
}

#[test]
#[ignore = "explicit retained cross-target objects for local native math qualification"]
fn export_scalar_libm_math_objects() -> Result<(), String> {
    let path = std::path::PathBuf::from(
        std::env::var_os("ALIGN_MATH_ABI_ARTIFACTS").ok_or("set a new artifact directory")?,
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
