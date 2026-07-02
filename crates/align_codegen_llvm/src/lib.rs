//! Backend: MIR -> LLVM IR -> object (`docs/impl/05-backend-llvm.md`).
//!
//! A pure-lowering stage. Align's semantic decisions (desugaring, fusion, SIMD-ization,
//! regions) are already done in MIR; this just maps MIR to LLVM IR mechanically and
//! does not recompute types (anti-rewrite, `00-overview.md`).
//!
//! M1 model: named locals are allocas (LLVM's mem2reg promotes them to SSA); reads are
//! loads, writes are stores; `if` is conditional branches; comparisons are `icmp`;
//! calls are `call`. The generated `main` is the C entry (crt0 calls it).

use std::collections::HashMap;
use std::path::Path;

use align_ast::{BinOp, UnOp};
use align_mir::{Block, Const, Function, Operand, Program, Rvalue, Slot, Stmt, Term, ValueId};
use align_sema::{payload_is_move, struct_is_move, ty_to_scalar, EnumDef, FloatTy, IntTy, Layout, Scalar, StructDef, TupleDef, Ty, scalar_to_ty, ERROR_VARIANT_CODE};

use inkwell::AddressSpace;
use inkwell::FloatPredicate;
use inkwell::IntPredicate;
use inkwell::OptimizationLevel;
use inkwell::basic_block::BasicBlock;
use inkwell::builder::Builder;
use inkwell::context::Context;
use inkwell::intrinsics::Intrinsic;
use inkwell::module::Module;
use inkwell::passes::PassBuilderOptions;
use inkwell::targets::{
    CodeModel, FileType, InitializationConfig, RelocMode, Target, TargetMachine,
};
use inkwell::types::{BasicMetadataTypeEnum, BasicType, BasicTypeEnum, FloatType, IntType, StructType};
use inkwell::values::{BasicValueEnum, FunctionValue, IntValue};

pub fn is_available() -> bool {
    true
}

#[derive(Debug)]
pub enum CodegenError {
    Lowering(String),
    Target(String),
}

impl std::fmt::Display for CodegenError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            CodegenError::Lowering(m) => write!(f, "lowering failed: {m}"),
            CodegenError::Target(m) => write!(f, "target/output failed: {m}"),
        }
    }
}

/// Which CPU the generated code targets. Align builds for the *cloud/container* reality — build once,
/// run on a varied fleet — so the default is a conservative, portable per-architecture baseline; a
/// host-specific build is opt-in (`draft.md` §3.4, `open-questions.md` "Build targets & portability").
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub enum BuildTarget {
    /// A safe, portable per-architecture baseline (the default): `x86-64-v2` on amd64 (≈2010+, no
    /// AVX), the generic `armv8-a` baseline on arm64. One binary runs across a mixed Intel / AMD /
    /// Graviton, feature-masked, or live-migrated fleet.
    #[default]
    Baseline,
    /// The build host's exact CPU + features (`--target-cpu native`): fastest on this machine, but
    /// the binary may `SIGILL` on a host with fewer features — opt-in, never for distribution.
    Native,
    /// An explicit LLVM CPU name (`--target-cpu x86-64-v3`): a portable performance tier you pick
    /// for a fleet you control (`x86-64-v3` = AVX2/FMA/BMI2, runs on any such host — the recommended
    /// server/container "fast" build). Features are derived from the CPU name.
    Cpu(String),
}

/// Build the `TargetMachine` for `target` — the one place that picks the CPU / feature string, so
/// the data-layout machine (`build_module`) and the emission machine (`write_object`) always agree.
fn create_target_machine(target: &BuildTarget) -> Result<TargetMachine, CodegenError> {
    Target::initialize_native(&InitializationConfig::default())
        .map_err(|e| CodegenError::Target(format!("native target init: {e}")))?;
    let triple = TargetMachine::get_default_triple();
    let t = Target::from_triple(&triple)
        .map_err(|e| CodegenError::Target(format!("triple resolution: {e}")))?;
    let (cpu, features) = match target {
        BuildTarget::Native => (
            TargetMachine::get_host_cpu_name().to_string(),
            TargetMachine::get_host_cpu_features().to_string(),
        ),
        BuildTarget::Cpu(name) => (name.clone(), String::new()),
        BuildTarget::Baseline => {
            let ts = triple.as_str().to_string_lossy().to_ascii_lowercase();
            // Conservative per-arch floor: bump amd64 to x86-64-v2 (still pre-AVX, so cloud-safe);
            // arm64 / anything else uses `generic` (= the architecture baseline, e.g. armv8-a).
            let cpu = if ts.starts_with("x86_64") || ts.starts_with("amd64") {
                "x86-64-v2"
            } else {
                "generic"
            };
            (cpu.to_string(), String::new())
        }
    };
    t.create_target_machine(
        &triple,
        &cpu,
        &features,
        OptimizationLevel::Default,
        RelocMode::PIC,
        CodeModel::Default,
    )
    .ok_or_else(|| CodegenError::Target("failed to create TargetMachine".to_string()))
}

/// Write the program as an object file.
pub fn emit_object(program: &Program, out: &Path, target: &BuildTarget) -> Result<(), CodegenError> {
    let ctx = Context::create();
    let module = ctx.create_module("align");
    build_module(&ctx, &module, program, target)?;
    write_object(&module, out, target)
}

/// Render the program as textual LLVM IR (`alignc emit-llvm`).
pub fn emit_llvm_ir(program: &Program, target: &BuildTarget) -> Result<String, CodegenError> {
    let ctx = Context::create();
    let module = ctx.create_module("align");
    build_module(&ctx, &module, program, target)?;
    Ok(module.print_to_string().to_string())
}

fn build_module<'c>(
    ctx: &'c Context,
    module: &Module<'c>,
    program: &Program,
    target: &BuildTarget,
) -> Result<(), CodegenError> {
    // Target layout (for struct field offsets in `json.decode`); also pin the module's data
    // layout so offsets match the emitted object.
    let tm = create_target_machine(target)?;
    let target_data = tm.get_target_data();
    module.set_data_layout(&target_data.get_data_layout());
    // Pin the target triple too, so emitted IR (`alignc emit-llvm`) is self-describing: an external
    // `opt`/`llc` reading it then knows the architecture and uses the right cost model / vectorizer
    // instead of falling back to a generic one. The driver's own object emission is unaffected (it
    // always drives `write_object` with the same TargetMachine).
    module.set_triple(&tm.get_triple());

    // Struct layouts → LLVM struct types, indexed by struct id. Two phases so a **nested** struct
    // field (`Struct(id)`) can reference another struct's type: first create every struct as a named
    // opaque type, then set each body (sema forbids non-`box` recursion, so the bodies are acyclic).
    let struct_types: Vec<StructType<'c>> =
        program.structs.iter().map(|s| ctx.opaque_struct_type(&s.name)).collect();
    // Field reordering (see `docs/impl/05-backend-llvm.md` §2): a non-`layout(C)` struct's field
    // order is language-unspecified, so codegen lays fields out in **descending alignment** (ties
    // keep declaration order) to eliminate padding. Source access is by name, so this is invisible.
    // `field_perm[sid][logical]` is the logical→physical index map — every field GEP / byte-offset
    // site must route the MIR (logical) field index through it. A `layout(C)` struct keeps
    // declaration order (identity map), so its byte layout — the FFI/`raw`/json boundary — is
    // unchanged.
    let field_perm: Vec<Vec<u32>> =
        program.structs.iter().map(|s| logical_to_physical(s, &program.structs)).collect();
    for ((s, st), perm) in program.structs.iter().zip(&struct_types).zip(&field_perm) {
        // `abi_type` maps each field (floats to their float type, `str` to the `{ ptr, len }` view,
        // a nested struct to its (now-created) struct type). Fields are emitted in physical order:
        // physical slot `p` holds the logical field whose map entry is `p`.
        let phys_order = physical_order(perm);
        let fields: Vec<BasicTypeEnum> =
            phys_order.iter().map(|&li| abi_type(ctx, s.fields[li as usize].ty, &struct_types, &[])).collect();
        st.set_body(&fields, false);
    }

    // Sum-type layouts → a non-union tagged struct `{ i32 tag, <every variant's payload flattened> }`,
    // indexed by enum id. Payloads are primitive scalars (S1b), so the tables are not consulted.
    let enum_types: Vec<StructType<'c>> = program
        .enums
        .iter()
        .map(|e| {
            let mut fields: Vec<BasicTypeEnum> = vec![ctx.i32_type().into()];
            for v in &e.variants {
                for &s in &v.payload {
                    fields.push(scalar_type(ctx, scalar_to_ty(s), &struct_types, &[]));
                }
            }
            ctx.struct_type(&fields, false)
        })
        .collect();

    // Tuple layouts → anonymous LLVM struct types, indexed by tuple id. Elements are primitive
    // scalars (PR1), so the struct-type table is not consulted here.
    let tuple_types: Vec<StructType<'c>> = program
        .tuples
        .iter()
        .map(|t| {
            let fields: Vec<BasicTypeEnum> =
                t.elems.iter().map(|s| scalar_type(ctx, scalar_to_ty(*s), &struct_types, &enum_types)).collect();
            ctx.struct_type(&fields, false)
        })
        .collect();

    // Pass 1: declare all functions so calls resolve regardless of order. A
    // `Result`-returning `main` is emitted under `align_main`; a C `main` wrapper is
    // generated after the bodies (see below).
    let mut funcs: HashMap<String, FunctionValue<'c>> = HashMap::new();
    for f in &program.fns {
        let fv = declare_fn(ctx, module, f, symbol_name(f), &struct_types, &enum_types, &tuple_types);
        funcs.insert(f.name.clone(), fv);
    }
    // Declare foreign (`extern "C"`) functions under their C symbol, so a `Rvalue::Call` keyed by
    // that name resolves. FFI-safe params/returns are scalars or `raw` (an opaque pointer) — all
    // covered by `abi_type`; a `str`/`slice` view param is passed as its data `ptr` (C `char*`).
    // No `mark_nounwind`: unlike an Align function, foreign code is outside our control, so we do not
    // assert it never unwinds.
    for ext in &program.externs {
        let param_types: Vec<BasicMetadataTypeEnum> = ext
            .params
            .iter()
            .map(|&ty| ffi_param_type(ctx, ty, &struct_types, &enum_types).into())
            .collect();
        let fn_ty = if ext.ret == Ty::Unit {
            ctx.void_type().fn_type(&param_types, false)
        } else {
            abi_type(ctx, ext.ret, &struct_types, &enum_types).fn_type(&param_types, false)
        };
        // Defensive: if the symbol is already in the module (e.g. it coincides with a symbol
        // declared earlier), reuse that declaration. A fresh `add_function` on a duplicate name
        // makes LLVM silently rename it (`@abs.1`), which then fails to link against the real
        // external symbol.
        let fv = module.get_function(&ext.name).unwrap_or_else(|| module.add_function(&ext.name, fn_ty, None));
        funcs.insert(ext.name.clone(), fv);
    }
    // Declare runtime builtins, keyed by the MIR call name they back.
    let print_ty = ctx.void_type().fn_type(&[ctx.i64_type().into()], false);
    funcs.insert(
        "print".to_string(),
        module.add_function("align_rt_print_i64", print_ty, None),
    );
    // Out-of-bounds index failure: report `(index, len)` and abort (`-> !`).
    funcs.insert(
        "bounds_fail".to_string(),
        module.add_function(
            "align_rt_bounds_fail",
            ctx.void_type().fn_type(&[ctx.i64_type().into(), ctx.i64_type().into()], false),
            None,
        ),
    );
    // Out-of-bounds range-slice failure: report `(start, end, len)` and abort (`-> !`).
    funcs.insert(
        "range_fail".to_string(),
        module.add_function(
            "align_rt_range_fail",
            ctx.void_type().fn_type(&[ctx.i64_type().into(), ctx.i64_type().into(), ctx.i64_type().into()], false),
            None,
        ),
    );
    // Integer division/remainder by zero: report and abort (`-> !`). Codegen emits the
    // `divisor == 0` guard inline (see MIR `lower_int_div`) and calls this on the failing path.
    funcs.insert(
        "div_fail".to_string(),
        module.add_function("align_rt_div_fail", ctx.void_type().fn_type(&[], false), None),
    );
    // Arena allocator (M3).
    let ptr = ctx.ptr_type(AddressSpace::default());
    let i64t = ctx.i64_type();
    let arena_begin = module.add_function("align_rt_arena_begin", ptr.fn_type(&[], false), None);
    mark_alloc_like(ctx, arena_begin);
    funcs.insert("arena_begin".to_string(), arena_begin);
    let arena_alloc = module.add_function(
        "align_rt_arena_alloc",
        ptr.fn_type(&[ptr.into(), i64t.into(), i64t.into()], false),
        None,
    );
    mark_bump_alloc(ctx, arena_alloc);
    funcs.insert("arena_alloc".to_string(), arena_alloc);
    funcs.insert(
        "arena_end".to_string(),
        module.add_function(
            "align_rt_arena_end",
            ctx.void_type().fn_type(&[ptr.into()], false),
            None,
        ),
    );
    // `task_group` runtime (slice ④b).
    let tg_begin = module.add_function("align_rt_tg_begin", ptr.fn_type(&[], false), None);
    mark_alloc_like(ctx, tg_begin);
    funcs.insert("tg_begin".to_string(), tg_begin);
    let tg_alloc = module.add_function(
        "align_rt_tg_alloc",
        ptr.fn_type(&[ptr.into(), i64t.into(), i64t.into()], false),
        None,
    );
    mark_bump_alloc(ctx, tg_alloc);
    funcs.insert("tg_alloc".to_string(), tg_alloc);
    funcs.insert(
        "tg_register".to_string(),
        module.add_function(
            "align_rt_tg_register",
            // (tg, tramp, thunk, env, slot, err_slot)
            ctx.void_type().fn_type(&[ptr.into(), ptr.into(), ptr.into(), ptr.into(), ptr.into(), ptr.into()], false),
            None,
        ),
    );
    funcs.insert(
        "tg_wait".to_string(),
        // Returns the first errored task's `err_slot` pointer (null if all succeeded).
        module.add_function("align_rt_tg_wait", ptr.fn_type(&[ptr.into()], false), None),
    );
    funcs.insert(
        "tg_end".to_string(),
        module.add_function("align_rt_tg_end", ctx.void_type().fn_type(&[ptr.into()], false), None),
    );
    // Free-standing heap allocation for owned arrays (MMv2 slice 4).
    let alloc = module.add_function("align_rt_alloc", ptr.fn_type(&[i64t.into()], false), None);
    mark_alloc_like(ctx, alloc);
    funcs.insert("alloc".to_string(), alloc);
    funcs.insert(
        "free".to_string(),
        module.add_function("align_rt_free", ctx.void_type().fn_type(&[ptr.into()], false), None),
    );
    // `chunks(n)`: (src_ptr, src_len, n, elem_size) -> { chunk_buf, count } (a `{ptr,len}`).
    funcs.insert(
        "chunks".to_string(),
        module.add_function(
            "align_rt_chunks",
            slice_struct_type(ctx).fn_type(&[ptr.into(), i64t.into(), i64t.into(), i64t.into()], false),
            None,
        ),
    );
    // `par_map`: (in_buf, count, in_stride, out_stride, thunk) -> out_buf. Allocates the output,
    // applies the per-function thunk to each element across threads, returns the owned buffer.
    let par_map = module.add_function(
        "align_rt_par_map",
        ptr.fn_type(&[ptr.into(), i64t.into(), i64t.into(), i64t.into(), ptr.into()], false),
        None,
    );
    // Only `noalias` on the return: the output buffer is a fresh allocation disjoint from the inputs.
    // NOT `nounwind` (it may `resume_unwind` a worker panic) and NOT `nofree` (it invokes the user
    // thunk, so we don't assert anything about what that does).
    add_enum_attr(ctx, par_map, inkwell::attributes::AttributeLoc::Return, "noalias");
    funcs.insert("par_map".to_string(), par_map);
    funcs.insert(
        "print_str".to_string(),
        module.add_function(
            "align_rt_print_str",
            ctx.void_type().fn_type(&[ptr.into(), ctx.i64_type().into()], false),
            None,
        ),
    );
    funcs.insert(
        "print_bool".to_string(),
        module.add_function(
            "align_rt_print_bool",
            ctx.void_type().fn_type(&[ctx.i32_type().into()], false),
            None,
        ),
    );
    funcs.insert(
        "print_char".to_string(),
        module.add_function(
            "align_rt_print_char",
            ctx.void_type().fn_type(&[ctx.i32_type().into()], false),
            None,
        ),
    );
    funcs.insert(
        "print_f32".to_string(),
        module.add_function(
            "align_rt_print_f32",
            ctx.void_type().fn_type(&[ctx.f32_type().into()], false),
            None,
        ),
    );
    funcs.insert(
        "print_f64".to_string(),
        module.add_function(
            "align_rt_print_f64",
            ctx.void_type().fn_type(&[ctx.f64_type().into()], false),
            None,
        ),
    );
    funcs.insert(
        "str_eq".to_string(),
        module.add_function(
            "align_rt_str_eq",
            ctx.i32_type().fn_type(
                &[ptr.into(), ctx.i64_type().into(), ptr.into(), ctx.i64_type().into()],
                false,
            ),
            None,
        ),
    );
    // `core.string` byte predicates — all share the `i32 (ptr, i64, ptr, i64)` signature of `str_eq`.
    for (key, sym) in [
        ("str_contains", "align_rt_str_contains"),
        ("str_starts_with", "align_rt_str_starts_with"),
        ("str_ends_with", "align_rt_str_ends_with"),
    ] {
        funcs.insert(
            key.to_string(),
            module.add_function(
                sym,
                ctx.i32_type().fn_type(
                    &[ptr.into(), ctx.i64_type().into(), ptr.into(), ctx.i64_type().into()],
                    false,
                ),
                None,
            ),
        );
    }
    // `s.find(needle)` / `s.rfind(needle)` → the byte index (i64) or -1 (→ Option<i64>); same args.
    for (key, sym) in [("str_find", "align_rt_str_find"), ("str_rfind", "align_rt_str_rfind")] {
        funcs.insert(
            key.to_string(),
            module.add_function(
                sym,
                ctx.i64_type().fn_type(
                    &[ptr.into(), ctx.i64_type().into(), ptr.into(), ctx.i64_type().into()],
                    false,
                ),
                None,
            ),
        );
    }
    // `s.eq_ignore_ascii_case(other)` → i32 (0/1), the predicate arg shape.
    funcs.insert(
        "str_eq_ignore_case".to_string(),
        module.add_function(
            "align_rt_str_eq_ignore_case",
            ctx.i32_type().fn_type(
                &[ptr.into(), ctx.i64_type().into(), ptr.into(), ctx.i64_type().into()],
                false,
            ),
            None,
        ),
    );
    // String builder (M5: `template` desugaring).
    let i64t2 = ctx.i64_type();
    let builder_new =
        module.add_function("align_rt_builder_new", ptr.fn_type(&[ptr.into(), i64t2.into()], false), None);
    mark_alloc_like(ctx, builder_new);
    funcs.insert("builder_new".to_string(), builder_new);
    funcs.insert(
        "builder_write".to_string(),
        module.add_function(
            "align_rt_builder_write",
            ctx.void_type().fn_type(&[ptr.into(), ptr.into(), i64t2.into()], false),
            None,
        ),
    );
    funcs.insert(
        "builder_write_int".to_string(),
        module.add_function(
            "align_rt_builder_write_int",
            ctx.void_type().fn_type(&[ptr.into(), i64t2.into()], false),
            None,
        ),
    );
    funcs.insert(
        "builder_write_str_int_str".to_string(),
        module.add_function(
            "align_rt_builder_write_str_int_str",
            ctx.void_type().fn_type(
                &[ptr.into(), ptr.into(), i64t2.into(), i64t2.into(), ptr.into(), i64t2.into()],
                false,
            ),
            None,
        ),
    );
    funcs.insert(
        "builder_write_bool".to_string(),
        module.add_function(
            "align_rt_builder_write_bool",
            ctx.void_type().fn_type(&[ptr.into(), ctx.i32_type().into()], false),
            None,
        ),
    );
    funcs.insert(
        "builder_write_char".to_string(),
        module.add_function(
            "align_rt_builder_write_char",
            ctx.void_type().fn_type(&[ptr.into(), ctx.i32_type().into()], false),
            None,
        ),
    );
    funcs.insert(
        "builder_write_f32".to_string(),
        module.add_function(
            "align_rt_builder_write_f32",
            ctx.void_type().fn_type(&[ptr.into(), ctx.f32_type().into()], false),
            None,
        ),
    );
    funcs.insert(
        "builder_write_f64".to_string(),
        module.add_function(
            "align_rt_builder_write_f64",
            ctx.void_type().fn_type(&[ptr.into(), ctx.f64_type().into()], false),
            None,
        ),
    );
    funcs.insert(
        "builder_write_json_str".to_string(),
        module.add_function(
            "align_rt_builder_write_json_str",
            ctx.void_type().fn_type(&[ptr.into(), ptr.into(), ctx.i64_type().into()], false),
            None,
        ),
    );
    funcs.insert(
        // json.decode(input, fields, n, out, out_size, phf, phf_len, phf_seed) -> i32 status
        // (0 = ok). The trailing 3 args are the compile-time perfect-hash field table (`phf_len = 0`
        // → linear scan).
        "json_decode".to_string(),
        module.add_function(
            "align_rt_json_decode",
            ctx.i32_type().fn_type(
                &[ptr.into(), i64t2.into(), ptr.into(), i64t2.into(), ptr.into(), i64t2.into(), ptr.into(), i64t2.into(), i64t2.into()],
                false,
            ),
            None,
        ),
    );
    funcs.insert(
        // json.decode into array (input, input_len, elem_tag, out: *{ptr,len}) -> i32 status.
        "json_decode_array".to_string(),
        module.add_function(
            "align_rt_json_decode_array",
            ctx.i32_type().fn_type(
                &[ptr.into(), i64t2.into(), ctx.i32_type().into(), ptr.into()],
                false,
            ),
            None,
        ),
    );
    funcs.insert(
        // fs.read_file (path_ptr, path_len, out: *{ptr,len}) -> i32 status (std.fs).
        "fs_read_file".to_string(),
        module.add_function(
            "align_rt_fs_read_file",
            ctx.i32_type().fn_type(&[ptr.into(), i64t2.into(), ptr.into()], false),
            None,
        ),
    );
    funcs.insert(
        // io.stdout.write (ptr, len) -> i32 status (std.io); writes bytes, no newline.
        "io_stdout_write".to_string(),
        module.add_function(
            "align_rt_io_stdout_write",
            ctx.i32_type().fn_type(&[ptr.into(), i64t2.into()], false),
            None,
        ),
    );
    funcs.insert(
        // io.stdout.write(builder) (b: *Builder) -> i32 status; writes the builder's bytes.
        "io_stdout_write_builder".to_string(),
        module.add_function(
            "align_rt_io_stdout_write_builder",
            ctx.i32_type().fn_type(&[ptr.into()], false),
            None,
        ),
    );
    // io.stdout.buffered() — the buffered stdout writer (std.io).
    funcs.insert(
        // io.stdout.buffered() / io.stderr.buffered(): (fd: i32) -> *BufferedWriter (opaque handle).
        "io_buf_new".to_string(),
        module.add_function("align_rt_io_buf_new", ptr.fn_type(&[ctx.i32_type().into()], false), None),
    );
    funcs.insert(
        // w.write(s) (w: *StdoutWriter, ptr, len) -> void; appends, flushing only when full.
        "io_buf_write".to_string(),
        module.add_function(
            "align_rt_io_buf_write",
            ctx.void_type().fn_type(&[ptr.into(), ptr.into(), i64t2.into()], false),
            None,
        ),
    );
    funcs.insert(
        // w.flush() (w: *StdoutWriter) -> i32 status; drains the buffer to the OS.
        "io_buf_flush".to_string(),
        module.add_function("align_rt_io_buf_flush", ctx.i32_type().fn_type(&[ptr.into()], false), None),
    );
    funcs.insert(
        // drop(w) (w: *StdoutWriter) -> void; final flush + free.
        "io_buf_free".to_string(),
        module.add_function("align_rt_io_buf_free", ctx.void_type().fn_type(&[ptr.into()], false), None),
    );
    funcs.insert(
        // json.decode into array<Struct> (input, input_len, fields, n, elem_size, out: *{ptr,len},
        // phf, phf_len, phf_seed) -> i32 status (MMv2 slice 8d; trailing 3 = perfect-hash table).
        "json_decode_struct_array".to_string(),
        module.add_function(
            "align_rt_json_decode_struct_array",
            ctx.i32_type().fn_type(
                &[ptr.into(), i64t2.into(), ptr.into(), i64t2.into(), i64t2.into(), ptr.into(), ptr.into(), i64t2.into(), i64t2.into()],
                false,
            ),
            None,
        ),
    );
    funcs.insert(
        // json.decode directly into soa<Struct> (input, input_len, fields, n, arena, out: *{ptr,len},
        // phf, phf_len, phf_seed) -> i32 status. Direct-fill rail: the runtime counts rows, arena-
        // allocates the columns, and fills them (no AoS / transpose). `arena` replaces `elem_size`.
        "json_decode_soa".to_string(),
        module.add_function(
            "align_rt_json_decode_soa",
            ctx.i32_type().fn_type(
                &[ptr.into(), i64t2.into(), ptr.into(), i64t2.into(), ptr.into(), ptr.into(), ptr.into(), i64t2.into(), i64t2.into()],
                false,
            ),
            None,
        ),
    );
    funcs.insert(
        "builder_finish".to_string(),
        module.add_function(
            "align_rt_builder_finish",
            slice_struct_type(ctx).fn_type(&[ptr.into()], false),
            None,
        ),
    );
    // group_by(.key).{sum,min,max}(.value): (keys, vals, len, out_keys, out_vals, cap) -> group count.
    let group_vty = ctx.i64_type().fn_type(&[ptr.into(), ptr.into(), i64t2.into(), ptr.into(), ptr.into(), i64t2.into()], false);
    for (key, sym) in [
        ("group_sum_i64", "align_rt_group_sum_i64"),
        ("group_min_i64", "align_rt_group_min_i64"),
        ("group_max_i64", "align_rt_group_max_i64"),
    ] {
        funcs.insert(key.to_string(), module.add_function(sym, group_vty, None));
    }
    funcs.insert(
        // group_by(.key).count(): (keys, len, out_keys, out_vals, cap) -> group count (no value col).
        "group_count_i64".to_string(),
        module.add_function(
            "align_rt_group_count_i64",
            ctx.i64_type().fn_type(&[ptr.into(), i64t2.into(), ptr.into(), ptr.into(), i64t2.into()], false),
            None,
        ),
    );
    // group_by(.str_key).{sum,min,max}(.i64_value) / .count() over an AoS array<Struct> — the
    // dictionary-id rail: (base, n, stride, key_off, val_off, out_keys, out_vals, cap) -> group count.
    let group_str_ty = ctx.i64_type().fn_type(
        &[ptr.into(), i64t2.into(), i64t2.into(), i64t2.into(), i64t2.into(), ptr.into(), ptr.into(), i64t2.into()],
        false,
    );
    for (key, sym) in [
        ("group_sum_str", "align_rt_group_sum_str"),
        ("group_min_str", "align_rt_group_min_str"),
        ("group_max_str", "align_rt_group_max_str"),
        ("group_count_str", "align_rt_group_count_str"),
    ] {
        funcs.insert(key.to_string(), module.add_function(sym, group_str_ty, None));
    }
    // group_by(.str_key).{sum,min,max}(.i64_value) / .count() over a soa<Struct> with a str key
    // column — the two-contiguous-column form: (key_col, val_col, n, out_keys, out_vals, cap) ->
    // count. Same 6-arg shape as the i64 `group_vty`; all four ops share it (count ignores val_col).
    for (key, sym) in [
        ("group_sum_str_cols", "align_rt_group_sum_str_cols"),
        ("group_min_str_cols", "align_rt_group_min_str_cols"),
        ("group_max_str_cols", "align_rt_group_max_str_cols"),
        ("group_count_str_cols", "align_rt_group_count_str_cols"),
    ] {
        funcs.insert(key.to_string(), module.add_function(sym, group_vty, None));
    }
    // Fused multi-aggregate str group-by: (base, n, stride, key_off, specs, k, out_keys, cap) -> count.
    // `specs` is a `[k x {i64 val_off, i64 op, ptr out_vals}]` table built at the call site.
    funcs.insert(
        "group_multi_str".to_string(),
        module.add_function(
            "align_rt_group_multi_str",
            ctx.i64_type().fn_type(
                &[ptr.into(), i64t2.into(), i64t2.into(), i64t2.into(), ptr.into(), i64t2.into(), ptr.into(), i64t2.into()],
                false,
            ),
            None,
        ),
    );
    // A2 dictionary reuse rail. `dict_encode`: (base, n, stride, key_off, out_ids, out_dict, cap) ->
    // dict size. `gather_i64`: (base, n, stride, off, out) -> (). `dict_lookup`: (ids, n, dict,
    // dict_len, out) -> ().
    funcs.insert(
        "dict_encode_str".to_string(),
        module.add_function(
            "align_rt_dict_encode_str",
            ctx.i64_type().fn_type(&[ptr.into(), i64t2.into(), i64t2.into(), i64t2.into(), ptr.into(), ptr.into(), i64t2.into()], false),
            None,
        ),
    );
    funcs.insert(
        "gather_i64".to_string(),
        module.add_function(
            "align_rt_gather_i64",
            ctx.void_type().fn_type(&[ptr.into(), i64t2.into(), i64t2.into(), i64t2.into(), ptr.into()], false),
            None,
        ),
    );
    funcs.insert(
        "dict_lookup".to_string(),
        module.add_function(
            "align_rt_dict_lookup",
            ctx.void_type().fn_type(&[ptr.into(), i64t2.into(), ptr.into(), i64t2.into(), ptr.into()], false),
            None,
        ),
    );
    // `str.clone()` → deep-copy into a heap-owned `string` `{ptr,len}` (MMv2 slice 7).
    funcs.insert(
        "str_clone".to_string(),
        module.add_function(
            "align_rt_str_clone",
            slice_struct_type(ctx).fn_type(&[ptr.into(), ctx.i64_type().into()], false),
            None,
        ),
    );
    // `core.hash` — wyhash over a byte view `{ptr,len}`. `hash64` → u64; `hash128` → {u64,u64}
    // returned by value (matching the `(u64,u64)` tuple struct, like `str_clone`'s `{ptr,len}`).
    funcs.insert(
        "hash64".to_string(),
        module.add_function(
            "align_rt_hash64",
            i64t.fn_type(&[ptr.into(), i64t.into()], false),
            None,
        ),
    );
    funcs.insert(
        "hash128".to_string(),
        module.add_function(
            "align_rt_hash128",
            ctx.struct_type(&[i64t.into(), i64t.into()], false).fn_type(&[ptr.into(), i64t.into()], false),
            None,
        ),
    );
    // `core.string` trims → a borrowed sub-`str` `{ptr,len}` of the receiver (no allocation).
    for (key, sym) in [
        ("str_trim", "align_rt_str_trim"),
        ("str_trim_start", "align_rt_str_trim_start"),
        ("str_trim_end", "align_rt_str_trim_end"),
    ] {
        funcs.insert(
            key.to_string(),
            module.add_function(
                sym,
                slice_struct_type(ctx).fn_type(&[ptr.into(), ctx.i64_type().into()], false),
                None,
            ),
        );
    }
    // Surface `builder` (MMv2 slice 7c): `to_string()` finishes into an owned `string`; `free`
    // drops an unfinished builder at scope exit.
    funcs.insert(
        "builder_into_string".to_string(),
        module.add_function(
            "align_rt_builder_into_string",
            slice_struct_type(ctx).fn_type(&[ptr.into()], false),
            None,
        ),
    );
    funcs.insert(
        "builder_free".to_string(),
        module.add_function(
            "align_rt_builder_free",
            ctx.void_type().fn_type(&[ptr.into()], false),
            None,
        ),
    );
    // Pass 1b: emit a thunk for each function used as a value (`FnValue`/`FnAddr`). A closure
    // value has the env-ABI `fn(env, args)`; a non-capturing / named function is wrapped by
    // `name$fnval(env, args) = name(args)` so all closure callees share that ABI (the env pointer
    // is null and ignored). Capturing closures (a later slice) instead point at an env-reading fn.
    let mut thunk_names: std::collections::BTreeSet<String> = std::collections::BTreeSet::new();
    for f in &program.fns {
        for b in &f.blocks {
            for s in &b.stmts {
                if let Stmt::Let(_, Rvalue::FnAddr(name)) = s {
                    thunk_names.insert(name.clone());
                }
            }
        }
    }
    for name in &thunk_names {
        let orig = *funcs
            .get(name)
            .ok_or_else(|| CodegenError::Lowering(format!("unknown function {name}")))?;
        let orig_ty = orig.get_type();
        let mut params: Vec<BasicMetadataTypeEnum> = vec![ptr.into()];
        params.extend(orig_ty.get_param_types().iter().copied());
        let thunk_ty = match orig_ty.get_return_type() {
            Some(rt) => rt.fn_type(&params, false),
            None => ctx.void_type().fn_type(&params, false),
        };
        let thunk = module.add_function(&format!("{name}$fnval"), thunk_ty, None);
        mark_nounwind(ctx, thunk);
        let bb = ctx.append_basic_block(thunk, "entry");
        let tb = ctx.create_builder();
        tb.position_at_end(bb);
        let fwd: Vec<inkwell::values::BasicMetadataValueEnum> =
            thunk.get_params().iter().skip(1).map(|p| (*p).into()).collect();
        let cs = tb.build_call(orig, &fwd, "r").map_err(|e| CodegenError::Lowering(e.to_string()))?;
        match cs.try_as_basic_value().basic() {
            Some(v) => tb.build_return(Some(&v)),
            None => tb.build_return(None),
        }
        .map_err(|e| CodegenError::Lowering(e.to_string()))?;
        funcs.insert(format!("{name}$fnval"), thunk);
    }

    // Pass 1c: a closure thunk per lifted function used as a *capturing* closure. The env-ABI
    // thunk `lifted$clos(env, explicit…)` loads the captured values out of `env` and forwards them
    // as the lifted function's trailing capture parameters: `lifted(explicit…, env.0, env.1, …)`.
    let mut closure_thunks: std::collections::BTreeMap<String, Vec<Ty>> = std::collections::BTreeMap::new();
    for f in &program.fns {
        for b in &f.blocks {
            for s in &b.stmts {
                if let Stmt::Let(_, Rvalue::Closure { lifted, capture_tys, .. }) = s {
                    closure_thunks.entry(lifted.clone()).or_insert_with(|| capture_tys.clone());
                }
            }
        }
    }
    for (lifted, capture_tys) in &closure_thunks {
        let orig = *funcs
            .get(lifted)
            .ok_or_else(|| CodegenError::Lowering(format!("unknown lifted function {lifted}")))?;
        let orig_ty = orig.get_type();
        let all_params = orig_ty.get_param_types();
        let n_explicit = all_params.len().checked_sub(capture_tys.len()).ok_or_else(|| {
            CodegenError::Lowering(format!(
                "lifted function {lifted} has {} parameters, fewer than its {} captures",
                all_params.len(),
                capture_tys.len()
            ))
        })?;
        let mut tparams: Vec<BasicMetadataTypeEnum> = vec![ptr.into()];
        tparams.extend(all_params[..n_explicit].iter().copied());
        let thunk_ty = match orig_ty.get_return_type() {
            Some(rt) => rt.fn_type(&tparams, false),
            None => ctx.void_type().fn_type(&tparams, false),
        };
        let thunk = module.add_function(&format!("{lifted}$clos"), thunk_ty, None);
        mark_nounwind(ctx, thunk);
        let bb = ctx.append_basic_block(thunk, "entry");
        let tb = ctx.create_builder();
        tb.position_at_end(bb);
        let env = thunk.get_nth_param(0).unwrap().into_pointer_value();
        let env_fields: Vec<BasicTypeEnum> = capture_tys.iter().map(|t| abi_type(ctx, *t, &struct_types, &enum_types)).collect();
        let env_struct = ctx.struct_type(&env_fields, false);
        // The explicit parameters are forwarded as-is; the captures are loaded from the env.
        let mut call_args: Vec<inkwell::values::BasicMetadataValueEnum> =
            thunk.get_params().iter().skip(1).map(|p| (*p).into()).collect();
        for (i, cty) in capture_tys.iter().enumerate() {
            let fld = tb
                .build_struct_gep(env_struct, env, i as u32, "capg")
                .map_err(|e| CodegenError::Lowering(e.to_string()))?;
            let v = tb
                .build_load(abi_type(ctx, *cty, &struct_types, &enum_types), fld, "capv")
                .map_err(|e| CodegenError::Lowering(e.to_string()))?;
            call_args.push(v.into());
        }
        let cs = tb.build_call(orig, &call_args, "r").map_err(|e| CodegenError::Lowering(e.to_string()))?;
        match cs.try_as_basic_value().basic() {
            Some(v) => tb.build_return(Some(&v)),
            None => tb.build_return(None),
        }
        .map_err(|e| CodegenError::Lowering(e.to_string()))?;
        funcs.insert(format!("{lifted}$clos"), thunk);
    }

    // Pass 1d: a `spawn` trampoline per result type `R`. `tramp$R(thunk, env, slot)` runs the
    // spawned closure (`thunk(env) -> R`) and stores the result into `slot` (the typed store is
    // why it is generated, not in the runtime). ④b-1 calls it sequentially at `wait`; ④b-2 runs
    // it on a worker thread.
    // A trampoline per (result type `R`, fallibility): `tramp(thunk, env, slot) -> i32` runs the
    // spawned closure and writes its result into `slot`, returning an error code (`0` = ok). A
    // fallible closure returns `Result<R, Error>`; the trampoline stores the `Ok` payload and
    // returns `0`, or returns the `Err` code (which `tg_wait` surfaces to `wait()?`).
    let lower = |e: inkwell::builder::BuilderError| CodegenError::Lowering(e.to_string());
    let i32t = ctx.i32_type();
    let mut tramp_keys: std::collections::BTreeMap<String, (Ty, bool)> = std::collections::BTreeMap::new();
    for f in &program.fns {
        for b in &f.blocks {
            for s in &b.stmts {
                if let Stmt::Let(_, Rvalue::SpawnTask { r, fallible, .. }) = s {
                    tramp_keys.insert(spawn_tramp_key(*r, *fallible), (*r, *fallible));
                }
            }
        }
    }
    // The builtin `Error` enum id (always registered), for fallible trampolines.
    let error_id = program.enums.iter().position(|e| e.name == "Error").map(|i| i as u32);
    for (key, (r, fallible)) in &tramp_keys {
        // `tramp(thunk, env, slot, err_slot) -> i32` (0 = ok, 1 = errored).
        let fn_ty = i32t.fn_type(&[ptr.into(), ptr.into(), ptr.into(), ptr.into()], false);
        let tramp = module.add_function(&format!("tramp${key}"), fn_ty, None);
        mark_nounwind(ctx, tramp);
        let bb = ctx.append_basic_block(tramp, "entry");
        let tb = ctx.create_builder();
        tb.position_at_end(bb);
        let thunk = tramp.get_nth_param(0).unwrap().into_pointer_value();
        let env = tramp.get_nth_param(1).unwrap();
        let slot = tramp.get_nth_param(2).unwrap().into_pointer_value();
        let err_slot = tramp.get_nth_param(3).unwrap().into_pointer_value();
        if *fallible {
            // The closure returns `Result<R, Error>` = `{ i8 tag, R ok, Error err }` (tag 0 = Ok).
            // On `Err`, write the full `Error` value to `err_slot` and return 1; on `Ok`, write R.
            let ok_s = ty_to_scalar(*r).ok_or_else(|| CodegenError::Lowering("fallible task Ok is not a scalar".into()))?;
            let err_s = Scalar::Enum(error_id.ok_or_else(|| CodegenError::Lowering("Error enum not registered".into()))?);
            let result_ty = result_struct_type(ctx, ok_s, err_s, &struct_types, &enum_types);
            let agg = tb
                .build_indirect_call(result_ty.fn_type(&[ptr.into()], false), thunk, &[env.into()], "r")
                .map_err(lower)?
                .try_as_basic_value()
                .basic()
                .ok_or_else(|| CodegenError::Lowering("spawn closure returned no value".into()))?
                .into_struct_value();
            let tag = tb.build_extract_value(agg, 0, "tag").map_err(lower)?.into_int_value();
            let ok = tb.build_extract_value(agg, 1, "ok").map_err(lower)?;
            let err = tb.build_extract_value(agg, 2, "err").map_err(lower)?;
            let is_err = tb
                .build_int_compare(IntPredicate::EQ, tag, ctx.i8_type().const_int(1, false), "iserr")
                .map_err(lower)?;
            let err_bb = ctx.append_basic_block(tramp, "err");
            let ok_bb = ctx.append_basic_block(tramp, "ok");
            tb.build_conditional_branch(is_err, err_bb, ok_bb).map_err(lower)?;
            tb.position_at_end(err_bb);
            tb.build_store(err_slot, err).map_err(lower)?;
            tb.build_return(Some(&i32t.const_int(1, false))).map_err(lower)?;
            tb.position_at_end(ok_bb);
            tb.build_store(slot, ok).map_err(lower)?;
            tb.build_return(Some(&i32t.const_zero())).map_err(lower)?;
        } else if *r == Ty::Unit {
            // A `()`-returning closure is `void(ptr)` in LLVM (not `i32(ptr)`); call it with a void
            // signature and store a dummy into the (i32-sized) slot.
            tb.build_indirect_call(ctx.void_type().fn_type(&[ptr.into()], false), thunk, &[env.into()], "")
                .map_err(lower)?;
            tb.build_store(slot, i32t.const_zero()).map_err(lower)?;
            tb.build_return(Some(&i32t.const_zero())).map_err(lower)?;
        } else {
            let rt = scalar_type(ctx, *r, &struct_types, &enum_types);
            let res = tb
                .build_indirect_call(rt.fn_type(&[ptr.into()], false), thunk, &[env.into()], "r")
                .map_err(lower)?
                .try_as_basic_value()
                .basic()
                .ok_or_else(|| CodegenError::Lowering("spawn closure returned no value".into()))?;
            tb.build_store(slot, res).map_err(lower)?;
            tb.build_return(Some(&i32t.const_zero())).map_err(lower)?;
        }
        funcs.insert(format!("tramp${key}"), tramp);
    }

    // Extern parameter types by C symbol — so a call to an extern can coerce a `str`/`slice` arg to
    // its data pointer (the declaration already uses `ptr` for those params).
    let extern_params: HashMap<String, Vec<Ty>> =
        program.externs.iter().map(|e| (e.name.clone(), e.params.clone())).collect();

    // Pass 2: define bodies.
    for f in &program.fns {
        let builder = ctx.create_builder();
        FnGen {
            ctx,
            module,
            builder: &builder,
            funcs: &funcs,
            extern_params: &extern_params,
            structs: &program.structs,
            struct_types: &struct_types,
            field_perm: &field_perm,
            enum_types: &enum_types,
            enums: &program.enums,
            tuple_types: &tuple_types,
            tuples: &program.tuples,
            target_data: &target_data,
            f,
            func: funcs[&f.name],
            slots: HashMap::new(),
            values: HashMap::new(),
            blocks: Vec::new(),
        }
        .emit_fn()?;
    }
    // A Result-returning main needs a C `main` wrapper that maps Ok/Err to an exit code (and, when
    // `main(args: array<str>)`, marshals argv into the `array<str>` argument).
    if let Some(f) = program.fns.iter().find(|f| f.name == "main" && matches!(f.ret, Ty::Result(..))) {
        emit_main_wrapper(ctx, module, funcs["main"], f.ret, !f.params.is_empty())?;
    }
    Ok(())
}

