//! Instrument-PGO driver-facing surface (production).
//!
//! A safe Rust wrapper over the one C++ shim entry the llvm-sys 221 C API cannot
//! reach (`align_pgo_run_pipeline` in `cpp/thinlto_shim.cpp`): constructing a
//! `PassBuilder` with a populated `std::optional<PGOOptions>` and running the
//! per-module default pipeline — exactly clang's `-fprofile-generate` /
//! `-fprofile-use` under the new pass manager. The shim is compiled
//! unconditionally by `build.rs`; this module is always available (no feature gate).
//!
//! CONTRACT (measured at S0, `docs/impl/07-roadmap.md` "Instrument-PGO design
//! SETTLED"): the shim's return code CANNOT signal a missing/corrupt profdata —
//! libLLVM diagnoses it on the `LLVMContext` and, WITHOUT a diagnostic handler
//! installed, exits the process. Therefore:
//!   * the CALLER pre-validates the profdata (existence / readability / non-empty /
//!     magic) BEFORE [`PgoAction::Use`] — this wrapper does NOT re-validate the file;
//!   * for a USE run this wrapper installs a context diagnostic handler around the
//!     pipeline, so a hash mismatch / stale profile is captured as a [`PgoRunReport`]
//!     warning (proceed) and an LLVM-reported error is turned into a typed
//!     [`CodegenError`] (hard fail) — never a silent process exit.

use std::ffi::{c_char, c_int, c_void, CStr, CString};
use std::path::Path;

use llvm_sys::prelude::{LLVMDiagnosticInfoRef, LLVMModuleRef};
use llvm_sys::target_machine::LLVMTargetMachineRef;

use crate::CodegenError;

// ---- shim FFI ------------------------------------------------------------

unsafe extern "C" {
    /// Run the per-module default pipeline with a populated `PGOOptions` in place on
    /// `module` (no object emission — the caller emits). `kind`: 0 = IRInstr (gen),
    /// 1 = IRUse (use). `profdata_path` is required for USE, may be null for GEN.
    /// Returns 0 on success, nonzero on argument errors (see the shim's contract note).
    fn align_pgo_run_pipeline(
        module: LLVMModuleRef,
        tm: LLVMTargetMachineRef,
        opt_level: c_int,
        kind: c_int,
        profdata_path: *const c_char,
        out_matched: *mut c_int,
        out_total: *mut c_int,
    ) -> c_int;
}

/// What the PGO pipeline should do to the module.
#[derive(Clone, Copy, Debug)]
pub enum PgoAction<'a> {
    /// Instrumentation generation (IRInstr): insert `__profc_`/`__profd_` counters +
    /// the `__llvm_profile_runtime` anchor + `llvm.used` pinning.
    Instrument,
    /// Profile use (IRUse): read the merged `.profdata` at `path` and attach `!prof`
    /// `branch_weights`. The caller MUST have validated `path` already.
    Use(&'a Path),
}

impl PgoAction<'_> {
    fn kind(self) -> c_int {
        match self {
            PgoAction::Instrument => 0,
            PgoAction::Use(_) => 1,
        }
    }
}

/// The outcome of a PGO pipeline run. For [`PgoAction::Use`], `warnings` holds every
/// LLVM diagnostic of Warning severity captured during the run (typically hash
/// mismatch / partially-stale profile notes) — the driver aggregates these into one
/// Align-voice report and proceeds. An LLVM diagnostic of Error severity is NOT
/// returned here: it is turned into a [`CodegenError`] instead (hard fail).
/// [`PgoAction::Instrument`] never installs a handler, so its report is always empty.
#[derive(Clone, Debug, Default)]
pub struct PgoRunReport {
    pub warnings: Vec<String>,
    /// The profile-match tally from the shim (see `align_pgo_run_pipeline`'s contract):
    /// `matched` = defined functions this run gave a PGO entry count (found in the profile
    /// with a matching structural hash), `total` = defined functions in the module. For a
    /// [`PgoAction::Use`] run `matched == 0 && total > 0` is the "0%-match" signal — the
    /// profile matched NONE of this module's functions (a likely wrong-program / incompatible
    /// profile) — which the driver surfaces as a prominent WARNING (the tally is approximate
    /// and a mismatched profile is performance-only; never a hard error). For
    /// [`PgoAction::Instrument`] the pass sets no entry counts, so `matched` is 0 and the
    /// value is meaningless (GEN reads no profile).
    pub matched_fns: u32,
    pub total_fns: u32,
}

