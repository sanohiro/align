//! Instrument-PGO S0 spike harness (feature `pgo-spike`, ignored tests).
//!
//! A FEASIBILITY PROTOTYPE, not driver wiring. It exercises the one new C++ shim
//! entry (`align_pgo_run_pipeline` in `cpp/thinlto_shim.cpp`, guarded by
//! `-DALIGN_PGO_SPIKE`) and answers the instrument-PGO S0 go/no-go gate:
//!
//!   1. End-to-end on a REAL Align binary: compile a branch-heavy Align program
//!      through the actual `alignc` frontend + `build_module` emit path, but run
//!      the shim GEN pipeline in place of the stock opt run, emit the object,
//!      link it with the profile runtime archive appended (under Align's real
//!      link hygiene — `--gc-sections` / `--as-needed`), run it with
//!      `LLVM_PROFILE_FILE` set, and assert a `.profraw` is written at normal
//!      exit. THE riskiest unknown: Align's M13 internalization + gc-sections
//!      link hygiene must not strip the counters or the runtime anchor.
//!      (`gen_program_writes_profraw_end_to_end`).
//!   2. Merge (`llvm-profdata-22 merge`) → rebuild the SAME program via the shim
//!      USE pipeline → assert `!prof` `branch_weights` metadata appears AND the
//!      in-process LLVM diagnostic handler saw no hash-mismatch / unprofiled
//!      degradation (`use_pipeline_applies_branch_weights_no_mismatch`).
//!   3. IR-shape: GEN module has `__profc_`/`__profd_`/`__llvm_prf` globals pinned
//!      by `llvm.used`; a no-PGO control has neither those nor `branch_weights`
//!      (`gen_ir_shape_has_counters`, `control_ir_shape_has_neither`).
//!   4. Timing smoke (`timing_smoke`).
//!
//! Run with:
//!   cargo test -p align_codegen_llvm --features pgo-spike -- --ignored --nocapture

#![allow(clippy::missing_safety_doc)]

use std::ffi::{c_char, c_int};
use std::path::Path;

use llvm_sys::prelude::LLVMModuleRef;
use llvm_sys::target_machine::LLVMTargetMachineRef;

// ---- shim FFI ------------------------------------------------------------

unsafe extern "C" {
    /// Run the per-module default pipeline with a populated `PGOOptions` in place
    /// on `module` (no object emission — the caller emits). `kind`: 0 = IRInstr
    /// (gen), 1 = IRUse (use). `profdata_path` is required for USE, may be null
    /// for GEN. Returns 0 on success.
    fn align_pgo_run_pipeline(
        module: LLVMModuleRef,
        tm: LLVMTargetMachineRef,
        opt_level: c_int,
        kind: c_int,
        profdata_path: *const c_char,
    ) -> c_int;
}

/// PGO pipeline action.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum PgoKind {
    /// Instrumentation generation (IRInstr) — insert counters + runtime anchor.
    Gen,
    /// Profile use (IRUse) — read a `.profdata` and attach `branch_weights`.
    Use,
}

impl PgoKind {
    fn as_c(self) -> c_int {
        match self {
            PgoKind::Gen => 0,
            PgoKind::Use => 1,
        }
    }
}