/// The LLVM symbol for a function: a `Result`-returning `main` is emitted as
/// `align_main` (the C `main` is a generated wrapper); everything else keeps its name.
fn symbol_name(f: &Function) -> &str {
    if f.name == "main" && matches!(f.ret, Ty::Result(..)) {
        "align_main"
    } else {
        &f.name
    }
}

/// Emit the C `main` for a `Result<(), Error>`-returning Align `main`: call it, and on
/// `Err(code)` report the error and exit with `code`, else exit 0.
fn emit_main_wrapper<'c>(
    ctx: &'c Context,
    module: &Module<'c>,
    align_main: FunctionValue<'c>,
    ret: Ty,
    has_args: bool,
) -> Result<(), CodegenError> {
    if !matches!(ret, Ty::Result(_, _)) {
        return Err(CodegenError::Lowering("main wrapper on a non-Result".into()));
    }
    let lower = |e: inkwell::builder::BuilderError| CodegenError::Lowering(e.to_string());
    let i32t = ctx.i32_type();
    let ptr_t = ctx.ptr_type(AddressSpace::default());
    // Returns the clamped (nonzero u8) exit code; reporting/clamping live in the runtime.
    let report = module.add_function(
        "align_rt_report_error",
        i32t.fn_type(&[i32t.into()], false),
        None,
    );
    // `main(args: array<str>)`: the C entry takes (argc, argv) and the runtime builds the
    // `array<str>` value; otherwise the C entry takes no args.
    let main_ty = if has_args {
        i32t.fn_type(&[i32t.into(), ptr_t.into()], false)
    } else {
        i32t.fn_type(&[], false)
    };
    let main = module.add_function("main", main_ty, None);
    mark_nounwind(ctx, main);
    let builder = ctx.create_builder();
    let entry = ctx.append_basic_block(main, "entry");
    builder.position_at_end(entry);

    // Marshal argv into the `array<str>` argument, or call with no args.
    let call_args: Vec<inkwell::values::BasicMetadataValueEnum> = if has_args {
        let args_build = module.add_function(
            "align_rt_args_build",
            slice_struct_type(ctx).fn_type(&[i32t.into(), ptr_t.into()], false),
            None,
        );
        let argc = main.get_nth_param(0).expect("argc").into_int_value();
        let argv = main.get_nth_param(1).expect("argv").into_pointer_value();
        let args_val = builder
            .build_call(args_build, &[argc.into(), argv.into()], "args")
            .map_err(lower)?
            .try_as_basic_value()
            .basic()
            .ok_or_else(|| CodegenError::Lowering("args_build returned void".into()))?;
        vec![args_val.into()]
    } else {
        vec![]
    };

    let res = builder
        .build_call(align_main, &call_args, "r")
        .map_err(lower)?
        .try_as_basic_value()
        .basic()
        .ok_or_else(|| CodegenError::Lowering("main returned void".into()))?
        .into_struct_value();
    // `res` already has the Result aggregate type (main's payloads are () / Error).
    let tag = builder.build_extract_value(res, 0, "tag").map_err(lower)?.into_int_value();
    let is_err = builder
        .build_int_compare(IntPredicate::NE, tag, ctx.i8_type().const_int(0, false), "iserr")
        .map_err(lower)?;
    let err_bb = ctx.append_basic_block(main, "err");
    let ok_bb = ctx.append_basic_block(main, "ok");
    builder.build_conditional_branch(is_err, err_bb, ok_bb).map_err(lower)?;

    builder.position_at_end(err_bb);
    // The err payload is the `Error` enum `{ i32 tag, i32 code }`. Its exit code: `Code(c)` → `c`
    // (the payload), a category → `tag + 1` (a small distinct nonzero code). `report_error` clamps.
    let err_enum = builder.build_extract_value(res, 2, "err").map_err(lower)?.into_struct_value();
    let etag = builder.build_extract_value(err_enum, 0, "etag").map_err(lower)?.into_int_value();
    let ecode = builder.build_extract_value(err_enum, 1, "ecode").map_err(lower)?.into_int_value();
    let is_code = builder
        .build_int_compare(IntPredicate::EQ, etag, i32t.const_int(ERROR_VARIANT_CODE as u64, false), "iscode")
        .map_err(lower)?;
    let cat_code = builder.build_int_add(etag, i32t.const_int(1, false), "catcode").map_err(lower)?;
    let code = builder.build_select(is_code, ecode, cat_code, "exitcode").map_err(lower)?.into_int_value();
    let exit = builder
        .build_call(report, &[code.into()], "exit")
        .map_err(lower)?
        .try_as_basic_value()
        .basic()
        .ok_or_else(|| CodegenError::Lowering("report returned void".into()))?
        .into_int_value();
    builder.build_return(Some(&exit)).map_err(lower)?;

    builder.position_at_end(ok_bb);
    builder.build_return(Some(&i32t.const_int(0, false))).map_err(lower)?;
    Ok(())
}

fn int_type<'c>(ctx: &'c Context, ty: Ty) -> IntType<'c> {
    match ty {
        Ty::Int(IntTy { bits, .. }) => match bits {
            8 => ctx.i8_type(),
            16 => ctx.i16_type(),
            32 => ctx.i32_type(),
            _ => ctx.i64_type(),
        },
        Ty::Bool => ctx.bool_type(),
        // char is a 32-bit scalar; Unit/Error/Struct don't reach scalar int positions.
        _ => ctx.i32_type(),
    }
}

fn float_type<'c>(ctx: &'c Context, ty: Ty) -> FloatType<'c> {
    match ty {
        Ty::Float(FloatTy { bits: 32 }) => ctx.f32_type(),
        _ => ctx.f64_type(),
    }
}

/// LLVM type for a scalar value (int / bool / char / float); structs go through
/// `struct_types`.
/// A scalar's LLVM type. `sx` is the struct-type table (needed when the scalar is a
/// struct payload — `Option`/`Result` can carry a struct).
fn scalar_type<'c>(ctx: &'c Context, ty: Ty, sx: &[StructType<'c>], ex: &[StructType<'c>]) -> BasicTypeEnum<'c> {
    match ty {
        Ty::Float(_) => float_type(ctx, ty).into(),
        Ty::Struct(id) => sx[id as usize].into(),
        Ty::StructArray(id, n) => sx[id as usize].array_type(n).into(),
        // A sum type lowers to its non-union tagged struct `{ i32 tag, … }`.
        Ty::Enum(id) => ex[id as usize].into(),
        // A `{ptr,len}` payload (an owned `string` in an Option/Result, slice 8a; also str/slice/
        // array views) lowers to the slice struct.
        Ty::Str | Ty::String | Ty::Slice(_) | Ty::Soa(_) | Ty::DynArray(_) => slice_struct_type(ctx).into(),
        // An AoS struct array is a `{ptr,len}` view too; an SoA one would be a different
        // representation (column buffers), so match the layout — `Layout::Soa` (M6) makes this
        // arm go non-exhaustive (a compile error pointing exactly here).
        Ty::DynStructArray(_, Layout::Aos) | Ty::DynSliceArray(_) => slice_struct_type(ctx).into(),
        // `Task<R>` (④b) is a box in the task_group region — a pointer, like `box<T>`.
        Ty::Task(_) => ctx.ptr_type(AddressSpace::default()).into(),
        // `vecN<T>` (M6) → the LLVM vector `<N x T>`.
        Ty::Vec(s, n) => vec_llvm_ty(ctx, scalar_to_ty(s), n),
        // A comparison `mask` (M6) → `<N x i1>` (one bool lane per vector lane; element-independent).
        Ty::Mask(_, n) => ctx.bool_type().vec_type(n).into(),
        _ => int_type(ctx, ty).into(),
    }
}

/// The LLVM vector type `<N x T>` for a `vecN<T>` value (M6). `elem` is a numeric scalar `Ty`
/// (int or float); the element decides whether to build a float or integer vector.
fn vec_llvm_ty<'c>(ctx: &'c Context, elem: Ty, n: u32) -> BasicTypeEnum<'c> {
    if matches!(elem, Ty::Float(_)) {
        float_type(ctx, elem).vec_type(n).into()
    } else {
        int_type(ctx, elem).vec_type(n).into()
    }
}

/// Field indices of an `Option`/`Result` aggregate whose payload is an owned (Move) type and
/// must be freed when the aggregate is dropped (MMv2 slice 8a). Some/Ok = field 1, Err = field 2.
/// Allocation-free (≤ 2 indices).
fn move_payload_fields(ty: Ty) -> impl Iterator<Item = u32> {
    let (ok, err) = match ty {
        Ty::Option(s) => (s.is_move().then_some(1), None),
        Ty::Result(o, e) => (o.is_move().then_some(1), e.is_move().then_some(2)),
        _ => (None, None),
    };
    ok.into_iter().chain(err)
}

/// `Option<T>` lowers to `{ i8 tag, T value }` (tag 1 = Some, 0 = None).
fn option_struct_type<'c>(ctx: &'c Context, s: Scalar, sx: &[StructType<'c>], ex: &[StructType<'c>]) -> StructType<'c> {
    ctx.struct_type(&[ctx.i8_type().into(), scalar_type(ctx, scalar_to_ty(s), sx, ex)], false)
}

/// `Result<T, E>` lowers to `{ i8 tag, T ok, E err }` (tag 0 = Ok, 1 = Err).
fn result_struct_type<'c>(ctx: &'c Context, ok: Scalar, err: Scalar, sx: &[StructType<'c>], ex: &[StructType<'c>]) -> StructType<'c> {
    ctx.struct_type(
        &[
            ctx.i8_type().into(),
            scalar_type(ctx, scalar_to_ty(ok), sx, ex),
            scalar_type(ctx, scalar_to_ty(err), sx, ex),
        ],
        false,
    )
}

/// `slice<T>` lowers to `{ T* ptr, i64 len }`.
fn slice_struct_type<'c>(ctx: &'c Context) -> StructType<'c> {
    ctx.struct_type(&[ctx.ptr_type(AddressSpace::default()).into(), ctx.i64_type().into()], false)
}

/// The LLVM representation of a `Ty::DictEncoded` value: three `{ptr,len}` slices `{ source (borrowed
/// AoS), ids (owned i64 column), dict (owned str dictionary) }`. `Drop` frees `ids` + `dict`.
fn dictenc_struct_type<'c>(ctx: &'c Context) -> StructType<'c> {
    let s = slice_struct_type(ctx);
    ctx.struct_type(&[s.into(), s.into(), s.into()], false)
}

/// Whether an FFI type is a `{ptr,len}` view (`str`/`slice<T>`, incl. `bytes` = `slice<u8>`). Such a
/// value is handed to C as its **data pointer** alone — the length travels separately (`s.len()`).
fn is_ffi_view(ty: Ty) -> bool {
    matches!(ty, Ty::Str | Ty::Slice(_))
}

/// The LLVM type of an `extern "C"` parameter: a `str`/`slice` view is passed as its data pointer
/// (C `char*`/`void*`); everything else keeps its normal ABI type.
fn ffi_param_type<'c>(ctx: &'c Context, ty: Ty, sx: &[StructType<'c>], ex: &[StructType<'c>]) -> BasicTypeEnum<'c> {
    if is_ffi_view(ty) {
        ctx.ptr_type(AddressSpace::default()).into()
    } else {
        abi_type(ctx, ty, sx, ex)
    }
}

/// The LLVM representation of a `Ty::Fn` value: a closure `{ fn_ptr, env_ptr }`. All closure
/// `fn_ptr`s use the env-ABI `fn(env, args)`; `env_ptr` is null for a non-capturing function.
fn closure_struct_type<'c>(ctx: &'c Context) -> StructType<'c> {
    let p = ctx.ptr_type(AddressSpace::default());
    ctx.struct_type(&[p.into(), p.into()], false)
}

/// LLVM type for a function parameter/return (scalars + `Option`/`Result`/`slice`/`str`,
/// and structs/struct-arrays by value).
fn abi_type<'c>(ctx: &'c Context, ty: Ty, sx: &[StructType<'c>], ex: &[StructType<'c>]) -> BasicTypeEnum<'c> {
    match ty {
        Ty::Option(s) => option_struct_type(ctx, s, sx, ex).into(),
        Ty::Result(o, e) => result_struct_type(ctx, o, e, sx, ex).into(),
        Ty::Box(_) | Ty::ArenaHandle | Ty::Builder | Ty::BufWriter | Ty::Raw => ctx.ptr_type(AddressSpace::default()).into(),
        // A function value is a closure `{fn_ptr, env_ptr}` here too — matching `llvm_type`, so an
        // `Ty::Fn` in an ABI position (later: fn-typed parameters/returns) is not silently `i32`.
        Ty::Fn(_) => closure_struct_type(ctx).into(),
        Ty::Slice(_) | Ty::Soa(_) | Ty::Str | Ty::String | Ty::DynArray(_) => slice_struct_type(ctx).into(),
        // AoS struct array = `{ptr,len}`; SoA (M6) differs → match the layout (forces revisit).
        Ty::DynStructArray(_, Layout::Aos) | Ty::DynSliceArray(_) => slice_struct_type(ctx).into(),
        _ => scalar_type(ctx, ty, sx, ex),
    }
}

/// Size/alignment (bytes) of a scalar's in-memory representation.
/// A symbol-safe key for a spawn trampoline, by result type `R` and fallibility (`tramp$<key>`).
fn spawn_tramp_key(ty: Ty, fallible: bool) -> String {
    format!("{}{}", task_tramp_key(ty), if fallible { "$f" } else { "" })
}

/// A symbol-safe key for a spawn result type `R`, naming its trampoline (`tramp$<key>`).
fn task_tramp_key(ty: Ty) -> String {
    match ty {
        Ty::Int(it) => format!("{}{}", if it.signed { 'i' } else { 'u' }, it.bits),
        Ty::Float(ft) => format!("f{}", ft.bits),
        Ty::Bool => "bool".to_string(),
        Ty::Char => "char".to_string(),
        Ty::Unit => "unit".to_string(),
        _ => "x".to_string(),
    }
}

/// The natural ABI alignment (in bytes) of a struct field, used only to *order* fields for padding
/// elimination — the actual byte offsets are always read back from the built LLVM struct type via
/// `offset_of_element`, so a slightly-off estimate can never miscompile, only leave padding. Scalars
/// use their width; a nested aggregate takes the max of its members; pointer-like views (`str`,
/// `slice`, `string`, `box`, `soa`, dynamic arrays, …) are pointer-aligned (8).
///
/// The *valid* struct-field domain (`is_field_ok` in `align_sema`) is
/// `Int`/`Float`/`Bool`/`Char`/`Str`/`String`/nested `Struct`; on that domain this **must** return the
/// same alignment as `align_sema::ty_size_align` (the ordering must agree, or the sema huge-struct-copy
/// lint's size would diverge from the real layout). It does. The `Unit` (→ 4, vs sema's 1) and `Array`
/// (→ `scalar_bytes.min(8)`, vs sema's 8) arms below are for types `is_field_ok` **rejects**, so they
/// never reach field ordering — kept only so the function is total. Scalars top out at 64-bit, so no
/// field is wider than 8-byte aligned; a future wider-aligned field type (a `vecN<T>` field is
/// 16-byte aligned) would need updating **both** this and `ty_size_align` **and** would be caught by
/// the `layout_parity` test (which checks both against the real LLVM ABI alignment).
fn field_abi_align(ty: Ty, structs: &[StructDef]) -> u64 {
    match ty {
        Ty::Int(it) => (it.bits / 8).max(1) as u64,
        Ty::Float(ft) => (ft.bits / 8) as u64,
        Ty::Bool => 1,
        Ty::Char => 4,
        Ty::Unit => 4,
        Ty::Struct(id) | Ty::StructArray(id, _) => structs[id as usize]
            .fields
            .iter()
            .map(|f| field_abi_align(f.ty, structs))
            .max()
            .unwrap_or(1),
        Ty::Array(s, _) => scalar_bytes(s).clamp(1, 8),
        _ => 8,
    }
}

/// The logical→physical field-index map for a struct. A non-`layout(C)` struct is laid out in
/// **descending alignment** (Rust's default) to eliminate padding; ties keep declaration order
/// (a *stable* sort), so the layout is deterministic. A `layout(C)` struct keeps declaration order
/// (identity map) — its byte layout is the FFI/`raw`/json boundary and must not move. The returned
/// vector `m` satisfies `m[logical] = physical`; invert with [`physical_order`] to emit the body.
fn logical_to_physical(s: &StructDef, structs: &[StructDef]) -> Vec<u32> {
    let n = s.fields.len();
    if s.c_repr {
        return (0..n as u32).collect();
    }
    // Physical order = logical indices sorted by descending alignment (stable → decl order on ties).
    let mut order: Vec<u32> = (0..n as u32).collect();
    order.sort_by_key(|&i| std::cmp::Reverse(field_abi_align(s.fields[i as usize].ty, structs)));
    // Invert: `map[logical] = physical`.
    let mut map = vec![0u32; n];
    for (phys, &logical) in order.iter().enumerate() {
        map[logical as usize] = phys as u32;
    }
    map
}

