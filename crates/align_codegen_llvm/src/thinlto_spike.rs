//! ThinLTO S0 spike harness (feature `thinlto-spike`, ignored tests).
//!
//! Historical FEASIBILITY PROTOTYPE, retained as a regression suite. It now drives
//! the PRODUCTION [`crate::thinlto`] wrappers (there is no separate spike FFI — that
//! was promoted to the production shim), so these tests double as an in-process check
//! of the driver-facing surface. They answer the S0 go/no-go questions:
//!
//!   1. Collapse question: can the legacy `ThinLTOCodeGenerator` C API (which
//!      llvm-sys DOES expose) build summaries from summary-LESS bitcode? -> NO
//!      (`collapse_thinltocodegen_rejects_summaryless`).
//!   2. Minimal mechanism: entry #1 (emit summary-bearing bitcode) + the existing
//!      llvm-sys `thinlto_codegen_*` C API drives a round-trip with cross-module
//!      inlining (`roundtrip_via_thinltocodegen`).
//!   3. Full 3-entry shim: entry #2 (thin-link -> import lists) + entry #3 (import +
//!      backend) inline across modules (`full_shim_link_and_backend`) — now exercising
//!      the S1 final form (serialized import list threaded into the backend).
//!   4. Determinism + timing smoke checks.
//!
//! Run with:
//!   cargo test -p align_codegen_llvm --features thinlto-spike -- --ignored --nocapture

#![allow(clippy::missing_safety_doc)]

#[cfg(test)]
mod tests {
    use crate::thinlto::{self, ImportEdge};
    use crate::{BuildTarget, Profile};
    use inkwell::context::Context;
    use inkwell::module::Module;
    use inkwell::targets::{InitializationConfig, Target};
    use std::ffi::{c_char, CString};
    use std::os::raw::c_int;
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
        let r = b.build_int_add(a, i32t.const_int(1, false), "r").unwrap();
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

    /// Run entry #1 on an inkwell module, producing summary-bearing bc, keyed by
    /// `id` (the spike uses the bc file path as the stable id).
    fn emit_prelink(m: &Module, id: &str, out: &Path) {
        // SAFETY: `m` is a live module with a datalayout (set_host_layout).
        let r = unsafe { thinlto::write_prelink_bc(m.as_mut_ptr(), id, 2, out) };
        r.unwrap_or_else(|e| panic!("entry1 failed for {out:?}: {e}"));
    }

    fn have_tool(t: &str) -> bool {
        Command::new(t).arg("--version").output().map(|o| o.status.success()).unwrap_or(false)
    }

    // ------------------------------------------------------------------
    // Q1 (collapse): ThinLTOCodeGenerator C API on summary-LESS bitcode.
    // ------------------------------------------------------------------
    #[test]
    #[ignore = "spike: requires clang-22 + LLVM C++ shim"]
    #[cfg(unix)]
    fn collapse_thinltocodegen_rejects_summaryless() {
        assert!(have_tool("clang-22"), "clang-22 required");
        let dir = scratch();
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

        let pid = unsafe { libc::fork() };
        assert!(pid >= 0, "fork failed");
        if pid == 0 {
            unsafe { run_thinlto_codegen(&[("a", &a_bytes), ("b", &b_bytes)], &["caller"], &dir.join("objs")) };
            unsafe { libc::_exit(0) };
        }
        let mut status: c_int = 0;
        unsafe { libc::waitpid(pid, &mut status, 0) };
        let exited_zero = libc::WIFEXITED(status) && libc::WEXITSTATUS(status) == 0;
        assert!(
            !exited_zero,
            "COLLAPSE=YES?? ThinLTOCodeGenerator survived summary-less bitcode (status={status})."
        );
        eprintln!("[collapse] ThinLTOCodeGenerator on summary-LESS bitcode -> child status={status}. Collapse path = NO.");
    }