/// Run the shim's PGO pipeline on a raw `LLVMModuleRef` + `LLVMTargetMachineRef`.
///
/// # Safety
/// `module` / `tm` must be live handles in the process LLVM; the module must have
/// a datalayout (`build_module` sets it). `profdata` must exist for [`PgoKind::Use`].
pub unsafe fn run_pgo_pipeline(
    module: LLVMModuleRef,
    tm: LLVMTargetMachineRef,
    opt_level: i32,
    kind: PgoKind,
    profdata: Option<&Path>,
) -> i32 {
    let cpath = profdata.map(|p| std::ffi::CString::new(p.to_str().unwrap()).unwrap());
    let ptr = cpath.as_ref().map_or(std::ptr::null(), |c| c.as_ptr());
    unsafe { align_pgo_run_pipeline(module, tm, opt_level, kind.as_c(), ptr) }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        apply_size_attrs, build_module, create_target_machine, BuildTarget, Profile,
    };
    use align_diag::Diagnostics;
    use align_lexer::tokenize;
    use align_mir::{lower_program, Program};
    use align_parser::parse_file;
    use align_sema::check_file;
    use inkwell::context::Context;
    use inkwell::targets::FileType;
    use std::ffi::{c_void, CStr};
    use std::path::PathBuf;
    use std::process::Command;
    use std::time::Instant;

    /// A branch-heavy Align program: a hot `classify` with a biased branch mix,
    /// driven 50k times from a `loop`. The bias is what PGO records and re-applies
    /// as `branch_weights`; the volume guarantees non-empty counters.
    const BRANCHY: &str = "\
fn classify(n: i64) -> i64 {\n\
\x20 if n % 2 == 0 {\n\
\x20   if n % 5 == 0 { return 30 }\n\
\x20   return 10\n\
\x20 }\n\
\x20 if n % 3 == 0 { return 3 }\n\
\x20 return 1\n\
}\n\
fn main() -> i32 {\n\
\x20 mut i := 0\n\
\x20 mut acc := 0\n\
\x20 loop {\n\
\x20   if i >= 50000 { break }\n\
\x20   acc = acc + classify(i)\n\
\x20   i = i + 1\n\
\x20 }\n\
\x20 if acc == 0 { return 7 }\n\
\x20 return 0\n\
}\n";

    /// A structurally DIFFERENT program (its `main` has a different CFG than
    /// `BRANCHY`'s), used to prove the diagnostic handler is live: applying
    /// `BRANCHY`'s profile here must raise a `main` hash-mismatch warning.
    const DIFFERENT: &str = "\
fn main() -> i32 {\n\
\x20 mut i := 0\n\
\x20 mut s := 0\n\
\x20 loop {\n\
\x20   if i >= 10 { break }\n\
\x20   s = s + i\n\
\x20   i = i + 1\n\
\x20 }\n\
\x20 if s == 0 { return 1 }\n\
\x20 return 0\n\
}\n";

    fn opt_o2() -> i32 {
        2
    }

    /// The forced-undefined symbol that pulls the profile runtime's atexit writer
    /// out of the archive on ELF (see the e2e link comment). This is the exact flag
    /// clang's driver injects for `-fprofile-generate`; an S1 driver must add it to
    /// the link line when building an instrumented binary.
    const FORCE_PROFILE_RUNTIME: &str = "-Wl,--undefined=__llvm_profile_runtime";

    fn scratch() -> PathBuf {
        let d =
            std::env::temp_dir().join(format!("align_pgo_spike_{}", std::process::id()));
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    fn have_tool(t: &str) -> bool {
        Command::new(t)
            .arg("--version")
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .status()
            .map(|s| s.success())
            .unwrap_or(false)
    }

    /// Compile Align source through the real frontend to MIR.
    fn to_mir(src: &str) -> Program {
        let mut d = Diagnostics::new();
        let toks = tokenize(0, src, &mut d);
        let f = parse_file(toks, &mut d);
        let hir = check_file(&f, &mut d);
        assert!(!d.has_errors(), "frontend diagnostics present");
        lower_program(&hir)
    }

    /// The `libalign_runtime.a` staticlib, located relative to the test binary
    /// (same discovery as the driver's `runtime_archive`). Both live in
    /// `target/<profile>/deps` (or one level up).
    fn runtime_archive() -> PathBuf {
        let exe = std::env::current_exe().unwrap();
        let dir = exe.parent().unwrap();
        for cand in [
            dir.join("libalign_runtime.a"),
            dir.join("../libalign_runtime.a"),
        ] {
            if cand.exists() {
                return cand;
            }
        }
        panic!(
            "cannot find libalign_runtime.a near {} (run `cargo build` first)",
            dir.display()
        );
    }

    /// The clang profile runtime archive (`libclang_rt.profile-*.a`) that defines
    /// the `__llvm_profile_runtime` anchor + the atexit `.profraw` writer.
    fn profile_runtime_archive() -> Option<PathBuf> {
        let out = Command::new("clang-22")
            .arg("-print-file-name=libclang_rt.profile-x86_64.a")
            .output()
            .ok()?;
        let p = PathBuf::from(String::from_utf8_lossy(&out.stdout).trim());
        // `-print-file-name` echoes the bare name back when it cannot resolve it.
        if p.is_absolute() && p.exists() {
            Some(p)
        } else {
            None
        }
    }

    // ---- diagnostic capture (in-process, for the USE hash-mismatch check) ----

    extern "C" fn collect_diag(
        info: llvm_sys::prelude::LLVMDiagnosticInfoRef,
        ctx: *mut c_void,
    ) {
        unsafe {
            let desc = llvm_sys::core::LLVMGetDiagInfoDescription(info);
            if !desc.is_null() {
                let s = CStr::from_ptr(desc).to_string_lossy().into_owned();
                llvm_sys::core::LLVMDisposeMessage(desc);
                let sink = &mut *(ctx as *mut Vec<String>);
                sink.push(s);
            }
        }
    }

    /// Install a diagnostic handler on `module`'s context that appends every LLVM
    /// diagnostic description into `sink`.
    /// SAFETY: `sink` must outlive every pipeline run on this context.
    unsafe fn capture_diags(module: LLVMModuleRef, sink: &mut Vec<String>) {
        unsafe {
            let ctx = llvm_sys::core::LLVMGetModuleContext(module);
            llvm_sys::core::LLVMContextSetDiagnosticHandler(
                ctx,
                Some(collect_diag),
                sink as *mut Vec<String> as *mut c_void,
            );
        }
    }

    // ---------------------------------------------------------------------
    // 3 (IR-shape): GEN module has counters + llvm.used pinning.
    // ---------------------------------------------------------------------
    #[test]
    #[ignore = "spike: requires the pgo-spike C++ shim"]
    fn gen_ir_shape_has_counters() {
        let program = to_mir(BRANCHY);
        let ctx = Context::create();
        let module = ctx.create_module("align");
        let tm = create_target_machine(&BuildTarget::Baseline, Profile::Release.codegen_opt_level())
            .unwrap();
        build_module(&ctx, &module, &program, &tm, None, &[], false).unwrap();
        apply_size_attrs(&ctx, &module, Profile::Release);

        let rc = unsafe {
            run_pgo_pipeline(module.as_mut_ptr(), tm.as_mut_ptr(), opt_o2(), PgoKind::Gen, None)
        };
        assert_eq!(rc, 0, "gen pipeline rc={rc}");

        let ir = module.print_to_string().to_string();
        assert!(ir.contains("__profc_"), "no __profc_ counters in gen IR");
        assert!(ir.contains("__profd_"), "no __profd_ data in gen IR");
        assert!(ir.contains("__llvm_prf"), "no __llvm_prf metadata section in gen IR");
        // The counters/data/names are pinned by llvm.used / llvm.compiler.used so
        // --gc-sections retains them (SHF_GNU_RETAIN). Both appear here.
        assert!(ir.contains("@llvm.used"), "names not pinned by @llvm.used in gen IR");
        assert!(
            ir.contains("@llvm.compiler.used"),
            "counter data not pinned by @llvm.compiler.used in gen IR"
        );
        // NOTE (friction): on ELF the module carries NO `__llvm_profile_runtime`
        // hook reference — LLVM omits it and relies on `__start/__stop___llvm_prf_*`
        // section brackets, so the LINK must force the archive member with
        // `-u __llvm_profile_runtime` (exactly what clang's driver does). The
        // absence here is expected and must NOT be asserted as present.
        assert!(
            !ir.contains("__llvm_profile_runtime"),
            "unexpected runtime hook in ELF gen IR — the -u link mechanism assumption changed"
        );
        eprintln!("[gen-ir] counters + data + names present; pinned by llvm.used/compiler.used.");
    }

    // ---------------------------------------------------------------------
    // 3 (control): no-PGO module has NEITHER counters NOR branch_weights.
    // ---------------------------------------------------------------------
    #[test]
    #[ignore = "spike: requires the pgo-spike C++ shim"]
    fn control_ir_shape_has_neither() {
        use inkwell::passes::PassBuilderOptions;
        let program = to_mir(BRANCHY);
        let ctx = Context::create();
        let module = ctx.create_module("align");
        let tm = create_target_machine(&BuildTarget::Baseline, Profile::Release.codegen_opt_level())
            .unwrap();
        build_module(&ctx, &module, &program, &tm, None, &[], false).unwrap();
        apply_size_attrs(&ctx, &module, Profile::Release);

        // The stock `default<O2>` pipeline — NO PGOOptions.
        module
            .run_passes("default<O2>", &tm, PassBuilderOptions::create())
            .unwrap();
        let ir = module.print_to_string().to_string();
        assert!(!ir.contains("__profc_"), "control IR unexpectedly has __profc_ counters");
        assert!(
            !ir.contains("branch_weights"),
            "control IR unexpectedly has branch_weights (no profile was applied)"
        );
        eprintln!("[control] no counters, no branch_weights — clean baseline.");
    }

    // ---------------------------------------------------------------------
    // 1 (end-to-end GEN): real Align binary writes a .profraw at exit.
    // ---------------------------------------------------------------------
    #[test]
    #[ignore = "spike: requires the pgo-spike C++ shim + cc + clang-22 profile rt"]
    fn gen_program_writes_profraw_end_to_end() {
        let dir = scratch();
        let obj = dir.join("branchy.gen.o");
        emit_gen_object(&obj);

        let Some(profrt) = profile_runtime_archive() else {
            eprintln!("[e2e] clang-22 profile runtime archive not found; skipping.");
            return;
        };
        let runtime = runtime_archive();
        let exe = dir.join("branchy_gen");

        // Align's REAL link line: the object, the runtime archive, hygiene flags
        // (--gc-sections / --as-needed), support libs — PLUS the profile runtime
        // archive appended AND `-u __llvm_profile_runtime` (FRICTION: on ELF the
        // instrumented object has no reference to the runtime, so the archive's
        // atexit-registering constructor is pulled only via this forced-undefined
        // symbol — exactly what clang's own driver passes; without it no .profraw
        // is written even though the link succeeds).
        let status = Command::new("cc")
            .arg(&obj)
            .arg(&runtime)
            .arg(&profrt)
            .arg("-o")
            .arg(&exe)
            .args(["-Wl,--gc-sections", "-Wl,--as-needed"])
            .arg(FORCE_PROFILE_RUNTIME)
            .args(["-lpthread", "-ldl", "-lm"])
            .status()
            .expect("run cc");
        assert!(status.success(), "link failed");

        let profraw = dir.join("branchy.profraw");
        let _ = std::fs::remove_file(&profraw);
        let run = Command::new(&exe)
            .env("LLVM_PROFILE_FILE", &profraw)
            .status()
            .expect("run instrumented binary");
        // A clean exit (not a signal) is what fires the atexit writer.
        assert!(
            run.code().is_some(),
            "instrumented program crashed (signal), did not exit cleanly: {run:?}"
        );
        assert!(
            profraw.exists() && std::fs::metadata(&profraw).unwrap().len() > 0,
            "no non-empty .profraw written to {} — counters or runtime were stripped",
            profraw.display()
        );
        eprintln!(
            "[e2e] instrumented Align binary wrote {} ({} bytes) at exit — counters SURVIVED \
             internalization + gc-sections.",
            profraw.display(),
            std::fs::metadata(&profraw).unwrap().len()
        );
    }

    // ---------------------------------------------------------------------
    // 1+2 (full round trip): gen → run → merge → use → branch_weights, no mismatch.
    // ---------------------------------------------------------------------
    #[test]
    #[ignore = "spike: requires the pgo-spike C++ shim + cc + clang-22 + llvm-profdata-22"]
    fn use_pipeline_applies_branch_weights_no_mismatch() {
        if !have_tool("llvm-profdata-22") {
            eprintln!("[use] llvm-profdata-22 not found; skipping.");
            return;
        }
        let Some(profrt) = profile_runtime_archive() else {
            eprintln!("[use] clang-22 profile runtime archive not found; skipping.");
            return;
        };
        let dir = scratch();

        // --- gen + run to produce a profile ---
        let obj = dir.join("branchy.rt.o");
        emit_gen_object(&obj);
        let runtime = runtime_archive();
        let exe = dir.join("branchy_rt");
        let ok = Command::new("cc")
            .arg(&obj)
            .arg(&runtime)
            .arg(&profrt)
            .arg("-o")
            .arg(&exe)
            .args(["-Wl,--gc-sections", "-Wl,--as-needed"])
            .arg(FORCE_PROFILE_RUNTIME)
            .args(["-lpthread", "-ldl", "-lm"])
            .status()
            .expect("cc")
            .success();
        assert!(ok, "link failed");
        let profraw = dir.join("branchy.rt.profraw");
        let _ = std::fs::remove_file(&profraw);
        assert!(
            Command::new(&exe)
                .env("LLVM_PROFILE_FILE", &profraw)
                .status()
                .expect("run")
                .code()
                .is_some(),
            "instrumented program crashed"
        );
        assert!(profraw.exists() && std::fs::metadata(&profraw).unwrap().len() > 0, "no profraw produced");

        // --- merge ---
        let profdata = dir.join("branchy.profdata");
        let merged = Command::new("llvm-profdata-22")
            .args(["merge", "-o"])
            .arg(&profdata)
            .arg(&profraw)
            .status()
            .expect("llvm-profdata merge")
            .success();
        assert!(merged, "llvm-profdata merge failed");

        // --- rebuild the SAME program via the USE pipeline ---
        let program = to_mir(BRANCHY);
        let ctx = Context::create();
        let module = ctx.create_module("align");
        let tm = create_target_machine(&BuildTarget::Baseline, Profile::Release.codegen_opt_level())
            .unwrap();
        build_module(&ctx, &module, &program, &tm, None, &[], false).unwrap();
        apply_size_attrs(&ctx, &module, Profile::Release);

        // Capture LLVM diagnostics (hash-mismatch / unprofiled warnings are emitted
        // as PGO DiagnosticInfo, which this handler intercepts in-process).
        let mut diags: Vec<String> = Vec::new();
        unsafe { capture_diags(module.as_mut_ptr(), &mut diags) };

        let rc = unsafe {
            run_pgo_pipeline(
                module.as_mut_ptr(),
                tm.as_mut_ptr(),
                opt_o2(),
                PgoKind::Use,
                Some(&profdata),
            )
        };
        assert_eq!(rc, 0, "use pipeline rc={rc}");

        let ir = module.print_to_string().to_string();
        assert!(
            ir.contains("branch_weights"),
            "no !prof branch_weights in use IR — the profile was not consumed"
        );

        // No hash-mismatch / unprofiled degradation for the hot code.
        let bad: Vec<&String> = diags
            .iter()
            .filter(|m| {
                let l = m.to_ascii_lowercase();
                l.contains("mismatch") || l.contains("unprofiled") || l.contains("out of date")
            })
            .collect();
        assert!(bad.is_empty(), "PGO degradation diagnostics: {bad:?}");

        // Prove the diagnostic handler is LIVE (so "no mismatch above" is meaningful,
        // not a dead sink): apply the SAME profile to a structurally-different module
        // whose `main` has a different CFG hash — LLVM must warn "hash mismatch", and
        // our in-process handler must capture it.
        let other = to_mir(DIFFERENT);
        let octx = Context::create();
        let omod = octx.create_module("align");
        let otm = create_target_machine(&BuildTarget::Baseline, Profile::Release.codegen_opt_level())
            .unwrap();
        build_module(&octx, &omod, &other, &otm, None, &[], false).unwrap();
        let mut odiags: Vec<String> = Vec::new();
        unsafe { capture_diags(omod.as_mut_ptr(), &mut odiags) };
        let orc = unsafe {
            run_pgo_pipeline(omod.as_mut_ptr(), otm.as_mut_ptr(), opt_o2(), PgoKind::Use, Some(&profdata))
        };
        assert_eq!(orc, 0, "use pipeline (mismatch module) rc={orc}");
        let saw_mismatch = odiags.iter().any(|m| m.to_ascii_lowercase().contains("mismatch"));
        assert!(
            saw_mismatch,
            "diagnostic handler did not capture the expected hash mismatch (handler may be dead): {odiags:?}"
        );

        eprintln!(
            "[use] branch_weights applied; {} diagnostic(s) on the matching module (none a mismatch); \
             {} on the mismatched control (mismatch captured => handler is live).",
            diags.len(),
            odiags.len()
        );
    }

    // ---------------------------------------------------------------------
    // 4: timing smoke (gen build / use build overhead vs plain, at spike scale).
    // ---------------------------------------------------------------------
    #[test]
    #[ignore = "spike: requires the pgo-spike C++ shim + llvm-profdata-22"]
    fn timing_smoke() {
        use inkwell::passes::PassBuilderOptions;
        let program = to_mir(BRANCHY);

        // plain O2
        let t = Instant::now();
        {
            let ctx = Context::create();
            let m = ctx.create_module("align");
            let tm = create_target_machine(&BuildTarget::Baseline, Profile::Release.codegen_opt_level())
                .unwrap();
            build_module(&ctx, &m, &program, &tm, None, &[], false).unwrap();
            m.run_passes("default<O2>", &tm, PassBuilderOptions::create()).unwrap();
        }
        let t_plain = t.elapsed();

        // gen
        let t = Instant::now();
        {
            let ctx = Context::create();
            let m = ctx.create_module("align");
            let tm = create_target_machine(&BuildTarget::Baseline, Profile::Release.codegen_opt_level())
                .unwrap();
            build_module(&ctx, &m, &program, &tm, None, &[], false).unwrap();
            unsafe { run_pgo_pipeline(m.as_mut_ptr(), tm.as_mut_ptr(), opt_o2(), PgoKind::Gen, None) };
        }
        let t_gen = t.elapsed();

        eprintln!("[timing] plain O2={t_plain:?}  gen O2={t_gen:?}  (spike scale, one module)");
    }

    // ---- shared: emit a GEN object from the real emit path ----

    /// Build the branch-heavy program's module exactly as `emit_object` does, run
    /// the shim GEN pipeline in place of the stock opt run, and emit the object.
    fn emit_gen_object(obj: &Path) {
        let program = to_mir(BRANCHY);
        let ctx = Context::create();
        let module = ctx.create_module("align");
        let tm = create_target_machine(&BuildTarget::Baseline, Profile::Release.codegen_opt_level())
            .unwrap();
        build_module(&ctx, &module, &program, &tm, None, &[], false).unwrap();
        apply_size_attrs(&ctx, &module, Profile::Release);
        let rc = unsafe {
            run_pgo_pipeline(module.as_mut_ptr(), tm.as_mut_ptr(), opt_o2(), PgoKind::Gen, None)
        };
        assert_eq!(rc, 0, "gen pipeline rc={rc}");
        tm.write_to_file(&module, FileType::Object, obj)
            .expect("emit object");
    }
}