/// Invert a logical→physical field map into `physical_order[physical] = logical` — the order in
/// which to emit the LLVM struct body (physical slot `p` holds logical field `physical_order[p]`).
fn physical_order(map: &[u32]) -> Vec<u32> {
    let mut order = vec![0u32; map.len()];
    for (logical, &phys) in map.iter().enumerate() {
        order[phys as usize] = logical as u32;
    }
    order
}

fn scalar_bytes(s: Scalar) -> u64 {
    match s {
        Scalar::Int(it) => (it.bits / 8).max(1) as u64,
        Scalar::Float(ft) => (ft.bits / 8) as u64,
        Scalar::Bool => 1,
        Scalar::Char => 4,
        // `()` is lowered as `i32` (the `int_type` fallback), so it must be sized as 4
        // bytes: a `box<()>` allocates `scalar_bytes` and then stores/loads an `i32`, so a
        // 1-byte size would overflow the allocation on the store and read OOB on the load.
        Scalar::Unit => 4,
        // Only used to size a `box<T>` payload, which is always a true scalar.
        Scalar::Struct(_) => unreachable!("a struct is not a box payload"),
        Scalar::String => unreachable!("an owned string is not a box payload"),
        Scalar::DynArray(_) => unreachable!("an owned array is not a box payload"),
        Scalar::DynStructArray(_) => unreachable!("an owned struct array is not a box payload"),
        // A `str` view is never a `box` payload (`box<str>` is rejected), but it *is* a valid
        // `array<str>` element — a `{ptr,len}` view, 16 bytes (the established str size, as in the
        // json field descriptor). Used to size a `group_by(.str_key)` output key buffer.
        Scalar::Str => 16,
        Scalar::Soa(_) => unreachable!("a soa view is not a box payload"),
        Scalar::Enum(_) => unreachable!("a sum type is not a box payload"),
        Scalar::Param(_) => unreachable!("a generic parameter is substituted before codegen"),
    }
}

fn is_signed(ty: Ty) -> bool {
    matches!(ty, Ty::Int(IntTy { signed: true, .. }))
}

/// The constant data `json.decode` codegen emits for one struct: the field-descriptor table, the
/// field count + struct store size, and the perfect-hash slot table (`phf_len = 0` → linear scan).
struct DecodeTable<'c> {
    descs: inkwell::values::PointerValue<'c>,
    n_fields: u64,
    store_size: u64,
    phf_ptr: inkwell::values::PointerValue<'c>,
    phf_len: u64,
    phf_seed: u64,
}

/// FNV-1a over `bytes`, seeded — the hash behind the compile-time perfect-hash field dispatch.
/// **MUST byte-for-byte match the runtime-side `json_phf_hash` in `align_runtime`** (the runtime
/// hashes each JSON key with this to find its slot; a divergence would mis-route a field).
fn phf_hash(bytes: &[u8], seed: u64) -> u64 {
    let mut h = 0xcbf2_9ce4_8422_2325u64 ^ seed;
    for &b in bytes {
        h ^= b as u64;
        h = h.wrapping_mul(0x100_0000_01b3);
    }
    h
}

/// Build a perfect-hash slot table for the field `names`: a power-of-two-sized `[i32]` where
/// `slots[hash(name, seed) & (len-1)] = field_index` (empty slots `-1`), plus the chosen `seed`.
/// Searches table sizes `next_pow2(n) .. ×8` and seeds `0..4096`; returns `None` if none is
/// collision-free (the caller then omits the table and the runtime uses a linear scan — so a
/// pathological name set degrades gracefully, never incorrectly). Empty / single-field structs
/// return `None` (a linear scan is already O(1) there).
fn build_phf(names: &[&str]) -> Option<(Vec<i32>, u64)> {
    let n = names.len();
    if n < 2 {
        return None;
    }
    let mut m = n.next_power_of_two();
    for _ in 0..4 {
        // Reuse one `slots` buffer across all seeds at this size (reset with `fill(-1)`) instead of
        // allocating a fresh vec per seed — up to 4096 fewer heap allocations per size.
        let mut slots = vec![-1i32; m];
        for seed in 0u64..4096 {
            slots.fill(-1);
            let mut ok = true;
            for (i, name) in names.iter().enumerate() {
                let slot = (phf_hash(name.as_bytes(), seed) & (m as u64 - 1)) as usize;
                if slots[slot] != -1 {
                    ok = false;
                    break;
                }
                slots[slot] = i as i32;
            }
            if ok {
                return Some((slots, seed));
            }
        }
        m *= 2;
    }
    None
}

fn int_bits(ty: Ty) -> u32 {
    match ty {
        Ty::Int(IntTy { bits, .. }) => bits as u32,
        Ty::Bool => 1,
        Ty::Char => 32,
        _ => 64,
    }
}

fn declare_fn<'c>(
    ctx: &'c Context,
    module: &Module<'c>,
    f: &Function,
    symbol: &str,
    struct_types: &[StructType<'c>],
    enum_types: &[StructType<'c>],
    tuple_types: &[StructType<'c>],
) -> FunctionValue<'c> {
    // Structs / struct-arrays / tuples pass and return by value as their aggregate LLVM type
    // (`abi_type` covers scalars + Option/Result/slice/str).
    let map = |ty: Ty| -> BasicTypeEnum<'c> {
        match ty {
            Ty::Struct(id) => struct_types[id as usize].into(),
            Ty::StructArray(id, n) => struct_types[id as usize].array_type(n).into(),
            Ty::Tuple(id) => tuple_types[id as usize].into(),
            // No array-typed params/returns arise yet (arrays coerce to slices at calls),
            // but mirror `llvm_type` so it stays correct once array annotations land.
            Ty::Array(s, n) => scalar_type(ctx, scalar_to_ty(s), struct_types, enum_types).array_type(n).into(),
            Ty::Enum(id) => enum_types[id as usize].into(),
            _ => abi_type(ctx, ty, struct_types, enum_types),
        }
    };
    let param_types: Vec<BasicMetadataTypeEnum> =
        f.params.iter().map(|s| map(f.slots[*s as usize]).into()).collect();
    let fn_ty = if f.ret == Ty::Unit {
        ctx.void_type().fn_type(&param_types, false)
    } else {
        map(f.ret).fn_type(&param_types, false)
    };
    let fv = module.add_function(symbol, fn_ty, None);
    mark_nounwind(ctx, fv);
    fv
}

/// Mark a function `nounwind`: Align functions never unwind — errors are `Result` values and a
/// fatal fault (`abort`) does not unwind (settled "no unwinding, immediate abort"; codegen emits
/// plain `call`, never `invoke`). The attribute lets LLVM drop exception edges / unwind tables and
/// inline more aggressively. Applied only to **Align-generated** functions (program fns, the C
/// `main` wrapper, fn-value / closure thunks) — never the external `align_rt_*` runtime
/// declarations, which are ordinary Rust functions and not promised `nounwind` here.
fn mark_nounwind<'c>(ctx: &'c Context, f: FunctionValue<'c>) {
    add_enum_attr(ctx, f, inkwell::attributes::AttributeLoc::Function, "nounwind");
}

/// Add a named zero-valued enum attribute at `loc`.
fn add_enum_attr<'c>(
    ctx: &'c Context,
    f: FunctionValue<'c>,
    loc: inkwell::attributes::AttributeLoc,
    name: &str,
) {
    let kind = inkwell::attributes::Attribute::get_named_enum_kind_id(name);
    f.add_attribute(loc, ctx.create_enum_attribute(kind, 0));
}

/// Attributes shared by every allocator-family runtime declaration, verified per function:
///
/// - `noalias` (return): each returns a *fresh* allocation (C `malloc`, a bump-region slice never
///   handed out before, or a `Box::into_raw`), disjoint from any pointer live before the call.
///   `noalias` is compatible with a possible null return (`align_rt_alloc`/`arena_alloc` hand back
///   null for an empty/invalid request), so the null-returning ones keep it.
/// - `nounwind` (function): none unwind — on OOM (C `malloc` null, or a Rust global-alloc failure)
///   they `abort`, and a panic (e.g. a `Vec` capacity overflow) can't escape the `extern "C"`
///   boundary either (it aborts), so no unwind ever leaves the call.
///
/// Deliberately **NOT** added: `willreturn`/`mustprogress` — each of these can `abort` on OOM, so
/// asserting it always returns to the caller would be unsound (a miscompile). Over-declaration on
/// the allocator hot path is the dangerous direction, so we stay conservative.
fn mark_alloc_common<'c>(ctx: &'c Context, f: FunctionValue<'c>) {
    add_enum_attr(ctx, f, inkwell::attributes::AttributeLoc::Return, "noalias");
    mark_nounwind(ctx, f);
}

/// A **single-shot** allocator that never frees memory reachable at entry (`align_rt_alloc` = one
/// `malloc`; the `*_begin` handle allocators + `builder_new` = one `Box::new`) — so it additionally
/// gets `nofree`.
fn mark_alloc_like<'c>(ctx: &'c Context, f: FunctionValue<'c>) {
    mark_alloc_common(ctx, f);
    add_enum_attr(ctx, f, inkwell::attributes::AttributeLoc::Function, "nofree");
}

/// A **bump** allocator (`align_rt_arena_alloc` / `align_rt_tg_alloc`): like `mark_alloc_like` but
/// **without `nofree`**. Growing the region does `Vec::push` on the chunk list, which can reallocate
/// that list's backing buffer — freeing memory allocated *before* the call — so `nofree` would be
/// unsound even though the returned bump pointer itself is fresh (`noalias` still holds: the chunk
/// buffers the pointer aliases into are never moved, only the chunk-*index* vector is).
fn mark_bump_alloc<'c>(ctx: &'c Context, f: FunctionValue<'c>) {
    mark_alloc_common(ctx, f);
}

struct FnGen<'c, 'a> {
    ctx: &'c Context,
    module: &'a Module<'c>,
    builder: &'a Builder<'c>,
    funcs: &'a HashMap<String, FunctionValue<'c>>,
    /// Extern parameter types by C symbol — to coerce a `str`/`slice` call argument to its data
    /// pointer when calling a foreign function.
    extern_params: &'a HashMap<String, Vec<Ty>>,
    structs: &'a [StructDef],
    struct_types: &'a [StructType<'c>],
    /// Logical→physical field-index map per struct id (`field_perm[sid][logical] = physical`).
    /// Non-`layout(C)` structs are reordered by descending alignment to eliminate padding; every
    /// field GEP / byte-offset site routes its MIR (logical) index through [`FnGen::pfield`].
    field_perm: &'a [Vec<u32>],
    /// Sum-type LLVM structs, indexed by the id in [`Ty::Enum`].
    enum_types: &'a [StructType<'c>],
    enums: &'a [EnumDef],
    /// Anonymous tuple types, indexed by the id in [`Ty::Tuple`].
    tuple_types: &'a [StructType<'c>],
    /// Tuple defs (element scalars) — to know which tuple elements are owned (Move) when dropping.
    tuples: &'a [TupleDef],
    /// Target layout — used to compute struct field byte offsets for `json.decode`.
    target_data: &'a inkwell::targets::TargetData,
    f: &'a Function,
    func: FunctionValue<'c>,
    slots: HashMap<Slot, inkwell::values::PointerValue<'c>>,
    values: HashMap<ValueId, BasicValueEnum<'c>>,
    blocks: Vec<BasicBlock<'c>>,
}

impl<'c, 'a> FnGen<'c, 'a> {
    fn err(&self, e: impl std::fmt::Display) -> CodegenError {
        CodegenError::Lowering(e.to_string())
    }

    /// Translate a MIR (logical) field index into the LLVM (physical) index for struct `struct_id`.
    /// Non-`layout(C)` structs are reordered by descending alignment (padding elimination); every
    /// struct-field GEP goes through here so the reorder stays invisible. `layout(C)` structs use the
    /// identity map, so their byte layout — the FFI/`raw`/json boundary — is unchanged.
    fn pfield(&self, struct_id: u32, logical: u32) -> u32 {
        self.field_perm[struct_id as usize][logical as usize]
    }

    /// The byte offset of logical field `logical` within struct `struct_id`, read from the built
    /// LLVM struct type at the field's *physical* position — so it is correct under reordering.
    fn field_byte_offset(&self, struct_id: u32, logical: u32) -> u64 {
        let st = self.struct_types[struct_id as usize];
        // Field indices are sema-validated, so a missing offset is a compiler bug — panic loudly
        // rather than defaulting to 0 (which would silently read the wrong field).
        self.target_data
            .offset_of_element(&st, self.pfield(struct_id, logical))
            .expect("valid struct field offset")
    }