    /// Drive the legacy ThinLTOCodeGenerator C API (llvm-sys `lto`) over the
    /// given (identifier, bitcode) modules. On summary-less input
    /// `thinlto_codegen_process` is expected to abort.
    unsafe fn run_thinlto_codegen(modules: &[(&str, &[u8])], preserve: &[&str], out_dir: &PathBuf) -> Vec<Vec<u8>> {
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
    // Entry #1 emits a real summary section.
    // ------------------------------------------------------------------
    #[test]
    #[ignore = "spike: requires LLVM C++ shim + llvm-bcanalyzer-22"]
    fn entry1_emits_summary_bearing_bitcode() {
        let dir = scratch();
        let ctx = Context::create();
        let m = build_caller(&ctx, "modA");
        let out = dir.join("modA.prelink.bc");
        emit_prelink(&m, "modA", &out);
        assert!(out.exists() && std::fs::metadata(&out).unwrap().len() > 0);

        if have_tool("llvm-bcanalyzer-22") {
            let dump = Command::new("llvm-bcanalyzer-22").arg("--dump").arg(&out).output().unwrap();
            let text = String::from_utf8_lossy(&dump.stdout);
            assert!(text.contains("GLOBALVAL_SUMMARY_BLOCK"), "entry1 output has NO summary block:\n{text}");
            eprintln!("[entry1] summary-bearing bitcode confirmed.");
        }
    }

    // ------------------------------------------------------------------
    // Q2 (minimal mechanism): entry1 + llvm-sys ThinLTOCodeGenerator.
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
        emit_prelink(&ma, "modA", &abc);
        emit_prelink(&mb, "modB", &bbc);

        let a_bytes = std::fs::read(&abc).unwrap();
        let b_bytes = std::fs::read(&bbc).unwrap();
        let objs = unsafe { run_thinlto_codegen(&[("modA", &a_bytes), ("modB", &b_bytes)], &["caller"], &dir.join("objs")) };
        assert!(!objs.is_empty(), "no objects produced");
        let found = objs.iter().any(|obj| object_defines_without_call(obj, &dir, "caller", "callee"));
        assert!(found, "no object had `caller` with `callee` inlined away");
        eprintln!("[roundtrip] entry1 + ThinLTOCodeGenerator: callee inlined. Minimal mechanism WORKS.");
    }

    // ------------------------------------------------------------------
    // Q3 (full 3-entry shim): entry2 import list + entry3 backend (S1 form).
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
        emit_prelink(&ma, "modA", &abc);
        emit_prelink(&mb, "modB", &bbc);

        let paths = vec![abc.clone(), bbc.clone()];
        let ids = vec!["modA".to_string(), "modB".to_string()];
        let preserve = vec!["caller".to_string()];

        // entry2: thin-link -> import edges + export set (keyed by stable ids).
        let plan = thinlto::thin_link(&paths, &ids, &preserve).expect("thin-link");
        eprintln!("[link] plan: {plan:?}");
        assert!(
            plan.imports.iter().any(|e| e.dest == "modA" && e.src == "modB"),
            "expected an import edge modA <- modB; got {plan:?}"
        );

        // entry3: backend for modA, threading modA's import list + the export set.
        let mod_a_imports: Vec<ImportEdge> = plan.imports.iter().filter(|e| e.dest == "modA").cloned().collect();
        let obj = dir.join("modA.o");
        thinlto::backend(&paths, &ids, 0, &preserve, &mod_a_imports, &plan.exports, &BuildTarget::Native, Profile::Release, &obj).expect("backend");
        assert!(obj.exists() && std::fs::metadata(&obj).unwrap().len() > 0);
        let obj_bytes = std::fs::read(&obj).unwrap();
        assert!(
            object_defines_without_call(&obj_bytes, &dir, "caller", "callee"),
            "modA backend object still calls callee (not inlined)"
        );
        eprintln!("[backend] entry3 imported+inlined callee via the threaded import list. S1 shim WORKS.");
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
        emit_prelink(&ma, "modA", &abc);
        emit_prelink(&mb, "modB", &bbc);

        let preserve = vec!["caller".to_string()];
        let norm = |mut e: Vec<ImportEdge>| {
            e.sort_by(|x, y| (&x.dest, &x.src, x.guid).cmp(&(&y.dest, &y.src, y.guid)));
            e
        };
        let ab = norm(thinlto::thin_link(&[abc.clone(), bbc.clone()], &["modA".into(), "modB".into()], &preserve).unwrap().imports);
        let ba = norm(thinlto::thin_link(&[bbc.clone(), abc.clone()], &["modB".into(), "modA".into()], &preserve).unwrap().imports);
        assert_eq!(ab, ba, "import edges differ across ingestion order after canonical sort");
        eprintln!("[determinism] import edges identical across [A,B] vs [B,A] ingestion: {ab:?}");
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
        emit_prelink(&ma, "modA", &abc);
        emit_prelink(&mb, "modB", &bbc);
        let t_prelink = t0.elapsed();

        let paths = vec![abc.clone(), bbc.clone()];
        let ids = vec!["modA".to_string(), "modB".to_string()];
        let preserve = vec!["caller".to_string()];
        let t1 = Instant::now();
        let plan = thinlto::thin_link(&paths, &ids, &preserve).unwrap();
        let t_link = t1.elapsed();

        let mod_a_imports: Vec<ImportEdge> = plan.imports.iter().filter(|e| e.dest == "modA").cloned().collect();
        let obj = dir.join("modA.t.o");
        let t2 = Instant::now();
        thinlto::backend(&paths, &ids, 0, &preserve, &mod_a_imports, &plan.exports, &BuildTarget::Native, Profile::Release, &obj).unwrap();
        let t_backend = t2.elapsed();

        eprintln!("[timing] prelink(x2)={t_prelink:?}  thin-link={t_link:?}  backend(1 unit)={t_backend:?}");
    }

    // ---- helpers -----------------------------------------------------

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
