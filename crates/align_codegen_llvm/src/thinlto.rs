//! ThinLTO driver-facing surface (production).
//!
//! Safe Rust wrappers over the three C++ shim entry points (`cpp/thinlto_shim.cpp`)
//! plus the preserve-set / opt-level helpers the `--thin-lto` driver path needs.
//! The shim is compiled unconditionally by `build.rs`; this module is always
//! available (no feature gate).
//!
//! Flow (serial, S1): per unit [`crate::emit_prelink_bc`] → summary-bearing `.bc`;
//! [`thin_link`] over all units → import edges; per unit [`backend`] (own bc + dep
//! bcs + its import list + the driver's `TargetMachine`) → object. Modules are
//! keyed by stable driver-chosen ids (the unit name), never temp-file paths.

use std::ffi::{c_char, c_int, c_void, CString};
use std::path::Path;

use llvm_sys::prelude::LLVMModuleRef;
use llvm_sys::target_machine::LLVMTargetMachineRef;

use align_mir::Program;

use crate::{create_target_machine, symbol_name, BuildTarget, CodegenError, Profile};

// ---- shim FFI ------------------------------------------------------------

unsafe extern "C" {
    /// Entry 1: stamp `stable_id`, run the ThinLTO prelink pipeline at `ir_opt_level`
    /// (0..3), and write summary-bearing bitcode to `out_path`.
    fn align_thinlto_write_prelink_bc(
        module: LLVMModuleRef,
        stable_id: *const c_char,
        ir_opt_level: c_int,
        out_path: *const c_char,
    ) -> c_int;

    /// Entry 2: thin-link. Calls `import_cb` for every import edge and `export_cb`
    /// for every (module, GUID) export pair (both keyed by the stable ids).
    #[allow(clippy::too_many_arguments)]
    fn align_thinlto_link(
        bc_paths: *const *const c_char,
        identifiers: *const *const c_char,
        n_modules: usize,
        preserve_syms: *const *const c_char,
        n_preserve: usize,
        import_cb: ImportCb,
        export_cb: ExportCb,
        ctx: *mut c_void,
    ) -> c_int;

    /// Entry 3: import (per the serialized import list) + promote/internalize (per
    /// the serialized export map) + backend + emit object.
    #[allow(clippy::too_many_arguments)]
    fn align_thinlto_backend(
        own_id: *const c_char,
        bc_paths: *const *const c_char,
        identifiers: *const *const c_char,
        n_modules: usize,
        own_idx: u32,
        preserve_syms: *const *const c_char,
        n_preserve: usize,
        imp_src_ids: *const *const c_char,
        imp_guids: *const u64,
        imp_kinds: *const c_int,
        n_imports: usize,
        exp_mods: *const *const c_char,
        exp_guids: *const u64,
        n_exports: usize,
        tm: LLVMTargetMachineRef,
        ir_opt_level: c_int,
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

type ExportCb =
    extern "C" fn(ctx: *mut c_void, mod_: *const c_char, mod_len: usize, guid: u64);

/// One cross-module import decision from the thin-link phase: module identities
/// are the driver's stable ids.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct ImportEdge {
    pub dest: String,
    pub src: String,
    pub guid: u64,
    pub is_definition: bool,
}

/// One exported global from the thin-link phase: `module` (a stable id) exports the
/// value with this GUID (it is referenced cross-module and must be promoted).
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct ExportEntry {
    pub module: String,
    pub guid: u64,
}

/// The complete thin-link decision set: per-destination import edges + the global
/// export set (both keyed by stable ids).
#[derive(Clone, Default, Debug)]
pub struct ThinLinkPlan {
    pub imports: Vec<ImportEdge>,
    pub exports: Vec<ExportEntry>,
}

// The FFI callbacks collect into a `ThinLinkPlan` behind `ctx`.
extern "C" fn collect_import(
    ctx: *mut c_void,
    dest_mod: *const c_char,
    dest_len: usize,
    src_mod: *const c_char,
    src_len: usize,
    guid: u64,
    is_definition: c_int,
) {
    if ctx.is_null() {
        return;
    }
    // SAFETY: `ctx` is the `*mut ThinLinkPlan` registered in [`thin_link`], valid for
    // the duration of the FFI call; the (ptr,len) slices point into libLLVM-owned
    // StringRefs valid during the callback.
    unsafe {
        let dest = slice_to_string(dest_mod, dest_len);
        let src = slice_to_string(src_mod, src_len);
        let plan = &mut *(ctx as *mut ThinLinkPlan);
        plan.imports.push(ImportEdge { dest, src, guid, is_definition: is_definition != 0 });
    }
}

extern "C" fn collect_export(ctx: *mut c_void, mod_: *const c_char, mod_len: usize, guid: u64) {
    if ctx.is_null() {
        return;
    }
    // SAFETY: as `collect_import` — `ctx` is the same live `*mut ThinLinkPlan`.
    unsafe {
        let module = slice_to_string(mod_, mod_len);
        let plan = &mut *(ctx as *mut ThinLinkPlan);
        plan.exports.push(ExportEntry { module, guid });
    }
}

/// # Safety
/// `ptr` is valid for `len` bytes, or null (treated as empty — `from_raw_parts(null, 0)`
/// is UB, so a null/zero StringRef becomes the empty string).
unsafe fn slice_to_string(ptr: *const c_char, len: usize) -> String {
    if ptr.is_null() || len == 0 {
        return String::new();
    }
    let bytes = unsafe { std::slice::from_raw_parts(ptr as *const u8, len) };
    String::from_utf8_lossy(bytes).into_owned()
}

// ---- opt-level / preserve-set helpers ------------------------------------

/// The middle-end opt level (0..3) for the ThinLTO prelink + backend pipelines.
/// ThinLTO is legal only on `release` (O2) / `fast` (O3); any other profile is
/// rejected by the driver before this is reached, so the fallback is O2.
pub fn ir_opt_level(profile: Profile) -> c_int {
    match profile {
        Profile::Fast => 3,
        _ => 2,
    }
}

/// The external-linkage DEFINED symbols of one unit — its contribution to the
/// ThinLTO preserve set. Mirrors [`crate::declare_fn`]'s linkage decision and
/// [`crate::emit_main_wrapper`] exactly:
///   * a `main` function yields the external C entry symbol `main` (a plain `-> i32`
///     main, or the generated wrapper for a `Result`/`Unit` main — never the internal
///     `align_main`);
///   * a `pub` non-entry function (`f.exportable`) keeps external linkage under its
///     mangled `module$name` symbol;
///   * an `--export` root keeps its symbol external (keyed on the LLVM `symbol`, as
///     `declare_fn` does — so `--export main` stays the documented no-op).
///
/// The driver unions these across units with `{main}` and the `--export` set to form
/// the fail-closed v1 preserve set.
pub fn exported_symbols(program: &Program, exports: &[String]) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    let push = |s: &str, out: &mut Vec<String>| {
        if !out.iter().any(|e| e == s) {
            out.push(s.to_string());
        }
    };
    for f in &program.fns {
        let sym = symbol_name(f);
        if f.name == "main" {
            // The external C entry is always `main` (direct i32 main or the wrapper).
            push("main", &mut out);
        }
        if f.exportable || exports.iter().any(|e| e == sym) {
            push(sym, &mut out);
        }
    }
    out
}