// ---- in-process diagnostic capture (USE runs) ----------------------------

/// Severity + description of one captured LLVM diagnostic.
struct CapturedDiag {
    severity: c_int,
    message: String,
}

/// The context diagnostic handler sink, pointed at by the raw `*mut c_void` the
/// handler receives. It MUST outlive every pipeline run on the context, and the
/// handler MUST be cleared before it drops (done in [`run_pgo_pipeline`]).
#[derive(Default)]
struct DiagSink {
    diags: Vec<CapturedDiag>,
}

// LLVM `LLVMDiagnosticSeverity`: 0 = Error, 1 = Warning, 2 = Remark, 3 = Note.
const LLVM_DS_ERROR: c_int = 0;
const LLVM_DS_WARNING: c_int = 1;

extern "C" fn collect_diag(info: LLVMDiagnosticInfoRef, ctx: *mut c_void) {
    if ctx.is_null() || info.is_null() {
        return;
    }
    // SAFETY: `ctx` is the `*mut DiagSink` registered in [`run_pgo_pipeline`], live for
    // the duration of the pipeline run; `info` is a valid diagnostic handle owned by
    // libLLVM for this callback. The description string is copied and freed here.
    unsafe {
        let severity = llvm_sys::core::LLVMGetDiagInfoSeverity(info) as c_int;
        let desc = llvm_sys::core::LLVMGetDiagInfoDescription(info);
        let message = if desc.is_null() {
            String::new()
        } else {
            let s = CStr::from_ptr(desc).to_string_lossy().into_owned();
            llvm_sys::core::LLVMDisposeMessage(desc);
            s
        };
        let sink = &mut *(ctx as *mut DiagSink);
        sink.diags.push(CapturedDiag { severity, message });
    }
}

