//! Expose target-selected indirect results before LLVM optimization. The shim
//! rewrites the complete verified module at once, after LLVM handles used by
//! ordinary lowering have finished their work. No semantic MIR ABI is changed.

use std::ffi::{CStr, c_char, c_int};

use inkwell::attributes::AttributeLoc;
use inkwell::builder::{Builder, BuilderError};
use inkwell::context::Context;
use inkwell::module::Module;
use inkwell::targets::TargetMachine;
use inkwell::types::FunctionType;
use inkwell::values::{
    AsValueRef, BasicMetadataValueEnum, CallSiteValue, FunctionValue, PointerValue,
};
use llvm_sys::prelude::{LLVMModuleRef, LLVMValueRef};
use llvm_sys::target_machine::LLVMTargetMachineRef;

use crate::CodegenError;

unsafe extern "C" {
    fn align_normalize_return_transport(
        module: LLVMModuleRef,
        target: LLVMTargetMachineRef,
        owned_functions: *const LLVMValueRef,
        count: usize,
        error: *mut *mut c_char,
    ) -> c_int;
}

/// Only checked program-function and closure/task calls use this helper.
/// Raw descriptor, runtime and foreign ABI calls do not acquire this marker.
pub(super) fn build_indirect_call<'ctx>(
    ctx: &'ctx Context,
    builder: &Builder<'ctx>,
    signature: FunctionType<'ctx>,
    callee: PointerValue<'ctx>,
    args: &[BasicMetadataValueEnum<'ctx>],
    name: &str,
) -> Result<CallSiteValue<'ctx>, BuilderError> {
    let call = builder.build_indirect_call(signature, callee, args, name)?;
    call.add_attribute(
        AttributeLoc::Function,
        ctx.create_string_attribute("align.program.return", ""),
    );
    Ok(call)
}

/// Checked native descriptor calls share the mechanical ABI normalization, but
/// receive no source-owned capture or shortened-storage-lifetime contract.
pub(super) fn build_native_indirect_call<'ctx>(
    ctx: &'ctx Context,
    builder: &Builder<'ctx>,
    signature: FunctionType<'ctx>,
    callee: PointerValue<'ctx>,
    args: &[BasicMetadataValueEnum<'ctx>],
    name: &str,
) -> Result<CallSiteValue<'ctx>, BuilderError> {
    let call = builder.build_indirect_call(signature, callee, args, name)?;
    call.add_attribute(
        AttributeLoc::Function,
        ctx.create_string_attribute("align.native.return", ""),
    );
    Ok(call)
}