    /// The address `ptr + offset` bytes, for `raw.load`/`raw.store` — an `i8` (byte-granular) GEP off
    /// the `raw` pointer by the i64 byte `offset`. The result is an opaque `ptr` (LLVM opaque
    /// pointers); the caller loads/stores it at **alignment 1** (an arbitrary byte offset may be
    /// misaligned for the scalar). A plain (non-`inbounds`) GEP is used deliberately: a raw pointer
    /// carries no allocation-bounds guarantee, and an `inbounds` GEP that steps outside the object
    /// would be poison — letting the optimizer assume in-bounds and drop later checks. Plain `gep`
    /// keeps unsafe pointer arithmetic well-defined (wrapping) as the caller intends.
    fn raw_elem_ptr(&mut self, ptr: &align_mir::Operand, offset: &align_mir::Operand) -> Result<inkwell::values::PointerValue<'c>, CodegenError> {
        let base = self.operand(ptr).into_pointer_value();
        let off = self.operand(offset).into_int_value();
        unsafe {
            self.builder
                .build_gep(self.ctx.i8_type(), base, &[off], "rawelem")
                .map_err(|e| self.err(e))
        }
    }

    /// The byte size of each field (= column element size) of a `soa<Struct>`, in declaration
    /// order. Fields are primitive scalars or `str` (sema-enforced); a `str` column element is the
    /// 16-byte `{ptr,len}` view (`scalar_bytes(Scalar::Str) == 16`).
    fn soa_field_sizes(&self, struct_id: u32) -> Vec<u64> {
        self.structs[struct_id as usize]
            .fields
            .iter()
            .map(|f| scalar_bytes(align_sema::ty_to_scalar(f.ty).expect("soa field is a scalar or str")))
            .collect()
    }

    /// Byte offset of column `field` within a `soa<Struct>` column-major buffer of `len` rows:
    /// `start_0 = 0`, `start_j = align_up(start_{j-1} + len*size_{j-1}, size_j)`. Each column is
    /// padded to the field's alignment (= its size for a primitive), so mixed-width columns stay
    /// naturally aligned for any `len`. Shared by [`Rvalue::IndexColumn`], [`Stmt::StoreColumn`],
    /// and the [`Rvalue::SoaAlloc`] total-size walk.
    fn soa_column_offset(
        &self,
        len: inkwell::values::IntValue<'c>,
        sizes: &[u64],
        field: usize,
    ) -> Result<inkwell::values::IntValue<'c>, CodegenError> {
        if field >= sizes.len() {
            return Err(self.err("soa column index out of bounds"));
        }
        // Use `len`'s own int type (it is i64 today, but stay robust to a width change).
        let ty = len.get_type();
        let mut off = ty.const_zero();
        for j in 1..=field {
            let adv = self.builder.build_int_mul(len, ty.const_int(sizes[j - 1], false), "coladv").map_err(|e| self.err(e))?;
            let sum = self.builder.build_int_add(off, adv, "colend").map_err(|e| self.err(e))?;
            let a = sizes[j]; // field alignment = its size (power of two)
            // align_up(sum, a) = (sum + a-1) & ~(a-1); a no-op for a 1-byte column, so skip it.
            off = if a > 1 {
                let bumped = self.builder.build_int_add(sum, ty.const_int(a - 1, false), "colbump").map_err(|e| self.err(e))?;
                self.builder.build_and(bumped, ty.const_int(!(a - 1), false), "colalign").map_err(|e| self.err(e))?
            } else {
                sum
            };
        }
        Ok(off)
    }

    /// Allocate stack storage hoisted to the top of the entry block (so an alloca inside a loop
    /// does not grow the stack each iteration), then restore the builder to the current position.
    fn alloca_at_entry(&self, ty: BasicTypeEnum<'c>, name: &str) -> Result<inkwell::values::PointerValue<'c>, CodegenError> {
        let saved = self.builder.get_insert_block().ok_or_else(|| self.err("no insertion block"))?;
        let entry = *self.blocks.get(self.f.entry as usize).ok_or_else(|| self.err("entry block not found"))?;
        match entry.get_first_instruction() {
            Some(inst) => self.builder.position_before(&inst),
            None => self.builder.position_at_end(entry),
        }
        let p = self.builder.build_alloca(ty, name).map_err(|e| self.err(e))?;
        self.builder.position_at_end(saved);
        Ok(p)
    }

    /// Find + declare + call an overloaded LLVM intrinsic by name, with the given overload types
    /// and call arguments.
    fn call_intrinsic(
        &self,
        name: &str,
        overloads: &[BasicTypeEnum<'c>],
        args: &[inkwell::values::BasicMetadataValueEnum<'c>],
    ) -> Result<BasicValueEnum<'c>, CodegenError> {
        let intr = Intrinsic::find(name).ok_or_else(|| self.err(format!("intrinsic {name} not found")))?;
        let f = intr
            .get_declaration(self.module, overloads)
            .ok_or_else(|| self.err(format!("could not declare intrinsic {name}")))?;
        self.builder
            .build_call(f, args, "intr")
            .map_err(|e| self.err(e))?
            .try_as_basic_value()
            .basic()
            .ok_or_else(|| self.err(format!("intrinsic {name} returned no value")))
    }

    /// Find + declare + call an overloaded binary integer intrinsic (`llvm.sadd.sat`,
    /// `llvm.umul.with.overflow`, …) on `int_ty`, returning its result value (`iN` for `.sat`,
    /// `{ iN, i1 }` for `.with.overflow`).
    fn call_overflow_intrinsic(
        &self,
        name: &str,
        int_ty: IntType<'c>,
        a: inkwell::values::IntValue<'c>,
        b: inkwell::values::IntValue<'c>,
    ) -> Result<BasicValueEnum<'c>, CodegenError> {
        let intr = Intrinsic::find(name).ok_or_else(|| self.err(format!("intrinsic {name} not found")))?;
        let f = intr
            .get_declaration(self.module, &[int_ty.into()])
            .ok_or_else(|| self.err(format!("could not declare intrinsic {name}")))?;
        self.builder
            .build_call(f, &[a.into(), b.into()], "intr")
            .map_err(|e| self.err(e))?
            .try_as_basic_value()
            .basic()
            .ok_or_else(|| self.err(format!("intrinsic {name} returned no value")))
    }

    /// Get (building it once) the `void(in_ptr, out_ptr)` thunk for `func` — load the input
    /// element (`in_ty`), call `func`, store its result through `out_ptr` — and return its pointer.
    /// The runtime `align_rt_par_map` calls this per element. Building it temporarily repositions
    /// the shared builder, then restores it.
    fn par_map_thunk(
        &self,
        func: &str,
        in_ty: BasicTypeEnum<'c>,
    ) -> Result<inkwell::values::PointerValue<'c>, CodegenError> {
        let name = format!("{func}$parthunk");
        if let Some(f) = self.module.get_function(&name) {
            return Ok(f.as_global_value().as_pointer_value());
        }
        let ptr_t = self.ctx.ptr_type(AddressSpace::default());
        let thunk = self.module.add_function(&name, self.ctx.void_type().fn_type(&[ptr_t.into(), ptr_t.into()], false), None);
        mark_nounwind(self.ctx, thunk);
        let saved = self.builder.get_insert_block();
        let entry = self.ctx.append_basic_block(thunk, "entry");
        self.builder.position_at_end(entry);
        let in_p = thunk.get_nth_param(0).unwrap().into_pointer_value();
        let out_p = thunk.get_nth_param(1).unwrap().into_pointer_value();
        let x = self.builder.build_load(in_ty, in_p, "x").map_err(|e| self.err(e))?;
        let r = self
            .builder
            .build_call(self.funcs[func], &[x.into()], "r")
            .map_err(|e| self.err(e))?
            .try_as_basic_value()
            .basic()
            .ok_or_else(|| self.err("par_map function must return a value"))?;
        self.builder.build_store(out_p, r).map_err(|e| self.err(e))?;
        self.builder.build_return(None).map_err(|e| self.err(e))?;
        match saved {
            Some(s) => self.builder.position_at_end(s),
            // No prior block: clear the position so later codegen doesn't append into the thunk.
            None => self.builder.clear_insertion_position(),
        }
        Ok(thunk.as_global_value().as_pointer_value())
    }

    fn emit_fn(&mut self) -> Result<(), CodegenError> {
        // Create an LLVM block per MIR block.
        for b in &self.f.blocks {
            let bb = self.ctx.append_basic_block(self.func, &format!("bb{}", b.id));
            self.blocks.push(bb);
        }

        // Allocate slots at the start of the entry block.
        let entry = self.blocks[self.f.entry as usize];
        self.builder.position_at_end(entry);
        for (i, ty) in self.f.slots.iter().enumerate() {
            let llty = self.llvm_type(*ty);
            let ptr = self
                .builder
                .build_alloca(llty, &format!("_{i}"))
                .map_err(|e| self.err(e))?;
            // Set the slot's alignment explicitly through the one alignment seam (`type_align`).
            // Today this is the natural ABI alignment (a no-op vs LLVM's default); at M6 a struct
            // declared `align(N)` returns `N` here, so its stack slot is over-aligned — the single
            // place that change lands (`open-questions.md` "`align(N)`").
            let inst = ptr
                .as_instruction()
                .ok_or_else(|| self.err("alloca did not yield an instruction"))?;
            inst.set_alignment(self.type_align(*ty)).map_err(|e| self.err(e))?;
            self.slots.insert(i as Slot, ptr);
        }

        // Emit each block.
        for b in &self.f.blocks {
            let bb = self.blocks[b.id as usize];
            self.builder.position_at_end(bb);
            self.gen_block(b)?;
        }
        Ok(())
    }

    fn gen_block(&mut self, b: &Block) -> Result<(), CodegenError> {
        for s in &b.stmts {
            match s {
                Stmt::Let(v, rv) => {
                    let result_ty = self.f.value_tys[*v as usize];
                    if let Some(val) = self.gen_rvalue(rv, result_ty)? {
                        self.values.insert(*v, val);
                    }
                }
                Stmt::Store(slot, op) => {
                    let val = self.operand(op);
                    let ptr = self.slots[slot];
                    self.builder.build_store(ptr, val).map_err(|e| self.err(e))?;
                }
                Stmt::StoreField(slot, path, op) => {
                    let field_ptr = self.field_path_ptr(*slot, path)?;
                    let val = self.operand(op);
                    self.builder.build_store(field_ptr, val).map_err(|e| self.err(e))?;
                }
                Stmt::StoreIndex(slot, idx, op) => {
                    let ep = self.elem_ptr(*slot, idx)?;
                    let val = self.operand(op);
                    self.builder.build_store(ep, val).map_err(|e| self.err(e))?;
                }
                Stmt::DropElem(slot, idx, sid) => {
                    // Free the owned fields of element `idx` before it is overwritten (Slice 4b).
                    let ep = self.elem_ptr(*slot, idx)?;
                    self.drop_struct_fields(ep, *sid)?;
                }
                Stmt::DropElemField(slot, idx, field) => {
                    // Free one owned `string` field of element `idx` before it is overwritten (4b).
                    let Ty::StructArray(sid, _) = self.f.slots[*slot as usize] else {
                        unreachable!("DropElemField on a non-struct-array slot");
                    };
                    let ep = self.elem_ptr(*slot, idx)?;
                    let st = self.struct_types[sid as usize];
                    let fp = self.builder.build_struct_gep(st, ep, self.pfield(sid, *field), "dropelemfld").map_err(|e| self.err(e))?;
                    let agg = self.builder.build_load(slice_struct_type(self.ctx), fp, "dropelemfldv").map_err(|e| self.err(e))?.into_struct_value();
                    let ptr = self.builder.build_extract_value(agg, 0, "dropelemfldptr").map_err(|e| self.err(e))?;
                    self.builder.build_call(self.funcs["free"], &[ptr.into()], "").map_err(|e| self.err(e))?;
                }
                Stmt::StoreElemField(slot, idx, field, op) => {
                    let ep = self.elem_field_ptr(*slot, idx, *field)?;
                    let val = self.operand(op);
                    self.builder.build_store(ep, val).map_err(|e| self.err(e))?;
                }
                Stmt::PtrStore(ptr, idx, op) => {
                    // `ptr[idx] <- val` into a raw element buffer; the element LLVM type is
                    // the stored value's type (opaque pointers, so the ptr carries none).
                    let p = self.operand(ptr).into_pointer_value();
                    let index = self.operand(idx).into_int_value();
                    let val = self.operand(op);
                    let ep = unsafe {
                        self.builder
                            .build_in_bounds_gep(val.get_type(), p, &[index], "ptrstore")
                            .map_err(|e| self.err(e))?
                    };
                    self.builder.build_store(ep, val).map_err(|e| self.err(e))?;
                }
                // `s.store(i, v)` — `<n x T>` store into `&buf[i]` at the element alignment.
                Stmt::VecStore { slice, index, value, elem, n: _ } => {
                    let sv = self.operand(slice).into_struct_value();
                    let buf = self.builder.build_extract_value(sv, 0, "vsbuf").map_err(|e| self.err(e))?.into_pointer_value();
                    let index = self.operand(index).into_int_value();
                    let val = self.operand(value);
                    let elem_lt = scalar_type(self.ctx, *elem, self.struct_types, self.enum_types);
                    let ep = unsafe {
                        self.builder.build_in_bounds_gep(elem_lt, buf, &[index], "vstoregep").map_err(|e| self.err(e))?
                    };
                    self.builder
                        .build_store(ep, val)
                        .map_err(|e| self.err(e))?
                        .set_alignment(self.type_align(*elem))
                        .map_err(|e| self.err(e))?;
                }
                Stmt::StoreColumn { base, len, index, field, struct_id, value } => {
                    // Scatter `value` into column `field` at row `index` of the soa buffer `base`:
                    // `column_base(field) + index*size_field`. The write counterpart of
                    // `Rvalue::IndexColumn`, sharing the same per-column `align_up` offset chain.
                    let buf = self.operand(base).into_pointer_value();
                    let len_v = self.operand(len).into_int_value();
                    let sizes = self.soa_field_sizes(*struct_id);
                    let off = self.soa_column_offset(len_v, &sizes, *field as usize)?;
                    let col_base = unsafe {
                        self.builder.build_in_bounds_gep(self.ctx.i8_type(), buf, &[off], "colbase").map_err(|e| self.err(e))?
                    };
                    let field_def = self
                        .structs
                        .get(*struct_id as usize)
                        .and_then(|s| s.fields.get(*field as usize))
                        .ok_or_else(|| self.err("soa column field index out of bounds"))?;
                    let fty = scalar_type(self.ctx, field_def.ty, self.struct_types, self.enum_types);
                    let idx_v = self.operand(index).into_int_value();
                    let ep = unsafe {
                        self.builder.build_in_bounds_gep(fty, col_base, &[idx_v], "colelem").map_err(|e| self.err(e))?
                    };
                    let val = self.operand(value);
                    self.builder.build_store(ep, val).map_err(|e| self.err(e))?;
                }
                Stmt::ArenaEnd(op) => {
                    let handle = self.operand(op).into();
                    self.builder
                        .build_call(self.funcs["arena_end"], &[handle], "")
                        .map_err(|e| self.err(e))?;
                }
                Stmt::RawFree(op) => {
                    // `raw.free(p)` → `align_rt_free(p)` (a null-safe libc `free`).
                    let p = self.operand(op).into();
                    self.builder
                        .build_call(self.funcs["free"], &[p], "")
                        .map_err(|e| self.err(e))?;
                }
                Stmt::RawStore { ptr, offset, value } => {
                    // `raw.store(p, off, v)` → store `v` at `p + off` bytes. GEP by the i8 (byte)
                    // offset yields a `ptr`; the store's value type fixes the width. An arbitrary byte
                    // offset may be misaligned for the scalar, so force alignment 1 (an unaligned
                    // store) — always correct, never LLVM-UB, at a possible perf cost on some targets.
                    let ep = self.raw_elem_ptr(ptr, offset)?;
                    let val = self.operand(value);
                    let st = self.builder.build_store(ep, val).map_err(|e| self.err(e))?;
                    st.set_alignment(1).map_err(|e| self.err(e))?;
                }
                Stmt::TgWait(op) => {
                    let handle = self.operand(op).into();
                    self.builder
                        .build_call(self.funcs["tg_wait"], &[handle], "")
                        .map_err(|e| self.err(e))?;
                }
                Stmt::TgEnd(op) => {
                    let handle = self.operand(op).into();
                    self.builder
                        .build_call(self.funcs["tg_end"], &[handle], "")
                        .map_err(|e| self.err(e))?;
                }
                Stmt::DropFlagInit(slot) => {
                    // Null-initialise the slot so a drop on a never-allocated / moved-out path is
                    // a no-op. A `builder` slot holds a bare pointer (null); an Option/Result with
                    // an owned payload zeroes the whole aggregate (so its payload reads {null,0});
                    // the owned `{ptr,len}` collections store `{null, 0}`.
                    let ty = self.f.slots[*slot as usize];
                    let z: BasicValueEnum = if ty == Ty::Builder || ty == Ty::BufWriter {
                        // A builder / buffered-writer slot holds a bare (nullable) handle pointer.
                        self.ctx.ptr_type(AddressSpace::default()).const_null().into()
                    } else if matches!(ty, Ty::StructArray(..)) {
                        // A fixed array of a Move struct: zero the whole `[N x %Struct]` so every
                        // element's owned fields read {null,0} until constructed — its per-element
                        // `Drop` then frees nulls on an unwritten element (no-op). (Slice 4a.)
                        self.llvm_type(ty).into_array_type().const_zero().into()
                    } else if payload_is_move(ty) || matches!(ty, Ty::Tuple(_) | Ty::Struct(_) | Ty::DictEncoded(..)) {
                        // Zero the whole aggregate so each owned field/element reads {null,0}. A Move
                        // struct is zeroed wholesale here; its recursive `Drop` then frees nulls on an
                        // unconstructed / moved-out path (no-op) — see `drop_struct_fields`.
                        self.llvm_type(ty).into_struct_type().const_zero().into()
                    } else {
                        slice_struct_type(self.ctx).const_zero().into()
                    };
                    self.builder.build_store(self.slots[slot], z).map_err(|e| self.err(e))?;
                }
                Stmt::NullTupleField(slot, idx) => {
                    // Null one owned `{ptr,len}` field of a tuple slot (after a partial field move),
                    // so the tuple's `Drop` frees null there.
                    let Ty::Tuple(tid) = self.f.slots[*slot as usize] else {
                        unreachable!("NullTupleField on a non-tuple slot");
                    };
                    let field_ptr = self
                        .builder
                        .build_struct_gep(self.tuple_types[tid as usize], self.slots[slot], *idx, "nulltupfld")
                        .map_err(|e| self.err(e))?;
                    self.builder
                        .build_store(field_ptr, slice_struct_type(self.ctx).const_zero())
                        .map_err(|e| self.err(e))?;
                }
                Stmt::NullStructField(slot, idx) => {
                    // Null one owned `string` `{ptr,len}` field of a struct slot (after a partial
                    // field move `n := u.name`), so the struct's recursive `Drop` frees null there.
                    let Ty::Struct(sid) = self.f.slots[*slot as usize] else {
                        unreachable!("NullStructField on a non-struct slot");
                    };
                    let field_ptr = self
                        .builder
                        .build_struct_gep(self.struct_types[sid as usize], self.slots[slot], self.pfield(sid, *idx), "nullstructfld")
                        .map_err(|e| self.err(e))?;
                    self.builder
                        .build_store(field_ptr, slice_struct_type(self.ctx).const_zero())
                        .map_err(|e| self.err(e))?;
                }
                Stmt::Drop(slot) => {
                    let ty = self.f.slots[*slot as usize];
                    if ty == Ty::Builder {
                        // An unfinished builder: free the builder object (null-safe — a moved-out
                        // builder's slot was nulled by `to_string`).
                        let p = self
                            .builder
                            .build_load(self.ctx.ptr_type(AddressSpace::default()), self.slots[slot], "dropb")
                            .map_err(|e| self.err(e))?;
                        self.builder
                            .build_call(self.funcs["builder_free"], &[p.into()], "")
                            .map_err(|e| self.err(e))?;
                    } else if ty == Ty::BufWriter {
                        // A buffered stdout writer: flush any remaining bytes best-effort, then free
                        // (null-safe). The drop-time flush is the safety net for output not explicitly
                        // `flush()`ed.
                        let p = self
                            .builder
                            .build_load(self.ctx.ptr_type(AddressSpace::default()), self.slots[slot], "dropw")
                            .map_err(|e| self.err(e))?;
                        self.builder
                            .build_call(self.funcs["io_buf_free"], &[p.into()], "")
                            .map_err(|e| self.err(e))?;
                    } else if payload_is_move(ty) {
                        // An Option/Result owning a Move payload: free each owned payload field's
                        // buffer pointer (null-safe — the inactive arm reads {null,0}, and a
                        // moved-out aggregate was nulled at the move site).
                        let aty = self.llvm_type(ty).into_struct_type();
                        let agg = self
                            .builder
                            .build_load(aty, self.slots[slot], "drop")
                            .map_err(|e| self.err(e))?
                            .into_struct_value();
                        for idx in move_payload_fields(ty) {
                            let payload = self
                                .builder
                                .build_extract_value(agg, idx, "droppl")
                                .map_err(|e| self.err(e))?
                                .into_struct_value();
                            let ptr = self.builder.build_extract_value(payload, 0, "dropplptr").map_err(|e| self.err(e))?;
                            self.builder
                                .build_call(self.funcs["free"], &[ptr.into()], "")
                                .map_err(|e| self.err(e))?;
                        }
                    } else if let Ty::Tuple(tid) = ty {
                        // A Move tuple: free each owned element's buffer pointer (null-safe — a
                        // moved-out tuple was zeroed, and Copy elements are skipped).
                        let aty = self.tuple_types[tid as usize];
                        let agg = self
                            .builder
                            .build_load(aty, self.slots[slot], "droptup")
                            .map_err(|e| self.err(e))?
                            .into_struct_value();
                        for (i, s) in self.tuples[tid as usize].elems.iter().enumerate() {
                            if !s.is_move() {
                                continue;
                            }
                            let elem = self
                                .builder
                                .build_extract_value(agg, i as u32, "droptupel")
                                .map_err(|e| self.err(e))?
                                .into_struct_value();
                            let ptr = self.builder.build_extract_value(elem, 0, "droptupptr").map_err(|e| self.err(e))?;
                            self.builder
                                .build_call(self.funcs["free"], &[ptr.into()], "")
                                .map_err(|e| self.err(e))?;
                        }
                    } else if let Ty::Struct(sid) = ty {
                        // A Move struct: recursively free each owned field's buffer, in declared order,
                        // recursing into nested Move-struct fields (null-safe — a moved-out struct was
                        // zeroed, and Copy fields are skipped).
                        self.drop_struct_fields(self.slots[slot], sid)?;
                    } else if let Ty::StructArray(sid, n) = ty {
                        // A fixed array of a Move struct: drop each element's owned fields in turn
                        // (null-safe — the slot was zeroed by `DropFlagInit`). Unrolled: `n` is a
                        // small compile-time constant. (Slice 4a.)
                        let arr_ty = self.llvm_type(ty);
                        let zero = self.ctx.i64_type().const_zero();
                        for i in 0..n {
                            let idx = self.ctx.i64_type().const_int(i as u64, false);
                            let elem_ptr = unsafe {
                                self.builder
                                    .build_in_bounds_gep(arr_ty, self.slots[slot], &[zero, idx], "dropelem")
                                    .map_err(|e| self.err(e))?
                            };
                            self.drop_struct_fields(elem_ptr, sid)?;
                        }
                    } else if let Ty::DictEncoded(..) = ty {
                        // A `dict_encoded` value owns its `ids` (field 1) + `dict` (field 2) buffers;
                        // free both (null-safe). Field 0 (`source`) borrows the AoS — never freed.
                        let agg = self
                            .builder
                            .build_load(dictenc_struct_type(self.ctx), self.slots[slot], "dropenc")
                            .map_err(|e| self.err(e))?
                            .into_struct_value();
                        for idx in [1u32, 2] {
                            let sl = self.builder.build_extract_value(agg, idx, "dropencsl").map_err(|e| self.err(e))?.into_struct_value();
                            let ptr = self.builder.build_extract_value(sl, 0, "dropencptr").map_err(|e| self.err(e))?;
                            self.builder
                                .build_call(self.funcs["free"], &[ptr.into()], "")
                                .map_err(|e| self.err(e))?;
                        }
                    } else {
                        // Load the owned `{ptr, len}`, extract the buffer pointer, free it (null-safe).
                        let agg = self
                            .builder
                            .build_load(slice_struct_type(self.ctx), self.slots[slot], "drop")
                            .map_err(|e| self.err(e))?
                            .into_struct_value();
                        let ptr = self.builder.build_extract_value(agg, 0, "dropptr").map_err(|e| self.err(e))?;
                        self.builder
                            .build_call(self.funcs["free"], &[ptr.into()], "")
                            .map_err(|e| self.err(e))?;
                    }
                }
                Stmt::DropValue(op) => {
                    // Free the buffer of an owned `{ptr, len}` value (an unbound temporary).
                    let agg = self.operand(op).into_struct_value();
                    let ptr = self.builder.build_extract_value(agg, 0, "dropvalptr").map_err(|e| self.err(e))?;
                    self.builder
                        .build_call(self.funcs["free"], &[ptr.into()], "")
                        .map_err(|e| self.err(e))?;
                }
            }
        }
        self.gen_term(&b.term)
    }

    fn gen_term(&mut self, t: &Term) -> Result<(), CodegenError> {
        match t {
            Term::Goto(target) => {
                self.builder
                    .build_unconditional_branch(self.blocks[*target as usize])
                    .map_err(|e| self.err(e))?;
            }
            Term::Branch(cond, then_bb, else_bb) => {
                let c = self.operand(cond).into_int_value();
                self.builder
                    .build_conditional_branch(
                        c,
                        self.blocks[*then_bb as usize],
                        self.blocks[*else_bb as usize],
                    )
                    .map_err(|e| self.err(e))?;
            }
            Term::Return(Some(op)) => {
                let v = self.operand(op);
                self.builder.build_return(Some(&v)).map_err(|e| self.err(e))?;
            }
            Term::Return(None) => {
                self.builder.build_return(None).map_err(|e| self.err(e))?;
            }
            Term::Unreachable => {
                self.builder.build_unreachable().map_err(|e| self.err(e))?;
            }
        }
        Ok(())
    }

    /// Lower an rvalue. Returns `None` for a value-less result (a void call).
    /// `result_ty` is the type of the value being defined (needed to build a bare `None`).
    fn gen_rvalue(&mut self, rv: &Rvalue, result_ty: Ty) -> Result<Option<BasicValueEnum<'c>>, CodegenError> {
        let v: BasicValueEnum<'c> = match rv {
            Rvalue::Use(op) => self.operand(op),
            Rvalue::Load(slot) => {
                let ty = self.llvm_type(self.f.slots[*slot as usize]);
                self.builder
                    .build_load(ty, self.slots[slot], "load")
                    .map_err(|e| self.err(e))?
            }
            Rvalue::Un(op, a) => match op {
                UnOp::Neg if matches!(self.f.operand_ty(a), Ty::Float(_)) => {
                    let a = self.operand(a).into_float_value();
                    self.builder.build_float_neg(a, "fneg").map_err(|e| self.err(e))?.into()
                }
                UnOp::Neg => {
                    let a = self.operand(a).into_int_value();
                    self.builder.build_int_neg(a, "neg").map_err(|e| self.err(e))?.into()
                }
                // `!` (boolean, i1) and `~` (integer bitwise complement) are both LLVM `not`.
                UnOp::Not | UnOp::BitNot => {
                    let a = self.operand(a).into_int_value();
                    self.builder.build_not(a, "not").map_err(|e| self.err(e))?.into()
                }
            },
            Rvalue::Cast { operand, from, to } => {
                let val = self.operand(operand);
                self.gen_cast(val, *from, *to)?
            }
            Rvalue::Bin(op, a, b) => self.gen_bin(*op, a, b)?,
            Rvalue::IntArith { op, mode, int_ty, a, b } => {
                let llvm_int = int_type(self.ctx, *int_ty);
                let signed = is_signed(*int_ty);
                let sign = if signed { 's' } else { 'u' };
                let opname = match op {
                    BinOp::Add => "add",
                    BinOp::Sub => "sub",
                    BinOp::Mul => "mul",
                    _ => return Err(self.err("IntArith op must be add/sub/mul")),
                };
                let av = self.operand(a).into_int_value();
                let bv = self.operand(b).into_int_value();
                match mode {
                    align_sema::ArithMode::Saturating if *op != BinOp::Mul => {
                        // add/sub: LLVM has the saturating intrinsic directly.
                        let name = format!("llvm.{sign}{opname}.sat");
                        self.call_overflow_intrinsic(&name, llvm_int, av, bv)?
                    }
                    align_sema::ArithMode::Saturating => {
                        // LLVM has NO `{s,u}mul.sat`; build it from `mul.with.overflow` + selecting
                        // the saturated extreme. Unsigned overflow → MAX; signed → MAX when the
                        // operands share a sign (product positive), else MIN.
                        let name = format!("llvm.{sign}mul.with.overflow");
                        let agg = self.call_overflow_intrinsic(&name, llvm_int, av, bv)?.into_struct_value();
                        let prod = self.builder.build_extract_value(agg, 0, "prod").map_err(|e| self.err(e))?.into_int_value();
                        let ovf = self.builder.build_extract_value(agg, 1, "of").map_err(|e| self.err(e))?.into_int_value();
                        let sat = if signed {
                            let smax = self.builder.build_right_shift(llvm_int.const_all_ones(), llvm_int.const_int(1, false), false, "smax").map_err(|e| self.err(e))?;
                            let smin = self.builder.build_not(smax, "smin").map_err(|e| self.err(e))?;
                            let zero = llvm_int.const_zero();
                            let a_neg = self.builder.build_int_compare(IntPredicate::SLT, av, zero, "an").map_err(|e| self.err(e))?;
                            let b_neg = self.builder.build_int_compare(IntPredicate::SLT, bv, zero, "bn").map_err(|e| self.err(e))?;
                            let same = self.builder.build_int_compare(IntPredicate::EQ, a_neg, b_neg, "ss").map_err(|e| self.err(e))?;
                            self.builder.build_select(same, smax, smin, "sat").map_err(|e| self.err(e))?.into_int_value()
                        } else {
                            llvm_int.const_all_ones()
                        };
                        self.builder.build_select(ovf, sat, prod, "satmul").map_err(|e| self.err(e))?
                    }
                    // `checked_*`: the `with.overflow` intrinsic returns `{ iN result, i1 overflow }`;
                    // build `Option<iN>` — tag 0 (None) on overflow, else tag 1 (Some) with the result.
                    align_sema::ArithMode::Checked => {
                        let Ty::Option(s) = result_ty else {
                            return Err(self.err("checked result is not an Option"));
                        };
                        let name = format!("llvm.{sign}{opname}.with.overflow");
                        let agg = self.call_overflow_intrinsic(&name, llvm_int, av, bv)?.into_struct_value();
                        let res = self.builder.build_extract_value(agg, 0, "res").map_err(|e| self.err(e))?;
                        let ovf = self.builder.build_extract_value(agg, 1, "of").map_err(|e| self.err(e))?.into_int_value();
                        let oty = option_struct_type(self.ctx, s, self.struct_types, self.enum_types);
                        let some_tag = self.ctx.i8_type().const_int(1, false);
                        let none_tag = self.ctx.i8_type().const_int(0, false);
                        let tag = self
                            .builder
                            .build_select(ovf, none_tag, some_tag, "tag")
                            .map_err(|e| self.err(e))?
                            .into_int_value();
                        let a0 = self
                            .builder
                            .build_insert_value(oty.const_zero(), tag, 0, "tag")
                            .map_err(|e| self.err(e))?
                            .into_struct_value();
                        self.builder
                            .build_insert_value(a0, res, 1, "val")
                            .map_err(|e| self.err(e))?
                            .into_struct_value()
                            .into()
                    }
                }
            }
            Rvalue::MathOp { fn_, ty, operands } => {
                // For an element-wise float vector (`vecN<f32>`), classify by the element type but
                // keep the **vector** as the intrinsic overload, so `call_intrinsic` emits the
                // vector form (e.g. `llvm.sqrt.v4f32`). Scalar `ty` classifies as itself.
                let elem = match ty {
                    Ty::Vec(s, _) => scalar_to_ty(*s),
                    t => *t,
                };
                let is_float = matches!(elem, Ty::Float(_));
                let signed = is_signed(elem);
                let overload = scalar_type(self.ctx, *ty, self.struct_types, self.enum_types);
                let ops: Vec<BasicValueEnum> = operands.iter().map(|o| self.operand(o)).collect();
                match fn_ {
                    align_sema::MathFn::Abs => {
                        if is_float {
                            self.call_intrinsic("llvm.fabs", &[overload], &[ops[0].into()])?
                        } else if signed {
                            // llvm.abs.iN(x, is_int_min_poison=false): INT_MIN.abs() = INT_MIN (defined wrap).
                            let no_poison = self.ctx.bool_type().const_zero();
                            self.call_intrinsic("llvm.abs", &[overload], &[ops[0].into(), no_poison.into()])?
                        } else {
                            // Unsigned abs is the identity.
                            ops[0]
                        }
                    }
                    align_sema::MathFn::Min | align_sema::MathFn::Max => {
                        let is_max = matches!(fn_, align_sema::MathFn::Max);
                        let name = if is_float {
                            // `minimum`/`maximum` (IEEE 754-2019), not `minnum`/`maxnum`: they
                            // propagate NaN and order ±0 deterministically — consistent across
                            // targets (Align values predictable, identical-across-builds results).
                            if is_max { "llvm.maximum" } else { "llvm.minimum" }
                        } else if signed {
                            if is_max { "llvm.smax" } else { "llvm.smin" }
                        } else if is_max {
                            "llvm.umax"
                        } else {
                            "llvm.umin"
                        };
                        self.call_intrinsic(name, &[overload], &[ops[0].into(), ops[1].into()])?
                    }
                    // Float-only unary intrinsics.
                    align_sema::MathFn::Sqrt => self.call_intrinsic("llvm.sqrt", &[overload], &[ops[0].into()])?,
                    align_sema::MathFn::Floor => self.call_intrinsic("llvm.floor", &[overload], &[ops[0].into()])?,
                    align_sema::MathFn::Ceil => self.call_intrinsic("llvm.ceil", &[overload], &[ops[0].into()])?,
                    align_sema::MathFn::Round => self.call_intrinsic("llvm.round", &[overload], &[ops[0].into()])?,
                    align_sema::MathFn::Trunc => self.call_intrinsic("llvm.trunc", &[overload], &[ops[0].into()])?,
                    // `pow(base, exp)`.
                    align_sema::MathFn::Pow => self.call_intrinsic("llvm.pow", &[overload], &[ops[0].into(), ops[1].into()])?,
                    // `fma(a, b, c)` = a*b + c, one rounding (scalar or vector overload).
                    align_sema::MathFn::Fma => self.call_intrinsic("llvm.fma", &[overload], &[ops[0].into(), ops[1].into(), ops[2].into()])?,
                }
            }
            Rvalue::Select { cond, a, b } => {
                // A `mask` cond (`<N x i1>`, from `select(mask, a, b)`) blends two vectors lane-wise;
                // a scalar `i1` cond (branchless `where`) blends two scalars.
                if matches!(self.f.operand_ty(cond), Ty::Mask(..)) {
                    let c = self.operand(cond).into_vector_value();
                    let av = self.operand(a).into_vector_value();
                    let bv = self.operand(b).into_vector_value();
                    self.builder.build_select(c, av, bv, "vsel").map_err(|e| self.err(e))?
                } else {
                    let c = self.operand(cond).into_int_value();
                    let av = self.operand(a);
                    let bv = self.operand(b);
                    self.builder.build_select(c, av, bv, "sel").map_err(|e| self.err(e))?
                }
            }
            Rvalue::Field(slot, path) => {
                let fty = abi_type(self.ctx, self.field_path_ty(*slot, path), self.struct_types, self.enum_types);
                let field_ptr = self.field_path_ptr(*slot, path)?;
                self.builder
                    .build_load(fty, field_ptr, "fld")
                    .map_err(|e| self.err(e))?
            }
            Rvalue::SoaColumn { base, struct_id, field } => {
                // Load the soa `{ ptr, len }`, then view the `field`-th column: the buffer is
                // column-major, so column `field` begins at `ptr + soa_column_offset(len, …)` (the
                // `align_up`-padded prefix of the preceding columns) and has the same `len`. This
                // MUST match the offset math used by `IndexColumn` / `StoreColumn` / `SoaAlloc`,
                // otherwise a materialized column slice and a per-element column read disagree.
                let sty = slice_struct_type(self.ctx);
                let soa = self
                    .builder
                    .build_load(sty, self.slots[base], "soa")
                    .map_err(|e| self.err(e))?
                    .into_struct_value();
                let ptr = self.builder.build_extract_value(soa, 0, "soaptr").map_err(|e| self.err(e))?.into_pointer_value();
                let len = self.builder.build_extract_value(soa, 1, "soalen").map_err(|e| self.err(e))?.into_int_value();
                let sizes = self.soa_field_sizes(*struct_id);
                let byte_off = self.soa_column_offset(len, &sizes, *field as usize)?;
                let new_ptr = unsafe {
                    self.builder
                        .build_in_bounds_gep(self.ctx.i8_type(), ptr, &[byte_off], "colptr")
                        .map_err(|e| self.err(e))?
                };
                let agg = self.builder.build_insert_value(sty.get_undef(), new_ptr, 0, "colptr").map_err(|e| self.err(e))?.into_struct_value();
                self.builder.build_insert_value(agg, len, 1, "collen").map_err(|e| self.err(e))?.into_struct_value().into()
            }
            Rvalue::OptionSome(op) => {
                let Ty::Option(s) = result_ty else {
                    return Err(self.err("Some result is not an Option"));
                };
                let oty = option_struct_type(self.ctx, s, self.struct_types, self.enum_types);
                let payload = self.operand(op);
                let tag = self.ctx.i8_type().const_int(1, false);
                // Start zeroed (not undef): an owned (Move) payload's drop frees the payload field
                // null-safely, so the inactive arm must read as {null,0}, not garbage (slice 8a).
                let agg = self
                    .builder
                    .build_insert_value(oty.const_zero(), tag, 0, "tag")
                    .map_err(|e| self.err(e))?
                    .into_struct_value();
                self.builder
                    .build_insert_value(agg, payload, 1, "some")
                    .map_err(|e| self.err(e))?
                    .into_struct_value()
                    .into()
            }
            Rvalue::OptionNone => {
                let Ty::Option(s) = result_ty else {
                    return Err(self.err("None result is not an Option"));
                };
                // All-zero aggregate → tag 0 (None).
                option_struct_type(self.ctx, s, self.struct_types, self.enum_types).const_zero().into()
            }
            Rvalue::OptionIsSome(op) => {
                let agg = self.operand(op).into_struct_value();
                let tag = self
                    .builder
                    .build_extract_value(agg, 0, "tag")
                    .map_err(|e| self.err(e))?
                    .into_int_value();
                self.builder
                    .build_int_compare(IntPredicate::EQ, tag, self.ctx.i8_type().const_int(1, false), "issome")
                    .map_err(|e| self.err(e))?
                    .into()
            }
            Rvalue::OptionUnwrap(op) => {
                let agg = self.operand(op).into_struct_value();
                self.builder
                    .build_extract_value(agg, 1, "some")
                    .map_err(|e| self.err(e))?
            }
            Rvalue::ResultOk(op) => {
                let Ty::Result(o, e) = result_ty else {
                    return Err(self.err("Ok result is not a Result"));
                };
                let rty = result_struct_type(self.ctx, o, e, self.struct_types, self.enum_types);
                let tag = self.ctx.i8_type().const_int(0, false);
                // Zeroed base (see OptionSome): the inactive `err` arm reads {null,0}, so an owned
                // (Move) payload there drops null-safely (slice 8a).
                let agg = self
                    .builder
                    .build_insert_value(rty.const_zero(), tag, 0, "tag")
                    .map_err(|e| self.err(e))?
                    .into_struct_value();
                self.builder
                    .build_insert_value(agg, self.operand(op), 1, "ok")
                    .map_err(|e| self.err(e))?
                    .into_struct_value()
                    .into()
            }
            Rvalue::ResultErr(op) => {
                let Ty::Result(o, e) = result_ty else {
                    return Err(self.err("Err result is not a Result"));
                };
                let rty = result_struct_type(self.ctx, o, e, self.struct_types, self.enum_types);
                let tag = self.ctx.i8_type().const_int(1, false);
                // Zeroed base (see OptionSome): the inactive `ok` arm reads {null,0}, so an owned
                // (Move) payload there drops null-safely (slice 8a).
                let agg = self
                    .builder
                    .build_insert_value(rty.const_zero(), tag, 0, "tag")
                    .map_err(|e| self.err(e))?
                    .into_struct_value();
                self.builder
                    .build_insert_value(agg, self.operand(op), 2, "err")
                    .map_err(|e| self.err(e))?
                    .into_struct_value()
                    .into()
            }
            Rvalue::ResultIsOk(op) => {
                let agg = self.operand(op).into_struct_value();
                let tag = self
                    .builder
                    .build_extract_value(agg, 0, "tag")
                    .map_err(|e| self.err(e))?
                    .into_int_value();
                self.builder
                    .build_int_compare(IntPredicate::EQ, tag, self.ctx.i8_type().const_int(0, false), "isok")
                    .map_err(|e| self.err(e))?
                    .into()
            }
            Rvalue::ResultUnwrapOk(op) => {
                let agg = self.operand(op).into_struct_value();
                self.builder
                    .build_extract_value(agg, 1, "ok")
                    .map_err(|e| self.err(e))?
            }
            Rvalue::ResultUnwrapErr(op) => {
                let agg = self.operand(op).into_struct_value();
                self.builder
                    .build_extract_value(agg, 2, "err")
                    .map_err(|e| self.err(e))?
            }
            Rvalue::MakeEnum { enum_id, variant, payload } => {
                // `{ i32 tag, … }`: store the variant tag, then this variant's payload fields.
                let sty = self.enum_types[*enum_id as usize];
                let tag = self.ctx.i32_type().const_int(*variant as u64, false);
                let mut agg = self
                    .builder
                    .build_insert_value(sty.const_zero(), tag, 0, "tag")
                    .map_err(|e| self.err(e))?
                    .into_struct_value();
                let base = self.enums[*enum_id as usize].variants[*variant as usize].field_base;
                for (j, op) in payload.iter().enumerate() {
                    agg = self
                        .builder
                        .build_insert_value(agg, self.operand(op), base + j as u32, "pl")
                        .map_err(|e| self.err(e))?
                        .into_struct_value();
                }
                agg.into()
            }
            Rvalue::EnumTagEq { scrutinee, variant, .. } => {
                let agg = self.operand(scrutinee).into_struct_value();
                let tag = self.builder.build_extract_value(agg, 0, "tag").map_err(|e| self.err(e))?.into_int_value();
                let want = self.ctx.i32_type().const_int(*variant as u64, false);
                self.builder
                    .build_int_compare(IntPredicate::EQ, tag, want, "tageq")
                    .map_err(|e| self.err(e))?
                    .into()
            }
            Rvalue::EnumPayload { enum_id, variant, slot, operand } => {
                let agg = self.operand(operand).into_struct_value();
                let base = self.enums[*enum_id as usize].variants[*variant as usize].field_base;
                self.builder
                    .build_extract_value(agg, base + *slot, "pl")
                    .map_err(|e| self.err(e))?
            }
            Rvalue::ArenaBegin => {
                let cs = self
                    .builder
                    .build_call(self.funcs["arena_begin"], &[], "arena")
                    .map_err(|e| self.err(e))?;
                cs.try_as_basic_value().basic().expect("arena_begin returns a pointer")
            }
            Rvalue::TgBegin => {
                let cs = self
                    .builder
                    .build_call(self.funcs["tg_begin"], &[], "tg")
                    .map_err(|e| self.err(e))?;
                cs.try_as_basic_value().basic().expect("tg_begin returns a pointer")
            }
            Rvalue::SpawnTask { tg, closure, capture_tys, r, fallible } => {
                let i64t = self.ctx.i64_type();
                let tgv = self.operand(tg).into_pointer_value();
                let clos = self.operand(closure).into_struct_value();
                let thunk = self.builder.build_extract_value(clos, 0, "thunk").map_err(|e| self.err(e))?;
                let frame_env = self.builder.build_extract_value(clos, 1, "fenv").map_err(|e| self.err(e))?.into_pointer_value();
                // Snapshot the captures into a fresh env in the task-group region (so a deferred
                // task reads its own captures, not a frame slot reused by a later `spawn`).
                let env: BasicValueEnum = if capture_tys.is_empty() {
                    self.ctx.ptr_type(AddressSpace::default()).const_null().into()
                } else {
                    let fields: Vec<BasicTypeEnum> = capture_tys.iter().map(|t| abi_type(self.ctx, *t, self.struct_types, self.enum_types)).collect();
                    let env_struct = self.ctx.struct_type(&fields, false);
                    let size = self.target_data.get_store_size(&env_struct);
                    let align = self.target_data.get_abi_alignment(&env_struct) as u64;
                    let re = self
                        .builder
                        .build_call(self.funcs["tg_alloc"], &[tgv.into(), i64t.const_int(size, false).into(), i64t.const_int(align, false).into()], "env")
                        .map_err(|e| self.err(e))?
                        .try_as_basic_value().basic().expect("tg_alloc returns a pointer").into_pointer_value();
                    self.builder
                        .build_memcpy(re, align as u32, frame_env, align as u32, i64t.const_int(size, false))
                        .map_err(|e| self.err(e))?;
                    re.into()
                };
                // The result slot (a `box<R>` in the region — the `Task<R>` handle).
                let rbytes = match r {
                    Ty::Int(_) | Ty::Float(_) | Ty::Bool | Ty::Char | Ty::Unit => {
                        let t = scalar_type(self.ctx, *r, self.struct_types, self.enum_types);
                        self.target_data.get_store_size(&t)
                    }
                    _ => return Err(self.err("a spawned task result must be a primitive scalar")),
                };
                let ralign = self.target_data.get_abi_alignment(&scalar_type(self.ctx, *r, self.struct_types, self.enum_types)) as u64;
                let slot = self
                    .builder
                    .build_call(self.funcs["tg_alloc"], &[tgv.into(), i64t.const_int(rbytes, false).into(), i64t.const_int(ralign, false).into()], "slot")
                    .map_err(|e| self.err(e))?
                    .try_as_basic_value().basic().expect("tg_alloc returns a pointer").into_pointer_value();
                // A fallible task also gets an `err_slot` (sized for the `Error` enum) the trampoline
                // writes its `Err` value into; a non-fallible task passes null.
                let err_slot = if *fallible {
                    let eid = self.enums.iter().position(|e| e.name == "Error").expect("Error enum registered");
                    let ety = self.enum_types[eid];
                    let ebytes = self.target_data.get_store_size(&ety);
                    let ealign = self.target_data.get_abi_alignment(&ety) as u64;
                    self.builder
                        .build_call(self.funcs["tg_alloc"], &[tgv.into(), i64t.const_int(ebytes, false).into(), i64t.const_int(ealign, false).into()], "errslot")
                        .map_err(|e| self.err(e))?
                        .try_as_basic_value().basic().expect("tg_alloc returns a pointer").into_pointer_value()
                } else {
                    self.ctx.ptr_type(AddressSpace::default()).const_null()
                };
                // The per-(R, fallibility) trampoline runs the closure and writes the slot at `wait`.
                let tramp = self.funcs[&format!("tramp${}", spawn_tramp_key(*r, *fallible))].as_global_value().as_pointer_value();
                self.builder
                    .build_call(self.funcs["tg_register"], &[tgv.into(), tramp.into(), thunk.into(), env.into(), slot.into(), err_slot.into()], "")
                    .map_err(|e| self.err(e))?;
                slot.into()
            }
            Rvalue::TgWaitResult { tg, fallible } => {
                let tgv = self.operand(tg).into();
                // `tg_wait` returns the first errored task's `err_slot` (null if all succeeded).
                let errp = self
                    .builder
                    .build_call(self.funcs["tg_wait"], &[tgv], "tgwait")
                    .map_err(|e| self.err(e))?
                    .try_as_basic_value()
                    .basic()
                    .expect("tg_wait returns a pointer")
                    .into_pointer_value();
                if *fallible {
                    // Build `Result<(), Error>`: null `errp` → `Ok(())`; else load the `Error` from
                    // `errp` → `Err(error)`. A branch (loading null would be UB), result via a slot.
                    let Ty::Result(o, e) = result_ty else {
                        return Err(self.err("wait result is not a Result"));
                    };
                    let rty = result_struct_type(self.ctx, o, e, self.struct_types, self.enum_types);
                    let ety = scalar_type(self.ctx, scalar_to_ty(e), self.struct_types, self.enum_types);
                    let rslot = self.alloca_at_entry(rty.into(), "waitr")?;
                    let is_err = self
                        .builder
                        .build_is_not_null(errp, "iserr")
                        .map_err(|e| self.err(e))?;
                    let err_bb = self.ctx.append_basic_block(self.func, "waiterr");
                    let ok_bb = self.ctx.append_basic_block(self.func, "waitok");
                    let join_bb = self.ctx.append_basic_block(self.func, "waitjoin");
                    self.builder.build_conditional_branch(is_err, err_bb, ok_bb).map_err(|e| self.err(e))?;
                    // Err: tag 1, err = *errp.
                    self.builder.position_at_end(err_bb);
                    let errv = self.builder.build_load(ety, errp, "errv").map_err(|e| self.err(e))?;
                    let e0 = self
                        .builder
                        .build_insert_value(rty.const_zero(), self.ctx.i8_type().const_int(1, false), 0, "etag")
                        .map_err(|e| self.err(e))?
                        .into_struct_value();
                    let ev = self.builder.build_insert_value(e0, errv, 2, "eerr").map_err(|e| self.err(e))?.into_struct_value();
                    self.builder.build_store(rslot, ev).map_err(|e| self.err(e))?;
                    self.builder.build_unconditional_branch(join_bb).map_err(|e| self.err(e))?;
                    // Ok: a zeroed Result (tag 0).
                    self.builder.position_at_end(ok_bb);
                    self.builder.build_store(rslot, rty.const_zero()).map_err(|e| self.err(e))?;
                    self.builder.build_unconditional_branch(join_bb).map_err(|e| self.err(e))?;
                    self.builder.position_at_end(join_bb);
                    self.builder.build_load(rty, rslot, "waitres").map_err(|e| self.err(e))?
                } else {
                    // Infallible group: ignore the (always-null) pointer, yield `()`.
                    self.ctx.i32_type().const_zero().into()
                }
            }
            Rvalue::HeapAlloc(handle, init) => {
                // A `box<T>` (heap.new) or a `Task<R>` (spawn) — both a boxed scalar in an arena.
                let (Ty::Box(s) | Ty::Task(s)) = result_ty else {
                    return Err(self.err("heap allocation result is not a box or task"));
                };
                let i64t = self.ctx.i64_type();
                let bytes = scalar_bytes(s);
                let argv = [
                    self.operand(handle).into(),
                    i64t.const_int(bytes, false).into(),
                    i64t.const_int(bytes, false).into(),
                ];
                let ptr = self
                    .builder
                    .build_call(self.funcs["arena_alloc"], &argv, "box")
                    .map_err(|e| self.err(e))?
                    .try_as_basic_value()
                    .basic()
                    .expect("arena_alloc returns a pointer")
                    .into_pointer_value();
                self.builder
                    .build_store(ptr, self.operand(init))
                    .map_err(|e| self.err(e))?;
                ptr.into()
            }
            Rvalue::RawAlloc(size) => {
                // `raw.alloc(size)` → `align_rt_alloc(size) -> ptr` (a flat libc `malloc`). The size
                // is a byte count (a non-negative quantity), so widen it to the i64 runtime signature
                // **zero-extending** — a narrower unsigned size with its MSB set (e.g. a `u32` ≥ 2 GiB)
                // must not become negative. `build_int_cast_sign_flag(.., false)` is a no-op at i64,
                // zero-extends narrower widths, and truncates wider ones. The result `raw` is a `ptr`.
                let sz = self.operand(size);
                let i64t = self.ctx.i64_type();
                let sz64 = self
                    .builder
                    .build_int_cast_sign_flag(sz.into_int_value(), i64t, false, "sizew")
                    .map_err(|e| self.err(e))?;
                self.builder
                    .build_call(self.funcs["alloc"], &[sz64.into()], "rawptr")
                    .map_err(|e| self.err(e))?
                    .try_as_basic_value()
                    .basic()
                    .expect("align_rt_alloc returns a pointer")
            }
            Rvalue::RawLoad { ptr, offset, scalar } => {
                // `raw.load(p, off)` → load the scalar at `p + off` bytes. GEP by the byte offset,
                // then load `scalar_type(scalar)`. An arbitrary byte offset may be misaligned for the
                // scalar, so force alignment 1 (an unaligned load) — always correct, never LLVM-UB.
                let ep = self.raw_elem_ptr(ptr, offset)?;
                let lty = scalar_type(self.ctx, align_sema::scalar_to_ty(*scalar), self.struct_types, self.enum_types);
                let loaded = self.builder.build_load(lty, ep, "rawval").map_err(|e| self.err(e))?;
                // The loaded type is a raw scalar (int/bool/char or float) or a `layout(C)` struct;
                // set the load's alignment to 1 (an arbitrary byte offset may be misaligned) via the
                // concrete value's instruction.
                let inst = match loaded {
                    inkwell::values::BasicValueEnum::IntValue(v) => v.as_instruction(),
                    inkwell::values::BasicValueEnum::FloatValue(v) => v.as_instruction(),
                    inkwell::values::BasicValueEnum::StructValue(v) => v.as_instruction(),
                    _ => None,
                };
                inst.ok_or_else(|| self.err("raw load is not an instruction"))?.set_alignment(1).map_err(|e| self.err(e))?;
                loaded
            }
            Rvalue::RawOffset { ptr, offset } => {
                // `raw.offset(p, n)` → `p + n` bytes, as a new `raw` pointer (a plain i8 GEP, no
                // `inbounds` — unsafe pointer arithmetic must stay well-defined out of bounds).
                self.raw_elem_ptr(ptr, offset)?.into()
            }
            Rvalue::BoxGet(op) => {
                let ty = scalar_type(self.ctx, result_ty, self.struct_types, self.enum_types);
                let ptr = self.operand(op).into_pointer_value();
                self.builder
                    .build_load(ty, ptr, "boxget")
                    .map_err(|e| self.err(e))?
            }
            Rvalue::Index(slot, idx) => {
                let ep = self.elem_ptr(*slot, idx)?;
                let ty = scalar_type(self.ctx, result_ty, self.struct_types, self.enum_types);
                self.builder.build_load(ty, ep, "idx").map_err(|e| self.err(e))?
            }
            Rvalue::IndexField(slot, idx, field) => {
                let ep = self.elem_field_ptr(*slot, idx, *field)?;
                let ty = abi_type(self.ctx, result_ty, self.struct_types, self.enum_types);
                self.builder.build_load(ty, ep, "idxfld").map_err(|e| self.err(e))?
            }
            // Build `<n x elem>` via an insertelement chain over a poison vector (M6).
            Rvalue::MakeVec { elems, elem, n } => {
                let vty = vec_llvm_ty(self.ctx, *elem, *n).into_vector_type();
                let mut acc = vty.get_undef();
                for (i, op) in elems.iter().enumerate() {
                    let val = self.operand(op);
                    let idx = self.ctx.i32_type().const_int(i as u64, false);
                    acc = self.builder.build_insert_element(acc, val, idx, "vins").map_err(|e| self.err(e))?;
                }
                acc.into()
            }
            // Read lane `lane` of a vector (`extractelement`).
            Rvalue::VecExtract { vec, lane, .. } => {
                let v = self.operand(vec).into_vector_value();
                let idx = self.ctx.i32_type().const_int(*lane as u64, false);
                self.builder.build_extract_element(v, idx, "vext").map_err(|e| self.err(e))?
            }
            // Write `value` into lane `lane` (`insertelement`), yielding the new vector.
            Rvalue::VecInsert { vec, value, lane } => {
                let v = self.operand(vec).into_vector_value();
                let val = self.operand(value);
                let idx = self.ctx.i32_type().const_int(*lane as u64, false);
                self.builder.build_insert_element(v, val, idx, "vins").map_err(|e| self.err(e))?.into()
            }
            // `vec.sum_where(mask)` — `select(mask, vec, 0)` then add all N lanes (M6).
            Rvalue::VecSumWhere { vec, mask, elem, n } => {
                let v = self.operand(vec).into_vector_value();
                let m = self.operand(mask).into_vector_value();
                let zero = vec_llvm_ty(self.ctx, *elem, *n).into_vector_type().const_zero();
                let masked = self.builder.build_select(m, v, zero, "swsel").map_err(|e| self.err(e))?.into_vector_value();
                self.horizontal_sum(masked, matches!(elem, Ty::Float(_)), *n)?
            }
            // `dot(a, b)` — multiply lane-wise, then a horizontal sum.
            Rvalue::VecDot { a, b, elem, n } => {
                let av = self.operand(a).into_vector_value();
                let bv = self.operand(b).into_vector_value();
                let is_float = matches!(elem, Ty::Float(_));
                let prod = if is_float {
                    self.builder.build_float_mul(av, bv, "dotmul").map_err(|e| self.err(e))?
                } else {
                    self.builder.build_int_mul(av, bv, "dotmul").map_err(|e| self.err(e))?
                };
                self.horizontal_sum(prod, is_float, *n)?
            }
            // `v.min()` / `v.max()` — fold the lanes with the scalar min/max intrinsic.
            Rvalue::VecMinMax { vec, elem, n, max } => {
                let v = self.operand(vec).into_vector_value();
                self.horizontal_minmax(v, *elem, *n, *max)?
            }
            // `v.sum()` — add all lanes (the shared horizontal sum).
            Rvalue::VecSum { vec, elem, n } => {
                let v = self.operand(vec).into_vector_value();
                self.horizontal_sum(v, matches!(elem, Ty::Float(_)), *n)?
            }
            // `s.load(i)` — `<n x T>` load from `&buf[i]` at the element alignment (the GEP yields an
            // element-aligned pointer, so the vector load must NOT assume the wider vector alignment).
            Rvalue::VecLoad { slice, index, elem, n } => {
                let sv = self.operand(slice).into_struct_value();
                let buf = self.builder.build_extract_value(sv, 0, "vlbuf").map_err(|e| self.err(e))?.into_pointer_value();
                let index = self.operand(index).into_int_value();
                let elem_lt = scalar_type(self.ctx, *elem, self.struct_types, self.enum_types);
                let ep = unsafe {
                    self.builder.build_in_bounds_gep(elem_lt, buf, &[index], "vloadgep").map_err(|e| self.err(e))?
                };
                let vty = vec_llvm_ty(self.ctx, *elem, *n).into_vector_type();
                let loaded = self.builder.build_load(vty, ep, "vload").map_err(|e| self.err(e))?;
                loaded
                    .into_vector_value()
                    .as_instruction()
                    .ok_or_else(|| self.err("vector load is not an instruction"))?
                    .set_alignment(self.type_align(*elem))
                    .map_err(|e| self.err(e))?;
                loaded
            }
            Rvalue::IndexFieldPtr { base, index, field, struct_id } => {
                // `base` is a `{ptr,len}` view of `[%Struct]`; GEP `%Struct, ptr, index, field`.
                let agg = self.operand(base).into_struct_value();
                let buf = self.builder.build_extract_value(agg, 0, "aosptr").map_err(|e| self.err(e))?.into_pointer_value();
                let st = self.struct_types[*struct_id as usize];
                let index = self.operand(index).into_int_value();
                let f = self.ctx.i32_type().const_int(self.pfield(*struct_id, *field) as u64, false);
                let ep = unsafe {
                    self.builder
                        .build_in_bounds_gep(st, buf, &[index, f], "aosfield")
                        .map_err(|e| self.err(e))?
                };
                let ty = abi_type(self.ctx, result_ty, self.struct_types, self.enum_types);
                self.builder.build_load(ty, ep, "idxfldp").map_err(|e| self.err(e))?
            }
            Rvalue::IndexColumn { base, index, field, struct_id } => {
                // `base` is a `{ptr,len}` column-major soa buffer. Each column j occupies
                // `len * size_j` bytes; its start is padded up to the field's alignment (= its size
                // for a primitive), so mixed-width columns (`bool` + `i64`) stay naturally aligned
                // for any `len`. Walk to column `field`'s byte offset, then element `index` is
                // `column_base + index*size_field`. Reads only the touched column.
                let agg = self.operand(base).into_struct_value();
                let buf = self.builder.build_extract_value(agg, 0, "soaptr").map_err(|e| self.err(e))?.into_pointer_value();
                let len = self.builder.build_extract_value(agg, 1, "soalen").map_err(|e| self.err(e))?.into_int_value();
                let sizes = self.soa_field_sizes(*struct_id);
                let off = self.soa_column_offset(len, &sizes, *field as usize)?;
                let col_base = unsafe {
                    self.builder.build_in_bounds_gep(self.ctx.i8_type(), buf, &[off], "colbase").map_err(|e| self.err(e))?
                };
                let fty = scalar_type(self.ctx, self.structs[*struct_id as usize].fields[*field as usize].ty, self.struct_types, self.enum_types);
                let index = self.operand(index).into_int_value();
                let ep = unsafe {
                    self.builder.build_in_bounds_gep(fty, col_base, &[index], "colelem").map_err(|e| self.err(e))?
                };
                let ty = abi_type(self.ctx, result_ty, self.struct_types, self.enum_types);
                self.builder.build_load(ty, ep, "idxcol").map_err(|e| self.err(e))?
            }
            // `s[index]` — gather a whole struct from a soa: load every column's element at `index`
            // and build the struct aggregate (the multi-column counterpart of `IndexColumn`).
            Rvalue::SoaGather { base, index, struct_id } => {
                let agg = self.operand(base).into_struct_value();
                let buf = self.builder.build_extract_value(agg, 0, "soaptr").map_err(|e| self.err(e))?.into_pointer_value();
                let len = self.builder.build_extract_value(agg, 1, "soalen").map_err(|e| self.err(e))?.into_int_value();
                let index = self.operand(index).into_int_value();
                let sizes = self.soa_field_sizes(*struct_id);
                let st = self.struct_types[*struct_id as usize];
                let fields = &self.structs[*struct_id as usize].fields;
                let mut acc = st.get_undef();
                for (f, field) in fields.iter().enumerate() {
                    let off = self.soa_column_offset(len, &sizes, f)?;
                    let col_base = unsafe {
                        self.builder.build_in_bounds_gep(self.ctx.i8_type(), buf, &[off], "gcolbase").map_err(|e| self.err(e))?
                    };
                    let fty = scalar_type(self.ctx, field.ty, self.struct_types, self.enum_types);
                    let ep = unsafe {
                        self.builder.build_in_bounds_gep(fty, col_base, &[index], "gcolelem").map_err(|e| self.err(e))?
                    };
                    let val = self.builder.build_load(fty, ep, "gload").map_err(|e| self.err(e))?;
                    // Column `f` is logical (soa layout is declaration-ordered); insert it at its
                    // physical slot in the reordered AoS struct aggregate.
                    acc = self.builder.build_insert_value(acc, val, self.pfield(*struct_id, f as u32), "ginsert").map_err(|e| self.err(e))?.into_struct_value();
                }
                acc.into()
            }
            Rvalue::IndexPtr { base, index, struct_id } => {
                // `base` is a `{ptr,len}` view of `[%Struct]`; GEP `%Struct, ptr, index` and load
                // the whole element (a `map(f)` consuming the struct by value).
                let agg = self.operand(base).into_struct_value();
                let buf = self.builder.build_extract_value(agg, 0, "aosptr").map_err(|e| self.err(e))?.into_pointer_value();
                let st = self.struct_types[*struct_id as usize];
                let index = self.operand(index).into_int_value();
                let ep = unsafe {
                    self.builder
                        .build_in_bounds_gep(st, buf, &[index], "aoselem")
                        .map_err(|e| self.err(e))?
                };
                self.builder.build_load(st, ep, "idxp").map_err(|e| self.err(e))?
            }
            Rvalue::MakeTuple { tuple_id, elems } => {
                // Build the tuple aggregate by inserting each element into an undef struct.
                let st = self.tuple_types[*tuple_id as usize];
                let mut agg = st.get_undef();
                for (i, el) in elems.iter().enumerate() {
                    let v = self.operand(el);
                    agg = self
                        .builder
                        .build_insert_value(agg, v, i as u32, "tup")
                        .map_err(|e| self.err(e))?
                        .into_struct_value();
                }
                agg.into()
            }
            Rvalue::TupleIndex { tuple, index } => {
                let agg = self.operand(tuple).into_struct_value();
                self.builder
                    .build_extract_value(agg, *index, "tupidx")
                    .map_err(|e| self.err(e))?
            }
            Rvalue::MakeSlice(slot, n) => {
                // ptr = &slot[0]; build { ptr, len } from the array alloca.
                let arr_ty = self.llvm_type(self.f.slots[*slot as usize]);
                let zero = self.ctx.i64_type().const_zero();
                let ptr0 = unsafe {
                    self.builder
                        .build_in_bounds_gep(arr_ty, self.slots[slot], &[zero, zero], "slcbase")
                        .map_err(|e| self.err(e))?
                };
                let sty = slice_struct_type(self.ctx);
                let agg = self
                    .builder
                    .build_insert_value(sty.get_undef(), ptr0, 0, "slcptr")
                    .map_err(|e| self.err(e))?
                    .into_struct_value();
                let len = self.ctx.i64_type().const_int(*n as u64, false);
                self.builder
                    .build_insert_value(agg, len, 1, "slclen")
                    .map_err(|e| self.err(e))?
                    .into_struct_value()
                    .into()
            }
            Rvalue::ArenaAlloc { handle, count, elem } => {
                // bytes = count * sizeof(elem); align = sizeof(elem). Bump-allocate in the arena.
                let scalar = align_sema::ty_to_scalar(*elem).expect("ArenaAlloc elem must be a scalar");
                let i64t = self.ctx.i64_type();
                let elem_bytes = i64t.const_int(scalar_bytes(scalar), false);
                let count_v = self.operand(count).into_int_value();
                let bytes = self.builder.build_int_mul(count_v, elem_bytes, "bytes").map_err(|e| self.err(e))?;
                self.builder
                    .build_call(self.funcs["arena_alloc"], &[self.operand(handle).into(), bytes.into(), elem_bytes.into()], "buf")
                    .map_err(|e| self.err(e))?
                    .try_as_basic_value()
                    .basic()
                    .expect("arena_alloc returns a pointer")
            }
            Rvalue::HeapAllocBuf { count, elem } => {
                // bytes = count * sizeof(elem); heap-allocate (freed by a later Drop).
                let scalar = align_sema::ty_to_scalar(*elem).expect("HeapAllocBuf elem must be a scalar");
                let i64t = self.ctx.i64_type();
                let elem_bytes = i64t.const_int(scalar_bytes(scalar), false);
                let count_v = self.operand(count).into_int_value();
                let bytes = self.builder.build_int_mul(count_v, elem_bytes, "bytes").map_err(|e| self.err(e))?;
                self.builder
                    .build_call(self.funcs["alloc"], &[bytes.into()], "buf")
                    .map_err(|e| self.err(e))?
                    .try_as_basic_value()
                    .basic()
                    .expect("alloc returns a pointer")
            }
            Rvalue::SoaAlloc { handle, len, struct_id } => {
                // Bump-allocate the column-major buffer for `len` rows. Total bytes = the end of the
                // last column: the column-offset walk to the last field, plus that column's own
                // `len*size` bytes. Buffer align = the widest field (so each column's `align_up`
                // padding, computed relative to base, actually lands on an aligned address).
                let len_v = self.operand(len).into_int_value();
                let sizes = self.soa_field_sizes(*struct_id);
                // A soa struct always has ≥1 field (sema-enforced); guard the underflow anyway.
                let last = sizes.len().checked_sub(1).ok_or_else(|| self.err("empty soa struct"))?;
                let i64t = self.ctx.i64_type();
                let last_off = self.soa_column_offset(len_v, &sizes, last)?;
                let last_bytes = self.builder.build_int_mul(len_v, i64t.const_int(sizes[last], false), "lastcol").map_err(|e| self.err(e))?;
                let total = self.builder.build_int_add(last_off, last_bytes, "soabytes").map_err(|e| self.err(e))?;
                let max_align = sizes.iter().copied().max().unwrap_or(1);
                self.builder
                    .build_call(self.funcs["arena_alloc"], &[self.operand(handle).into(), total.into(), i64t.const_int(max_align, false).into()], "soabuf")
                    .map_err(|e| self.err(e))?
                    .try_as_basic_value()
                    .basic()
                    .expect("arena_alloc returns a pointer")
            }
            Rvalue::MakeDynArray { ptr, len } => {
                // Build the owned array value `{ ptr, len }` (same layout as a slice).
                let p = self.operand(ptr);
                let l = self.operand(len);
                let sty = slice_struct_type(self.ctx);
                let agg = self
                    .builder
                    .build_insert_value(sty.get_undef(), p, 0, "arrptr")
                    .map_err(|e| self.err(e))?
                    .into_struct_value();
                self.builder
                    .build_insert_value(agg, l, 1, "arrlen")
                    .map_err(|e| self.err(e))?
                    .into_struct_value()
                    .into()
            }
            Rvalue::GroupAgg { keys, vals, out_keys, out_vals, op } => {
                // Extract the key/value column pointers + length from the `{ptr,len}` slices and call
                // the runtime hash-aggregate for the op; `cap` = the column length (an upper bound on
                // groups). `count` has no value column (the runtime entry point takes no values).
                use align_sema::hir::GroupOp;
                let kagg = self.operand(keys).into_struct_value();
                let kptr = self.builder.build_extract_value(kagg, 0, "kptr").map_err(|e| self.err(e))?;
                let klen = self.builder.build_extract_value(kagg, 1, "klen").map_err(|e| self.err(e))?;
                let ok = self.operand(out_keys);
                let ov = self.operand(out_vals);
                let call = if let GroupOp::Count = op {
                    self.builder.build_call(
                        self.funcs["group_count_i64"],
                        &[kptr.into(), klen.into(), ok.into(), ov.into(), klen.into()],
                        "groupagg",
                    )
                } else {
                    let vptr = self
                        .builder
                        .build_extract_value(self.operand(vals).into_struct_value(), 0, "vptr")
                        .map_err(|e| self.err(e))?;
                    let f = match op {
                        GroupOp::Sum => "group_sum_i64",
                        GroupOp::Min => "group_min_i64",
                        GroupOp::Max => "group_max_i64",
                        GroupOp::Count => unreachable!(),
                    };
                    self.builder.build_call(
                        self.funcs[f],
                        &[kptr.into(), vptr.into(), klen.into(), ok.into(), ov.into(), klen.into()],
                        "groupagg",
                    )
                };
                call.map_err(|e| self.err(e))?
                    .try_as_basic_value()
                    .basic()
                    .expect("group aggregate returns the group count (i64)")
            }
            Rvalue::GroupAggStrCols { keys, vals, out_keys, out_vals, op } => {
                // The soa str-key form: extract the str key-column ptr + length and the i64
                // value-column ptr from the two `{ptr,len}` column slices, and call the runtime
                // two-column str aggregate. `cap` = column length (upper bound on groups). All four
                // ops share one signature; `count` ignores the value ptr (which is the key column).
                use align_sema::hir::GroupOp;
                let kagg = self.operand(keys).into_struct_value();
                let kptr = self.builder.build_extract_value(kagg, 0, "kptr").map_err(|e| self.err(e))?;
                let klen = self.builder.build_extract_value(kagg, 1, "klen").map_err(|e| self.err(e))?;
                let vptr = self
                    .builder
                    .build_extract_value(self.operand(vals).into_struct_value(), 0, "vptr")
                    .map_err(|e| self.err(e))?;
                let ok = self.operand(out_keys);
                let ov = self.operand(out_vals);
                let f = match op {
                    GroupOp::Sum => "group_sum_str_cols",
                    GroupOp::Min => "group_min_str_cols",
                    GroupOp::Max => "group_max_str_cols",
                    GroupOp::Count => "group_count_str_cols",
                };
                self.builder
                    .build_call(
                        self.funcs[f],
                        &[kptr.into(), vptr.into(), klen.into(), ok.into(), ov.into(), klen.into()],
                        "groupaggstrcols",
                    )
                    .map_err(|e| self.err(e))?
                    .try_as_basic_value()
                    .basic()
                    .expect("group aggregate returns the group count (i64)")
            }
            Rvalue::GroupAggStr { base, struct_id, key_field, value_field, op, out_keys, out_vals } => {
                // Load the AoS array `{ptr,len}`, derive the per-row stride (= the struct's alloc
                // size, LLVM's `[%Struct]` element stride) and the key/value byte offsets from the
                // struct layout, and call the runtime dictionary-encoding aggregate for the op. `cap`
                // = the row count (an upper bound on the group count).
                use align_sema::hir::GroupOp;
                let st = self.struct_types[*struct_id as usize];
                let store = self.target_data.get_store_size(&st);
                let align = self.target_data.get_abi_alignment(&st) as u64;
                let stride = store.div_ceil(align) * align; // alloc size = align_up(store, align)
                // Field indices are logical (MIR); `field_byte_offset` maps to the physical slot.
                let key_off = self.field_byte_offset(*struct_id, *key_field);
                // `count` has no value field; the runtime entry ignores `val_off`, so pass 0.
                let val_off = value_field.map(|v| self.field_byte_offset(*struct_id, v)).unwrap_or(0);
                let f = match op {
                    GroupOp::Sum => "group_sum_str",
                    GroupOp::Min => "group_min_str",
                    GroupOp::Max => "group_max_str",
                    GroupOp::Count => "group_count_str",
                };
                let agg = self.builder.build_load(slice_struct_type(self.ctx), self.slots[base], "aosbase").map_err(|e| self.err(e))?.into_struct_value();
                let bptr = self.builder.build_extract_value(agg, 0, "bptr").map_err(|e| self.err(e))?;
                let blen = self.builder.build_extract_value(agg, 1, "blen").map_err(|e| self.err(e))?;
                let i64t = self.ctx.i64_type();
                let ok = self.operand(out_keys);
                let ov = self.operand(out_vals);
                self.builder
                    .build_call(
                        self.funcs[f],
                        &[
                            bptr.into(),
                            blen.into(),
                            i64t.const_int(stride, false).into(),
                            i64t.const_int(key_off, false).into(),
                            i64t.const_int(val_off, false).into(),
                            ok.into(),
                            ov.into(),
                            blen.into(),
                        ],
                        "groupstr",
                    )
                    .map_err(|e| self.err(e))?
                    .try_as_basic_value()
                    .basic()
                    .expect("str group aggregate returns the group count (i64)")
            }
            Rvalue::GroupAggMultiStr { base, struct_id, key_field, aggs, out_keys, out_vals } => {
                // Like `GroupAggStr` but builds a `[k x {i64 val_off, i64 op, ptr out_vals}]` spec
                // table on the stack and calls the one-pass fused runtime.
                use align_sema::hir::GroupOp;
                let st = self.struct_types[*struct_id as usize];
                let store = self.target_data.get_store_size(&st);
                let align = self.target_data.get_abi_alignment(&st) as u64;
                let stride = store.div_ceil(align) * align;
                let key_off = self.field_byte_offset(*struct_id, *key_field);
                let i64t = self.ctx.i64_type();
                let ptr_ty = self.ctx.ptr_type(AddressSpace::default());
                let spec_ty = self.ctx.struct_type(&[i64t.into(), i64t.into(), ptr_ty.into()], false);
                let k = aggs.len() as u64;
                // One alloca for the whole spec array (hoisted to the entry block).
                let specs_arr = self.alloca_at_entry(spec_ty.array_type(k as u32).into(), "gmspecs")?;
                for (j, ((op, value_field), out)) in aggs.iter().zip(out_vals.iter()).enumerate() {
                    let val_off = value_field
                        .map(|v| self.field_byte_offset(*struct_id, v))
                        .unwrap_or(0);
                    let op_tag = match op {
                        GroupOp::Sum => 0,
                        GroupOp::Min => 1,
                        GroupOp::Max => 2,
                        GroupOp::Count => 3,
                    };
                    let entry = unsafe {
                        self.builder
                            .build_in_bounds_gep(spec_ty.array_type(k as u32), specs_arr, &[i64t.const_zero(), i64t.const_int(j as u64, false)], "gmspec")
                            .map_err(|e| self.err(e))?
                    };
                    let mut spec_val = spec_ty.get_undef();
                    spec_val = self.builder.build_insert_value(spec_val, i64t.const_int(val_off, false), 0, "gmvoff").map_err(|e| self.err(e))?.into_struct_value();
                    spec_val = self.builder.build_insert_value(spec_val, i64t.const_int(op_tag, false), 1, "gmop").map_err(|e| self.err(e))?.into_struct_value();
                    spec_val = self.builder.build_insert_value(spec_val, self.operand(out), 2, "gmout").map_err(|e| self.err(e))?.into_struct_value();
                    self.builder.build_store(entry, spec_val).map_err(|e| self.err(e))?;
                }
                let agg = self.builder.build_load(slice_struct_type(self.ctx), self.slots[base], "aosbase").map_err(|e| self.err(e))?.into_struct_value();
                let bptr = self.builder.build_extract_value(agg, 0, "bptr").map_err(|e| self.err(e))?;
                let blen = self.builder.build_extract_value(agg, 1, "blen").map_err(|e| self.err(e))?;
                let ok = self.operand(out_keys);
                self.builder
                    .build_call(
                        self.funcs["group_multi_str"],
                        &[
                            bptr.into(),
                            blen.into(),
                            i64t.const_int(stride, false).into(),
                            i64t.const_int(key_off, false).into(),
                            specs_arr.into(),
                            i64t.const_int(k, false).into(),
                            ok.into(),
                            blen.into(),
                        ],
                        "groupmultistr",
                    )
                    .map_err(|e| self.err(e))?
                    .try_as_basic_value()
                    .basic()
                    .expect("multi-aggregate str group returns the group count (i64)")
            }
            Rvalue::DictEncode { base, struct_id, key_field, out_ids, out_dict } => {
                // Load the AoS `{ptr,len}`, derive the per-row stride + key byte offset (like
                // `GroupAggStr`), and intern the str key column → dense ids + dictionary. `cap` = the
                // row count (an upper bound on the distinct count).
                let st = self.struct_types[*struct_id as usize];
                let store = self.target_data.get_store_size(&st);
                let align = self.target_data.get_abi_alignment(&st) as u64;
                let stride = store.div_ceil(align) * align;
                let key_off = self.field_byte_offset(*struct_id, *key_field);
                let agg = self.builder.build_load(slice_struct_type(self.ctx), self.slots[base], "encbase").map_err(|e| self.err(e))?.into_struct_value();
                let bptr = self.builder.build_extract_value(agg, 0, "encptr").map_err(|e| self.err(e))?;
                let blen = self.builder.build_extract_value(agg, 1, "enclen").map_err(|e| self.err(e))?;
                let i64t = self.ctx.i64_type();
                let oi = self.operand(out_ids);
                let od = self.operand(out_dict);
                self.builder
                    .build_call(
                        self.funcs["dict_encode_str"],
                        &[bptr.into(), blen.into(), i64t.const_int(stride, false).into(), i64t.const_int(key_off, false).into(), oi.into(), od.into(), blen.into()],
                        "dictenc",
                    )
                    .map_err(|e| self.err(e))?
                    .try_as_basic_value()
                    .basic()
                    .expect("dict_encode returns the dictionary size (i64)")
            }
            Rvalue::MakeDictEncoded { source, ids, dict } => {
                // Assemble the 3-slice `dict_encoded` aggregate `{ source, ids, dict }`.
                let ty = dictenc_struct_type(self.ctx);
                let s = self.operand(source);
                let i = self.operand(ids);
                let d = self.operand(dict);
                let agg = self.builder.build_insert_value(ty.get_undef(), s, 0, "encsrc").map_err(|e| self.err(e))?.into_struct_value();
                let agg = self.builder.build_insert_value(agg, i, 1, "encids").map_err(|e| self.err(e))?.into_struct_value();
                self.builder.build_insert_value(agg, d, 2, "encdict").map_err(|e| self.err(e))?.into_struct_value().into()
            }
            Rvalue::DictField { base, idx } => {
                // Extract one `{ptr,len}` slice (0 = source, 1 = ids, 2 = dict) from a `dict_encoded` slot.
                let agg = self.builder.build_load(dictenc_struct_type(self.ctx), self.slots[base], "encfldload").map_err(|e| self.err(e))?.into_struct_value();
                self.builder.build_extract_value(agg, *idx, "encfld").map_err(|e| self.err(e))?
            }
            Rvalue::GatherColumnI64 { source, struct_id, field, out } => {
                // Copy the strided i64 `field` column of the AoS `source` into the contiguous `out`.
                let st = self.struct_types[*struct_id as usize];
                let store = self.target_data.get_store_size(&st);
                let align = self.target_data.get_abi_alignment(&st) as u64;
                let stride = store.div_ceil(align) * align;
                let off = self.field_byte_offset(*struct_id, *field);
                let agg = self.operand(source).into_struct_value();
                let sptr = self.builder.build_extract_value(agg, 0, "gthptr").map_err(|e| self.err(e))?;
                let slen = self.builder.build_extract_value(agg, 1, "gthlen").map_err(|e| self.err(e))?;
                let i64t = self.ctx.i64_type();
                let o = self.operand(out);
                self.builder
                    .build_call(
                        self.funcs["gather_i64"],
                        &[sptr.into(), slen.into(), i64t.const_int(stride, false).into(), i64t.const_int(off, false).into(), o.into()],
                        "",
                    )
                    .map_err(|e| self.err(e))?;
                return Ok(None);
            }
            Rvalue::DictLookup { ids, n, dict, out } => {
                // Label a dense-id column back to str views: out[i] = dict[ids[i]].
                let ids_ptr = self.operand(ids);
                let nn = self.operand(n);
                let dagg = self.operand(dict).into_struct_value();
                let dptr = self.builder.build_extract_value(dagg, 0, "dictptr").map_err(|e| self.err(e))?;
                let dlen = self.builder.build_extract_value(dagg, 1, "dictlen").map_err(|e| self.err(e))?;
                let o = self.operand(out);
                self.builder
                    .build_call(self.funcs["dict_lookup"], &[ids_ptr.into(), nn.into(), dptr.into(), dlen.into(), o.into()], "")
                    .map_err(|e| self.err(e))?;
                return Ok(None);
            }
            Rvalue::Chunks { src, n, elem } => {
                // Split the `{ptr,len}` `src` into length-`n` slices via the runtime; the result is
                // the chunk array's `{chunk_buf, count}` (also a `{ptr,len}`).
                let agg = self.operand(src).into_struct_value();
                let src_ptr = self.builder.build_extract_value(agg, 0, "srcptr").map_err(|e| self.err(e))?;
                let src_len = self.builder.build_extract_value(agg, 1, "srclen").map_err(|e| self.err(e))?;
                let n = self.operand(n);
                let scalar = align_sema::ty_to_scalar(*elem).expect("chunks element is a scalar");
                let esz = self.ctx.i64_type().const_int(scalar_bytes(scalar), false);
                self.builder
                    .build_call(self.funcs["chunks"], &[src_ptr.into(), src_len.into(), n.into(), esz.into()], "chunks")
                    .map_err(|e| self.err(e))?
                    .try_as_basic_value()
                    .basic()
                    .expect("align_rt_chunks returns a {ptr,len}")
            }
            Rvalue::ParMapParallel { src, func, elem_in, elem_out } => {
                // Heap-allocate the output buffer, then run `func` over the input in parallel via
                // a per-`func` thunk; the result is the owned `{ out_buf, count }` array.
                let agg = self.operand(src).into_struct_value();
                let in_ptr = self.builder.build_extract_value(agg, 0, "inptr").map_err(|e| self.err(e))?;
                let count = self.builder.build_extract_value(agg, 1, "incnt").map_err(|e| self.err(e))?.into_int_value();
                let in_ty = self.llvm_type(*elem_in);
                let out_ty = self.llvm_type(*elem_out);
                let i64t = self.ctx.i64_type();
                let in_stride = i64t.const_int(self.target_data.get_store_size(&in_ty), false);
                let out_stride = i64t.const_int(self.target_data.get_store_size(&out_ty), false);
                let thunk = self.par_map_thunk(func, in_ty)?;
                // The runtime allocates the output (overflow-guarded), runs the thunk across
                // threads, and returns the owned buffer.
                let out_buf = self
                    .builder
                    .build_call(
                        self.funcs["par_map"],
                        &[in_ptr.into(), count.into(), in_stride.into(), out_stride.into(), thunk.into()],
                        "obuf",
                    )
                    .map_err(|e| self.err(e))?
                    .try_as_basic_value()
                    .basic()
                    .expect("align_rt_par_map returns a pointer")
                    .into_pointer_value();
                // Result owned array `{ out_buf, count }`.
                let sty = slice_struct_type(self.ctx);
                let r = self
                    .builder
                    .build_insert_value(sty.get_undef(), out_buf, 0, "pmptr")
                    .map_err(|e| self.err(e))?
                    .into_struct_value();
                self.builder
                    .build_insert_value(r, count, 1, "pmlen")
                    .map_err(|e| self.err(e))?
                    .into_struct_value()
                    .into()
            }
            Rvalue::StrLit(s) => {
                let (ptr, len) = self.str_global(s);
                let sty = slice_struct_type(self.ctx);
                let agg = self
                    .builder
                    .build_insert_value(sty.get_undef(), ptr, 0, "strptr")
                    .map_err(|e| self.err(e))?
                    .into_struct_value();
                self.builder
                    .build_insert_value(agg, len, 1, "strlen")
                    .map_err(|e| self.err(e))?
                    .into_struct_value()
                    .into()
            }
            Rvalue::StrClone(op) => {
                // Extract the source `{ptr,len}` view, deep-copy the bytes into a fresh heap
                // buffer, and yield the owned `string` `{ptr,len}` the runtime returns.
                let agg = self.operand(op).into_struct_value();
                let ptr = self.builder.build_extract_value(agg, 0, "srcptr").map_err(|e| self.err(e))?;
                let len = self.builder.build_extract_value(agg, 1, "srclen").map_err(|e| self.err(e))?;
                self.builder
                    .build_call(self.funcs["str_clone"], &[ptr.into(), len.into()], "strclone")
                    .map_err(|e| self.err(e))?
                    .try_as_basic_value()
                    .basic()
                    .expect("str_clone returns a {ptr,len}")
            }
            Rvalue::StrTrim { kind, recv } => {
                // Extract the receiver `{ptr,len}` and call the trim; the runtime returns a sub-view
                // `{ptr,len}` aliasing the same bytes (no allocation).
                let fk = match kind {
                    align_sema::hir::StrTrimKind::Both => "str_trim",
                    align_sema::hir::StrTrimKind::Start => "str_trim_start",
                    align_sema::hir::StrTrimKind::End => "str_trim_end",
                };
                let agg = self.operand(recv).into_struct_value();
                let ptr = self.builder.build_extract_value(agg, 0, "trimptr").map_err(|e| self.err(e))?;
                let len = self.builder.build_extract_value(agg, 1, "trimlen").map_err(|e| self.err(e))?;
                self.builder
                    .build_call(self.funcs[fk], &[ptr.into(), len.into()], "strtrim")
                    .map_err(|e| self.err(e))?
                    .try_as_basic_value()
                    .basic()
                    .expect("str trim returns a {ptr,len}")
            }
            Rvalue::StrPredicate { kind, haystack, needle } => {
                use align_sema::hir::StrPredKind;
                // Extract both `{ptr,len}` views; the runtime call + result shaping differ per kind.
                let ha = self.operand(haystack).into_struct_value();
                let ne = self.operand(needle).into_struct_value();
                let hp = self.builder.build_extract_value(ha, 0, "hp").map_err(|e| self.err(e))?;
                let hl = self.builder.build_extract_value(ha, 1, "hl").map_err(|e| self.err(e))?;
                let np = self.builder.build_extract_value(ne, 0, "np").map_err(|e| self.err(e))?;
                let nl = self.builder.build_extract_value(ne, 1, "nl").map_err(|e| self.err(e))?;
                let args = [hp.into(), hl.into(), np.into(), nl.into()];
                match kind {
                    // The bool scans: an `i32` (0/1) returned as a `bool` (`i1`).
                    StrPredKind::Contains | StrPredKind::StartsWith | StrPredKind::EndsWith | StrPredKind::EqIgnoreCase => {
                        let fk = match kind {
                            StrPredKind::Contains => "str_contains",
                            StrPredKind::StartsWith => "str_starts_with",
                            StrPredKind::EndsWith => "str_ends_with",
                            StrPredKind::EqIgnoreCase => "str_eq_ignore_case",
                            StrPredKind::Find | StrPredKind::Rfind => unreachable!(),
                        };
                        let r = self
                            .builder
                            .build_call(self.funcs[fk], &args, "strpred")
                            .map_err(|e| self.err(e))?
                            .try_as_basic_value()
                            .basic()
                            .expect("str predicate returns i32")
                            .into_int_value();
                        let zero = self.ctx.i32_type().const_zero();
                        self.builder
                            .build_int_compare(IntPredicate::NE, r, zero, "strpredb")
                            .map_err(|e| self.err(e))?
                            .into()
                    }
                    // `find` / `rfind`: an `i64` index (`-1` = absent) shaped into an `Option<i64>`.
                    StrPredKind::Find | StrPredKind::Rfind => {
                        let Ty::Option(s) = result_ty else {
                            return Err(self.err("find result is not an Option"));
                        };
                        let fk = if matches!(kind, StrPredKind::Rfind) { "str_rfind" } else { "str_find" };
                        let idx = self
                            .builder
                            .build_call(self.funcs[fk], &args, "strfind")
                            .map_err(|e| self.err(e))?
                            .try_as_basic_value()
                            .basic()
                            .expect("str_find returns i64")
                            .into_int_value();
                        let i64t = self.ctx.i64_type();
                        // found = idx >= 0; tag = found as i8; payload = found ? idx : 0.
                        let found = self
                            .builder
                            .build_int_compare(IntPredicate::SGE, idx, i64t.const_zero(), "found")
                            .map_err(|e| self.err(e))?;
                        let tag = self.builder.build_int_z_extend(found, self.ctx.i8_type(), "tag").map_err(|e| self.err(e))?;
                        let payload = self
                            .builder
                            .build_select(found, idx, i64t.const_zero(), "fpayload")
                            .map_err(|e| self.err(e))?;
                        let oty = option_struct_type(self.ctx, s, self.struct_types, self.enum_types);
                        let agg = self
                            .builder
                            .build_insert_value(oty.const_zero(), tag, 0, "ftag")
                            .map_err(|e| self.err(e))?
                            .into_struct_value();
                        self.builder
                            .build_insert_value(agg, payload, 1, "fsome")
                            .map_err(|e| self.err(e))?
                            .into_struct_value()
                            .into()
                    }
                }
            }
            Rvalue::BuilderNew { capacity } => {
                // Open a builder with a null arena: the finished `string` is heap-owned
                // (`into_string` copies into a fresh malloc'd buffer), not arena-tied. `capacity`
                // pre-sizes the backing buffer so appends don't reallocate (0 = default).
                let null = self.ctx.ptr_type(AddressSpace::default()).const_null();
                let cap = self.operand(capacity);
                self.builder
                    .build_call(self.funcs["builder_new"], &[null.into(), cap.into()], "builder")
                    .map_err(|e| self.err(e))?
                    .try_as_basic_value()
                    .basic()
                    .expect("builder_new returns a pointer")
            }
            Rvalue::BuilderWriteStr(bld, s) => {
                let b = self.operand(bld).into();
                let agg = self.operand(s).into_struct_value();
                let ptr = self.builder.build_extract_value(agg, 0, "wptr").map_err(|e| self.err(e))?;
                let len = self.builder.build_extract_value(agg, 1, "wlen").map_err(|e| self.err(e))?;
                self.builder
                    .build_call(self.funcs["builder_write"], &[b, ptr.into(), len.into()], "")
                    .map_err(|e| self.err(e))?;
                return Ok(None);
            }
            Rvalue::BuilderWriteInt(bld, n) => {
                let b = self.operand(bld).into();
                // Widen the integer to `i64` (the runtime arg width), like `print`.
                let ty = self.f.operand_ty(n);
                let v = self.operand(n).into_int_value();
                let i64t = self.ctx.i64_type();
                let wide = if int_bits(ty) < 64 {
                    if is_signed(ty) {
                        self.builder.build_int_s_extend(v, i64t, "sext").map_err(|e| self.err(e))?
                    } else {
                        self.builder.build_int_z_extend(v, i64t, "zext").map_err(|e| self.err(e))?
                    }
                } else {
                    v
                };
                self.builder
                    .build_call(self.funcs["builder_write_int"], &[b, wide.into()], "")
                    .map_err(|e| self.err(e))?;
                return Ok(None);
            }
            Rvalue::BuilderWriteStrIntStr(bld, s1, n, s2) => {
                // Fused `write(str1); write_int(n); write(str2)`: pass both `str`s as `ptr,len` (like
                // BuilderWriteStr) and widen the int to i64 (like BuilderWriteInt).
                let b = self.operand(bld).into();
                let a1 = self.operand(s1).into_struct_value();
                let p1 = self.builder.build_extract_value(a1, 0, "wptr1").map_err(|e| self.err(e))?;
                let l1 = self.builder.build_extract_value(a1, 1, "wlen1").map_err(|e| self.err(e))?;
                let ty = self.f.operand_ty(n);
                let v = self.operand(n).into_int_value();
                let i64t = self.ctx.i64_type();
                let wide = if int_bits(ty) < 64 {
                    if is_signed(ty) {
                        self.builder.build_int_s_extend(v, i64t, "sext").map_err(|e| self.err(e))?
                    } else {
                        self.builder.build_int_z_extend(v, i64t, "zext").map_err(|e| self.err(e))?
                    }
                } else {
                    v
                };
                let a2 = self.operand(s2).into_struct_value();
                let p2 = self.builder.build_extract_value(a2, 0, "wptr2").map_err(|e| self.err(e))?;
                let l2 = self.builder.build_extract_value(a2, 1, "wlen2").map_err(|e| self.err(e))?;
                self.builder
                    .build_call(
                        self.funcs["builder_write_str_int_str"],
                        &[b, p1.into(), l1.into(), wide.into(), p2.into(), l2.into()],
                        "",
                    )
                    .map_err(|e| self.err(e))?;
                return Ok(None);
            }
            Rvalue::BuilderWriteBool(bld, v) => {
                // Widen the i1 to i32 (the runtime arg width), like `print(bool)`.
                let b = self.operand(bld).into();
                let val = self.operand(v).into_int_value();
                let wide = self.builder.build_int_z_extend(val, self.ctx.i32_type(), "bext").map_err(|e| self.err(e))?;
                self.builder
                    .build_call(self.funcs["builder_write_bool"], &[b, wide.into()], "")
                    .map_err(|e| self.err(e))?;
                return Ok(None);
            }
            Rvalue::BuilderWriteChar(bld, c) => {
                // A `char` is a u32 scalar; the runtime emits its UTF-8.
                let b = self.operand(bld).into();
                let val = self.operand(c);
                self.builder
                    .build_call(self.funcs["builder_write_char"], &[b, val.into()], "")
                    .map_err(|e| self.err(e))?;
                return Ok(None);
            }
            Rvalue::BuilderWriteFloat(bld, x) => {
                // Pick the runtime fn by float width, like `print(float)`.
                let b = self.operand(bld).into();
                let ty = self.f.operand_ty(x);
                let val = self.operand(x);
                let callee = if ty == Ty::Float(FloatTy { bits: 32 }) { "builder_write_f32" } else { "builder_write_f64" };
                self.builder
                    .build_call(self.funcs[callee], &[b, val.into()], "")
                    .map_err(|e| self.err(e))?;
                return Ok(None);
            }
            Rvalue::BuilderToString(bld) => {
                // Finish into an owned `string` `{ptr,len}` (a fresh heap buffer); the builder
                // object is freed by the runtime.
                let b = self.operand(bld).into();
                self.builder
                    .build_call(self.funcs["builder_into_string"], &[b], "tostr")
                    .map_err(|e| self.err(e))?
                    .try_as_basic_value()
                    .basic()
                    .expect("builder_into_string returns a {ptr,len}")
            }
            Rvalue::Template(pieces, arena) => self.gen_template(pieces, arena.as_ref())?,
            Rvalue::JsonDecode { struct_id, input, out } => self.gen_json_decode(*struct_id, input, *out)?,
            Rvalue::JsonDecodeArray { elem, input, out } => self.gen_json_decode_array(*elem, input, *out)?,
            Rvalue::JsonDecodeStructArray { struct_id, input, out } => self.gen_json_decode_struct_array(*struct_id, input, *out)?,
            Rvalue::JsonDecodeSoa { struct_id, input, out, arena } => self.gen_json_decode_soa(*struct_id, input, *out, arena)?,
            Rvalue::FsReadFile { path, out } => self.gen_fs_read_file(path, *out)?,
            Rvalue::IoStdoutWrite { arg } => self.gen_io_stdout_write(arg)?,
            Rvalue::IoStdoutWriteBuilder { builder } => {
                let b = self.operand(builder).into();
                let cs = self
                    .builder
                    .build_call(self.funcs["io_stdout_write_builder"], &[b], "sowb")
                    .map_err(|e| self.err(e))?;
                cs.try_as_basic_value().basic().expect("io_stdout_write_builder returns i32")
            }
            Rvalue::BufWriterNew { fd } => {
                let fd = self.ctx.i32_type().const_int(*fd as u64, true);
                self.builder
                    .build_call(self.funcs["io_buf_new"], &[fd.into()], "bufw")
                    .map_err(|e| self.err(e))?
                    .try_as_basic_value()
                    .basic()
                    .expect("io_buf_new returns a pointer")
            }
            Rvalue::BufWriterWrite(w, s) => {
                let wp = self.operand(w).into();
                let agg = self.operand(s).into_struct_value();
                let ptr = self.builder.build_extract_value(agg, 0, "wptr").map_err(|e| self.err(e))?;
                let len = self.builder.build_extract_value(agg, 1, "wlen").map_err(|e| self.err(e))?;
                self.builder
                    .build_call(self.funcs["io_buf_write"], &[wp, ptr.into(), len.into()], "")
                    .map_err(|e| self.err(e))?;
                return Ok(None);
            }
            Rvalue::BufWriterFlush(w) => {
                let wp = self.operand(w).into();
                let cs = self
                    .builder
                    .build_call(self.funcs["io_buf_flush"], &[wp], "bufflush")
                    .map_err(|e| self.err(e))?;
                cs.try_as_basic_value().basic().expect("io_buf_flush returns i32")
            }
            Rvalue::SliceLen(op) => {
                let agg = self.operand(op).into_struct_value();
                self.builder.build_extract_value(agg, 1, "len").map_err(|e| self.err(e))?
            }
            Rvalue::SlicePtr(op) => {
                let agg = self.operand(op).into_struct_value();
                self.builder.build_extract_value(agg, 0, "ptr").map_err(|e| self.err(e))?
            }
            Rvalue::SliceIndex(s, idx) => {
                let agg = self.operand(s).into_struct_value();
                let ptr = self.builder.build_extract_value(agg, 0, "ptr").map_err(|e| self.err(e))?.into_pointer_value();
                let ty = scalar_type(self.ctx, result_ty, self.struct_types, self.enum_types);
                let index = self.operand(idx).into_int_value();
                let ep = unsafe {
                    self.builder
                        .build_in_bounds_gep(ty, ptr, &[index], "slcidx")
                        .map_err(|e| self.err(e))?
                };
                self.builder.build_load(ty, ep, "slcload").map_err(|e| self.err(e))?
            }
            Rvalue::SubSlice { base, start, len, elem } => {
                // Offset the base pointer by `start` elements (the `elem` type sets the GEP stride —
                // `i8` bytes for a `str`) and pair it with the precomputed `len`, yielding a borrowed
                // `{ptr,len}` view of the same backing storage (no allocation).
                let agg = self.operand(base).into_struct_value();
                let ptr = self.builder.build_extract_value(agg, 0, "subptr").map_err(|e| self.err(e))?.into_pointer_value();
                let ety = scalar_type(self.ctx, *elem, self.struct_types, self.enum_types);
                let start_v = self.operand(start).into_int_value();
                let newptr = unsafe {
                    self.builder
                        .build_in_bounds_gep(ety, ptr, &[start_v], "subgep")
                        .map_err(|e| self.err(e))?
                };
                let l = self.operand(len);
                let sty = slice_struct_type(self.ctx);
                let s0 = self
                    .builder
                    .build_insert_value(sty.get_undef(), newptr, 0, "subvptr")
                    .map_err(|e| self.err(e))?
                    .into_struct_value();
                self.builder
                    .build_insert_value(s0, l, 1, "subvlen")
                    .map_err(|e| self.err(e))?
                    .into_struct_value()
                    .into()
            }
            Rvalue::BoxClone(handle, src) => {
                let Ty::Box(s) = result_ty else {
                    return Err(self.err("clone result is not a box"));
                };
                let ty = scalar_type(self.ctx, scalar_to_ty(s), self.struct_types, self.enum_types);
                let i64t = self.ctx.i64_type();
                let bytes = scalar_bytes(s);
                // Allocate a fresh box, then copy the value over.
                let new_ptr = self
                    .builder
                    .build_call(
                        self.funcs["arena_alloc"],
                        &[self.operand(handle).into(), i64t.const_int(bytes, false).into(), i64t.const_int(bytes, false).into()],
                        "clone",
                    )
                    .map_err(|e| self.err(e))?
                    .try_as_basic_value()
                    .basic()
                    .expect("arena_alloc returns a pointer")
                    .into_pointer_value();
                let src_ptr = self.operand(src).into_pointer_value();
                let val = self.builder.build_load(ty, src_ptr, "cloneval").map_err(|e| self.err(e))?;
                self.builder.build_store(new_ptr, val).map_err(|e| self.err(e))?;
                new_ptr.into()
            }
            // `error(code)` is identity on the i32 code (the M2 Error repr).
            Rvalue::Call(name, args) if name == "error" => self.operand(&args[0]),
            Rvalue::Call(name, args) if name == "print" => return self.gen_print(args),
            Rvalue::Call(name, args) if name == "hash64" || name == "hash128" => {
                return self.gen_hash(name, args);
            }
            Rvalue::Call(name, args) => {
                let callee = self.funcs[name];
                // A foreign call passes a `str`/`slice` argument as its data pointer (the extern's
                // declared param type is `ptr`); every other argument passes as its value.
                let argv: Vec<inkwell::values::BasicMetadataValueEnum> = match self.extern_params.get(name) {
                    Some(param_tys) => {
                        let mut v = Vec::with_capacity(args.len());
                        for (i, o) in args.iter().enumerate() {
                            let val = self.operand(o);
                            let coerced = if param_tys.get(i).copied().is_some_and(is_ffi_view) {
                                self.builder
                                    .build_extract_value(val.into_struct_value(), 0, "ffiptr")
                                    .map_err(|e| self.err(e))?
                            } else {
                                val
                            };
                            v.push(coerced.into());
                        }
                        v
                    }
                    None => args.iter().map(|o| self.operand(o).into()).collect(),
                };
                let cs = self
                    .builder
                    .build_call(callee, &argv, "call")
                    .map_err(|e| self.err(e))?;
                return Ok(cs.try_as_basic_value().basic());
            }
            Rvalue::FnAddr(name) => {
                // A non-capturing function value: `{ thunk_ptr, null_env }`.
                let thunk = self
                    .funcs
                    .get(&format!("{name}$fnval"))
                    .ok_or_else(|| self.err(format!("no function-value thunk for {name}")))?;
                let fn_ptr = thunk.as_global_value().as_pointer_value();
                let null_env = self.ctx.ptr_type(AddressSpace::default()).const_null();
                let cty = closure_struct_type(self.ctx);
                let a0 = self
                    .builder
                    .build_insert_value(cty.const_zero(), fn_ptr, 0, "cf")
                    .map_err(|e| self.err(e))?
                    .into_struct_value();
                self.builder
                    .build_insert_value(a0, null_env, 1, "ce")
                    .map_err(|e| self.err(e))?
                    .into_struct_value()
                    .into()
            }
            Rvalue::Closure { lifted, captures, capture_tys } => {
                // A capturing closure: copy the captures into a frame-local env, then build
                // `{ thunk_ptr, env_ptr }` where the thunk unpacks the env into the lifted fn's
                // trailing capture parameters.
                let env_fields: Vec<BasicTypeEnum> =
                    capture_tys.iter().map(|t| abi_type(self.ctx, *t, self.struct_types, self.enum_types)).collect();
                let env_struct = self.ctx.struct_type(&env_fields, false);
                let env_ptr = self.alloca_at_entry(env_struct.into(), "clos_env")?;
                for (i, op) in captures.iter().enumerate() {
                    let v = self.operand(op);
                    let fld = self
                        .builder
                        .build_struct_gep(env_struct, env_ptr, i as u32, "capg")
                        .map_err(|e| self.err(e))?;
                    self.builder.build_store(fld, v).map_err(|e| self.err(e))?;
                }
                let thunk = self
                    .funcs
                    .get(&format!("{lifted}$clos"))
                    .ok_or_else(|| self.err(format!("no closure thunk for {lifted}")))?;
                let fn_ptr = thunk.as_global_value().as_pointer_value();
                let cty = closure_struct_type(self.ctx);
                let a0 = self
                    .builder
                    .build_insert_value(cty.const_zero(), fn_ptr, 0, "cf")
                    .map_err(|e| self.err(e))?
                    .into_struct_value();
                self.builder
                    .build_insert_value(a0, env_ptr, 1, "ce")
                    .map_err(|e| self.err(e))?
                    .into_struct_value()
                    .into()
            }
            Rvalue::CallIndirect { callee, args, param_tys, ret_ty } => {
                // Extract `{ fn_ptr, env_ptr }` and call with the env-ABI `fn(env, args)`.
                let clos = self.operand(callee).into_struct_value();
                let fn_ptr = self.builder.build_extract_value(clos, 0, "cf").map_err(|e| self.err(e))?.into_pointer_value();
                let env = self.builder.build_extract_value(clos, 1, "ce").map_err(|e| self.err(e))?;
                let mut param_meta: Vec<BasicMetadataTypeEnum> =
                    vec![self.ctx.ptr_type(AddressSpace::default()).into()];
                param_meta.extend(param_tys.iter().map(|t| BasicMetadataTypeEnum::from(self.llvm_type(*t))));
                let fn_ty = self.llvm_type(*ret_ty).fn_type(&param_meta, false);
                let mut argv: Vec<inkwell::values::BasicMetadataValueEnum> = vec![env.into()];
                argv.extend(args.iter().map(|o| inkwell::values::BasicMetadataValueEnum::from(self.operand(o))));
                let cs = self
                    .builder
                    .build_indirect_call(fn_ty, fn_ptr, &argv, "icall")
                    .map_err(|e| self.err(e))?;
                return Ok(cs.try_as_basic_value().basic());
            }
        };
        Ok(Some(v))
    }

    /// LLVM type for a value/slot of any type (scalars, `Option`, structs).
    fn llvm_type(&self, ty: Ty) -> BasicTypeEnum<'c> {
        match ty {
            Ty::Struct(id) => self.struct_types[id as usize].into(),
            Ty::Tuple(id) => self.tuple_types[id as usize].into(),
            Ty::Option(s) => option_struct_type(self.ctx, s, self.struct_types, self.enum_types).into(),
            Ty::Result(o, e) => result_struct_type(self.ctx, o, e, self.struct_types, self.enum_types).into(),
            Ty::Box(_) | Ty::ArenaHandle | Ty::Builder | Ty::BufWriter | Ty::Raw => self.ctx.ptr_type(AddressSpace::default()).into(),
            Ty::Fn(_) => closure_struct_type(self.ctx).into(),
            Ty::Array(s, n) => scalar_type(self.ctx, scalar_to_ty(s), self.struct_types, self.enum_types).array_type(n).into(),
            Ty::StructArray(id, n) => self.struct_types[id as usize].array_type(n).into(),
            Ty::Slice(_) | Ty::Soa(_) | Ty::Str | Ty::String | Ty::DynArray(_) => slice_struct_type(self.ctx).into(),
            // AoS struct array = `{ptr,len}`; SoA (M6) differs → match the layout (forces revisit).
            Ty::DynStructArray(_, Layout::Aos) | Ty::DynSliceArray(_) => slice_struct_type(self.ctx).into(),
            Ty::DictEncoded(..) => dictenc_struct_type(self.ctx).into(),
            _ => scalar_type(self.ctx, ty, self.struct_types, self.enum_types),
        }
    }

    /// `&slot[index]` via an array GEP (indices `[0, index]` into the `[N x T]` alloca).
    fn elem_ptr(&self, slot: Slot, idx: &Operand) -> Result<inkwell::values::PointerValue<'c>, CodegenError> {
        let arr_ty = self.llvm_type(self.f.slots[slot as usize]);
        let zero = self.ctx.i64_type().const_zero();
        let index = self.operand(idx).into_int_value();
        unsafe {
            self.builder
                .build_in_bounds_gep(arr_ty, self.slots[&slot], &[zero, index], "elemptr")
                .map_err(|e| self.err(e))
        }
    }

    /// The **single alignment seam**: the byte alignment to use for a value/allocation of `ty`.
    /// A struct (or struct array) declared `align(N)` returns `N`; everything else returns the
    /// natural ABI alignment LLVM derives from the type. Reserved for M6 `align(N)` — today every
    /// struct's `align` is `None`, so this is always the natural alignment (`open-questions.md`).
    /// Routing all alignment through here means honoring a custom `align(N)` is a one-line change.
    fn type_align(&self, ty: Ty) -> u32 {
        let custom = match ty {
            // A struct value, and a fixed AoS array of it (`[N x %Struct]`, whose alignment is the
            // element's), take the struct's declared alignment. A `DynStructArray` slot holds a
            // `{ptr,len}` view, not the struct — its element-buffer alignment is a heap/runtime
            // concern (M6), so the slot itself stays naturally aligned.
            Ty::Struct(id) | Ty::StructArray(id, _) => self.structs[id as usize].align,
            _ => None,
        };
        // `align(N)` only ever *over*-aligns: take the max of the declared and the natural ABI
        // alignment, so a too-small `align(N)` can never under-align a value (which would be UB).
        let natural = self.target_data.get_abi_alignment(&self.llvm_type(ty));
        custom.map_or(natural, |c| c.max(natural))
    }

    /// `&slot[index].field` — GEP `[0, index, field]` into a `[N x %Struct]` alloca.
    fn elem_field_ptr(&self, slot: Slot, idx: &Operand, field: u32) -> Result<inkwell::values::PointerValue<'c>, CodegenError> {
        let arr_ty = self.llvm_type(self.f.slots[slot as usize]);
        let zero = self.ctx.i64_type().const_zero();
        let index = self.operand(idx).into_int_value();
        let sid = self.array_elem_struct_id(slot);
        let f = self.ctx.i32_type().const_int(self.pfield(sid, field) as u64, false);
        unsafe {
            self.builder
                .build_in_bounds_gep(arr_ty, self.slots[&slot], &[zero, index, f], "elemfield")
                .map_err(|e| self.err(e))
        }
    }

    /// The struct id held by a slot (assumes a struct-typed slot).
    fn slot_struct_id(&self, slot: Slot) -> u32 {
        match self.f.slots[slot as usize] {
            Ty::Struct(id) => id,
            other => unreachable!("field access on non-struct slot of type {other:?}"),
        }
    }

    /// The element struct id of a fixed struct-array slot (`[N x %Struct]`) — for mapping a logical
    /// element-field index to its physical position.
    fn array_elem_struct_id(&self, slot: Slot) -> u32 {
        match self.f.slots[slot as usize] {
            Ty::StructArray(id, _) => id,
            other => unreachable!("element-field access on non-struct-array slot of type {other:?}"),
        }
    }

    /// `&slot.f0.f1.…` via a chain of struct GEPs (each level needs its pointee struct type — LLVM
    /// 19 opaque pointers). Returns the pointer to the innermost field. `path` has length ≥ 1.
    fn field_path_ptr(&self, slot: Slot, path: &[u32]) -> Result<inkwell::values::PointerValue<'c>, CodegenError> {
        let mut sid = self.slot_struct_id(slot);
        let mut ptr = self.slots[&slot];
        for (k, &idx) in path.iter().enumerate() {
            let st = self.struct_types[sid as usize];
            let pidx = self.pfield(sid, idx);
            ptr = self.builder.build_struct_gep(st, ptr, pidx, "fldptr").map_err(|e| self.err(e))?;
            if k + 1 < path.len() {
                sid = match self.structs[sid as usize].fields[idx as usize].ty {
                    Ty::Struct(nid) => nid,
                    other => unreachable!("nested field path through non-struct {other:?}"),
                };
            }
        }
        Ok(ptr)
    }

    /// Recursively free the owned fields of a Move struct at `base` (a pointer to the struct value),
    /// in declared order. A `string` field's `{ptr,len}` buffer is freed; a nested Move-struct field
    /// recurses. Null-safe: an unconstructed / moved-out struct was zeroed (`DropFlagInit`), so each
    /// owned leaf reads `{null,0}` and `free(null)` is a no-op. Copy fields (scalars, `str` borrows,
    /// plain-data nested structs) are skipped. (Slice 3 of `08-nested-structs.md`.)
    fn drop_struct_fields(&self, base: inkwell::values::PointerValue<'c>, struct_id: u32) -> Result<(), CodegenError> {
        let st = self.struct_types[struct_id as usize];
        // Snapshot (index, field type) so we don't hold a borrow of `self.structs` across the
        // builder/recursion calls (`Ty` is `Copy`).
        let fields: Vec<(u32, Ty)> = self.structs[struct_id as usize].fields.iter().enumerate().map(|(i, f)| (i as u32, f.ty)).collect();
        for (i, fty) in fields {
            // `i` is the logical field index; the GEP needs its physical (reordered) slot.
            let pi = self.pfield(struct_id, i);
            match fty {
                // An owned `string` field — free its heap buffer (field 0 of the `{ptr,len}`).
                Ty::String => {
                    let fp = self.builder.build_struct_gep(st, base, pi, "dropfld").map_err(|e| self.err(e))?;
                    let agg = self
                        .builder
                        .build_load(slice_struct_type(self.ctx), fp, "dropfldv")
                        .map_err(|e| self.err(e))?
                        .into_struct_value();
                    let ptr = self.builder.build_extract_value(agg, 0, "dropfldptr").map_err(|e| self.err(e))?;
                    self.builder.build_call(self.funcs["free"], &[ptr.into()], "").map_err(|e| self.err(e))?;
                }
                // A nested Move struct — recurse into it (a plain-data nested struct is Copy → skip).
                Ty::Struct(nid) if struct_is_move(nid, self.structs) => {
                    let fp = self.builder.build_struct_gep(st, base, pi, "dropnest").map_err(|e| self.err(e))?;
                    self.drop_struct_fields(fp, nid)?;
                }
                // A nested Move-struct *array* field — drop each element (defensive: struct fields
                // reject array types today — `is_field_ok` — so this is unreachable, but keeping the
                // owned case here means a future array-valued field can't silently fail-open and leak).
                Ty::StructArray(eid, n) if struct_is_move(eid, self.structs) => {
                    let fp = self.builder.build_struct_gep(st, base, pi, "dropnestarr").map_err(|e| self.err(e))?;
                    let arr_ty = self.struct_types[eid as usize].array_type(n);
                    let zero = self.ctx.i64_type().const_zero();
                    for e in 0..n {
                        let idx = self.ctx.i64_type().const_int(e as u64, false);
                        let ep = unsafe {
                            self.builder.build_in_bounds_gep(arr_ty, fp, &[zero, idx], "dropnestel").map_err(|e| self.err(e))?
                        };
                        self.drop_struct_fields(ep, eid)?;
                    }
                }
                _ => {}
            }
        }
        Ok(())
    }

    /// The type of the innermost field reached by `path` from `slot`'s struct.
    fn field_path_ty(&self, slot: Slot, path: &[u32]) -> Ty {
        let mut sid = self.slot_struct_id(slot);
        for (k, &idx) in path.iter().enumerate() {
            let fty = self.structs[sid as usize].fields[idx as usize].ty;
            if k + 1 == path.len() {
                return fty;
            }
            sid = match fty {
                Ty::Struct(nid) => nid,
                other => unreachable!("nested field path through non-struct {other:?}"),
            };
        }
        unreachable!("empty field path")
    }

    /// Builtin `print`: widen the integer argument to i64 and call the runtime.
    /// `hash64(data)` / `hash128(data)` — split the byte view `{ptr,len}` and call the runtime.
    /// `str`, `string`, and `slice<u8>` all lower to the same `{ptr, i64}` struct, so one path
    /// serves every input type. `hash64` returns an i64; `hash128` returns a `{i64,i64}` tuple value.
    fn gen_hash(&mut self, name: &str, args: &[Operand]) -> Result<Option<BasicValueEnum<'c>>, CodegenError> {
        let agg = self.operand(&args[0]).into_struct_value();
        let ptr = self.builder.build_extract_value(agg, 0, "hptr").map_err(|e| self.err(e))?;
        let len = self.builder.build_extract_value(agg, 1, "hlen").map_err(|e| self.err(e))?;
        let cs = self
            .builder
            .build_call(self.funcs[name], &[ptr.into(), len.into()], "hash")
            .map_err(|e| self.err(e))?;
        Ok(cs.try_as_basic_value().basic())
    }

    fn gen_print(&mut self, args: &[Operand]) -> Result<Option<BasicValueEnum<'c>>, CodegenError> {
        let arg = &args[0];
        let ty = self.f.operand_ty(arg);
        // print(str)/print(string): pass { ptr, len } to the runtime (a `string` reads as a `str`).
        if ty == Ty::Str || ty == Ty::String {
            let agg = self.operand(arg).into_struct_value();
            let ptr = self.builder.build_extract_value(agg, 0, "sptr").map_err(|e| self.err(e))?;
            let len = self.builder.build_extract_value(agg, 1, "slen").map_err(|e| self.err(e))?;
            self.builder
                .build_call(self.funcs["print_str"], &[ptr.into(), len.into()], "")
                .map_err(|e| self.err(e))?;
            return Ok(None);
        }
        // print(bool): widen i1 to i32 and emit `true`/`false`.
        if ty == Ty::Bool {
            let v = self.operand(arg).into_int_value();
            let wide = self.builder.build_int_z_extend(v, self.ctx.i32_type(), "bext").map_err(|e| self.err(e))?;
            self.builder
                .build_call(self.funcs["print_bool"], &[wide.into()], "")
                .map_err(|e| self.err(e))?;
            return Ok(None);
        }
        // print(char): pass the u32 scalar; the runtime emits its UTF-8.
        if ty == Ty::Char {
            let v = self.operand(arg).into_int_value();
            self.builder
                .build_call(self.funcs["print_char"], &[v.into()], "")
                .map_err(|e| self.err(e))?;
            return Ok(None);
        }
        // print(float): the runtime renders the shortest round-trip decimal.
        if matches!(ty, Ty::Float(_)) {
            let v = self.operand(arg).into_float_value();
            let callee = if ty == Ty::Float(FloatTy { bits: 32 }) { "print_f32" } else { "print_f64" };
            self.builder
                .build_call(self.funcs[callee], &[v.into()], "")
                .map_err(|e| self.err(e))?;
            return Ok(None);
        }
        let v = self.operand(arg).into_int_value();
        let i64t = self.ctx.i64_type();
        let wide = if int_bits(ty) < 64 {
            if is_signed(ty) {
                self.builder.build_int_s_extend(v, i64t, "sext").map_err(|e| self.err(e))?
            } else {
                self.builder.build_int_z_extend(v, i64t, "zext").map_err(|e| self.err(e))?
            }
        } else {
            v
        };
        let callee = self.funcs["print"];
        self.builder
            .build_call(callee, &[wide.into()], "")
            .map_err(|e| self.err(e))?;
        Ok(None)
    }

    /// `value as to` — an explicit numeric/char conversion. `char` is treated as a 32-bit unsigned
    /// integer; sema guarantees `from`/`to` are concrete primitives and that `char` never pairs
    /// with a float. Equal-type casts are elided in MIR, so this always changes representation.
    fn gen_cast(&self, value: BasicValueEnum<'c>, from: Ty, to: Ty) -> Result<BasicValueEnum<'c>, CodegenError> {
        let from_float = matches!(from, Ty::Float(_));
        let to_float = matches!(to, Ty::Float(_));
        match (from_float, to_float) {
            // int/char → int/char: truncate / sign-or-zero-extend (sign from the *source*).
            (false, false) => {
                let v = value.into_int_value();
                let dst = int_type(self.ctx, to);
                Ok(self.builder.build_int_cast_sign_flag(v, dst, is_signed(from), "cast").map_err(|e| self.err(e))?.into())
            }
            // int → float: `sitofp` / `uitofp` (source signedness).
            (false, true) => {
                let v = value.into_int_value();
                let dst = float_type(self.ctx, to);
                Ok(if is_signed(from) {
                    self.builder.build_signed_int_to_float(v, dst, "cast").map_err(|e| self.err(e))?.into()
                } else {
                    self.builder.build_unsigned_int_to_float(v, dst, "cast").map_err(|e| self.err(e))?.into()
                })
            }
            // float → int: the *saturating* conversion (out-of-range clamps to MIN/MAX, NaN → 0) —
            // no UB, matching the settled "never silent / no UB" rule. LLVM `llvm.fpto{s,u}i.sat`.
            (true, false) => {
                let dst = int_type(self.ctx, to);
                let src = float_type(self.ctx, from);
                let name = if is_signed(to) { "llvm.fptosi.sat" } else { "llvm.fptoui.sat" };
                self.call_intrinsic(name, &[dst.into(), src.into()], &[value.into()])
            }
            // float → float: `fpext` (widen) / `fptrunc` (narrow).
            (true, true) => {
                let v = value.into_float_value();
                let dst = float_type(self.ctx, to);
                Ok(self.builder.build_float_cast(v, dst, "cast").map_err(|e| self.err(e))?.into())
            }
        }
    }

    fn gen_bin(&mut self, op: BinOp, a: &Operand, b: &Operand) -> Result<BasicValueEnum<'c>, CodegenError> {
        if self.f.operand_ty(a) == Ty::Str {
            return self.gen_str_eq(op, a, b);
        }
        // A `vecN<T>` operand (M6): a comparison yields a `<N x i1>` mask, arithmetic stays a vector.
        // Either operand may be the vector — `operand_as_vector` splats the scalar one (broadcast),
        // and the operand order (lhs, rhs) is preserved for the non-commutative ops.
        let vt = match (self.f.operand_ty(a), self.f.operand_ty(b)) {
            (Ty::Vec(e, n), _) | (_, Ty::Vec(e, n)) => Some((scalar_to_ty(e), n)),
            _ => None,
        };
        if let Some((et, n)) = vt {
            if is_comparison(op) {
                return self.gen_vec_cmp(op, a, b, et, n);
            }
            return self.gen_vec_bin(op, a, b, et, n);
        }
        if matches!(self.f.operand_ty(a), Ty::Float(_)) {
            return self.gen_float_bin(op, a, b);
        }
        let signed = is_signed(self.f.operand_ty(a));
        let l = self.operand(a).into_int_value();
        let r = self.operand(b).into_int_value();
        let bld = self.builder;
        let v = match op {
            BinOp::Add => bld.build_int_add(l, r, "add"),
            BinOp::Sub => bld.build_int_sub(l, r, "sub"),
            BinOp::Mul => bld.build_int_mul(l, r, "mul"),
            BinOp::Div if signed => bld.build_int_signed_div(l, r, "sdiv"),
            BinOp::Div => bld.build_int_unsigned_div(l, r, "udiv"),
            BinOp::Rem if signed => bld.build_int_signed_rem(l, r, "srem"),
            BinOp::Rem => bld.build_int_unsigned_rem(l, r, "urem"),
            // Logical `&&`/`||` on `bool` (i1) — and the integer bitwise `& | ^`.
            BinOp::And | BinOp::BitAnd => bld.build_and(l, r, "and"),
            BinOp::Or | BinOp::BitOr => bld.build_or(l, r, "or"),
            BinOp::BitXor => bld.build_xor(l, r, "xor"),
            // Shifts: mask the amount to the value's bit width (defined "mod width" behavior, and
            // avoids LLVM poison from an out-of-range shift). Both operands share the value's int
            // type (sema), so no width conversion is needed. `>>` is arithmetic on a signed value.
            BinOp::Shl | BinOp::Shr => {
                let width = l.get_type().get_bit_width();
                let mask = l.get_type().const_int((width - 1) as u64, false);
                let amt = bld.build_and(r, mask, "shamt").map_err(|e| self.err(e))?;
                if op == BinOp::Shl {
                    bld.build_left_shift(l, amt, "shl")
                } else {
                    bld.build_right_shift(l, amt, signed, "shr")
                }
            }
            BinOp::Eq => bld.build_int_compare(IntPredicate::EQ, l, r, "eq"),
            BinOp::Ne => bld.build_int_compare(IntPredicate::NE, l, r, "ne"),
            BinOp::Lt => bld.build_int_compare(pred(signed, Cmp::Lt), l, r, "lt"),
            BinOp::Le => bld.build_int_compare(pred(signed, Cmp::Le), l, r, "le"),
            BinOp::Gt => bld.build_int_compare(pred(signed, Cmp::Gt), l, r, "gt"),
            BinOp::Ge => bld.build_int_compare(pred(signed, Cmp::Ge), l, r, "ge"),
        };
        Ok(v.map_err(|e| self.err(e))?.into())
    }

    /// Intern a string's bytes as a private constant; return `(&bytes, len)`.
    fn str_global(&self, s: &str) -> (inkwell::values::PointerValue<'c>, IntValue<'c>) {
        let arr = self.ctx.const_string(s.as_bytes(), false);
        let g = self.module.add_global(arr.get_type(), None, "str");
        g.set_initializer(&arr);
        g.set_constant(true);
        (g.as_pointer_value(), self.ctx.i64_type().const_int(s.len() as u64, false))
    }

    /// `template "..."` → builder_new, a write per piece, then builder_finish → str.
    /// `json.decode` into struct `struct_id`: zero the out slot, build a field-descriptor
    /// table `[{ name_ptr, name_len: i64, tag: i32, offset: i64 }]` (tag = byte width for
    /// ints, 0 for bool; offset from the target layout), and call the runtime parser. Returns
    /// the i32 status.
    /// Emit the constant `{name_ptr, name_len, tag, offset}` field-descriptor table for decoding
    /// struct `struct_id`, returning `(table base, n_fields, struct store size)`. The table is a
    /// private constant global (no per-call alloca → safe inside a loop). Shared by single-struct
    /// and `array<Struct>` decode (MMv2 slice 8d).
    fn decode_field_table(&mut self, struct_id: u32) -> DecodeTable<'c> {
        let sty = self.struct_types[struct_id as usize];
        let ptr_ty = self.ctx.ptr_type(AddressSpace::default());
        let i64t = self.ctx.i64_type();
        let i32t = self.ctx.i32_type();
        let desc_ty = self.ctx.struct_type(&[ptr_ty.into(), i64t.into(), i32t.into(), i64t.into()], false);
        let fields = self.structs[struct_id as usize].fields.clone();
        let descs: Vec<inkwell::values::StructValue> = fields
            .iter()
            .enumerate()
            .map(|(i, f)| {
                let (name_ptr, name_len) = self.str_global(&f.name);
                // tag = (signed << 16) | (kind << 8) | byte-width. kind: 0 = int, 1 = bool,
                // 2 = float, 3 = str. Bit 16 is the int sign flag (1 = signed, 0 = unsigned); it
                // lets the runtime range-check the parsed value before writing (only meaningful for
                // kind 0). It sits above the kind/width bytes, so the existing `tag & 0xff` (width)
                // and `(tag >> 8) & 0xff` (kind) decoders are unchanged.
                // A `str` field is a `{ptr,len}` view (16 bytes) written zero-copy into the input.
                let tag: u64 = match f.ty {
                    Ty::Int(it) => ((it.signed as u64) << 16) | (it.bits / 8) as u64,
                    Ty::Bool => (1 << 8) | 1,
                    Ty::Float(ft) => (2 << 8) | (ft.bits / 8) as u64,
                    Ty::Str => (3 << 8) | 16,
                    _ => unreachable!("json.decode field is int/float/bool/str (sema-checked)"),
                };
                // The descriptor lists fields by *name* in logical order, but the byte offset must be
                // the field's physical slot (fields are alignment-reordered for non-`layout(C)`
                // structs); `field_byte_offset` maps logical→physical.
                let offset = self.field_byte_offset(struct_id, i as u32);
                desc_ty.const_named_struct(&[
                    name_ptr.into(),
                    name_len.into(),
                    i32t.const_int(tag, false).into(),
                    i64t.const_int(offset, false).into(),
                ])
            })
            .collect();
        let table_val = desc_ty.const_array(&descs);
        let table = self.module.add_global(table_val.get_type(), None, "jfields");
        table.set_initializer(&table_val);
        table.set_constant(true);

        // Compile-time perfect-hash table for the field names: O(1) key → index lookup at runtime
        // instead of a linear scan over `descs` (the win on wide schemas). If no collision-free
        // (seed, size) is found, `phf_ptr` is null / `phf_len` is 0 and the runtime falls back to
        // the linear scan — so this is a pure speedup, never a correctness dependency.
        let names: Vec<&str> = fields.iter().map(|f| f.name.as_str()).collect();
        let (phf_ptr, phf_len, phf_seed) = match build_phf(&names) {
            Some((slots, seed)) => {
                let vals: Vec<_> = slots.iter().map(|&s| i32t.const_int(s as u64, true)).collect();
                let arr = i32t.const_array(&vals);
                let g = self.module.add_global(arr.get_type(), None, "jphf");
                g.set_initializer(&arr);
                g.set_constant(true);
                (g.as_pointer_value(), slots.len() as u64, seed)
            }
            None => (ptr_ty.const_null(), 0, 0),
        };

        DecodeTable {
            descs: table.as_pointer_value(),
            n_fields: fields.len() as u64,
            store_size: self.target_data.get_store_size(&sty),
            phf_ptr,
            phf_len,
            phf_seed,
        }
    }

    fn gen_json_decode(&mut self, struct_id: u32, input: &Operand, out: Slot) -> Result<BasicValueEnum<'c>, CodegenError> {
        let sty = self.struct_types[struct_id as usize];
        let out_ptr = self.slots[&out];
        // Zero the struct so missing fields read as 0/false.
        self.builder.build_store(out_ptr, sty.const_zero()).map_err(|e| self.err(e))?;

        let agg = self.operand(input).into_struct_value();
        let in_ptr = self.builder.build_extract_value(agg, 0, "jin_p").map_err(|e| self.err(e))?;
        let in_len = self.builder.build_extract_value(agg, 1, "jin_l").map_err(|e| self.err(e))?;

        let i64t = self.ctx.i64_type();
        let t = self.decode_field_table(struct_id);
        let n = i64t.const_int(t.n_fields, false);
        let size = i64t.const_int(t.store_size, false);
        let phf_len = i64t.const_int(t.phf_len, false);
        let phf_seed = i64t.const_int(t.phf_seed, false);
        let cs = self
            .builder
            .build_call(
                self.funcs["json_decode"],
                &[in_ptr.into(), in_len.into(), t.descs.into(), n.into(), out_ptr.into(), size.into(), t.phf_ptr.into(), phf_len.into(), phf_seed.into()],
                "jdec",
            )
            .map_err(|e| self.err(e))?;
        Ok(cs.try_as_basic_value().basic().expect("json_decode returns i32"))
    }

    /// `json.decode` into an owned `array<Struct>` (MMv2 slice 8d): zero the out `{ptr,len}` slot,
    /// then call the runtime AoS parser with the same field table as the single-struct path plus
    /// the element stride. The returned buffer is owned (freed by `Drop`); its `str` fields point
    /// into the input. Returns the i32 status.
    fn gen_json_decode_struct_array(&mut self, struct_id: u32, input: &Operand, out: Slot) -> Result<BasicValueEnum<'c>, CodegenError> {
        let out_ptr = self.slots[&out];
        // Zero the {ptr,len} so a failed decode reads {null,0} (its Drop frees null).
        self.builder.build_store(out_ptr, slice_struct_type(self.ctx).const_zero()).map_err(|e| self.err(e))?;

        let agg = self.operand(input).into_struct_value();
        let in_ptr = self.builder.build_extract_value(agg, 0, "jin_p").map_err(|e| self.err(e))?;
        let in_len = self.builder.build_extract_value(agg, 1, "jin_l").map_err(|e| self.err(e))?;

        let i64t = self.ctx.i64_type();
        let t = self.decode_field_table(struct_id);
        let n = i64t.const_int(t.n_fields, false);
        let elem_size = i64t.const_int(t.store_size, false);
        let phf_len = i64t.const_int(t.phf_len, false);
        let phf_seed = i64t.const_int(t.phf_seed, false);
        let cs = self
            .builder
            .build_call(
                self.funcs["json_decode_struct_array"],
                &[in_ptr.into(), in_len.into(), t.descs.into(), n.into(), elem_size.into(), out_ptr.into(), t.phf_ptr.into(), phf_len.into(), phf_seed.into()],
                "jdecsa",
            )
            .map_err(|e| self.err(e))?;
        Ok(cs.try_as_basic_value().basic().expect("json_decode_struct_array returns i32"))
    }

    /// `json.decode` directly into a `soa<Struct>` (the direct-fill rail): zero the out `{ptr,len}`
    /// soa-view slot, then call the runtime that counts rows, arena-allocates the columns, and fills
    /// them — same field table as the AoS path, but it passes the arena handle (the column buffer is
    /// region-tied) instead of an element stride. The returned soa view is arena-tied (no `Drop`).
    /// Returns the i32 status.
    fn gen_json_decode_soa(&mut self, struct_id: u32, input: &Operand, out: Slot, arena: &Operand) -> Result<BasicValueEnum<'c>, CodegenError> {
        let out_ptr = self.slots[&out];
        // Zero the {ptr,len} so a failed decode reads {null,0}.
        self.builder.build_store(out_ptr, slice_struct_type(self.ctx).const_zero()).map_err(|e| self.err(e))?;

        let agg = self.operand(input).into_struct_value();
        let in_ptr = self.builder.build_extract_value(agg, 0, "jin_p").map_err(|e| self.err(e))?;
        let in_len = self.builder.build_extract_value(agg, 1, "jin_l").map_err(|e| self.err(e))?;
        let arena_v = self.operand(arena);

        let i64t = self.ctx.i64_type();
        let t = self.decode_field_table(struct_id);
        let n = i64t.const_int(t.n_fields, false);
        let phf_len = i64t.const_int(t.phf_len, false);
        let phf_seed = i64t.const_int(t.phf_seed, false);
        let cs = self
            .builder
            .build_call(
                self.funcs["json_decode_soa"],
                &[in_ptr.into(), in_len.into(), t.descs.into(), n.into(), arena_v.into(), out_ptr.into(), t.phf_ptr.into(), phf_len.into(), phf_seed.into()],
                "jdecsoa",
            )
            .map_err(|e| self.err(e))?;
        Ok(cs.try_as_basic_value().basic().expect("json_decode_soa returns i32"))
    }

    /// `json.decode` into an owned `array<elem>`: zero the out `{ptr,len}` slot, then call the
    /// runtime array parser with the element tag `(kind << 8) | width`. Returns the i32 status.
    fn gen_json_decode_array(&mut self, elem: Ty, input: &Operand, out: Slot) -> Result<BasicValueEnum<'c>, CodegenError> {
        let out_ptr = self.slots[&out];
        // Zero the {ptr,len} so a failed decode reads {null,0} (its Drop / unused value frees null).
        self.builder.build_store(out_ptr, slice_struct_type(self.ctx).const_zero()).map_err(|e| self.err(e))?;

        let agg = self.operand(input).into_struct_value();
        let in_ptr = self.builder.build_extract_value(agg, 0, "jin_p").map_err(|e| self.err(e))?;
        let in_len = self.builder.build_extract_value(agg, 1, "jin_l").map_err(|e| self.err(e))?;
        // Same tag encoding as struct fields: (signed << 16) | (kind << 8) | byte-width. kind 0 =
        // int, 1 = bool, 2 = float. Bit 16 is the int sign flag (1 = signed) for the runtime
        // range-check.
        let tag: u64 = match elem {
            Ty::Int(it) => ((it.signed as u64) << 16) | (it.bits / 8) as u64,
            Ty::Bool => (1 << 8) | 1,
            Ty::Float(ft) => (2 << 8) | (ft.bits / 8) as u64,
            _ => unreachable!("json.decode array element is int/float/bool (sema-checked)"),
        };
        let tag_v = self.ctx.i32_type().const_int(tag, false);
        let cs = self
            .builder
            .build_call(
                self.funcs["json_decode_array"],
                &[in_ptr.into(), in_len.into(), tag_v.into(), out_ptr.into()],
                "jdeca",
            )
            .map_err(|e| self.err(e))?;
        Ok(cs.try_as_basic_value().basic().expect("json_decode_array returns i32"))
    }

    /// `fs.read_file(path)`: zero the out `{ptr,len}` slot, then call the runtime reader with the
    /// path `{ptr,len}`. The runtime writes the owned `string` (heap buffer freed by `Drop`) to
    /// `out`. Returns the i32 status (0 = ok).
    fn gen_fs_read_file(&mut self, path: &Operand, out: Slot) -> Result<BasicValueEnum<'c>, CodegenError> {
        let out_ptr = self.slots[&out];
        // Zero the {ptr,len} so a failed read reads {null,0} (its Drop frees null).
        self.builder.build_store(out_ptr, slice_struct_type(self.ctx).const_zero()).map_err(|e| self.err(e))?;

        let agg = self.operand(path).into_struct_value();
        let p_ptr = self.builder.build_extract_value(agg, 0, "path_p").map_err(|e| self.err(e))?;
        let p_len = self.builder.build_extract_value(agg, 1, "path_l").map_err(|e| self.err(e))?;
        let cs = self
            .builder
            .build_call(self.funcs["fs_read_file"], &[p_ptr.into(), p_len.into(), out_ptr.into()], "frf")
            .map_err(|e| self.err(e))?;
        Ok(cs.try_as_basic_value().basic().expect("fs_read_file returns i32"))
    }

    /// `io.stdout.write(arg)`: pass the `str` arg's `{ptr,len}` to the runtime writer (no newline).
    /// Returns the i32 status (0 = ok).
    fn gen_io_stdout_write(&mut self, arg: &Operand) -> Result<BasicValueEnum<'c>, CodegenError> {
        let agg = self.operand(arg).into_struct_value();
        let s_ptr = self.builder.build_extract_value(agg, 0, "out_p").map_err(|e| self.err(e))?;
        let s_len = self.builder.build_extract_value(agg, 1, "out_l").map_err(|e| self.err(e))?;
        let cs = self
            .builder
            .build_call(self.funcs["io_stdout_write"], &[s_ptr.into(), s_len.into()], "sow")
            .map_err(|e| self.err(e))?;
        Ok(cs.try_as_basic_value().basic().expect("io_stdout_write returns i32"))
    }

    fn gen_template(&mut self, pieces: &[align_mir::TemplatePiece], arena: Option<&Operand>) -> Result<BasicValueEnum<'c>, CodegenError> {
        // Pass the enclosing arena handle (or a null pointer = leak) to builder_new.
        let arena_ptr = match arena {
            Some(op) => self.operand(op),
            None => self.ctx.ptr_type(AddressSpace::default()).const_null().into(),
        };
        // A template/json.encode builder uses the default capacity (0) — static-part presizing is a
        // separate future opt; the user-facing `builder(capacity)` is the capacity surface.
        let zero = self.ctx.i64_type().const_zero();
        let bptr = self
            .builder
            .build_call(self.funcs["builder_new"], &[arena_ptr.into(), zero.into()], "b")
            .map_err(|e| self.err(e))?
            .try_as_basic_value()
            .basic()
            .expect("builder_new returns a pointer");
        let i64t = self.ctx.i64_type();
        for piece in pieces {
            match piece {
                align_mir::TemplatePiece::Static(s) => {
                    let (ptr, len) = self.str_global(s);
                    self.builder
                        .build_call(self.funcs["builder_write"], &[bptr.into(), ptr.into(), len.into()], "")
                        .map_err(|e| self.err(e))?;
                }
                align_mir::TemplatePiece::StrHole(op) => {
                    let agg = self.operand(op).into_struct_value();
                    let ptr = self.builder.build_extract_value(agg, 0, "p").map_err(|e| self.err(e))?;
                    let len = self.builder.build_extract_value(agg, 1, "l").map_err(|e| self.err(e))?;
                    self.builder
                        .build_call(self.funcs["builder_write"], &[bptr.into(), ptr.into(), len.into()], "")
                        .map_err(|e| self.err(e))?;
                }
                align_mir::TemplatePiece::IntHole(op) => {
                    let ty = self.f.operand_ty(op);
                    let v = self.operand(op).into_int_value();
                    // Use the actual LLVM width (robust even if `ty` is the error type).
                    let wide = if v.get_type().get_bit_width() < 64 {
                        if is_signed(ty) {
                            self.builder.build_int_s_extend(v, i64t, "sext").map_err(|e| self.err(e))?
                        } else {
                            self.builder.build_int_z_extend(v, i64t, "zext").map_err(|e| self.err(e))?
                        }
                    } else {
                        v
                    };
                    self.builder
                        .build_call(self.funcs["builder_write_int"], &[bptr.into(), wide.into()], "")
                        .map_err(|e| self.err(e))?;
                }
                align_mir::TemplatePiece::BoolHole(op) => {
                    let v = self.operand(op).into_int_value();
                    let wide = self.builder.build_int_z_extend(v, self.ctx.i32_type(), "bext").map_err(|e| self.err(e))?;
                    self.builder
                        .build_call(self.funcs["builder_write_bool"], &[bptr.into(), wide.into()], "")
                        .map_err(|e| self.err(e))?;
                }
                align_mir::TemplatePiece::CharHole(op) => {
                    let v = self.operand(op).into_int_value();
                    self.builder
                        .build_call(self.funcs["builder_write_char"], &[bptr.into(), v.into()], "")
                        .map_err(|e| self.err(e))?;
                }
                align_mir::TemplatePiece::FloatHole(op) => {
                    let ty = self.f.operand_ty(op);
                    let v = self.operand(op).into_float_value();
                    let callee = if ty == Ty::Float(FloatTy { bits: 32 }) { "builder_write_f32" } else { "builder_write_f64" };
                    self.builder
                        .build_call(self.funcs[callee], &[bptr.into(), v.into()], "")
                        .map_err(|e| self.err(e))?;
                }
                align_mir::TemplatePiece::JsonStrHole(op) => {
                    let agg = self.operand(op).into_struct_value();
                    let ptr = self.builder.build_extract_value(agg, 0, "jp").map_err(|e| self.err(e))?;
                    let len = self.builder.build_extract_value(agg, 1, "jl").map_err(|e| self.err(e))?;
                    self.builder
                        .build_call(self.funcs["builder_write_json_str"], &[bptr.into(), ptr.into(), len.into()], "")
                        .map_err(|e| self.err(e))?;
                }
            }
        }
        Ok(self
            .builder
            .build_call(self.funcs["builder_finish"], &[bptr.into()], "s")
            .map_err(|e| self.err(e))?
            .try_as_basic_value()
            .basic()
            .expect("builder_finish returns a str"))
    }

    /// `str == str` / `str != str` via the runtime `align_rt_str_eq`.
    fn gen_str_eq(&mut self, op: BinOp, a: &Operand, b: &Operand) -> Result<BasicValueEnum<'c>, CodegenError> {
        let sa = self.operand(a).into_struct_value();
        let sb = self.operand(b).into_struct_value();
        let ext = |b: &Builder<'c>, v: inkwell::values::StructValue<'c>, i, n| {
            b.build_extract_value(v, i, n)
        };
        let pa = ext(self.builder, sa, 0, "pa").map_err(|e| self.err(e))?;
        let la = ext(self.builder, sa, 1, "la").map_err(|e| self.err(e))?;
        let pb = ext(self.builder, sb, 0, "pb").map_err(|e| self.err(e))?;
        let lb = ext(self.builder, sb, 1, "lb").map_err(|e| self.err(e))?;
        let r = self
            .builder
            .build_call(self.funcs["str_eq"], &[pa.into(), la.into(), pb.into(), lb.into()], "streq")
            .map_err(|e| self.err(e))?
            .try_as_basic_value()
            .basic()
            .expect("str_eq returns i32")
            .into_int_value();
        let zero = self.ctx.i32_type().const_zero();
        // r != 0  ⇒  equal.
        let eq = self
            .builder
            .build_int_compare(IntPredicate::NE, r, zero, "eq")
            .map_err(|e| self.err(e))?;
        let v = match op {
            BinOp::Eq => eq,
            BinOp::Ne => self.builder.build_not(eq, "ne").map_err(|e| self.err(e))?,
            _ => return Err(self.err("str supports only == / !=")),
        };
        Ok(v.into())
    }

    /// Elementwise vector arithmetic for `vecN<T>` (M6). The element `Ty` selects the float or
    /// integer LLVM builder; inkwell's `build_int_*`/`build_float_*` accept a `VectorValue`, so the
    /// op applies lane-wise. sema restricts vector ops to `+`/`-`/`*`/`/`.
    /// Fetch an operand as a `<N x elem>` vector — a vector value as-is, or a **scalar broadcast**
    /// across all N lanes (M6: `a + 2.0`, `scores > 80`). The all-lane insertelement chain folds to a
    /// hardware broadcast at `-O2`.
    fn operand_as_vector(&mut self, op: &Operand, elem: Ty, n: u32) -> Result<inkwell::values::VectorValue<'c>, CodegenError> {
        if matches!(self.f.operand_ty(op), Ty::Vec(..)) {
            return Ok(self.operand(op).into_vector_value());
        }
        // The canonical splat: insert the scalar into lane 0, then `shufflevector` with an all-zero
        // mask broadcasts lane 0 to every lane — two instructions regardless of width `N`.
        let scalar = self.operand(op);
        let vty = vec_llvm_ty(self.ctx, elem, n).into_vector_type();
        let undef = vty.get_undef();
        let init = self.builder.build_insert_element(undef, scalar, self.ctx.i32_type().const_zero(), "splat_init").map_err(|e| self.err(e))?;
        let mask = self.ctx.i32_type().vec_type(n).const_zero();
        self.builder.build_shuffle_vector(init, undef, mask, "splat").map_err(|e| self.err(e))
    }

    /// Sum the `n` lanes of a vector into the element scalar (M6 reductions — `sum_where`, `dot`).
    /// An extract-and-add chain; the optimizer turns it into a hardware reduction.
    fn horizontal_sum(&self, v: inkwell::values::VectorValue<'c>, is_float: bool, n: u32) -> Result<BasicValueEnum<'c>, CodegenError> {
        // A vector type always has width ≥ 1 (`vecN` is 2/4/8/16) — guard the lane-0 extract below.
        assert!(n > 0, "vector width must be at least 1");
        let mut acc = self.builder.build_extract_element(v, self.ctx.i32_type().const_zero(), "hs0").map_err(|e| self.err(e))?;
        for i in 1..n {
            let idx = self.ctx.i32_type().const_int(i as u64, false);
            let lane = self.builder.build_extract_element(v, idx, "hsl").map_err(|e| self.err(e))?;
            acc = if is_float {
                self.builder.build_float_add(acc.into_float_value(), lane.into_float_value(), "hsfadd").map_err(|e| self.err(e))?.into()
            } else {
                self.builder.build_int_add(acc.into_int_value(), lane.into_int_value(), "hsadd").map_err(|e| self.err(e))?.into()
            };
        }
        Ok(acc)
    }

    /// Fold the `n` lanes of a vector into the element scalar with the scalar min/max intrinsic
    /// (M6 `v.min()`/`v.max()`) — the same `llvm.{s,u}{min,max}` / `llvm.{minimum,maximum}` as the
    /// `core.math` scalar `a.min(b)`/`a.max(b)`, so the reduction matches that semantics exactly.
    fn horizontal_minmax(&self, v: inkwell::values::VectorValue<'c>, elem: Ty, n: u32, max: bool) -> Result<BasicValueEnum<'c>, CodegenError> {
        assert!(n > 0, "vector width must be at least 1");
        let (name, overload): (&str, BasicTypeEnum<'c>) = if matches!(elem, Ty::Float(_)) {
            (if max { "llvm.maximum" } else { "llvm.minimum" }, float_type(self.ctx, elem).into())
        } else if is_signed(elem) {
            (if max { "llvm.smax" } else { "llvm.smin" }, int_type(self.ctx, elem).into())
        } else {
            (if max { "llvm.umax" } else { "llvm.umin" }, int_type(self.ctx, elem).into())
        };
        let mut acc = self.builder.build_extract_element(v, self.ctx.i32_type().const_zero(), "hm0").map_err(|e| self.err(e))?;
        for i in 1..n {
            let idx = self.ctx.i32_type().const_int(i as u64, false);
            let lane = self.builder.build_extract_element(v, idx, "hml").map_err(|e| self.err(e))?;
            acc = self.call_intrinsic(name, &[overload], &[acc.into(), lane.into()])?;
        }
        Ok(acc)
    }

    fn gen_vec_bin(&mut self, op: BinOp, a: &Operand, b: &Operand, elem: Ty, n: u32) -> Result<BasicValueEnum<'c>, CodegenError> {
        let l = self.operand_as_vector(a, elem, n)?;
        let r = self.operand_as_vector(b, elem, n)?;
        let bld = self.builder;
        // sema restricts vector ops to `+`/`-`/`*`/`/`; guard here too so any future sema hole is a
        // clean codegen error, never a panic.
        if !matches!(op, BinOp::Add | BinOp::Sub | BinOp::Mul | BinOp::Div) {
            return Err(self.err("unsupported vector operator (only + - * / are lowered)"));
        }
        let v = if matches!(elem, Ty::Float(_)) {
            match op {
                BinOp::Sub => bld.build_float_sub(l, r, "vfsub"),
                BinOp::Mul => bld.build_float_mul(l, r, "vfmul"),
                BinOp::Div => bld.build_float_div(l, r, "vfdiv"),
                _ => bld.build_float_add(l, r, "vfadd"),
            }
        } else {
            let signed = is_signed(elem);
            match op {
                BinOp::Sub => bld.build_int_sub(l, r, "vsub"),
                BinOp::Mul => bld.build_int_mul(l, r, "vmul"),
                BinOp::Div if signed => bld.build_int_signed_div(l, r, "vsdiv"),
                BinOp::Div => bld.build_int_unsigned_div(l, r, "vudiv"),
                _ => bld.build_int_add(l, r, "vadd"),
            }
        };
        Ok(v.map_err(|e| self.err(e))?.into())
    }

    /// A `vecN<T>` comparison (M6) → a `<N x i1>` mask. The element `Ty` selects integer vs float
    /// comparison; inkwell's `build_int_compare`/`build_float_compare` accept `VectorValue`, so the
    /// predicate applies lane-wise. (Reuses the scalar predicate mapping — `pred`/`FloatPredicate`.)
    fn gen_vec_cmp(&mut self, op: BinOp, a: &Operand, b: &Operand, elem: Ty, n: u32) -> Result<BasicValueEnum<'c>, CodegenError> {
        let l = self.operand_as_vector(a, elem, n)?;
        let r = self.operand_as_vector(b, elem, n)?;
        let bld = self.builder;
        let v = if matches!(elem, Ty::Float(_)) {
            let p = match op {
                BinOp::Eq => FloatPredicate::OEQ,
                // UNE (not ONE): IEEE 754 requires `NaN != x` to be true.
                BinOp::Ne => FloatPredicate::UNE,
                BinOp::Lt => FloatPredicate::OLT,
                BinOp::Le => FloatPredicate::OLE,
                BinOp::Gt => FloatPredicate::OGT,
                _ => FloatPredicate::OGE,
            };
            bld.build_float_compare(p, l, r, "vfcmp")
        } else {
            let signed = is_signed(elem);
            let p = match op {
                BinOp::Eq => IntPredicate::EQ,
                BinOp::Ne => IntPredicate::NE,
                BinOp::Lt => pred(signed, Cmp::Lt),
                BinOp::Le => pred(signed, Cmp::Le),
                BinOp::Gt => pred(signed, Cmp::Gt),
                _ => pred(signed, Cmp::Ge),
            };
            bld.build_int_compare(p, l, r, "vicmp")
        };
        Ok(v.map_err(|e| self.err(e))?.into())
    }

    fn gen_float_bin(&mut self, op: BinOp, a: &Operand, b: &Operand) -> Result<BasicValueEnum<'c>, CodegenError> {
        let l = self.operand(a).into_float_value();
        let r = self.operand(b).into_float_value();
        let bld = self.builder;
        let v: BasicValueEnum<'c> = match op {
            BinOp::Add => bld.build_float_add(l, r, "fadd").map_err(|e| self.err(e))?.into(),
            BinOp::Sub => bld.build_float_sub(l, r, "fsub").map_err(|e| self.err(e))?.into(),
            BinOp::Mul => bld.build_float_mul(l, r, "fmul").map_err(|e| self.err(e))?.into(),
            BinOp::Div => bld.build_float_div(l, r, "fdiv").map_err(|e| self.err(e))?.into(),
            BinOp::Rem => bld.build_float_rem(l, r, "frem").map_err(|e| self.err(e))?.into(),
            BinOp::Eq => bld.build_float_compare(FloatPredicate::OEQ, l, r, "feq").map_err(|e| self.err(e))?.into(),
            // UNE (unordered-or-not-equal), not ONE: IEEE 754 requires `NaN != x` to be
            // true, and ONE (ordered-and-not-equal) returns false when either side is NaN.
            BinOp::Ne => bld.build_float_compare(FloatPredicate::UNE, l, r, "fne").map_err(|e| self.err(e))?.into(),
            BinOp::Lt => bld.build_float_compare(FloatPredicate::OLT, l, r, "flt").map_err(|e| self.err(e))?.into(),
            BinOp::Le => bld.build_float_compare(FloatPredicate::OLE, l, r, "fle").map_err(|e| self.err(e))?.into(),
            BinOp::Gt => bld.build_float_compare(FloatPredicate::OGT, l, r, "fgt").map_err(|e| self.err(e))?.into(),
            BinOp::Ge => bld.build_float_compare(FloatPredicate::OGE, l, r, "fge").map_err(|e| self.err(e))?.into(),
            BinOp::And | BinOp::Or | BinOp::BitAnd | BinOp::BitOr | BinOp::BitXor | BinOp::Shl | BinOp::Shr => {
                unreachable!("logical/bitwise operators are not valid on floats (sema-checked)")
            }
        };
        Ok(v)
    }

    fn operand(&self, op: &Operand) -> BasicValueEnum<'c> {
        match op {
            Operand::Const(Const::Int(v, ty)) => {
                int_type(self.ctx, *ty).const_int(*v as u64, is_signed(*ty)).into()
            }
            Operand::Const(Const::Float(v, ty)) => float_type(self.ctx, *ty).const_float(*v).into(),
            Operand::Const(Const::Char(v)) => self.ctx.i32_type().const_int(*v as u64, false).into(),
            Operand::Const(Const::Bool(v)) => self.ctx.bool_type().const_int(*v as u64, false).into(),
            Operand::Const(Const::Unit) => self.ctx.i32_type().const_int(0, false).into(),
            Operand::Value(id) => self.values[id],
            Operand::Arg(i) => self.func.get_nth_param(*i).expect("param index in range"),
        }
    }
}