/// Whether a unit defines a `main` (so its object owns the external C `main`) — a
/// convenience for the driver's preserve-set assembly and diagnostics.
pub fn defines_main(program: &Program) -> bool {
    program.fns.iter().any(|f| f.name == "main")
}

// ---- entry-point wrappers ------------------------------------------------

/// Entry 1 wrapper: run the prelink pipeline on `module` (a raw `LLVMModuleRef`) and
/// write summary-bearing bitcode to `out`. `stable_id` is the module's chosen identity.
///
/// # Safety
/// `module` must be a valid `LLVMModuleRef` in the process LLVM, with a datalayout
/// (ThinLTO requires one — `build_module` sets it).
pub unsafe fn write_prelink_bc(
    module: LLVMModuleRef,
    stable_id: &str,
    ir_opt_level: c_int,
    out: &Path,
) -> Result<(), CodegenError> {
    let c_id = cstr(stable_id)?;
    let c_out = path_cstr(out)?;
    let rc = unsafe {
        align_thinlto_write_prelink_bc(module, c_id.as_ptr(), ir_opt_level, c_out.as_ptr())
    };
    if rc != 0 {
        return Err(CodegenError::Lowering(format!(
            "ThinLTO prelink (unit '{stable_id}') failed (shim rc={rc})"
        )));
    }
    Ok(())
}

/// Entry 2 wrapper: thin-link over the summary-bearing `bc_paths` (identified by the
/// parallel `ids`), preserving `preserve`. Returns the complete decision set (import
/// edges + export set), all keyed by stable ids.
pub fn thin_link(
    bc_paths: &[std::path::PathBuf],
    ids: &[String],
    preserve: &[String],
) -> Result<ThinLinkPlan, CodegenError> {
    assert_eq!(bc_paths.len(), ids.len(), "one id per bc path");
    let c_paths = path_cstrs(bc_paths)?;
    let path_ptrs = ptr_vec(&c_paths);
    let c_ids = str_cstrs(ids)?;
    let id_ptrs = ptr_vec(&c_ids);
    let c_pres = str_cstrs(preserve)?;
    let pres_ptrs = ptr_vec(&c_pres);

    let mut plan = ThinLinkPlan::default();
    let rc = unsafe {
        align_thinlto_link(
            path_ptrs.as_ptr(),
            id_ptrs.as_ptr(),
            path_ptrs.len(),
            pres_ptrs.as_ptr(),
            pres_ptrs.len(),
            collect_import,
            collect_export,
            &mut plan as *mut ThinLinkPlan as *mut c_void,
        )
    };
    if rc != 0 {
        return Err(CodegenError::Lowering(format!("ThinLTO thin-link failed (shim rc={rc})")));
    }
    Ok(plan)
}