/// Run the shim's PGO pipeline on a raw `LLVMModuleRef` + `LLVMTargetMachineRef`.
///
/// For [`PgoAction::Use`] a context diagnostic handler is installed for the duration
/// of the run and cleared before returning (so the context never references the freed
/// sink on a later diagnostic). Returns the captured Warning-severity diagnostics; an
/// Error-severity diagnostic or a nonzero shim rc becomes a [`CodegenError`].
///
/// # Safety
/// `module` / `tm` must be live handles in the process LLVM; the module must have a
/// datalayout (`build_module` sets it). For [`PgoAction::Use`] the profile path must
/// already have been validated by the caller (this does NOT re-check the file).
pub unsafe fn run_pgo_pipeline(
    module: LLVMModuleRef,
    tm: LLVMTargetMachineRef,
    opt_level: i32,
    action: PgoAction<'_>,
) -> Result<PgoRunReport, CodegenError> {
    let profdata_c: Option<CString> = match action {
        PgoAction::Use(p) => {
            let s = p.to_str().ok_or_else(|| {
                CodegenError::Target(format!("non-UTF-8 profdata path '{}'", p.display()))
            })?;
            Some(CString::new(s).map_err(|_| {
                CodegenError::Target(format!("interior NUL in profdata path '{}'", p.display()))
            })?)
        }
        PgoAction::Instrument => None,
    };
    let path_ptr = profdata_c.as_ref().map_or(std::ptr::null(), |c| c.as_ptr());

    // A USE run installs a diagnostic handler so a stale/mismatched profile surfaces as
    // captured diagnostics instead of a silent process exit. GEN needs none.
    let mut sink = DiagSink::default();
    let ctx = unsafe { llvm_sys::core::LLVMGetModuleContext(module) };
    let install_handler = matches!(action, PgoAction::Use(_));
    if install_handler {
        // SAFETY: `sink` outlives the run below and the handler is cleared before it
        // drops; `ctx` is the module's live context.
        unsafe {
            llvm_sys::core::LLVMContextSetDiagnosticHandler(
                ctx,
                Some(collect_diag),
                &mut sink as *mut DiagSink as *mut c_void,
            );
        }
    }

    let mut matched: c_int = 0;
    let mut total: c_int = 0;
    let rc = unsafe {
        align_pgo_run_pipeline(
            module,
            tm,
            opt_level,
            action.kind(),
            path_ptr,
            &mut matched,
            &mut total,
        )
    };

    // Clear the handler BEFORE `sink` drops — any later diagnostic on this context (e.g.
    // during object emission) must not dereference the freed sink pointer.
    if install_handler {
        // SAFETY: `ctx` is still the module's live context.
        unsafe { llvm_sys::core::LLVMContextSetDiagnosticHandler(ctx, None, std::ptr::null_mut()) };
    }

    if rc != 0 {
        return Err(CodegenError::Target(format!(
            "PGO pipeline failed (shim rc={rc})"
        )));
    }

    // Partition captured diagnostics. FAIL-CLOSED policy (S2 note): the diagnostic handler is
    // installed ONLY around the PGO pipeline run above, so every diagnostic it captured came from that
    // run. An Error severity is a hard failure (a profile libLLVM rejects outright must never be a
    // silent proceed). EVERY Warning severity is surfaced verbatim in the report — we deliberately do
    // NOT keyword-filter for "profile"/"mismatch", because a real PGO degradation warning with unusual
    // wording (a future LLVM rewording, a counter-overflow warning, …) must not be dropped. Remark (2)
    // / Note (3) severities are the only thing dropped: those are ordinary optimization remarks (e.g.
    // "loop not vectorized: call instruction cannot be vectorized") libLLVM also routes here.
    let mut errors: Vec<String> = Vec::new();
    let mut warnings: Vec<String> = Vec::new();
    for d in sink.diags {
        if d.severity == LLVM_DS_ERROR {
            errors.push(d.message);
        } else if d.severity == LLVM_DS_WARNING {
            warnings.push(d.message);
        }
    }
    if !errors.is_empty() {
        return Err(CodegenError::Target(format!(
            "PGO profile use reported error(s): {}",
            errors.join("; ")
        )));
    }
    Ok(PgoRunReport {
        warnings,
        matched_fns: matched.max(0) as u32,
        total_fns: total.max(0) as u32,
    })
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
    use std::path::PathBuf;
    use std::process::Command;
    use std::time::Instant;

    /// A branch-heavy Align program: a hot `classify` with a biased branch mix, driven
    /// 50k times from a `loop`. The bias is what PGO records and re-applies as
    /// `branch_weights`; the volume guarantees non-empty counters.
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

    /// A structurally DIFFERENT program (its `main` has a different CFG than `BRANCHY`'s),
    /// used to prove the diagnostic handler is live: applying `BRANCHY`'s profile here
    /// must raise a `main` hash-mismatch warning captured into the [`PgoRunReport`].
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

    /// The forced-undefined symbol that pulls the profile runtime's atexit writer out of
    /// the archive on ELF. The exact flag clang's driver injects for `-fprofile-generate`.
    const FORCE_PROFILE_RUNTIME: &str = "-Wl,--undefined=__llvm_profile_runtime";

    fn opt_o2() -> i32 {
        2
    }

    fn scratch() -> PathBuf {
        let d = std::env::temp_dir().join(format!("align_pgo_prod_{}", std::process::id()));
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

    fn to_mir(src: &str) -> Program {
        let mut d = Diagnostics::new();
        let toks = tokenize(0, src, &mut d);
        let f = parse_file(toks, &mut d);
        let hir = check_file(&f, &mut d);
        assert!(!d.has_errors(), "frontend diagnostics present");
        lower_program(&hir)
    }

    fn runtime_archive() -> PathBuf {
        let exe = std::env::current_exe().unwrap();
        let dir = exe.parent().unwrap();
        for cand in [dir.join("libalign_runtime.a"), dir.join("../libalign_runtime.a")] {
            if cand.exists() {
                return cand;
            }
        }
        panic!("cannot find libalign_runtime.a near {} (run `cargo build` first)", dir.display());
    }

    fn profile_runtime_archive() -> Option<PathBuf> {
        let out = Command::new("clang-22")
            .arg("-print-file-name=libclang_rt.profile-x86_64.a")
            .output()
            .ok()?;
        let p = PathBuf::from(String::from_utf8_lossy(&out.stdout).trim());
        (p.is_absolute() && p.exists()).then_some(p)
    }

    /// Build the branch-heavy program via the real emit path, run the shim GEN pipeline
    /// in place of the stock opt run, and emit the object.
    fn emit_gen_object(obj: &Path) {
        let program = to_mir(BRANCHY);
        let ctx = Context::create();
        let module = ctx.create_module("align");
        let tm = create_target_machine(&BuildTarget::Baseline, Profile::Release.codegen_opt_level())
            .unwrap();
        build_module(&ctx, &module, &program, &tm, None, &[], false).unwrap();
        apply_size_attrs(&ctx, &module, Profile::Release);
        let report = unsafe {
            run_pgo_pipeline(module.as_mut_ptr(), tm.as_mut_ptr(), opt_o2(), PgoAction::Instrument)
        }
        .expect("gen pipeline");
        assert!(report.warnings.is_empty(), "gen produced no warnings");
        tm.write_to_file(&module, FileType::Object, obj).expect("emit object");
    }

    // -- 3 (IR-shape): GEN module has counters + llvm.used pinning. -----------
    #[test]
    #[ignore = "requires the PGO shim + cc + clang-22 profile rt"]
    fn gen_ir_shape_has_counters() {
        let program = to_mir(BRANCHY);
        let ctx = Context::create();
        let module = ctx.create_module("align");
        let tm = create_target_machine(&BuildTarget::Baseline, Profile::Release.codegen_opt_level())
            .unwrap();
        build_module(&ctx, &module, &program, &tm, None, &[], false).unwrap();
        apply_size_attrs(&ctx, &module, Profile::Release);

        let report = unsafe {
            run_pgo_pipeline(module.as_mut_ptr(), tm.as_mut_ptr(), opt_o2(), PgoAction::Instrument)
        }
        .expect("gen pipeline");
        assert!(report.warnings.is_empty());

        let ir = module.print_to_string().to_string();
        assert!(ir.contains("__profc_"), "no __profc_ counters in gen IR");
        assert!(ir.contains("__profd_"), "no __profd_ data in gen IR");
        assert!(ir.contains("__llvm_prf"), "no __llvm_prf metadata section in gen IR");
        assert!(ir.contains("@llvm.used"), "names not pinned by @llvm.used in gen IR");
        assert!(ir.contains("@llvm.compiler.used"), "counter data not pinned in gen IR");
        // On ELF the module carries NO `__llvm_profile_runtime` reference — the `-u` link
        // mechanism supplies it; its absence here is expected.
        assert!(
            !ir.contains("__llvm_profile_runtime"),
            "unexpected runtime hook in ELF gen IR — the -u link mechanism assumption changed"
        );
    }

    // -- 3 (control): no-PGO module has NEITHER counters NOR branch_weights. ---
    #[test]
    #[ignore = "requires the PGO shim"]
    fn control_ir_shape_has_neither() {
        use inkwell::passes::PassBuilderOptions;
        let program = to_mir(BRANCHY);
        let ctx = Context::create();
        let module = ctx.create_module("align");
        let tm = create_target_machine(&BuildTarget::Baseline, Profile::Release.codegen_opt_level())
            .unwrap();
        build_module(&ctx, &module, &program, &tm, None, &[], false).unwrap();
        apply_size_attrs(&ctx, &module, Profile::Release);
        module.run_passes("default<O2>", &tm, PassBuilderOptions::create()).unwrap();
        let ir = module.print_to_string().to_string();
        assert!(!ir.contains("__profc_"), "control IR unexpectedly has __profc_ counters");
        assert!(!ir.contains("branch_weights"), "control IR unexpectedly has branch_weights");
    }

    // -- 1 (end-to-end GEN): a real Align binary writes a .profraw at exit. ----
    #[test]
    #[ignore = "requires the PGO shim + cc + clang-22 profile rt"]
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
        assert!(run.code().is_some(), "instrumented program crashed: {run:?}");
        assert!(
            profraw.exists() && std::fs::metadata(&profraw).unwrap().len() > 0,
            "no non-empty .profraw written to {}",
            profraw.display()
        );
    }

    // -- 1+2 (full round trip): gen -> run -> merge -> use -> branch_weights. --
    #[test]
    #[ignore = "requires the PGO shim + cc + clang-22 + llvm-profdata-22"]
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
        assert!(profraw.exists() && std::fs::metadata(&profraw).unwrap().len() > 0);

        let profdata = dir.join("branchy.profdata");
        assert!(
            Command::new("llvm-profdata-22")
                .args(["merge", "-o"])
                .arg(&profdata)
                .arg(&profraw)
                .status()
                .expect("llvm-profdata merge")
                .success(),
            "llvm-profdata merge failed"
        );

        // Rebuild the SAME program via the USE pipeline — no mismatch, branch_weights present.
        let program = to_mir(BRANCHY);
        let ctx = Context::create();
        let module = ctx.create_module("align");
        let tm = create_target_machine(&BuildTarget::Baseline, Profile::Release.codegen_opt_level())
            .unwrap();
        build_module(&ctx, &module, &program, &tm, None, &[], false).unwrap();
        apply_size_attrs(&ctx, &module, Profile::Release);
        let report = unsafe {
            run_pgo_pipeline(module.as_mut_ptr(), tm.as_mut_ptr(), opt_o2(), PgoAction::Use(&profdata))
        }
        .expect("use pipeline");
        let ir = module.print_to_string().to_string();
        assert!(ir.contains("branch_weights"), "no !prof branch_weights — profile not consumed");
        let bad: Vec<&String> = report
            .warnings
            .iter()
            .filter(|m| {
                let l = m.to_ascii_lowercase();
                l.contains("mismatch") || l.contains("unprofiled") || l.contains("out of date")
            })
            .collect();
        assert!(bad.is_empty(), "PGO degradation warnings on matching module: {bad:?}");

        // Prove the handler is LIVE: the same profile on a structurally-different module
        // must produce a captured hash-mismatch warning in the report.
        let other = to_mir(DIFFERENT);
        let octx = Context::create();
        let omod = octx.create_module("align");
        let otm = create_target_machine(&BuildTarget::Baseline, Profile::Release.codegen_opt_level())
            .unwrap();
        build_module(&octx, &omod, &other, &otm, None, &[], false).unwrap();
        let oreport = unsafe {
            run_pgo_pipeline(omod.as_mut_ptr(), otm.as_mut_ptr(), opt_o2(), PgoAction::Use(&profdata))
        }
        .expect("use pipeline (mismatch module)");
        assert!(
            oreport.warnings.iter().any(|m| m.to_ascii_lowercase().contains("mismatch")),
            "handler did not capture the expected hash mismatch (handler may be dead): {:?}",
            oreport.warnings
        );
    }

    // -- 4: timing smoke (gen vs plain, at test scale). -----------------------
    #[test]
    #[ignore = "requires the PGO shim"]
    fn timing_smoke() {
        use inkwell::passes::PassBuilderOptions;
        let program = to_mir(BRANCHY);

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

        let t = Instant::now();
        {
            let ctx = Context::create();
            let m = ctx.create_module("align");
            let tm = create_target_machine(&BuildTarget::Baseline, Profile::Release.codegen_opt_level())
                .unwrap();
            build_module(&ctx, &m, &program, &tm, None, &[], false).unwrap();
            unsafe {
                run_pgo_pipeline(m.as_mut_ptr(), tm.as_mut_ptr(), opt_o2(), PgoAction::Instrument)
            }
            .expect("gen pipeline");
        }
        let t_gen = t.elapsed();

        eprintln!("[timing] plain O2={t_plain:?}  gen O2={t_gen:?}  (test scale, one module)");
    }
}