enum Cmp {
    Lt,
    Le,
    Gt,
    Ge,
}

/// Whether `op` is a comparison (`==`/`!=`/`<`/`<=`/`>`/`>=`) — used to route a `vecN<T>` operand to
/// the mask-producing comparison path instead of vector arithmetic.
fn is_comparison(op: BinOp) -> bool {
    matches!(op, BinOp::Eq | BinOp::Ne | BinOp::Lt | BinOp::Le | BinOp::Gt | BinOp::Ge)
}

fn pred(signed: bool, c: Cmp) -> IntPredicate {
    use IntPredicate::*;
    match (signed, c) {
        (true, Cmp::Lt) => SLT,
        (true, Cmp::Le) => SLE,
        (true, Cmp::Gt) => SGT,
        (true, Cmp::Ge) => SGE,
        (false, Cmp::Lt) => ULT,
        (false, Cmp::Le) => ULE,
        (false, Cmp::Gt) => UGT,
        (false, Cmp::Ge) => UGE,
    }
}

fn write_object(module: &Module, out: &Path, target: &BuildTarget) -> Result<(), CodegenError> {
    let tm = create_target_machine(target)?;

    // Run the LLVM middle-end optimization pipeline (`-O2`) before emitting. Without this, only the
    // backend codegen passes run — the lifted-lambda calls, fused `map`/`reduce`/`where` loops, and
    // bounds checks are left un-inlined and un-vectorized. The default `-O2` pipeline inlines the
    // per-element calls, hoists invariants (LICM), and runs the loop / SLP vectorizers, so the
    // data-oriented core actually lowers to SIMD. Purely additive — no IR is generated differently.
    module
        .run_passes("default<O2>", &tm, PassBuilderOptions::create())
        .map_err(|e| CodegenError::Target(format!("optimization pipeline: {e}")))?;

    tm.write_to_file(module, FileType::Object, out)
        .map_err(|e| CodegenError::Target(e.to_string()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use align_diag::Diagnostics;
    use align_lexer::tokenize;
    use align_mir::lower_program;
    use align_parser::parse_file;
    use align_sema::check_file;

    fn ir(src: &str) -> String {
        let mut d = Diagnostics::new();
        let toks = tokenize(0, src, &mut d);
        let f = parse_file(toks, &mut d);
        let hir = check_file(&f, &mut d);
        assert!(!d.has_errors());
        emit_llvm_ir(&lower_program(&hir), &BuildTarget::Baseline).unwrap()
    }

    /// Layout parity: the sema `(size, align)` computation (`align_sema::ty_size_align` /
    /// `struct_size_align`, which the huge-struct-copy lint trusts) must equal the **real** LLVM ABI
    /// size/alignment of the struct as codegen lays it out (descending-alignment field order via
    /// `logical_to_physical`). This pins the two independent hand-written layout computations
    /// (`field_abi_align` here vs `ty_size_align` in sema) against LLVM ground truth, so any future
    /// drift — or a new wider-aligned field type added to `is_field_ok` without updating both — fails
    /// loudly. Covers every valid struct-field type, mixed widths that force a reorder, `str`/`string`
    /// views, nested structs, and `layout(C)` (declaration order preserved). `align(N)` over-alignment
    /// is deliberately excluded: it is applied at the alloca seam (`type_align`), not baked into the
    /// LLVM struct type, so it is an orthogonal concern.
    #[test]
    fn sema_and_codegen_struct_layout_agree() {
        fn i(bits: u8, signed: bool) -> Ty {
            Ty::Int(IntTy { bits, signed })
        }
        fn f(bits: u8) -> Ty {
            Ty::Float(FloatTy { bits })
        }
        fn sdef(name: &str, c_repr: bool, fields: &[Ty]) -> StructDef {
            StructDef {
                name: name.to_string(),
                fields: fields
                    .iter()
                    .enumerate()
                    .map(|(k, &ty)| align_sema::hir::FieldDef { name: format!("f{k}"), ty })
                    .collect(),
                align: None,
                c_repr,
            }
        }

        // Structs 0..=2 are nested targets referenced by later structs (ids are positional).
        let structs = vec![
            sdef("Inner0", false, &[i(8, false), i(64, true)]),          // 0: reorders internally
            sdef("InnerC", true, &[i(8, false), i(64, true)]),           // 1: layout(C), decl order
            sdef("Pair", false, &[i(32, true), i(32, true)]),            // 2
            // every scalar alone
            sdef("Si8", false, &[i(8, true)]),
            sdef("Si16", false, &[i(16, true)]),
            sdef("Si32", false, &[i(32, true)]),
            sdef("Si64", false, &[i(64, true)]),
            sdef("Su8", false, &[i(8, false)]),
            sdef("Sf32", false, &[f(32)]),
            sdef("Sf64", false, &[f(64)]),
            sdef("Sbool", false, &[Ty::Bool]),
            sdef("Schar", false, &[Ty::Char]),
            sdef("Sstr", false, &[Ty::Str]),
            sdef("Sstring", false, &[Ty::String]),
            // mixed widths that force a reorder (the padding-elimination cases)
            sdef("Mix1", false, &[i(8, true), i(64, true), i(8, true)]),
            sdef("Mix2", false, &[Ty::Bool, f(64), i(16, true), i(8, true)]),
            sdef("Mix3", false, &[i(8, true), Ty::Str, Ty::Bool, i(32, true)]),
            sdef("Mix4", false, &[Ty::Char, i(8, false), i(64, false), Ty::Bool, f(32)]),
            // the same field set with layout(C) — must NOT reorder
            sdef("MixC", true, &[i(8, true), i(64, true), i(8, true)]),
            // nested structs (reordered + layout(C) inner)
            sdef("Nest1", false, &[i(8, true), Ty::Struct(0), i(16, true)]),
            sdef("Nest2", false, &[Ty::Struct(1), Ty::Bool, Ty::Struct(2)]),
        ];

        let ctx = Context::create();
        let tm = create_target_machine(&BuildTarget::Baseline).expect("target machine");
        let td = tm.get_target_data();

        // Build the LLVM struct types exactly as `codegen` does (opaque, then body in physical order).
        let struct_types: Vec<StructType> = structs.iter().map(|s| ctx.opaque_struct_type(&s.name)).collect();
        for (s, st) in structs.iter().zip(&struct_types) {
            let perm = logical_to_physical(s, &structs);
            let order = physical_order(&perm);
            let fields: Vec<BasicTypeEnum> =
                order.iter().map(|&li| abi_type(&ctx, s.fields[li as usize].ty, &struct_types, &[])).collect();
            st.set_body(&fields, false);
        }

        for (id, s) in structs.iter().enumerate() {
            let st = struct_types[id];
            let llvm = (td.get_abi_size(&st), td.get_abi_alignment(&st) as u64);
            let sema = align_sema::struct_abi_layout(id as u32, &structs);
            assert_eq!(sema, llvm, "layout parity mismatch on `{}` (sema {sema:?} vs LLVM {llvm:?})", s.name);
        }
    }

    #[test]
    fn align_functions_are_marked_nounwind() {
        // Align functions never unwind, so codegen marks them `nounwind` (drops exception edges /
        // unwind tables, enables more inlining). Every Align-defined function carries it...
        let out = ir("fn sq(x: i64) -> i64 = x * x\nfn main() -> i32 = sq(7) as i32\n");
        assert!(out.contains("define i64 @sq(i64 %0) #0"));
        assert!(out.contains("define i32 @main() #0"));
        assert!(out.contains("attributes #0 = { nounwind }"));
        // ...but the external runtime declarations (ordinary Rust fns) are NOT promised nounwind.
        let out2 = ir("fn main() -> i32 {\n  print(1)\n  return 0\n}\n");
        assert!(out2.contains("declare void @align_rt_print_i64(i64)\n"));
    }

    #[test]
    fn emitted_ir_is_self_describing() {
        // `emit-llvm` output pins both the data layout AND the target triple, so an external
        // `opt`/`llc` reading it uses the right cost model / vectorizer instead of a generic one.
        let out = ir("fn main() -> i32 = 0\n");
        assert!(out.contains("target datalayout = \""), "want a data layout:\n{out}");
        assert!(out.contains("target triple = \""), "want a target triple:\n{out}");
    }

    #[test]
    fn allocator_declarations_carry_noalias_and_hygiene_attrs() {
        // Every runtime builtin is declared unconditionally, so a trivial program still emits them.
        let out = ir("fn main() -> i32 = 0\n");
        // The allocator family returns a fresh allocation → `noalias` on the return value.
        for sym in [
            "align_rt_alloc",
            "align_rt_arena_alloc",
            "align_rt_tg_alloc",
            "align_rt_arena_begin",
            "align_rt_tg_begin",
            "align_rt_builder_new",
            "align_rt_par_map", // fresh output buffer
        ] {
            assert!(out.contains(&format!("declare noalias ptr @{sym}")), "want noalias on {sym}:\n{out}");
        }
        // Single-shot allocators also get `nofree` + `nounwind` — but deliberately NOT `willreturn`
        // (they `abort` on OOM, so asserting they always return would be a miscompile).
        assert!(out.contains("nofree"), "want nofree on the single-shot alloc family:\n{out}");
        assert!(!out.contains("willreturn"), "must NOT claim willreturn (alloc can abort):\n{out}");

        // The **bump** allocators must NOT carry `nofree`: growing the region `Vec::push`es the
        // chunk list, which can reallocate (free) memory allocated before the call. Resolve each
        // one's attribute-group number and assert that group has no `nofree`.
        let group_has_nofree = |sym: &str| -> bool {
            let decl = out.lines().find(|l| l.contains(&format!("@{sym}("))).expect("decl present");
            let n = decl.rsplit('#').next().and_then(|s| s.trim().parse::<u32>().ok());
            match n {
                None => false, // no attribute group at all → certainly no nofree
                Some(n) => out
                    .lines()
                    .find(|l| l.starts_with(&format!("attributes #{n} = ")))
                    .is_some_and(|l| l.contains("nofree")),
            }
        };
        assert!(!group_has_nofree("align_rt_arena_alloc"), "bump alloc must not be nofree:\n{out}");
        assert!(!group_has_nofree("align_rt_tg_alloc"), "bump alloc must not be nofree:\n{out}");
        // ...while a single-shot one does.
        assert!(group_has_nofree("align_rt_alloc"), "single-shot alloc should be nofree:\n{out}");
    }

    #[test]
    fn phf_hash_is_pinned() {
        // Pins the hash of a known input so the codegen and runtime implementations can't silently
        // drift apart (the runtime's `json_phf_hash` must produce the same value — see its test).
        assert_eq!(phf_hash(b"score", 0), 0xc10e_63fb_e7f6_24f5);
    }

    #[test]
    fn build_phf_is_collision_free_and_covers_each_field() {
        let names = ["id", "score", "age", "rank", "active", "name", "lat", "lon"];
        let (slots, seed) = build_phf(&names).expect("a small distinct name set has a PHF");
        assert!(slots.len().is_power_of_two());
        // Every field maps to its own slot, and a round-trip through the slot recovers its index.
        for (i, n) in names.iter().enumerate() {
            let slot = (phf_hash(n.as_bytes(), seed) & (slots.len() as u64 - 1)) as usize;
            assert_eq!(slots[slot], i as i32, "field {n} should resolve to index {i}");
        }
    }

    #[test]
    fn build_phf_declines_trivial_sets() {
        // 0/1-field structs use the linear scan (already O(1)); no table is emitted.
        assert!(build_phf(&[]).is_none());
        assert!(build_phf(&["only"]).is_none());
    }

    #[test]
    fn m0_emits_main_returning_i32() {
        let text = ir("fn main() -> i32 {\n  x := 1\n  return x\n}\n");
        assert!(text.contains("define i32 @main()"), "got:\n{text}");
    }

    #[test]
    fn fib_emits_calls_and_branch() {
        let src = "fn fib(n: i64) -> i64 {\n  if n < 2 { return n }\n  return fib(n - 1) + fib(n - 2)\n}\n";
        let text = ir(src);
        assert!(text.contains("define i64 @fib(i64"), "got:\n{text}");
        assert!(text.contains("call i64 @fib"), "expected recursive calls:\n{text}");
        assert!(text.contains("icmp slt"), "expected signed comparison:\n{text}");
    }
}
