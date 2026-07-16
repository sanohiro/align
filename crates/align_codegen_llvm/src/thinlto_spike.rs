//! ThinLTO S0 spike harness (feature `thinlto-spike`, ignored tests).
//!
//! This module is a FEASIBILITY PROTOTYPE, not driver wiring. It exercises the
//! C++ shim (`cpp/thinlto_shim.cpp`) and answers the S0 go/no-go questions:
//!
//!   1. Collapse question: can the legacy `ThinLTOCodeGenerator` C API (which
//!      llvm-sys DOES expose) build summaries from summary-LESS bitcode? -> NO
//!      (`collapse_thinltocodegen_rejects_summaryless`).
//!   2. Minimal mechanism: shim entry #1 (emit summary-bearing bitcode) +
//!      existing llvm-sys `thinlto_codegen_*` C API drives a full round-trip
//!      with cross-module inlining (`roundtrip_via_thinltocodegen`).
//!   3. Full 3-entry shim: entry #2 (thin-link -> import lists) + entry #3
//!      (import + backend) inline across modules (`full_shim_link_and_backend`).
//!   4. Determinism + timing smoke checks.
//!
//! Run with:
//!   cargo test -p align_codegen_llvm --features thinlto-spike -- --ignored --nocapture

#![allow(clippy::missing_safety_doc)]

use std::ffi::{c_char, c_void, CString};
use std::os::raw::c_int;

// ---- shim FFI ------------------------------------------------------------

unsafe extern "C" {
    /// Entry 1: prelink pipeline + emit summary-bearing bitcode to `out_path`.
    fn align_thinlto_write_prelink_bc(module: *mut c_void, out_path: *const c_char) -> c_int;

    /// Entry 2: thin-link. Calls `cb` for every import edge.
    fn align_thinlto_link(
        bc_paths: *const *const c_char,
        n_modules: usize,
        preserve_syms: *const *const c_char,
        n_preserve: usize,
        cb: ImportCb,
        ctx: *mut c_void,
    ) -> c_int;

    /// Entry 3: import functions into one module + backend + emit object.
    #[allow(clippy::too_many_arguments)]
    fn align_thinlto_backend(
        own_identifier: *const c_char,
        bc_paths: *const *const c_char,
        n_modules: usize,
        identifiers: *const *const c_char,
        own_idx: u32,
        preserve_syms: *const *const c_char,
        n_preserve: usize,
        cpu: *const c_char,
        features: *const c_char,
        out_obj: *const c_char,
    ) -> c_int;
}

type ImportCb = extern "C" fn(
    ctx: *mut c_void,
    dest_mod: *const c_char,
    dest_len: usize,
    src_mod: *const c_char,
    src_len: usize,
    guid: u64,
    is_definition: c_int,
);

#[derive(Clone, PartialEq, Eq, Debug)]
pub struct ImportEdge {
    pub dest: String,
    pub src: String,
    pub guid: u64,
    pub is_definition: bool,
}

extern "C" fn collect_edge(
    ctx: *mut c_void,
    dest_mod: *const c_char,
    dest_len: usize,
    src_mod: *const c_char,
    src_len: usize,
    guid: u64,
    is_definition: c_int,
) {
    unsafe {
        let dest = slice_to_string(dest_mod, dest_len);
        let src = slice_to_string(src_mod, src_len);
        let out = &mut *(ctx as *mut Vec<ImportEdge>);
        out.push(ImportEdge {
            dest,
            src,
            guid,
            is_definition: is_definition != 0,
        });
    }
}

unsafe fn slice_to_string(ptr: *const c_char, len: usize) -> String {
    // A StringRef can be empty with a null data pointer; from_raw_parts(null, 0)
    // is UB, so treat null/zero as the empty string.
    if ptr.is_null() || len == 0 {
        return String::new();
    }
    let bytes = unsafe { std::slice::from_raw_parts(ptr as *const u8, len) };
    String::from_utf8_lossy(bytes).into_owned()
}

// ---- public spike helpers (used by tests) --------------------------------