/// All supplied handles are live members of the same module. Call only after
/// ordinary lowering has stopped using its FunctionValue/argument handles.
pub(super) fn normalize(
    module: &Module<'_>,
    target: &TargetMachine,
    owned_functions: &[FunctionValue<'_>],
) -> Result<(), CodegenError> {
    let owned: Vec<_> = owned_functions
        .iter()
        .map(AsValueRef::as_value_ref)
        .collect();
    let mut error = std::ptr::null_mut();
    // SAFETY: LLVM owns these call-scoped handles in its one matched library.
    // The shim keeps no handle, never publishes a partial module, and allocates
    // only an optional LLVM error message for us to free below.
    let status = unsafe {
        align_normalize_return_transport(
            module.as_mut_ptr(),
            target.as_mut_ptr(),
            owned.as_ptr(),
            owned.len(),
            &mut error,
        )
    };
    let message = if error.is_null() {
        None
    } else {
        // SAFETY: the shim returned an LLVMCreateMessage allocation, terminated
        // by NUL. Copy it before releasing it with the matching LLVM allocator.
        let message = unsafe { CStr::from_ptr(error) }
            .to_string_lossy()
            .into_owned();
        unsafe { llvm_sys::core::LLVMDisposeMessage(error) };
        Some(message)
    };
    if status == 0 {
        Ok(())
    } else {
        let message = message
            .as_deref()
            .unwrap_or("return transport refused without a diagnostic");
        Err(CodegenError::Lowering(format!(
            "return transport: {message}"
        )))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use inkwell::memory_buffer::MemoryBuffer;
    use inkwell::values::InstructionOpcode;

    fn parse<'ctx>(ctx: &'ctx Context, source: &str) -> Result<Module<'ctx>, String> {
        ctx.create_module_from_ir(MemoryBuffer::create_from_memory_range_copy(
            std::ffi::CString::new(source)
                .map_err(|error| error.to_string())?
                .as_bytes_with_nul(),
            "return-owner",
        ))
        .map_err(|error| error.to_string())
    }

    fn target() -> Result<TargetMachine, String> {
        crate::create_target_machine(
            &crate::BuildTarget::Baseline,
            inkwell::OptimizationLevel::Default,
        )
        .map_err(|error| error.to_string())
    }

    #[test]
    fn materialized_results_forward_without_changing_call_count() -> Result<(), String> {
        let ctx = Context::create();
        let module = parse(
            &ctx,
            r#"
%Big = type { [27 x i64] }
declare %Big @make(i64)
declare i64 @consume(ptr)
define i64 @probe(i64 %n) {
entry:
  %first = alloca %Big, align 8
  %second = alloca %Big, align 8
  %a = call %Big @make(i64 %n)
  store %Big %a, ptr %first, align 8
  %b = call %Big @make(i64 %n)
  store %Big %b, ptr %second, align 8
  %x = call i64 @consume(ptr %first)
  %y = call i64 @consume(ptr %second)
  %sum = add i64 %x, %y
  ret i64 %sum
}
"#,
        )?;
        let tm = target()?;
        module.set_data_layout(&tm.get_target_data().get_data_layout());
        module.set_triple(&tm.get_triple());
        let owned = [module.get_function("make").ok_or("make")?];
        normalize(&module, &tm, &owned).map_err(|error| error.to_string())?;
        let raw = module.print_to_string().to_string();
        assert!(
            raw.contains("llvm.memcpy"),
            "the full transfer must remain visible: {raw}"
        );
        assert_eq!(raw.matches("call void @make").count(), 2);
        crate::run_opt_pipeline(&module, &tm, "default<O2>").map_err(|error| error.to_string())?;
        let optimized = module.print_to_string().to_string();
        assert_eq!(optimized.matches("alloca %Big").count(), 2, "{optimized}");
        assert_eq!(
            optimized.matches("call void @make").count(),
            2,
            "{optimized}"
        );
        assert!(!optimized.contains("call.result.storage"), "{optimized}");
        assert!(!optimized.contains("llvm.memcpy"), "{optimized}");
        Ok(())
    }

    #[test]
    fn result_transport_preserves_parameter_contracts_and_indirect_edges() -> Result<(), String> {
        let ctx = Context::create();
        let module = parse(
            &ctx,
            r#"
%Big = type { [27 x i64] }
declare %Big @make(ptr nonnull readonly captures(none))
define %Big @probe(ptr %input, ptr %callee) {
entry:
  %a = call %Big @make(ptr nonnull readonly %input)
  %b = call %Big %callee(ptr %input) #0
  ret %Big %a
}
attributes #0 = { "align.program.return" }
"#,
        )?;
        let tm = target()?;
        module.set_data_layout(&tm.get_target_data().get_data_layout());
        module.set_triple(&tm.get_triple());
        let owned: Vec<_> = module.get_functions().collect();
        normalize(&module, &tm, &owned).map_err(|error| error.to_string())?;
        let make = module.get_function("make").ok_or("rewritten make")?;
        assert!(make.get_type().get_return_type().is_none());
        for name in ["nonnull", "readonly"] {
            let id = inkwell::attributes::Attribute::get_named_enum_kind_id(name);
            assert!(
                make.get_enum_attribute(AttributeLoc::Param(1), id)
                    .is_some(),
                "{name}"
            );
        }
        let raw = module.print_to_string().to_string();
        assert!(raw.contains("call void %callee(ptr sret(%Big)"), "{raw}");
        assert!(
            !raw.contains("align.program.return"),
            "markers must not persist"
        );
        let rewritten: Vec<_> = module
            .get_functions()
            .filter(|f| !f.get_name().to_bytes().starts_with(b"llvm."))
            .collect();
        normalize(&module, &tm, &rewritten).map_err(|error| error.to_string())?;
        assert_eq!(module.print_to_string().to_string(), raw);
        Ok(())
    }

    #[test]
    fn observable_aliases_and_native_calls_keep_their_contracts() -> Result<(), String> {
        for owned_make in [false, true] {
            let ctx = Context::create();
            let module = parse(
                &ctx,
                r#"
%Big = type { [27 x i64] }
declare %Big @make(ptr)
declare i64 @consume(ptr)
define i64 @probe() {
entry:
  %dst = alloca %Big, align 8
  store %Big zeroinitializer, ptr %dst, align 8
  %value = call %Big @make(ptr %dst)
  store %Big %value, ptr %dst, align 8
  %r = call i64 @consume(ptr %dst)
  ret i64 %r
}
"#,
            )?;
            let tm = target()?;
            module.set_data_layout(&tm.get_target_data().get_data_layout());
            module.set_triple(&tm.get_triple());
            let owned = if owned_make {
                vec![module.get_function("make").ok_or("make")?]
            } else {
                vec![]
            };
            normalize(&module, &tm, &owned).map_err(|error| error.to_string())?;
            crate::run_opt_pipeline(&module, &tm, "default<O2>")
                .map_err(|error| error.to_string())?;
            let text = module.print_to_string().to_string();
            if owned_make {
                assert_eq!(
                    text.matches("alloca %Big").count(),
                    2,
                    "old destination is observable: {text}"
                );
            } else {
                assert!(
                    text.contains("call %Big @make"),
                    "native ABI must be untouched: {text}"
                );
                assert!(!text.contains("sret(%Big)"), "{text}");
            }
        }
        Ok(())
    }

    #[test]
    fn loop_result_storage_is_entry_allocated_on_the_fallback_path() -> Result<(), String> {
        let ctx = Context::create();
        let module = parse(
            &ctx,
            r#"
%Big = type { [27 x i64] }
declare %Big @make(i64)
define i64 @probe(i64 %n) {
entry:
  br label %loop
loop:
  %i = phi i64 [ 0, %entry ], [ %next, %loop ]
  %value = call %Big @make(i64 %i)
  %first = extractvalue %Big %value, 0, 0
  %last = extractvalue %Big %value, 0, 26
  %next = add i64 %i, 1
  %more = icmp ult i64 %next, %n
  br i1 %more, label %loop, label %exit
exit:
  %sum = add i64 %first, %last
  ret i64 %sum
}
"#,
        )?;
        let tm = target()?;
        module.set_data_layout(&tm.get_target_data().get_data_layout());
        module.set_triple(&tm.get_triple());
        normalize(&module, &tm, &[module.get_function("make").ok_or("make")?])
            .map_err(|error| error.to_string())?;
        let probe = module.get_function("probe").ok_or("probe")?;
        let entry = probe.get_first_basic_block().ok_or("entry")?;
        let mut allocations = 0;
        for block in probe.get_basic_blocks() {
            for instruction in block.get_instructions() {
                if instruction.get_opcode() == InstructionOpcode::Alloca {
                    allocations += 1;
                    assert_eq!(block, entry, "loop execution must not grow the frame");
                }
            }
        }
        assert_eq!(allocations, 1);
        let text = module.print_to_string().to_string();
        assert!(text.contains("llvm.lifetime.start"));
        assert!(text.contains("llvm.lifetime.end"));
        Ok(())
    }

    #[test]
    fn unsupported_call_edges_refuse_before_module_mutation() -> Result<(), String> {
        let ctx = Context::create();
        let module = parse(
            &ctx,
            r#"
%Big = type { [27 x i64] }
declare %Big @make(i64)
define %Big @forward(i64 %n) {
entry:
  %r = musttail call %Big @make(i64 %n)
  ret %Big %r
}
"#,
        )?;
        let tm = target()?;
        module.set_data_layout(&tm.get_target_data().get_data_layout());
        module.set_triple(&tm.get_triple());
        let before = module.print_to_string().to_string();
        let owned: Vec<_> = module.get_functions().collect();
        let error =
            normalize(&module, &tm, &owned).expect_err("musttail cannot point into this frame");
        assert!(
            error
                .to_string()
                .contains("unsupported indirect return call edge"),
            "{error}"
        );
        assert_eq!(module.print_to_string().to_string(), before);
        let other = parse(&ctx, "declare i64 @other()\n")?;
        let wrong_owner = other.get_function("other").ok_or("other")?;
        assert!(normalize(&module, &tm, &[wrong_owner]).is_err());
        assert_eq!(module.print_to_string().to_string(), before);
        Ok(())
    }

    #[test]
    fn target_mismatch_and_nonlocal_materialization_do_not_guess() -> Result<(), String> {
        let ctx = Context::create();
        let tm = target()?;
        let module = parse(&ctx, "declare { [27 x i64] } @make()\n")?;
        let before = module.print_to_string().to_string();
        let error = normalize(&module, &tm, &[module.get_function("make").ok_or("make")?])
            .expect_err("missing target context");
        assert!(
            error
                .to_string()
                .contains("module and target return context disagree")
        );
        assert_eq!(module.print_to_string().to_string(), before);
        for store in [
            "store %Big %r, ptr %destination, align 8",
            "store volatile %Big %r, ptr %local, align 8",
            "call void @effect()\n store %Big %r, ptr %local, align 8",
            "%field = getelementptr { i64, %Big }, ptr %outer, i32 0, i32 1\n store %Big %r, ptr %field, align 8",
        ] {
            let ctx = Context::create();
            let source = format!(
                r#"
%Big = type {{ [27 x i64] }}
declare %Big @make()
declare void @effect()
define void @probe(ptr %destination) {{
entry:
 %local = alloca %Big, align 8
 %outer = alloca {{ i64, %Big }}, align 8
 %r = call %Big @make()
 {store}
 ret void
}}
"#
            );
            let module = parse(&ctx, &source)?;
            module.set_data_layout(&tm.get_target_data().get_data_layout());
            module.set_triple(&tm.get_triple());
            normalize(&module, &tm, &[module.get_function("make").ok_or("make")?])
                .map_err(|error| error.to_string())?;
            let text = module.print_to_string().to_string();
            assert!(
                text.contains("load %Big, ptr %call.result.storage"),
                "{text}"
            );
            assert!(
                !text.contains("llvm.memcpy"),
                "not a sole adjacent local materialization: {text}"
            );
        }
        Ok(())
    }

    #[test]
    fn native_descriptor_edges_match_generated_definitions_without_new_effect_facts()
    -> Result<(), String> {
        let ctx = Context::create();
        let tm = target()?;
        let module = parse(
            &ctx,
            r#"
%Big = type { [27 x i64] }
@descriptor = internal constant ptr @make
define internal %Big @make(i64 %n) {
entry:
 %value = insertvalue %Big zeroinitializer, i64 %n, 0, 26
 ret %Big %value
}
define i64 @probe() {
entry:
 %callee = load ptr, ptr @descriptor
 %result = call %Big %callee(i64 93) #0
 %last = extractvalue %Big %result, 0, 26
 ret i64 %last
}
attributes #0 = { "align.native.return" }
"#,
        )?;
        module.set_data_layout(&tm.get_target_data().get_data_layout());
        module.set_triple(&tm.get_triple());
        let owned: Vec<_> = module.get_functions().collect();
        normalize(&module, &tm, &owned).map_err(|error| error.to_string())?;
        let raw = module.print_to_string().to_string();
        let call = raw
            .lines()
            .find(|line| line.contains("call void %callee"))
            .ok_or("native edge")?;
        assert!(call.contains("sret(%Big)"), "{raw}");
        assert!(
            !call.contains("captures("),
            "no native capture assertion: {raw}"
        );
        assert!(
            !raw.contains("llvm.lifetime"),
            "native fallback keeps the implicit storage lifetime: {raw}"
        );
        assert!(!raw.contains("align.native.return"));
        crate::run_opt_pipeline(&module, &tm, "default<O2>").map_err(|error| error.to_string())?;
        let optimized = module.print_to_string().to_string();
        assert!(
            optimized.contains("ret i64 93"),
            "generated target and native caller must agree after devirtualization: {optimized}"
        );
        Ok(())
    }

    /// Link independently emitted implicit and explicit objects in both
    /// directions. This is a native ABI test, including register-return reverse
    /// controls; checking only two rewritten modules could hide ABI drift.
    #[test]
    fn native_implicit_explicit_return_abi_matrix() -> Result<(), String> {
        use inkwell::targets::FileType;
        use std::process::Command;
        let shapes = [
            ("i64", "41", "", "i64", "41"),
            (
                "{ i64, double }",
                "{ i64 41, double 2.0 }",
                ", 0",
                "i64",
                "41",
            ),
            (
                "{ double, double, double }",
                "{ double 1.0, double 2.0, double 3.0 }",
                ", 2",
                "double",
                "3.0",
            ),
            (
                "{ i8, [3 x i64], double }",
                "{ i8 7, [3 x i64] [i64 11, i64 22, i64 33], double 4.0 }",
                ", 1, 2",
                "i64",
                "33",
            ),
            ("{ [27 x i64] }", "big", ", 0, 26", "i64", "93"),
            (
                "{ i64, { i8, [4 x i64] } }",
                "{ i64 1, { i8, [4 x i64] } { i8 2, [4 x i64] [i64 3, i64 4, i64 5, i64 6] } }",
                ", 1, 1, 3",
                "i64",
                "6",
            ),
            (
                "{ <4 x i64>, i64 }",
                "{ <4 x i64> <i64 1, i64 2, i64 3, i64 4>, i64 57 }",
                ", 1",
                "i64",
                "57",
            ),
            (
                "{ {}, i64 }",
                "{ {} zeroinitializer, i64 71 }",
                ", 1",
                "i64",
                "71",
            ),
        ];
        let mut provider = String::new();
        let mut caller = String::new();
        for (i, (ty, value, path, leaf, expected)) in shapes.iter().enumerate() {
            let value = if *value == "big" {
                format!(
                    "{{ [27 x i64] [{}] }}",
                    (0..27).map(|_| "i64 93").collect::<Vec<_>>().join(", ")
                )
            } else {
                (*value).to_owned()
            };
            // The trailing integer/float arguments detect an erroneous shift of
            // physical argument registers when the hidden result is introduced.
            provider.push_str(&format!("define {ty} @make{i}(i64 %a, double %b, i64 %c) {{\nentry:\n %aok = icmp eq i64 %a, 17\n %bok = fcmp oeq double %b, 19.0\n %cok = icmp eq i64 %c, 93\n %ab = and i1 %aok, %bok\n %ok = and i1 %ab, %cok\n %r = select i1 %ok, {ty} {value}, {ty} zeroinitializer\n ret {ty} %r\n}}\n"));
            caller.push_str(&format!("declare {ty} @make{i}(i64, double, i64)\n"));
            caller.push_str(&format!("define i32 @check{i}() {{\nentry:\n %r = call {ty} @make{i}(i64 17, double 19.0, i64 93)\n"));
            let operand = if path.is_empty() {
                "%r"
            } else {
                caller.push_str(&format!(" %v = extractvalue {ty} %r{path}\n"));
                "%v"
            };
            let compare = if *leaf == "double" {
                "fcmp oeq"
            } else {
                "icmp eq"
            };
            caller.push_str(&format!(" %ok = {compare} {leaf} {operand}, {expected}\n %bad = xor i1 %ok, true\n %code = zext i1 %bad to i32\n ret i32 %code\n}}\n"));
        }
        provider.push_str("define {} @empty() { ret {} zeroinitializer }\n");
        caller.push_str(
            "declare {} @empty()\ndefine i32 @main() {\nentry:\n %empty = call {} @empty()\n",
        );
        for i in 0..shapes.len() {
            caller.push_str(&format!(" %c{i} = call i32 @check{i}()\n"));
            if i == 0 {
                caller.push_str(" %sum0 = add i32 %c0, 0\n");
            } else {
                caller.push_str(&format!(" %sum{i} = add i32 %sum{}, %c{i}\n", i - 1));
            }
        }
        caller.push_str(&format!(" ret i32 %sum{}\n}}\n", shapes.len() - 1));
        let nonce = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_err(|error| error.to_string())?
            .as_nanos();
        let dir =
            std::env::temp_dir().join(format!("align-return-abi-{}-{nonce}", std::process::id()));
        std::fs::create_dir(&dir).map_err(|error| error.to_string())?;
        struct Cleanup(std::path::PathBuf);
        impl Drop for Cleanup {
            fn drop(&mut self) {
                let _ = std::fs::remove_dir_all(&self.0);
            }
        }
        let _cleanup = Cleanup(dir.clone());
        for explicit_provider in [false, true] {
            let ctx = Context::create();
            let tm = target()?;
            for (name, source, explicit) in [
                ("provider", &provider, explicit_provider),
                ("caller", &caller, !explicit_provider),
            ] {
                let module = parse(&ctx, source)?;
                module.set_data_layout(&tm.get_target_data().get_data_layout());
                module.set_triple(&tm.get_triple());
                if explicit {
                    let owned: Vec<_> = module.get_functions().collect();
                    normalize(&module, &tm, &owned).map_err(|error| error.to_string())?;
                }
                module.verify().map_err(|error| error.to_string())?;
                tm.write_to_file(&module, FileType::Object, &dir.join(format!("{name}.o")))
                    .map_err(|error| error.to_string())?;
            }
            let exe = dir.join("probe");
            let link = run_bounded(
                Command::new("cc")
                    .arg(dir.join("provider.o"))
                    .arg(dir.join("caller.o"))
                    .arg("-o")
                    .arg(&exe),
            )?;
            assert!(link.success(), "native ABI link: {link}");
            let run = run_bounded(&mut Command::new(&exe))?;
            assert_eq!(
                run.code(),
                Some(0),
                "explicit provider={explicit_provider}: {run:?}"
            );
        }
        Ok(())
    }

    fn run_bounded(
        command: &mut std::process::Command,
    ) -> Result<std::process::ExitStatus, String> {
        use std::time::{Duration, Instant};
        struct Child(std::process::Child);
        impl Drop for Child {
            fn drop(&mut self) {
                let _ = self.0.kill();
                let _ = self.0.wait();
            }
        }
        let mut child = Child(command.spawn().map_err(|error| error.to_string())?);
        let deadline = Instant::now() + Duration::from_secs(30);
        loop {
            match child.0.try_wait() {
                Ok(Some(status)) => return Ok(status),
                Ok(None) => {}
                Err(error) if error.kind() == std::io::ErrorKind::Interrupted => {}
                Err(error) => return Err(format!("ABI child status: {error}")),
            }
            assert!(
                Instant::now() < deadline,
                "ABI owner exceeded its execution deadline"
            );
            std::thread::sleep(Duration::from_millis(10));
        }
    }
}