/// Entry 3 wrapper: import (per `imports` = this unit's edges) + ThinLTO backend +
/// emit the object for unit `own_idx`. Builds the driver's own `TargetMachine`
/// (identical cpu/features/reloc/code-model to [`create_target_machine`]) and passes
/// its handle to the shim, so the backend's codegen matches the non-ThinLTO path.
#[allow(clippy::too_many_arguments)]
pub fn backend(
    bc_paths: &[std::path::PathBuf],
    ids: &[String],
    own_idx: usize,
    preserve: &[String],
    imports: &[ImportEdge],
    exports: &[ExportEntry],
    target: &BuildTarget,
    profile: Profile,
    out_obj: &Path,
) -> Result<(), CodegenError> {
    assert_eq!(bc_paths.len(), ids.len(), "one id per bc path");
    assert!(own_idx < ids.len(), "own_idx in range");

    let c_paths = path_cstrs(bc_paths)?;
    let path_ptrs = ptr_vec(&c_paths);
    let c_ids = str_cstrs(ids)?;
    let id_ptrs = ptr_vec(&c_ids);
    let c_pres = str_cstrs(preserve)?;
    let pres_ptrs = ptr_vec(&c_pres);
    let c_own = cstr(&ids[own_idx])?;
    let c_out = path_cstr(out_obj)?;

    // Serialized import list for this unit (parallel arrays).
    let imp_src: Vec<CString> = imports.iter().map(|e| cstr(&e.src)).collect::<Result<_, _>>()?;
    let imp_src_ptrs = ptr_vec(&imp_src);
    let imp_guids: Vec<u64> = imports.iter().map(|e| e.guid).collect();
    let imp_kinds: Vec<c_int> = imports.iter().map(|e| e.is_definition as c_int).collect();

    // Serialized global export map (all modules — the promote/internalize step walks
    // the whole index).
    let exp_mods: Vec<CString> = exports.iter().map(|e| cstr(&e.module)).collect::<Result<_, _>>()?;
    let exp_mod_ptrs = ptr_vec(&exp_mods);
    let exp_guids: Vec<u64> = exports.iter().map(|e| e.guid).collect();

    // The driver's TargetMachine — kept alive across the FFI call.
    let tm = create_target_machine(target, profile.codegen_opt_level())?;

    let rc = unsafe {
        align_thinlto_backend(
            c_own.as_ptr(),
            path_ptrs.as_ptr(),
            id_ptrs.as_ptr(),
            path_ptrs.len(),
            own_idx as u32,
            pres_ptrs.as_ptr(),
            pres_ptrs.len(),
            imp_src_ptrs.as_ptr(),
            imp_guids.as_ptr(),
            imp_kinds.as_ptr(),
            imp_src_ptrs.len(),
            exp_mod_ptrs.as_ptr(),
            exp_guids.as_ptr(),
            exp_mod_ptrs.len(),
            tm.as_mut_ptr(),
            ir_opt_level(profile),
            c_out.as_ptr(),
        )
    };
    // Keep `tm` explicitly alive until after the call (it owns the LLVMTargetMachine
    // the shim borrows).
    drop(tm);
    if rc != 0 {
        return Err(CodegenError::Lowering(format!(
            "ThinLTO backend (unit '{}') failed (shim rc={rc})",
            ids[own_idx]
        )));
    }
    Ok(())
}

// ---- small conversion helpers --------------------------------------------

fn cstr(s: &str) -> Result<CString, CodegenError> {
    CString::new(s).map_err(|_| {
        CodegenError::Lowering(format!("interior NUL in ThinLTO identifier '{s}'"))
    })
}

fn path_cstr(p: &Path) -> Result<CString, CodegenError> {
    let s = p
        .to_str()
        .ok_or_else(|| CodegenError::Lowering(format!("non-UTF-8 path '{}'", p.display())))?;
    cstr(s)
}

fn str_cstrs(v: &[String]) -> Result<Vec<CString>, CodegenError> {
    v.iter().map(|s| cstr(s)).collect()
}

fn path_cstrs(v: &[std::path::PathBuf]) -> Result<Vec<CString>, CodegenError> {
    v.iter().map(|p| path_cstr(p)).collect()
}

fn ptr_vec(v: &[CString]) -> Vec<*const c_char> {
    v.iter().map(|c| c.as_ptr()).collect()
}