/// Entry 1: run the ThinLTO prelink pipeline on `module` (a raw LLVMModuleRef,
/// e.g. from inkwell `Module::as_mut_ptr()`) and emit summary-bearing bitcode
/// to `out`. Returns the shim rc (0 = ok).
///
/// # Safety
/// `module` must be a valid `LLVMModuleRef` living in the same process LLVM.
pub unsafe fn write_prelink_bc(module: *mut c_void, out: &std::path::Path) -> i32 {
    let c_out = CString::new(out.to_str().unwrap()).unwrap();
    unsafe { align_thinlto_write_prelink_bc(module, c_out.as_ptr()) }
}

/// Entry 3: import + backend + emit object for one unit. Module identities are
/// the bitcode buffer identifiers (their file paths). Returns the shim rc.
#[allow(clippy::too_many_arguments)]
pub fn backend(
    own_identifier: &str,
    bc_paths: &[&str],
    identifiers: &[&str],
    own_idx: u32,
    preserve: &[&str],
    cpu: &str,
    features: &str,
    out_obj: &std::path::Path,
) -> i32 {
    let c_own = CString::new(own_identifier).unwrap();
    let c_paths: Vec<CString> = bc_paths.iter().map(|p| CString::new(*p).unwrap()).collect();
    let path_ptrs: Vec<*const c_char> = c_paths.iter().map(|c| c.as_ptr()).collect();
    let c_ids: Vec<CString> = identifiers.iter().map(|p| CString::new(*p).unwrap()).collect();
    let id_ptrs: Vec<*const c_char> = c_ids.iter().map(|c| c.as_ptr()).collect();
    let c_pres: Vec<CString> = preserve.iter().map(|p| CString::new(*p).unwrap()).collect();
    let pres_ptrs: Vec<*const c_char> = c_pres.iter().map(|c| c.as_ptr()).collect();
    let c_cpu = CString::new(cpu).unwrap();
    let c_feat = CString::new(features).unwrap();
    let c_out = CString::new(out_obj.to_str().unwrap()).unwrap();
    unsafe {
        align_thinlto_backend(
            c_own.as_ptr(),
            path_ptrs.as_ptr(),
            path_ptrs.len(),
            id_ptrs.as_ptr(),
            own_idx,
            pres_ptrs.as_ptr(),
            pres_ptrs.len(),
            c_cpu.as_ptr(),
            c_feat.as_ptr(),
            c_out.as_ptr(),
        )
    }
}

/// Run thin-link over the given summary-bearing bitcode files, preserving
/// `preserve`. Returns the import edges.
pub fn thin_link(bc_paths: &[&str], preserve: &[&str]) -> Vec<ImportEdge> {
    let c_paths: Vec<CString> = bc_paths.iter().map(|p| CString::new(*p).unwrap()).collect();
    let path_ptrs: Vec<*const c_char> = c_paths.iter().map(|c| c.as_ptr()).collect();
    let c_pres: Vec<CString> = preserve.iter().map(|p| CString::new(*p).unwrap()).collect();
    let pres_ptrs: Vec<*const c_char> = c_pres.iter().map(|c| c.as_ptr()).collect();

    let mut edges: Vec<ImportEdge> = Vec::new();
    let rc = unsafe {
        align_thinlto_link(
            path_ptrs.as_ptr(),
            path_ptrs.len(),
            pres_ptrs.as_ptr(),
            pres_ptrs.len(),
            collect_edge,
            &mut edges as *mut Vec<ImportEdge> as *mut c_void,
        )
    };
    assert_eq!(rc, 0, "align_thinlto_link failed rc={rc}");
    edges
}

#[cfg(test)]
mod tests {
    use super::*;
    use inkwell::context::Context;
    use inkwell::module::Module;
    use inkwell::targets::{InitializationConfig, Target};
    use std::path::{Path, PathBuf};
    use std::process::Command;
    use std::time::Instant;

    fn scratch() -> PathBuf {
        let d = std::env::temp_dir().join(format!("align_thinlto_spike_{}", std::process::id()));
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    /// Give a module the host triple + data layout (ThinLTO requires a
    /// datalayout; inkwell modules have none by default).
    fn set_host_layout(m: &Module) {
        use inkwell::targets::{CodeModel, RelocMode, TargetMachine};
        Target::initialize_native(&InitializationConfig::default()).unwrap();
        let triple = TargetMachine::get_default_triple();
        let target = Target::from_triple(&triple).unwrap();
        let tm = target
            .create_target_machine(
                &triple,
                "",
                "",
                inkwell::OptimizationLevel::Default,
                RelocMode::PIC,
                CodeModel::Default,
            )
            .unwrap();
        m.set_triple(&triple);
        m.set_data_layout(&tm.get_target_data().get_data_layout());
    }

    /// Build the callee module B: `i32 callee(i32 x) { return x*2 + 1; }`.
    fn build_callee<'c>(ctx: &'c Context, name: &str) -> Module<'c> {
        let m = ctx.create_module(name);
        set_host_layout(&m);
        let i32t = ctx.i32_type();
        let fnty = i32t.fn_type(&[i32t.into()], false);
        let f = m.add_function("callee", fnty, None);
        let bb = ctx.append_basic_block(f, "entry");
        let b = ctx.create_builder();
        b.position_at_end(bb);
        let x = f.get_nth_param(0).unwrap().into_int_value();
        let a = b.build_int_mul(x, i32t.const_int(2, false), "a").unwrap();
        let r = b
            .build_int_add(a, i32t.const_int(1, false), "r")
            .unwrap();
        b.build_return(Some(&r)).unwrap();
        m
    }

    /// Build the caller module A calling B's `callee` twice.
    fn build_caller<'c>(ctx: &'c Context, name: &str) -> Module<'c> {
        let m = ctx.create_module(name);
        set_host_layout(&m);
        let i32t = ctx.i32_type();
        let fnty = i32t.fn_type(&[i32t.into()], false);
        let callee = m.add_function("callee", fnty, None); // external declaration
        let caller = m.add_function("caller", fnty, None);
        let bb = ctx.append_basic_block(caller, "entry");
        let b = ctx.create_builder();
        b.position_at_end(bb);
        let x = caller.get_nth_param(0).unwrap().into_int_value();
        let c1 = b
            .build_call(callee, &[x.into()], "c1")
            .unwrap()
            .try_as_basic_value()
            .basic()
            .unwrap()
            .into_int_value();
        let x7 = b.build_int_add(x, i32t.const_int(7, false), "x7").unwrap();
        let c2 = b
            .build_call(callee, &[x7.into()], "c2")
            .unwrap()
            .try_as_basic_value()
            .basic()
            .unwrap()
            .into_int_value();
        let r = b.build_int_add(c1, c2, "r").unwrap();
        b.build_return(Some(&r)).unwrap();
        m
    }

    /// Run shim entry #1 on an inkwell module, producing summary-bearing bc.
    fn emit_prelink(m: &Module, out: &Path) {
        let rc = unsafe { write_prelink_bc(m.as_mut_ptr() as *mut c_void, out) };
        assert_eq!(rc, 0, "entry1 failed rc={rc} for {out:?}");
    }

    fn have_tool(t: &str) -> bool {
        Command::new(t).arg("--version").output().map(|o| o.status.success()).unwrap_or(false)
    }

    // ------------------------------------------------------------------
    // Q1 (collapse): ThinLTOCodeGenerator C API on summary-LESS bitcode.
    // Expectation: it crashes (report_fatal_error/abort). We run it in a
    // forked child so the crash does not take down the test process.
    // ------------------------------------------------------------------
    #[test]
    #[ignore = "spike: requires clang-22 + LLVM C++ shim"]
    fn collapse_thinltocodegen_rejects_summaryless() {
        assert!(have_tool("clang-22"), "clang-22 required");
        let dir = scratch();
        // Summary-LESS bitcode via clang (no -flto): mirrors what
        // LLVMWriteBitcodeToMemoryBuffer produces.
        let ac = dir.join("a.c");
        let bc = dir.join("b.c");
        std::fs::write(&ac, "extern int callee(int);\nint caller(int x){return callee(x)+callee(x+7);}\n").unwrap();
        std::fs::write(&bc, "int callee(int x){return x*2+1;}\n").unwrap();
        let abc = dir.join("a.plain.bc");
        let bbc = dir.join("b.plain.bc");
        for (src, out) in [(&ac, &abc), (&bc, &bbc)] {
            let ok = Command::new("clang-22")
                .args(["-O2", "-emit-llvm", "-c"])
                .arg(src)
                .arg("-o")
                .arg(out)
                .status()
                .unwrap()
                .success();
            assert!(ok, "clang failed");
        }

        let a_bytes = std::fs::read(&abc).unwrap();
        let b_bytes = std::fs::read(&bbc).unwrap();

        // Fork: the child feeds summary-less bitcode to the ThinLTOCodeGenerator
        // C API and (if it survives) exits 0. We assert it did NOT exit 0.
        let pid = unsafe { libc::fork() };
        assert!(pid >= 0, "fork failed");
        if pid == 0 {
            // child
            unsafe { run_thinlto_codegen(&[("a", &a_bytes), ("b", &b_bytes)], &["caller"], &dir.join("objs")) };
            // If we get here it did not crash.
            unsafe { libc::_exit(0) };
        }
        // parent
        let mut status: c_int = 0;
        unsafe { libc::waitpid(pid, &mut status, 0) };
        let exited_zero = libc::WIFEXITED(status) && libc::WEXITSTATUS(status) == 0;
        assert!(
            !exited_zero,
            "COLLAPSE=YES?? ThinLTOCodeGenerator survived summary-less bitcode (status={status}). \
             Expected a crash/abort. Re-examine the collapse conclusion."
        );
        eprintln!(
            "[collapse] ThinLTOCodeGenerator on summary-LESS bitcode -> child status={status} \
             (signaled={}, exitcode={}). Collapse path = NO.",
            libc::WIFSIGNALED(status),
            if libc::WIFEXITED(status) { libc::WEXITSTATUS(status) } else { -1 }
        );
    }

    /// Drive the legacy ThinLTOCodeGenerator C API (llvm-sys `lto`) over the
    /// given (identifier, bitcode) modules, preserving `preserve`. Objects are
    /// written to `out_dir` (LTOObjectBuffer's fields are private, so we use the
    /// on-disk generated-objects path). Returns the object byte buffers. On
    /// summary-less input `thinlto_codegen_process` is expected to abort.
    unsafe fn run_thinlto_codegen(
        modules: &[(&str, &[u8])],
        preserve: &[&str],
        out_dir: &PathBuf,
    ) -> Vec<Vec<u8>> {
        use llvm_sys::lto::*;
        Target::initialize_native(&InitializationConfig::default()).unwrap();
        std::fs::create_dir_all(out_dir).unwrap();
        let c_dir = CString::new(out_dir.to_str().unwrap()).unwrap();
        let mut out = Vec::new();
        unsafe {
            let cg = thinlto_create_codegen();
            assert!(!cg.is_null());
            thinlto_codegen_set_pic_model(cg, lto_codegen_model::LTO_CODEGEN_PIC_MODEL_DYNAMIC);
            thinlto_set_generated_objects_dir(cg, c_dir.as_ptr());
            let mut ids: Vec<CString> = Vec::new();
            for (id, bytes) in modules {
                let cid = CString::new(*id).unwrap();
                thinlto_codegen_add_module(cg, cid.as_ptr(), bytes.as_ptr() as *const c_char, bytes.len() as c_int);
                ids.push(cid);
            }
            for p in preserve {
                let cp = CString::new(*p).unwrap();
                thinlto_codegen_add_must_preserve_symbol(cg, cp.as_ptr(), p.len() as c_int);
            }
            thinlto_codegen_process(cg);
            let n = thinlto_module_get_num_object_files(cg);
            for i in 0..n {
                let cpath = thinlto_module_get_object_file(cg, i);
                let path = std::ffi::CStr::from_ptr(cpath).to_string_lossy().into_owned();
                if let Ok(bytes) = std::fs::read(&path) {
                    out.push(bytes);
                }
            }
            thinlto_codegen_dispose(cg);
        }
        out
    }

    // ------------------------------------------------------------------
    // Entry #1 emits a real summary section (proves shim links & bridges).
    // ------------------------------------------------------------------
    #[test]
    #[ignore = "spike: requires LLVM C++ shim + llvm-bcanalyzer-22"]
    fn entry1_emits_summary_bearing_bitcode() {
        let dir = scratch();
        let ctx = Context::create();
        let m = build_caller(&ctx, "modA");
        let out = dir.join("modA.prelink.bc");
        emit_prelink(&m, &out);
        assert!(out.exists() && std::fs::metadata(&out).unwrap().len() > 0);

        if have_tool("llvm-bcanalyzer-22") {
            let dump = Command::new("llvm-bcanalyzer-22").arg("--dump").arg(&out).output().unwrap();
            let text = String::from_utf8_lossy(&dump.stdout);
            assert!(
                text.contains("GLOBALVAL_SUMMARY_BLOCK"),
                "entry1 output has NO summary block:\n{text}"
            );
            eprintln!("[entry1] summary-bearing bitcode confirmed (GLOBALVAL_SUMMARY_BLOCK present).");
        } else {
            eprintln!("[entry1] llvm-bcanalyzer-22 not found; only checked nonempty output.");
        }
    }

    // ------------------------------------------------------------------
    // Q2 (minimal mechanism): entry1 + existing llvm-sys ThinLTOCodeGenerator
    // -> full round-trip, cross-module inlining.
    // ------------------------------------------------------------------
    #[test]
    #[ignore = "spike: requires LLVM C++ shim + objdump"]
    fn roundtrip_via_thinltocodegen() {
        let dir = scratch();
        let ctx_a = Context::create();
        let ctx_b = Context::create();
        let ma = build_caller(&ctx_a, "modA");
        let mb = build_callee(&ctx_b, "modB");
        let abc = dir.join("modA.rt.bc");
        let bbc = dir.join("modB.rt.bc");
        emit_prelink(&ma, &abc);
        emit_prelink(&mb, &bbc);

        let a_bytes = std::fs::read(&abc).unwrap();
        let b_bytes = std::fs::read(&bbc).unwrap();
        let objs = unsafe { run_thinlto_codegen(&[("modA", &a_bytes), ("modB", &b_bytes)], &["caller"], &dir.join("objs")) };
        assert!(!objs.is_empty(), "no objects produced");

        // Find the object that defines `caller` and assert it has no reference
        // to `callee` (inlined away).
        let found = objs.iter().any(|obj| object_defines_without_call(obj, &dir, "caller", "callee"));
        assert!(found, "no object had `caller` with `callee` inlined away");
        eprintln!("[roundtrip] entry1 + ThinLTOCodeGenerator: callee inlined into caller. Minimal mechanism WORKS.");
    }

    // ------------------------------------------------------------------
    // Q3 (full 3-entry shim): entry2 import list + entry3 backend.
    // ------------------------------------------------------------------
    #[test]
    #[ignore = "spike: requires LLVM C++ shim + objdump"]
    fn full_shim_link_and_backend() {
        let dir = scratch();
        let ctx_a = Context::create();
        let ctx_b = Context::create();
        let ma = build_caller(&ctx_a, "modA");
        let mb = build_callee(&ctx_b, "modB");
        let abc = dir.join("modA.full.bc");
        let bbc = dir.join("modB.full.bc");
        emit_prelink(&ma, &abc);
        emit_prelink(&mb, &bbc);
        let abc_s = abc.to_str().unwrap();
        let bbc_s = bbc.to_str().unwrap();

        // entry2: thin-link -> import edges. The combined index keys modules by
        // their MemoryBuffer identifier (the bc file path we pass to getFile).
        let edges = thin_link(&[abc_s, bbc_s], &["caller"]);
        eprintln!("[link] import edges: {edges:?}");
        assert!(
            edges.iter().any(|e| e.dest == abc_s && e.src == bbc_s),
            "expected an import edge modA <- modB; got {edges:?}"
        );

        // entry3: backend for modA.
        let obj = dir.join("modA.o");
        run_backend(abc_s, &[abc_s, bbc_s], &[abc_s, bbc_s], 0, &["caller"], &obj);
        assert!(obj.exists() && std::fs::metadata(&obj).unwrap().len() > 0);
        let obj_bytes = std::fs::read(&obj).unwrap();
        assert!(
            object_defines_without_call(&obj_bytes, &dir, "caller", "callee"),
            "modA backend object still calls callee (not inlined)"
        );
        eprintln!("[backend] entry3 imported+inlined callee into caller. Full 3-entry shim WORKS.");
    }

    // ------------------------------------------------------------------
    // Q3 (determinism): import lists identical across ingestion order.
    // ------------------------------------------------------------------
    #[test]
    #[ignore = "spike: requires LLVM C++ shim"]
    fn determinism_import_lists_order_independent() {
        let dir = scratch();
        let ctx_a = Context::create();
        let ctx_b = Context::create();
        let ma = build_caller(&ctx_a, "modA");
        let mb = build_callee(&ctx_b, "modB");
        let abc = dir.join("modA.det.bc");
        let bbc = dir.join("modB.det.bc");
        emit_prelink(&ma, &abc);
        emit_prelink(&mb, &bbc);
        let abc_s = abc.to_str().unwrap();
        let bbc_s = bbc.to_str().unwrap();

        let norm = |mut e: Vec<ImportEdge>| {
            e.sort_by(|x, y| (&x.dest, &x.src, x.guid).cmp(&(&y.dest, &y.src, y.guid)));
            e
        };
        let ab = norm(thin_link(&[abc_s, bbc_s], &["caller"]));
        let ba = norm(thin_link(&[bbc_s, abc_s], &["caller"]));
        assert_eq!(ab, ba, "import edges differ across ingestion order after canonical sort");
        eprintln!("[determinism] import edges identical across [A,B] vs [B,A] ingestion (after sort): {ab:?}");
    }

    // ------------------------------------------------------------------
    // Timing smoke: rough wall-time of the three phases.
    // ------------------------------------------------------------------
    #[test]
    #[ignore = "spike: requires LLVM C++ shim + objdump"]
    fn timing_smoke() {
        let dir = scratch();
        let ctx_a = Context::create();
        let ctx_b = Context::create();
        let ma = build_caller(&ctx_a, "modA");
        let mb = build_callee(&ctx_b, "modB");
        let abc = dir.join("modA.t.bc");
        let bbc = dir.join("modB.t.bc");

        let t0 = Instant::now();
        emit_prelink(&ma, &abc);
        emit_prelink(&mb, &bbc);
        let t_prelink = t0.elapsed();

        let abc_s = abc.to_str().unwrap();
        let bbc_s = bbc.to_str().unwrap();
        let t1 = Instant::now();
        let _ = thin_link(&[abc_s, bbc_s], &["caller"]);
        let t_link = t1.elapsed();

        let obj = dir.join("modA.t.o");
        let t2 = Instant::now();
        run_backend(abc_s, &[abc_s, bbc_s], &[abc_s, bbc_s], 0, &["caller"], &obj);
        let t_backend = t2.elapsed();

        eprintln!(
            "[timing] prelink(x2)={:?}  thin-link={:?}  backend(1 unit)={:?}",
            t_prelink, t_link, t_backend
        );
    }

    // ---- helpers -----------------------------------------------------

    fn run_backend(
        own_id: &str,
        bc_paths: &[&str],
        identifiers: &[&str],
        own_idx: u32,
        preserve: &[&str],
        out_obj: &Path,
    ) {
        let rc = backend(own_id, bc_paths, identifiers, own_idx, preserve, "", "", out_obj);
        assert_eq!(rc, 0, "align_thinlto_backend failed rc={rc}");
    }

    /// True if `obj` (an ELF .o in memory) defines `def_sym` and its
    /// disassembly (with relocations) contains no reference to `call_sym`.
    fn object_defines_without_call(obj: &[u8], dir: &Path, def_sym: &str, call_sym: &str) -> bool {
        if !have_tool("objdump") {
            eprintln!("objdump not found; skipping inline verification");
            return true;
        }
        let path = dir.join(format!("verify_{}.o", std::process::id()));
        std::fs::write(&path, obj).unwrap();
        let out = Command::new("objdump").args(["-dr", "--no-show-raw-insn"]).arg(&path).output().unwrap();
        let text = String::from_utf8_lossy(&out.stdout);
        let defines = text.contains(&format!("<{def_sym}>:"));
        let references_call = text.contains(call_sym);
        defines && !references_call
    }
}
