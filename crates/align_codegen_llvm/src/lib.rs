//! Backend: MIR -> LLVM IR -> object (`docs/impl/05-backend-llvm.md`).
//!
//! A pure-lowering stage. Align's semantic decisions (desugaring, fusion, SIMD-ization,
//! regions) are already done in MIR; this just maps MIR to LLVM IR mechanically and
//! does not recompute types (anti-rewrite, `00-overview.md`).
//!
//! M1 model: named locals are allocas (LLVM's mem2reg promotes them to SSA); reads are
//! loads, writes are stores; `if` is conditional branches; comparisons are `icmp`;
//! calls are `call`. The generated `main` is the C entry (crt0 calls it).

use std::collections::{HashMap, HashSet};
use std::path::Path;

/// ThinLTO driver-facing surface (production): safe wrappers over the C++ shim's
/// three summary-based entry points, plus the preserve-set / opt-level helpers.
pub mod thinlto;

/// ThinLTO S0 feasibility spike (feature-gated; historical S0 go/no-go probes).
#[cfg(feature = "thinlto-spike")]
pub mod thinlto_spike;

/// Instrument-PGO driver-facing surface (production): the safe wrapper over the
/// C++ shim's `align_pgo_run_pipeline` entry for `--pgo-instrument` / `--pgo-use`.
pub mod pgo;

use align_ast::{BinOp, UnOp};
use align_mir::{Block, Const, ConstElem, Function, Operand, Program, Rvalue, Slot, Stmt, Term, ValueId};
use align_sema::{enum_is_move, payload_is_move, struct_is_move, ty_to_scalar, EnumDef, FloatTy, IntTy, Layout, Scalar, StructDef, TupleDef, Ty, scalar_to_ty, ERROR_VARIANT_CODE};

use inkwell::AddressSpace;
use inkwell::FloatPredicate;
use inkwell::IntPredicate;
use inkwell::OptimizationLevel;
use inkwell::basic_block::BasicBlock;
use inkwell::builder::Builder;
use inkwell::context::Context;
use inkwell::debug_info::{
    AsDIScope, DIFlags, DIFlagsConstants, DIFile, DISubprogram, DWARFEmissionKind,
    DWARFSourceLanguage, DebugInfoBuilder,
};
use inkwell::intrinsics::Intrinsic;
use inkwell::memory_buffer::MemoryBuffer;
use inkwell::module::{Linkage, Module};
use inkwell::passes::PassBuilderOptions;
use inkwell::targets::{
    CodeModel, FileType, InitializationConfig, RelocMode, Target, TargetMachine,
};
use inkwell::types::{BasicMetadataTypeEnum, BasicType, BasicTypeEnum, FloatType, IntType, StructType};
use inkwell::values::{BasicValue, BasicValueEnum, FunctionValue, IntValue};

pub fn is_available() -> bool {
    true
}

/// The major version of the LLVM this compiler is built against (the `inkwell` `llvm22-1` feature /
/// llvm-sys 221). Toolchain discovery uses it to find version-suffixed LLVM tools (`llvm-readobj-22`,
/// apt.llvm.org naming). **Update on every LLVM upgrade**, together with the `inkwell` feature in
/// `Cargo.toml` and the `LLVM_SYS_*_PREFIX` env-var name in `align_driver::llvm_tool`.
pub const LLVM_TOOL_VERSION: &str = "22";

/// The object-file format the build targets — the classification every format-dependent toolchain
/// step shares (linker-flag spelling in the driver, `alignc size` inspection). Lives here, next to
/// the other triple sniffing (the baseline-CPU floor in [`create_target_machine`], the SysV check in
/// `build_module`), so triple classification stays in one place: codegen.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ObjectFormat {
    /// ELF (Linux and the other System-V platforms): GNU-style linker flags, `DT_NEEDED` deps.
    Elf,
    /// Mach-O (macOS / Darwin): ld64 linker flags, `LC_LOAD_DYLIB` deps.
    MachO,
}

/// Classify the build target's object format from the default (host) triple.
///
/// Windows is fail-closed: an explicit error, never a guess — the driver must not invoke the
/// linker with the wrong flag dialect there. Any other unrecognized triple is presumed ELF (the
/// System-V default shared by Linux and the BSDs, where the GNU-style flag set applies).
///
/// Cross-compilation seam (M15+): when builds take an explicit target triple, this widens to take
/// the `BuildTarget` as an argument instead of reading the host default triple.
pub fn target_object_format() -> Result<ObjectFormat, String> {
    let triple = TargetMachine::get_default_triple();
    let ts = triple.as_str().to_string_lossy().to_ascii_lowercase();
    if ts.contains("apple") || ts.contains("darwin") {
        Ok(ObjectFormat::MachO)
    } else if ts.contains("windows") {
        Err(format!("linking for target '{ts}' is not supported yet"))
    } else {
        Ok(ObjectFormat::Elf)
    }
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

/// A build profile (`--profile`): the single knob that selects the optimization/size trade-off. It
/// is the one mechanism through which the profile plumbs — it owns *both* the middle-end pipeline
/// string ([`Profile::pipeline`]) and the profile-dependent linker choices ([`Profile::strip`]), so
/// no scattered `match profile` ifs live in the driver (`docs/impl/07-roadmap.md` M13 Slice 4).
///
/// Deliberately the **stock** LLVM pipelines (`default<O0|O2|O3|Os|Oz>`), not a custom pass order —
/// per the external-optimization consultation, no custom pipeline until remarks + benchmarks justify
/// one (`docs/open-questions.md` → "External optimization consultation").
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Profile {
    /// `dev` → `default<O0>`: no optimization, fastest builds, best debuggability. Symbols kept.
    Dev,
    /// `release` → `default<O2>`: the balanced default. This is today's behavior (a build with no
    /// `--profile` flag runs `default<O2>`), so `release` is the default → no behavior change without
    /// the flag. Symbols kept (a stack trace / `perf` on a crashing release binary stays useful).
    #[default]
    Release,
    /// `fast` → `default<O3>`: maximum speed (more aggressive inlining / vectorization / unrolling),
    /// larger code. Symbols kept.
    Fast,
    /// `small` → `default<Os>`: optimize for size while keeping O2-like speed. Stripped.
    Small,
    /// `tiny` → `default<Oz>`: minimize size at any cost. Stripped.
    Tiny,
}

impl Profile {
    /// The stock LLVM pass-pipeline string this profile runs (fed verbatim to
    /// `Module::run_passes` via [`run_opt_pipeline`]). Exactly the `default<O*>` set — no custom order.
    pub fn pipeline(self) -> &'static str {
        match self {
            Profile::Dev => "default<O0>",
            Profile::Release => "default<O2>",
            Profile::Fast => "default<O3>",
            Profile::Small => "default<Os>",
            Profile::Tiny => "default<Oz>",
        }
    }

    /// The codegen (machine-code) opt level for the `TargetMachine` — a separate dimension from the
    /// IR pass `pipeline()`. `small`/`tiny` stay at `Default` on purpose, same as clang: size
    /// reduction is carried by the `optsize`/`minsize` fn attrs plus the `default<Os|Oz>` pipeline;
    /// lowering the codegen level does not shrink size, it only slows the code down.
    pub fn codegen_opt_level(self) -> OptimizationLevel {
        match self {
            Profile::Dev => OptimizationLevel::None,
            Profile::Release => OptimizationLevel::Default,
            Profile::Fast => OptimizationLevel::Aggressive,
            Profile::Small => OptimizationLevel::Default,
            Profile::Tiny => OptimizationLevel::Default,
        }
    }

    /// The codegen opt level as a stable string for the S3 cache key (`none`/`less`/`default`/
    /// `aggressive`). Mirrors [`Profile::codegen_opt_level`] without leaking the inkwell enum type to
    /// the driver.
    pub fn codegen_opt_name(self) -> &'static str {
        match self.codegen_opt_level() {
            OptimizationLevel::None => "none",
            OptimizationLevel::Less => "less",
            OptimizationLevel::Default => "default",
            OptimizationLevel::Aggressive => "aggressive",
        }
    }

    /// Whether all symbols should be stripped from the final image (spelled per object format by
    /// the driver: ELF links with `-Wl,--strip-all`, Mach-O runs `strip` post-link). The
    /// size profiles (`small`/`tiny`) strip — the symbol table is pure size a size build does not
    /// want; the speed profiles (`dev`/`release`/`fast`) keep symbols so a crash backtrace / `perf`
    /// stays useful. (Pre-release, changeable — documented at the decision site.)
    pub fn strip(self) -> bool {
        matches!(self, Profile::Small | Profile::Tiny)
    }

    /// The exact profile name (the spelling accepted on the CLI). No aliases.
    pub fn name(self) -> &'static str {
        match self {
            Profile::Dev => "dev",
            Profile::Release => "release",
            Profile::Fast => "fast",
            Profile::Small => "small",
            Profile::Tiny => "tiny",
        }
    }

    /// Parse a `--profile` value. Exact names only (no aliases, no prefixes); any other value returns
    /// `None` so the caller emits a diagnostic rather than silently guessing.
    pub fn parse(s: &str) -> Option<Profile> {
        match s {
            "dev" => Some(Profile::Dev),
            "release" => Some(Profile::Release),
            "fast" => Some(Profile::Fast),
            "small" => Some(Profile::Small),
            "tiny" => Some(Profile::Tiny),
            _ => None,
        }
    }

    /// All profile names, for usage/diagnostic text (kept in sync with [`Profile::parse`]).
    pub const NAMES: &'static str = "dev, release, fast, small, tiny";
}

/// Resolve the `(cpu, features)` pair codegen targets — the single place the CPU/feature string is
/// picked, shared by [`create_target_machine`] (which builds the machine) and
/// [`resolve_target_identity`] (which reports the resolved identity for the cache key). `triple_lower`
/// is the default triple lowercased. Keeping this one function guarantees the cache key hashes the
/// EXACT cpu/features codegen will use — `native` is never keyed as the literal string `"native"`, it
/// is resolved here to the host's concrete cpu + feature set (doc-10 §6.2).
fn resolve_cpu_features(target: &BuildTarget, triple_lower: &str) -> (String, String) {
    match target {
        BuildTarget::Native => (
            TargetMachine::get_host_cpu_name().to_string(),
            TargetMachine::get_host_cpu_features().to_string(),
        ),
        BuildTarget::Cpu(name) => (name.clone(), String::new()),
        BuildTarget::Baseline => {
            // Conservative per-arch floor: bump amd64 to x86-64-v2 (still pre-AVX, so cloud-safe);
            // arm64 / anything else uses `generic` (= the architecture baseline, e.g. armv8-a).
            let cpu = if triple_lower.starts_with("x86_64") || triple_lower.starts_with("amd64") {
                "x86-64-v2"
            } else {
                "generic"
            };
            (cpu.to_string(), String::new())
        }
    }
}

/// The relocation model codegen uses (kept in sync with [`create_target_machine`]'s `RelocMode::PIC`),
/// as a stable string for the cache key.
const RELOC_MODEL: &str = "PIC";
/// The code model codegen uses (kept in sync with [`create_target_machine`]'s `CodeModel::Default`),
/// as a stable string for the cache key.
const CODE_MODEL: &str = "Default";

/// The resolved, concrete codegen target identity — the target-dependent part of the S3 codegen cache
/// key. `cpu`/`features` are already resolved (never the literal `"native"`), so two hosts that
/// resolve `native` differently get distinct keys (doc-10 §6.2).
pub struct ResolvedTarget {
    pub triple: String,
    pub cpu: String,
    pub features: String,
    pub reloc_model: &'static str,
    pub code_model: &'static str,
}

/// Initialize the native LLVM target exactly once per process. LLVM's `initialize_native` mutates
/// process-global target registries, so it must not race; a `Once` serializes the first call and makes
/// every later call a cheap no-op. **The parallel build driver calls this on the main thread BEFORE
/// spawning codegen workers** (`docs/impl/07-roadmap.md` M15 S3 "LLVM target-init once on the main
/// thread before the scope"); per-thread `create_target_machine` calls it too, so a direct caller is
/// still safe. A failed init is remembered so every caller sees the same error.
pub fn ensure_target_initialized() -> Result<(), CodegenError> {
    use std::sync::OnceLock;
    static INIT: OnceLock<Result<(), String>> = OnceLock::new();
    INIT.get_or_init(|| {
        Target::initialize_native(&InitializationConfig::default()).map_err(|e| format!("native target init: {e}"))
    })
    .clone()
    .map_err(CodegenError::Target)
}

/// Resolve the concrete codegen identity for `target` (triple + resolved cpu/features + reloc/code
/// model) without building a `TargetMachine`. Used by the driver to build the codegen cache key from
/// the SAME resolution [`create_target_machine`] uses, so a cache hit implies byte-identical codegen.
pub fn resolve_target_identity(target: &BuildTarget) -> Result<ResolvedTarget, CodegenError> {
    ensure_target_initialized()?;
    let triple = TargetMachine::get_default_triple();
    let triple_str = triple.as_str().to_string_lossy().to_string();
    let (cpu, features) = resolve_cpu_features(target, &triple_str.to_ascii_lowercase());
    Ok(ResolvedTarget { triple: triple_str, cpu, features, reloc_model: RELOC_MODEL, code_model: CODE_MODEL })
}

/// The exact LLVM version this compiler is dynamically linked against, `"major.minor.patch"`, read at
/// runtime from `LLVMGetVersion` (never a hand-typed constant — doc-10 §6.2: a minor/patch change can
/// alter pass pipelines or object bytes, so the codegen cache key must pin the exact version).
pub fn llvm_version() -> String {
    let (mut major, mut minor, mut patch) = (0u32, 0u32, 0u32);
    // SAFETY: `LLVMGetVersion` writes three `unsigned` out-params; the pointers are to live u32 locals.
    unsafe {
        llvm_sys::core::LLVMGetVersion(&mut major, &mut minor, &mut patch);
    }
    format!("{major}.{minor}.{patch}")
}

/// Build the `TargetMachine` for `target` — the one place that picks the CPU / feature string, so
/// the data-layout machine (`build_module`) and the emission machine (`write_object`) always agree.
///
/// `opt` is the codegen (machine-code) opt level — a separate dimension from the IR pass pipeline
/// (`Profile::pipeline()`). Object emission threads the profile's [`Profile::codegen_opt_level`];
/// the diagnostic lenses (`emit_llvm_ir` / `collect_opt_remarks`) pin `Default` so their IR shape
/// stays profile-independent.
fn create_target_machine(target: &BuildTarget, opt: OptimizationLevel) -> Result<TargetMachine, CodegenError> {
    ensure_target_initialized()?;
    let triple = TargetMachine::get_default_triple();
    let t = Target::from_triple(&triple)
        .map_err(|e| CodegenError::Target(format!("triple resolution: {e}")))?;
    let (cpu, features) = resolve_cpu_features(target, &triple.as_str().to_string_lossy().to_ascii_lowercase());
    t.create_target_machine(
        &triple,
        &cpu,
        &features,
        opt,
        // Kept in sync with RELOC_MODEL / CODE_MODEL (the strings the cache key hashes).
        RelocMode::PIC,
        CodeModel::Default,
    )
    .ok_or_else(|| CodegenError::Target("failed to create TargetMachine".to_string()))
}

/// Write the program as an object file.
///
/// `exports` names the program functions (matched against `Function::name`, NOT the LLVM-symbol
/// `main`/`align_main` split) that keep `external` linkage instead of the default whole-program
/// `internal` (M13 Slice 1) — the explicit export-roots mechanism (`emit-obj --export`,
/// `docs/impl/07-roadmap.md` M13 Codex-audit item 1). Empty = every program function stays
/// internal, today's default behavior.
///
/// Creates exactly one `TargetMachine` for the whole compile and threads it through both
/// `build_module` (data layout / triple / ABI classification) and `write_object` (optimization +
/// object emission), so the two stages always agree on the same target settings.
/// `rt_lto` = the baked `--rt-lto` runtime bitcode (`Some(bytes)`), or `None` for the default
/// (flag-off) path, which stays byte-identical to before this slice. When `Some`, the fast-path
/// string primitives' bodies are linked into the RAW module and internalized before the single opt
/// run, so the optimizer can inline them (probe: `str_eq` 2.1×) — M14 Slice 2. Probe-then-annotate:
/// the baked bitcode is parsed FIRST; only if that succeeds are the guarded declares left
/// un-curated. An unparseable artifact emits a loud diagnostic and falls back to the flag-off path
/// with the curated contract intact (never silently dropped).
/// The shared module-construction prologue for every whole-unit emit path — the ordinary object
/// ([`emit_object`]), instrument-PGO ([`emit_object_pgo`]), and ThinLTO prelink ([`emit_prelink_bc`]).
///
/// It builds the [`TargetMachine`] and the fully-populated [`Module`] (probe the baked `--rt-lto`
/// bitcode → `build_module` → merge/internalize the fast-path bodies on a hit → size-attr sweep), then
/// returns both for the caller's optimize+emit tail. Extracting it makes the "identical construction"
/// contract these three paths share COMPILER-ENFORCED rather than three hand-copied prologues that can
/// silently drift. The caller owns the [`Context`] (a `Module` borrows it), so it is passed in.
fn build_program_module<'c>(
    ctx: &'c Context,
    program: &Program,
    target: &BuildTarget,
    profile: Profile,
    exports: &[String],
    rt_lto: Option<&[u8]>,
) -> Result<(Module<'c>, TargetMachine), CodegenError> {
    let module = ctx.create_module("align");
    let tm = create_target_machine(target, profile.codegen_opt_level())?;
    // Probe the baked bitcode before deciding whether to skip curating the guarded declares.
    let rt_module = rt_lto.and_then(|bc| probe_rt_lto(ctx, bc));
    build_module(ctx, &module, program, &tm, None, exports, rt_module.is_some())?;
    if let Some(rt) = rt_module {
        // On a datalayout mismatch `link_in_rt_lto` falls back on its own (loud diagnostic, guarded
        // declares re-curated, no merge) — see its doc comment; nothing left to do here either way.
        link_in_rt_lto(ctx, &module, rt)?;
    }
    // Size profiles get their `optsize`/`minsize` sweep here — object path only, so the diagnostic
    // lenses (`emit_llvm_ir` / `collect_opt_remarks`) see a byte-identical module structure.
    apply_size_attrs(ctx, &module, profile);
    Ok((module, tm))
}

pub fn emit_object(program: &Program, out: &Path, target: &BuildTarget, profile: Profile, exports: &[String], rt_lto: Option<&[u8]>) -> Result<(), CodegenError> {
    let ctx = Context::create();
    let (module, tm) = build_program_module(&ctx, program, target, profile, exports, rt_lto)?;
    write_object(&module, out, &tm, profile.pipeline())
}

/// Emit one unit's object under instrument-PGO (`--pgo-instrument` / `--pgo-use`, S1).
///
/// Module construction is IDENTICAL to [`emit_object`] (data layout / ABI / `--rt-lto`
/// merge / size attrs) — only the optimization step differs: instead of `write_object`'s
/// stock `default<O*>` opt run, the RAW module is handed to the PGO shim, which runs the
/// per-module default pipeline with a populated `PGOOptions` (GEN inserts counters; USE
/// attaches `!prof branch_weights`). Object emission then stays on the same
/// `tm.write_to_file` seam. `pgo::PgoAction::Use` requires a caller-validated profile
/// (see [`pgo::run_pgo_pipeline`]'s contract). Returns the run's [`pgo::PgoRunReport`]
/// (USE staleness warnings; empty for GEN) so the driver can aggregate one report.
///
/// This is a SEPARATE entry from [`emit_object`] so the flag-off path stays byte-identical.
pub fn emit_object_pgo(
    program: &Program,
    out: &Path,
    target: &BuildTarget,
    profile: Profile,
    exports: &[String],
    rt_lto: Option<&[u8]>,
    action: pgo::PgoAction<'_>,
) -> Result<pgo::PgoRunReport, CodegenError> {
    let ctx = Context::create();
    let (module, tm) = build_program_module(&ctx, program, target, profile, exports, rt_lto)?;
    // The per-module default pipeline opt level matches a normal build (release = O2,
    // fast = O3) — reusing the ThinLTO opt-level mapping (both are middle-end levels).
    let opt = thinlto::ir_opt_level(profile);
    // SAFETY: `module.as_mut_ptr()` / `tm.as_mut_ptr()` are live handles in the process
    // LLVM with a datalayout set by `build_module`; `module`/`tm`/`ctx` outlive the call.
    let report =
        unsafe { pgo::run_pgo_pipeline(module.as_mut_ptr(), tm.as_mut_ptr(), opt, action)? };
    tm.write_to_file(&module, FileType::Object, out)
        .map_err(|e| CodegenError::Target(e.to_string()))?;
    Ok(report)
}

/// Build one unit's **ThinLTO prelink bitcode** (`--thin-lto`, S1): identical module
/// construction to [`emit_object`] (data layout / ABI / `--rt-lto` merge / size attrs),
/// but instead of `write_object`'s opt+emit, hand the RAW module to the shim, which runs
/// the ThinLTO pre-link pipeline, builds the module summary index, and writes
/// summary-bearing bitcode to `out_bc`. The shim owns optimization from here (the driver
/// must NOT also run `write_object`). `stable_id` is the module's chosen identity (the
/// unit name), keyed consistently through the thin-link and backend phases. `rt_lto` is
/// the baked `--thin-lto` + `--rt-lto` composition artifact (unchanged placement: merged
/// into the raw module before the prelink pipeline), or `None`.
pub fn emit_prelink_bc(
    program: &Program,
    out_bc: &Path,
    target: &BuildTarget,
    profile: Profile,
    exports: &[String],
    rt_lto: Option<&[u8]>,
    stable_id: &str,
) -> Result<(), CodegenError> {
    let ctx = Context::create();
    // `_tm` is unused past construction (the ThinLTO prelink shim takes no TargetMachine) but is kept
    // named so it outlives the shim call alongside `ctx`/`module`.
    let (module, _tm) = build_program_module(&ctx, program, target, profile, exports, rt_lto)?;
    // The module (and its `ctx`) must outlive the shim call; `write_prelink_bc` writes the
    // bitcode synchronously and returns before `module`/`ctx`/`tm` drop here.
    // SAFETY: `module.as_mut_ptr()` is a live LLVMModuleRef in the process LLVM with a
    // datalayout set by `build_module`.
    unsafe {
        thinlto::write_prelink_bc(module.as_mut_ptr(), stable_id, thinlto::ir_opt_level(profile), out_bc)
    }
}

/// Parse the baked `--rt-lto` bitcode for [`emit_object`] / [`emit_llvm_ir`], turning a parse failure
/// into the fail-loud fallback: a diagnostic on stderr naming the cause, then `None` so the caller
/// re-curates the guarded declares and emits the flag-off object. Kept out of `parse_rt_lto_module`
/// (which stays a pure `Result`) so the "diagnose + fall back" policy lives at exactly one seam.
fn probe_rt_lto<'c>(ctx: &'c Context, bitcode: &[u8]) -> Option<Module<'c>> {
    match parse_rt_lto_module(ctx, bitcode) {
        Ok(m) => Some(m),
        Err(e) => {
            eprintln!(
                "alignc: --rt-lto disabled: cannot parse baked runtime bitcode ({e}); \
                 falling back to the runtime staticlib. This is a compiler build defect, not a \
                 problem with your program."
            );
            None
        }
    }
}

/// Render the program as textual LLVM IR (`alignc emit-llvm`).
///
/// `optimized` selects the lens: `false` (`--stage raw`) prints exactly what codegen emitted —
/// pre-optimization, the traditional `emit-llvm` view; `true` (`--stage optimized`) runs the same
/// `-O2` middle-end pipeline `write_object` uses (via [`run_opt_pipeline`]) before printing, so the
/// output is "what LLVM did" — inlined lambdas, fused loops, vectorized `<N x T>` bodies.
///
/// `exports` is the same export-roots list as [`emit_object`] (external linkage instead of
/// `internal`); empty = every program function stays internal.
///
/// Creates exactly one `TargetMachine` for the whole call and reuses it for both `build_module`
/// and (when `optimized`) the opt pipeline.
/// `rt_lto` is the observation lens for the `--rt-lto` gates. `Some(bytes)` links the fast-path
/// string bitcode into the RAW module and internalizes it (the merge is part of what codegen
/// produces pre-opt), regardless of `optimized`, so both lenses are available:
/// - `--stage raw --rt-lto` = post-link / **pre-opt**: the guarded four carry bodies with their
///   `rt_contract` attrs shed, `str_cmp` is still an attributed declare — the attr-xor + link view.
/// - `--stage optimized --rt-lto` = after the one `O2` run: the merged/inlined shape (an
///   `x == "literal"` filter with no `call @align_rt_str_eq`).
///
/// `None` is the default path, byte-identical to before this slice (no link, no probe).
pub fn emit_llvm_ir(program: &Program, target: &BuildTarget, optimized: bool, exports: &[String], rt_lto: Option<&[u8]>) -> Result<String, CodegenError> {
    let ctx = Context::create();
    let module = ctx.create_module("align");
    // Diagnostic lens: codegen opt pinned to `Default` (no size attrs, pipeline `O2` below) so the
    // IR-shape suite stays profile-independent and byte-identical.
    let tm = create_target_machine(target, OptimizationLevel::Default)?;
    // Probe-then-annotate mirrors `emit_object`. Linked into the raw module (before any opt run) so
    // `--stage raw --rt-lto` exposes the pre-opt merged shape for the attr-xor gate.
    let rt_module = rt_lto.and_then(|bc| probe_rt_lto(&ctx, bc));
    build_module(&ctx, &module, program, &tm, None, exports, rt_module.is_some())?;
    if let Some(rt) = rt_module {
        link_in_rt_lto(&ctx, &module, rt)?;
    }
    if optimized {
        run_opt_pipeline(&module, &tm, "default<O2>")?;
    }
    Ok(module.print_to_string().to_string())
}

/// Source identity for opt-in debug-info emission (`explain-opt`): the filename and directory that
/// name the module's single `DIFile`, so LLVM's optimization remarks anchor to
/// `<file>:<line>:<col>` (`docs/impl/09-explain-opt.md`, Slice 3b).
pub struct DebugInfo {
    pub file: String,
    pub directory: String,
}

/// Compile `program` with opt-in debug locations, run the `-O2` pipeline, and return LLVM's raw
/// optimization-remark strings (each `"<file>:<line>:<col>: <message>"`) captured via the
/// diagnostic handler (`docs/impl/09-explain-opt.md`, Slice 3b, Mechanism A). The driver's
/// `explain-opt` translates these into `OptRecord`s.
///
/// **Process-global side effect**: the first call enables `-pass-remarks*` via
/// `LLVMParseCommandLineOptions` (behind a `Once`) — it stays on for the process. Keep this strictly
/// on the `explain-opt` path; a normal build / the IR-shape suite must never call it.
pub fn collect_opt_remarks(
    program: &Program,
    target: &BuildTarget,
    debug: &DebugInfo,
) -> Result<Vec<String>, CodegenError> {
    ensure_remark_cl_opts();
    let ctx = Context::create();
    let module = ctx.create_module("align");
    // Diagnostic lens: codegen opt pinned to `Default` (see `emit_llvm_ir`), so remark output does
    // not vary with the build profile.
    let tm = create_target_machine(target, OptimizationLevel::Default)?;

    // Heap-box the sink so its address is stable while it is registered as the handler userdata.
    let mut sink: Box<Vec<String>> = Box::default();
    let sink_ptr = (&mut *sink as *mut Vec<String>).cast::<std::ffi::c_void>();
    // SAFETY: `diag_handler` only reads the remark severity + description (disposing the C string)
    // and pushes into the `Vec<String>` behind `sink_ptr`. `sink` outlives the handler: the guard
    // below detaches the handler before `sink` is dropped, so no callback can fire after it is freed.
    unsafe {
        llvm_sys::core::LLVMContextSetDiagnosticHandler(ctx.raw(), Some(diag_handler), sink_ptr);
    }
    // RAII guard: detaches the handler on every exit path, including unwind (a panic inside
    // `build_module` / `run_opt_pipeline`), so a panic can never leave the handler dangling while
    // `sink` is freed.
    let _detach_guard = DiagnosticHandlerGuard { ctx: ctx.raw() };

    // `explain-opt` never auto-enables `--rt-lto` (the default lens stays curated-declare shape).
    let built = build_module(&ctx, &module, program, &tm, Some(debug), &[], false);
    let ran = built.and_then(|()| run_opt_pipeline(&module, &tm, "default<O2>"));

    drop(_detach_guard);
    ran?;
    Ok(*sink)
}

/// Detaches the context diagnostic handler on drop. Ensures the handler is cleared before its
/// userdata (the remarks sink) can be freed on any exit path, including unwind.
struct DiagnosticHandlerGuard {
    ctx: llvm_sys::prelude::LLVMContextRef,
}

impl Drop for DiagnosticHandlerGuard {
    fn drop(&mut self) {
        // SAFETY: clears the handler registered on this same context in `collect_opt_remarks`.
        unsafe {
            llvm_sys::core::LLVMContextSetDiagnosticHandler(self.ctx, None, std::ptr::null_mut());
        }
    }
}

/// Enable LLVM's optimization-remark emission once per process. `-pass-remarks*` are `cl::opt`
/// globals that `OptimizationRemarkEmitter` reads; setting them makes the passes emit remarks
/// through the context diagnostic handler. Behind a `Once` because command-line options can only be
/// parsed once and the state is process-global.
fn ensure_remark_cl_opts() {
    use std::sync::Once;
    static ONCE: Once = Once::new();
    ONCE.call_once(|| {
        // These outlive the synchronous `LLVMParseCommandLineOptions` call, which copies the option
        // values into the `cl::opt` globals; freeing them afterward is fine.
        let prog = std::ffi::CString::new("alignc").unwrap();
        let opts = [
            std::ffi::CString::new("-pass-remarks=.*").unwrap(),
            std::ffi::CString::new("-pass-remarks-missed=.*").unwrap(),
            std::ffi::CString::new("-pass-remarks-analysis=.*").unwrap(),
        ];
        let mut argv: Vec<*const std::ffi::c_char> = vec![prog.as_ptr()];
        argv.extend(opts.iter().map(|o| o.as_ptr()));
        let overview = std::ffi::CString::new("").unwrap();
        // SAFETY: `argv` holds `argc` valid, NUL-terminated C strings that live across the call.
        unsafe {
            llvm_sys::support::LLVMParseCommandLineOptions(
                argv.len() as std::ffi::c_int,
                argv.as_ptr(),
                overview.as_ptr(),
            );
        }
    });
}

/// Context diagnostic handler: collect each remark's flat `"<file>:<line>:<col>: <message>"`
/// description into the `Vec<String>` behind `userdata`. Must not panic across the FFI boundary and
/// must dispose the LLVM-owned description string.
extern "C" fn diag_handler(
    di: llvm_sys::prelude::LLVMDiagnosticInfoRef,
    userdata: *mut std::ffi::c_void,
) {
    // SAFETY: called by LLVM during `run_passes` with a valid `di`; `userdata` is the `*mut
    // Vec<String>` registered in `collect_opt_remarks`, valid until the handler is detached.
    unsafe {
        if di.is_null() || userdata.is_null() {
            return;
        }
        // Keep only remarks (severity 2); ignore errors/warnings/notes here.
        if llvm_sys::core::LLVMGetDiagInfoSeverity(di) as i32
            != llvm_sys::LLVMDiagnosticSeverity::LLVMDSRemark as i32
        {
            return;
        }
        let desc = llvm_sys::core::LLVMGetDiagInfoDescription(di);
        if desc.is_null() {
            return;
        }
        let s = std::ffi::CStr::from_ptr(desc).to_string_lossy().into_owned();
        llvm_sys::core::LLVMDisposeMessage(desc);
        let sink = &mut *(userdata.cast::<Vec<String>>());
        sink.push(s);
    }
}

/// Debug-info context for one module (opt-in): the `DebugInfoBuilder`, the shared `DIFile`, and a
/// shared `void()` subroutine type used for every function's `DISubprogram`. We describe only enough
/// to anchor remarks (file + line/col), not parameter/local types — the honest minimum for
/// `explain-opt` (real DWARF type info is out of scope, `docs/impl/09-explain-opt.md`).
struct DebugCtx<'c> {
    dib: DebugInfoBuilder<'c>,
    file: DIFile<'c>,
    subty: inkwell::debug_info::DISubroutineType<'c>,
    scope: inkwell::debug_info::DIScope<'c>,
}

fn build_module<'c>(
    ctx: &'c Context,
    module: &Module<'c>,
    program: &Program,
    tm: &TargetMachine,
    debug: Option<&DebugInfo>,
    exports: &[String],
    rt_lto_skip_guarded: bool,
) -> Result<(), CodegenError> {
    // Target layout (for struct field offsets in `json.decode`); also pin the module's data
    // layout so offsets match the emitted object.
    let target_data = tm.get_target_data();
    module.set_data_layout(&target_data.get_data_layout());
    // Pin the target triple too, so emitted IR (`alignc emit-llvm`) is self-describing: an external
    // `opt`/`llc` reading it then knows the architecture and uses the right cost model / vectorizer
    // instead of falling back to a generic one. The driver's own object emission is unaffected (it
    // always drives `write_object` with the same TargetMachine).
    module.set_triple(&tm.get_triple());

    // Opt-in debug info (explain-opt / a future `-g`). Emitting DILocations anchors LLVM's
    // optimization remarks to real source lines; off by default so normal builds and the IR-shape
    // baseline stay byte-identical. The verifier drops all debug metadata (with a warning) unless
    // the module declares the debug-info version, so stamp it first (inkwell's DI builder does not).
    let debug_ctx: Option<DebugCtx<'c>> = debug.map(|dbg| {
        let ver = ctx.i32_type().const_int(3, false);
        module.add_basic_value_flag(
            "Debug Info Version",
            inkwell::module::FlagBehavior::Warning,
            ver,
        );
        let (dib, cu) = module.create_debug_info_builder(
            true,
            DWARFSourceLanguage::C,
            &dbg.file,
            &dbg.directory,
            "alignc",
            /* is_optimized */ true,
            "",
            0,
            "",
            DWARFEmissionKind::Full,
            0,
            false,
            false,
            "",
            "",
        );
        let file = cu.get_file();
        let subty = dib.create_subroutine_type(file, None, &[], DIFlags::ZERO);
        let scope = cu.as_debug_info_scope();
        DebugCtx { dib, file, subty, scope }
    });

    // Struct layouts → LLVM struct types, indexed by struct id. Two phases so a **nested** struct
    // field (`Struct(id)`) can reference another struct's type: first create every struct as a named
    // opaque type, then set each body (sema forbids non-`box` recursion, so the bodies are acyclic).
    let struct_types: Vec<StructType<'c>> =
        program.structs.iter().map(|s| ctx.opaque_struct_type(&s.name)).collect();
    // Sum-type layouts → a non-union tagged struct `{ i32 tag, <every variant's payload flattened> }`,
    // indexed by enum id. Built **before** the struct bodies (J1b) because a struct field may now be a
    // sum type, so `set_struct_body` needs the enum types to exist. Enum payloads are scalars or
    // (opaque, not-yet-bodied) structs — a literal LLVM struct may reference an opaque type by value;
    // it becomes sized once the struct bodies below are set. (Enum payloads are never another enum —
    // `enum_payload_ok` — so this construction needs no enum type in turn.)
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
    // Field reordering (see `docs/impl/05-backend-llvm.md` §2): a non-`layout(C)` struct's field
    // order is language-unspecified, so codegen lays fields out in **descending alignment** (ties
    // keep declaration order) to eliminate padding. Source access is by name, so this is invisible.
    // `field_perm[sid][logical]` is the logical→physical index map — every field GEP / byte-offset
    // site must route the MIR (logical) field index through it. A `layout(C)` struct keeps
    // declaration order (identity map), so its byte layout — the FFI/`raw`/json boundary — is
    // unchanged.
    let field_perm: Vec<Vec<u32>> =
        program.structs.iter().map(|s| logical_to_physical(s, &program.structs, &program.enums)).collect();
    for ((s, st), perm) in program.structs.iter().zip(&struct_types).zip(&field_perm) {
        set_struct_body(ctx, *st, s, perm, &struct_types, &enum_types, &target_data);
    }

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

    // Pass 1: declare all functions so calls resolve regardless of order. A `Result`- or
    // `Unit`-returning `main` is emitted under `align_main`; a C `main` wrapper is
    // generated after the bodies (see below).
    let mut funcs: HashMap<String, FunctionValue<'c>> = HashMap::new();
    for f in &program.fns {
        let fv = declare_fn(ctx, module, f, symbol_name(f), &struct_types, &enum_types, &tuple_types, exports);
        funcs.insert(f.name.clone(), fv);
    }
    // M15 S2 (per-unit): imported `pub` functions this unit calls but does not define. Each is an
    // external, bodyless `declare` under the same Align ABI a defining unit emits (`declare_imported_fn`
    // mirrors `declare_fn`'s signature computation), keyed by the mangled `module$name` so a
    // `Rvalue::Call` resolves. The linker binds it to the owning unit's exported definition. Empty in
    // the whole-program path (every callee body is in `program.fns`), so this loop is a no-op there.
    for imp in &program.imported_fns {
        // A name collision with a locally-defined function would be a driver bug (a unit must not both
        // define and import the same symbol); prefer the local definition and skip the declare.
        if funcs.contains_key(&imp.name) {
            continue;
        }
        let fv = declare_imported_fn(ctx, module, imp, &struct_types, &enum_types, &tuple_types);
        funcs.insert(imp.name.clone(), fv);
    }
    // A by-value struct in an `extern "C"` signature uses the SysV AMD64 register ABI, which we
    // implement for x86-64 Linux only. On any other target we refuse rather than guess a per-target
    // register rule (that is the one FFI corner a wrong rule *silently miscompiles*) — the user must
    // pass the struct by pointer (`raw`) instead. Scalar/`raw`/view externs are unaffected.
    let triple = tm.get_triple();
    let triple_s = triple.as_str().to_string_lossy().to_ascii_lowercase();
    let x86_64_sysv = triple_s.starts_with("x86_64") && triple_s.contains("linux");
    for ext in &program.externs {
        let uses_byval_struct = matches!(ext.ret, Ty::Struct(_))
            || ext.params.iter().any(|p| matches!(p, Ty::Struct(_)));
        if uses_byval_struct && !x86_64_sysv {
            return Err(CodegenError::Lowering(format!(
                "extern '{}' passes or returns a struct by value, which is only supported on x86-64 SysV (Linux) — the target is '{}'; pass the struct by pointer (`raw`) instead",
                ext.name, triple_s,
            )));
        }
    }

    // The SysV ABI plan for each `extern "C"` symbol: how every parameter and the return value cross
    // the C boundary (a `layout(C)` struct by value flattens to `i64`/`double` register slots; a
    // `str`/`slice` view passes as its data pointer; everything else is direct). Computed once and
    // reused for both the declaration signature (here) and the coerced call site.
    let extern_abi: HashMap<String, ExternAbi> = program
        .externs
        .iter()
        .map(|e| {
            let params = e
                .params
                .iter()
                .map(|&ty| match ty {
                    Ty::Struct(id) => {
                        match classify_struct_abi(id, &struct_types[id as usize], &program.structs[id as usize], &target_data) {
                            Some(abi) => ParamAbi::StructRegs(abi),
                            None => ParamAbi::StructMemory,
                        }
                    }
                    _ if is_ffi_view(ty) => ParamAbi::ViewPtr,
                    _ => ParamAbi::Direct,
                })
                .collect();
            let ret = match e.ret {
                Ty::Struct(id) => {
                    match classify_struct_abi(id, &struct_types[id as usize], &program.structs[id as usize], &target_data) {
                        Some(abi) => ReturnAbi::StructRegs(abi),
                        None => ReturnAbi::StructMemory,
                    }
                }
                _ => ReturnAbi::Direct,
            };
            (e.name.clone(), ExternAbi { params, ret })
        })
        .collect();

    // Declare foreign (`extern "C"`) functions under their C symbol, so a `Rvalue::Call` keyed by
    // that name resolves. FFI-safe params/returns are scalars/`raw`/views/`layout(C)` structs — the
    // signature reflects the SysV coerce plan above (flattened register slots for a by-value struct).
    // No `mark_nounwind`: unlike an Align function, foreign code is outside our control, so we do not
    // assert it never unwinds.
    for ext in &program.externs {
        let abi = &extern_abi[&ext.name];
        // Reject any signature where a by-value struct argument would fall to the MEMORY-class
        // `byval` ABI because preceding arguments exhaust the class registers (the SysV all-or-
        // nothing rule) — we cannot reproduce `byval` by flattening. See `check_sysv_struct_args_fit`.
        check_sysv_struct_args_fit(&ext.name, abi, &ext.params, &program.structs)?;
        let mut param_types: Vec<BasicMetadataTypeEnum> = Vec::with_capacity(ext.params.len());
        for (pa, &ty) in abi.params.iter().zip(&ext.params) {
            match pa {
                ParamAbi::Direct => param_types.push(abi_type(ctx, ty, &struct_types, &enum_types).into()),
                ParamAbi::ViewPtr => param_types.push(ctx.ptr_type(AddressSpace::default()).into()),
                // A by-value struct flattens to one `i64`/`double` per eightbyte — byte-identical to
                // clang's own flattened parameter form. This is sound only because
                // `check_sysv_struct_args_fit` has already rejected the register-exhaustion boundary
                // where clang would switch to a `byval` pointer (which flattening cannot mimic).
                ParamAbi::StructRegs(sabi) => {
                    for &eb in &sabi.ebs {
                        param_types.push(eb.llvm(ctx).into());
                    }
                }
                ParamAbi::StructMemory => {
                    let sname = match ty {
                        Ty::Struct(id) => program.structs[id as usize].name.as_str(),
                        _ => "?",
                    };
                    return Err(CodegenError::Lowering(format!(
                        "extern '{}': passing struct '{sname}' by value needs the > 16-byte MEMORY-class ABI (a `byval` pointer), which is not supported in FFI v1 — pass it by pointer (`raw`) instead",
                        ext.name,
                    )));
                }
            }
        }
        let fn_ty = match &abi.ret {
            ReturnAbi::Direct => {
                if ext.ret == Ty::Unit {
                    ctx.void_type().fn_type(&param_types, false)
                } else {
                    abi_type(ctx, ext.ret, &struct_types, &enum_types).fn_type(&param_types, false)
                }
            }
            ReturnAbi::StructRegs(sabi) => struct_ret_type(ctx, sabi).fn_type(&param_types, false),
            ReturnAbi::StructMemory => {
                let sname = match ext.ret {
                    Ty::Struct(id) => program.structs[id as usize].name.as_str(),
                    _ => "?",
                };
                return Err(CodegenError::Lowering(format!(
                    "extern '{}': returning struct '{sname}' by value needs the > 16-byte MEMORY-class ABI (an `sret` pointer), which is not supported in FFI v1 — return it through an out-pointer (`raw`) parameter instead",
                    ext.name,
                )));
            }
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
    // `map_into` destination/source length mismatch: report `(dst_len, src_len)` and abort (`-> !`).
    funcs.insert(
        "len_mismatch_fail".to_string(),
        module.add_function(
            "align_rt_len_mismatch_fail",
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
    // A `str` range endpoint that splits a UTF-8 scalar: report `(index, len)` and abort (`-> !`).
    funcs.insert(
        "utf8_boundary_fail".to_string(),
        module.add_function(
            "align_rt_utf8_boundary_fail",
            ctx.void_type().fn_type(&[ctx.i64_type().into(), ctx.i64_type().into()], false),
            None,
        ),
    );
    // Integer division/remainder by zero: report and abort (`-> !`). Codegen emits the
    // `divisor == 0` guard inline (see MIR `lower_int_div`) and calls this on the failing path.
    funcs.insert(
        "div_fail".to_string(),
        module.add_function("align_rt_div_fail", ctx.void_type().fn_type(&[], false), None),
    );
    funcs.insert(
        "alloc_size_fail".to_string(),
        module.add_function("align_rt_alloc_size_fail", ctx.void_type().fn_type(&[], false), None),
    );
    // `std.process` (M11) — `process.exit(code)` (cleanup runs first, in MIR) and `process.abort()`.
    // Both are diverging (`-> !`); MIR emits `Unreachable` after the call (like `bounds_fail`), so no
    // `noreturn` attribute is required for correctness.
    funcs.insert(
        "process_exit".to_string(),
        module.add_function("align_rt_process_exit", ctx.void_type().fn_type(&[ctx.i64_type().into()], false), None),
    );
    funcs.insert(
        "process_abort".to_string(),
        module.add_function("align_rt_process_abort", ctx.void_type().fn_type(&[], false), None),
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
    // `core.string` byte predicates + the `Ord(str)` comparator — all share the
    // `i32 (ptr, i64, ptr, i64)` signature of `str_eq` (`str_cmp` returns -1/0/1 instead of 0/1).
    for (key, sym) in [
        ("str_contains", "align_rt_str_contains"),
        ("str_starts_with", "align_rt_str_starts_with"),
        ("str_ends_with", "align_rt_str_ends_with"),
        ("str_cmp", "align_rt_str_cmp"),
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
    // doc-13 §6.6 / §11 P3 — repeated-needle plan hoisting. `str_finder_new(nptr, nlen) -> plan` is
    // allocator-class (like `builder_new`/`array_builder_new`: `noalias`/`nounwind`/`nofree`, NOT
    // `willreturn` — a `Box` allocation aborts on OOM). `str_finder_find(plan, hptr, hlen) -> i64`
    // gets `memory(argmem: read)` + `readonly captures(none)` on both pointers via `rt_contract`
    // (verified: no CPU-feature detect at find time). `str_finder_free(plan)` is a plain null-safe
    // deallocator declare (mirrors `builder_free`/`array_builder_free` — free fns take no attrs).
    let str_finder_new = module.add_function(
        "align_rt_str_finder_new",
        ctx.ptr_type(AddressSpace::default()).fn_type(&[ptr.into(), ctx.i64_type().into()], false),
        None,
    );
    mark_alloc_like(ctx, str_finder_new);
    funcs.insert("str_finder_new".to_string(), str_finder_new);
    funcs.insert(
        "str_finder_find".to_string(),
        module.add_function(
            "align_rt_str_finder_find",
            ctx.i64_type().fn_type(&[ptr.into(), ptr.into(), ctx.i64_type().into()], false),
            None,
        ),
    );
    funcs.insert(
        "str_finder_free".to_string(),
        module.add_function("align_rt_str_finder_free", ctx.void_type().fn_type(&[ptr.into()], false), None),
    );
    // String builder (M5: `template` desugaring).
    let i64t2 = ctx.i64_type();
    let builder_new =
        module.add_function("align_rt_builder_new", ptr.fn_type(&[ptr.into(), i64t2.into()], false), None);
    mark_alloc_like(ctx, builder_new);
    funcs.insert("builder_new".to_string(), builder_new);
    funcs.insert(
        "builder_init_stack".to_string(),
        module.add_function(
            "align_rt_builder_init_stack",
            ptr.fn_type(&[ptr.into(), ptr.into(), i64t2.into()], false),
            None,
        ),
    );
    funcs.insert(
        "builder_write".to_string(),
        module.add_function(
            "align_rt_builder_write",
            ctx.void_type().fn_type(&[ptr.into(), ptr.into(), i64t2.into()], false),
            None,
        ),
    );
    funcs.insert(
        "builder_pop_comma".to_string(),
        module.add_function("align_rt_builder_pop_comma", ctx.void_type().fn_type(&[ptr.into()], false), None),
    );
    funcs.insert(
        "json_encode_struct_array".to_string(),
        module.add_function(
            "align_rt_json_encode_struct_array",
            // (builder, ptr, len, descs, n_descs, esz) -> void
            ctx.void_type().fn_type(&[ptr.into(), ptr.into(), i64t2.into(), ptr.into(), i64t2.into(), i64t2.into()], false),
            None,
        ),
    );
    funcs.insert(
        "json_encode_scalar_array".to_string(),
        module.add_function(
            "align_rt_json_encode_scalar_array",
            // (builder, ptr, len, elem_tag: i32) -> void
            ctx.void_type().fn_type(&[ptr.into(), ptr.into(), i64t2.into(), ctx.i32_type().into()], false),
            None,
        ),
    );
    funcs.insert(
        "json_encode_object".to_string(),
        module.add_function(
            "align_rt_json_encode_object",
            // (builder, base, descs, n_descs) -> void
            ctx.void_type().fn_type(&[ptr.into(), ptr.into(), ptr.into(), i64t2.into()], false),
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
        // json.decode into a shape-directed union (input, input_len, union_desc, out) -> i32 status
        // (JSON completeness J1b): parse one JSON value, select the variant by shape class, write the
        // payload + tag into `out`.
        "json_decode_union".to_string(),
        module.add_function(
            "align_rt_json_decode_union",
            ctx.i32_type().fn_type(&[ptr.into(), i64t2.into(), ptr.into(), ptr.into()], false),
            None,
        ),
    );
    funcs.insert(
        // json.encode of a union value (builder, base, union_desc) -> void (JSON completeness J1b):
        // write the live variant's payload bare into the builder.
        "json_encode_union".to_string(),
        module.add_function(
            "align_rt_json_encode_union",
            ctx.void_type().fn_type(&[ptr.into(), ptr.into(), ptr.into()], false),
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
        // json.decode into a bare scalar (input, input_len, elem_tag, out: *scalar) -> i32 status (T1b).
        "json_decode_scalar".to_string(),
        module.add_function(
            "align_rt_json_decode_scalar",
            ctx.i32_type().fn_type(&[ptr.into(), i64t2.into(), ctx.i32_type().into(), ptr.into()], false),
            None,
        ),
    );
    // ── json.doc (J4) ──────────────────────────────────────────────────────────────────────────
    funcs.insert(
        // json.doc(input, input_len, arena, out: *{tape,node}) -> i32 status (0 = ok). Parse into an
        // arena-backed tape; on malformed input returns 1 (out stays zeroed → Err(Error.Invalid)).
        "json_doc_parse".to_string(),
        module.add_function(
            "align_rt_json_doc_parse",
            ctx.i32_type().fn_type(&[ptr.into(), i64t2.into(), ptr.into(), ptr.into()], false),
            None,
        ),
    );
    funcs.insert(
        // d.kind()(tape, node) -> i32 the json.kind tag (6 = Missing). Total.
        "json_doc_kind".to_string(),
        module.add_function("align_rt_json_doc_kind", ctx.i32_type().fn_type(&[ptr.into(), i64t2.into()], false), None),
    );
    funcs.insert(
        // d.get(tape, node, key, key_len, out: *{tape,node}) -> void. Writes the child handle (Missing if absent).
        "json_doc_get".to_string(),
        module.add_function(
            "align_rt_json_doc_get",
            ctx.void_type().fn_type(&[ptr.into(), i64t2.into(), ptr.into(), i64t2.into(), ptr.into()], false),
            None,
        ),
    );
    funcs.insert(
        // d.at(tape, node, index, out: *{tape,node}) -> void. Writes the element handle (Missing if OOB).
        "json_doc_at".to_string(),
        module.add_function(
            "align_rt_json_doc_at",
            ctx.void_type().fn_type(&[ptr.into(), i64t2.into(), i64t2.into(), ptr.into()], false),
            None,
        ),
    );
    funcs.insert(
        // d.len(tape, node) -> i64 the member/element count (0 on a non-container / Missing).
        "json_doc_len".to_string(),
        module.add_function("align_rt_json_doc_len", i64t2.fn_type(&[ptr.into(), i64t2.into()], false), None),
    );
    funcs.insert(
        // d.key(tape, node, index, out: *{ptr,len}) -> i32 present flag. Writes the index-th object key view.
        "json_doc_key".to_string(),
        module.add_function(
            "align_rt_json_doc_key",
            ctx.i32_type().fn_type(&[ptr.into(), i64t2.into(), i64t2.into(), ptr.into()], false),
            None,
        ),
    );
    funcs.insert(
        // d.elems(tape, node, arena, out: *{ptr,len}) -> void. Materializes the level's handle buffer.
        "json_doc_elems".to_string(),
        module.add_function(
            "align_rt_json_doc_elems",
            ctx.void_type().fn_type(&[ptr.into(), i64t2.into(), ptr.into(), ptr.into()], false),
            None,
        ),
    );
    // The three leaf accessors share the (tape, node, out) -> i32 present-flag shape; the out slot's
    // type (str view / i64 / f64 / u8) differs but is an opaque `ptr` at the ABI.
    for (name, sym) in [
        ("json_doc_as_str", "align_rt_json_doc_as_str"),
        ("json_doc_as_i64", "align_rt_json_doc_as_i64"),
        ("json_doc_as_f64", "align_rt_json_doc_as_f64"),
        ("json_doc_as_bool", "align_rt_json_doc_as_bool"),
    ] {
        funcs.insert(
            name.to_string(),
            module.add_function(sym, ctx.i32_type().fn_type(&[ptr.into(), i64t2.into(), ptr.into()], false), None),
        );
    }
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
        // fs.write_file (path_ptr, path_len, data_ptr, data_len) -> i32 errno-status.
        "fs_write_file".to_string(),
        module.add_function(
            "align_rt_fs_write_file",
            ctx.i32_type().fn_type(&[ptr.into(), i64t2.into(), ptr.into(), i64t2.into()], false),
            None,
        ),
    );
    funcs.insert(
        // fs.write_file (path_ptr, path_len, b: *Builder) -> i32 errno-status.
        "fs_write_file_builder".to_string(),
        module.add_function(
            "align_rt_fs_write_file_builder",
            ctx.i32_type().fn_type(&[ptr.into(), i64t2.into(), ptr.into()], false),
            None,
        ),
    );
    funcs.insert(
        // fs.exists (path_ptr, path_len) -> i32 (1/0; every error folds to 0).
        "fs_exists".to_string(),
        module.add_function("align_rt_fs_exists", ctx.i32_type().fn_type(&[ptr.into(), i64t2.into()], false), None),
    );
    funcs.insert(
        // fs.remove (path_ptr, path_len) -> i32 errno-status.
        "fs_remove".to_string(),
        module.add_function("align_rt_fs_remove", ctx.i32_type().fn_type(&[ptr.into(), i64t2.into()], false), None),
    );
    funcs.insert(
        // fs.read_dir (path_ptr, path_len, out: *{ptr,len}) -> i32 errno-status (owned array<string>).
        "fs_read_dir".to_string(),
        module.add_function("align_rt_fs_read_dir", ctx.i32_type().fn_type(&[ptr.into(), i64t2.into(), ptr.into()], false), None),
    );
    funcs.insert(
        // dns.resolve (host_ptr, host_len, out: *{ptr,len}) -> i32 status (owned array<string>).
        "dns_resolve".to_string(),
        module.add_function("align_rt_dns_resolve", ctx.i32_type().fn_type(&[ptr.into(), i64t2.into(), ptr.into()], false), None),
    );
    funcs.insert(
        // tcp.connect (host_ptr, host_len, port: i64, out: **TcpConn) -> i32 status (owned conn).
        "tcp_connect".to_string(),
        module.add_function("align_rt_tcp_connect", ctx.i32_type().fn_type(&[ptr.into(), i64t2.into(), i64t2.into(), ptr.into()], false), None),
    );
    funcs.insert(
        // drop(c) (c: *TcpConn) -> void; close its fd.
        "tcp_conn_free".to_string(),
        module.add_function("align_rt_tcp_conn_free", ctx.void_type().fn_type(&[ptr.into()], false), None),
    );
    funcs.insert(
        // c.reader() (c: *TcpConn) -> *Reader (borrowed over the conn's fd, owns_fd:false).
        "tcp_conn_reader".to_string(),
        module.add_function("align_rt_tcp_conn_reader", ptr.fn_type(&[ptr.into()], false), None),
    );
    funcs.insert(
        // c.writer() (c: *TcpConn) -> *Writer (borrowed over the conn's fd, owns_fd:false).
        "tcp_conn_writer".to_string(),
        module.add_function("align_rt_tcp_conn_writer", ptr.fn_type(&[ptr.into()], false), None),
    );
    funcs.insert(
        // tcp.listen (host_ptr, host_len, port: i64, out: **TcpListener) -> i32 status (owned listener).
        "tcp_listen".to_string(),
        module.add_function("align_rt_tcp_listen", ctx.i32_type().fn_type(&[ptr.into(), i64t2.into(), i64t2.into(), ptr.into()], false), None),
    );
    funcs.insert(
        // drop(l) (l: *TcpListener) -> void; close its fd.
        "tcp_listener_free".to_string(),
        module.add_function("align_rt_tcp_listener_free", ctx.void_type().fn_type(&[ptr.into()], false), None),
    );
    funcs.insert(
        // l.accept() (l: *TcpListener, out: **TcpConn) -> i32 status (owned accepted conn).
        "tcp_accept".to_string(),
        module.add_function("align_rt_tcp_accept", ctx.i32_type().fn_type(&[ptr.into(), ptr.into()], false), None),
    );
    funcs.insert(
        // udp.bind (host_ptr, host_len: i64, port: i64, out: **UdpSocket) -> i32 status (owned socket).
        "udp_bind".to_string(),
        module.add_function("align_rt_udp_bind", ctx.i32_type().fn_type(&[ptr.into(), i64t2.into(), i64t2.into(), ptr.into()], false), None),
    );
    funcs.insert(
        // drop(u) (u: *UdpSocket) -> void; close its fd.
        "udp_socket_free".to_string(),
        module.add_function("align_rt_udp_socket_free", ctx.void_type().fn_type(&[ptr.into()], false), None),
    );
    funcs.insert(
        // u.send_to (u: *UdpSocket, data_ptr, data_len: i64, host_ptr, host_len: i64, port: i64)
        // -> i64 (bytes sent, or -(status) on error).
        "udp_send_to".to_string(),
        module.add_function("align_rt_udp_send_to", i64t2.fn_type(&[ptr.into(), ptr.into(), i64t2.into(), ptr.into(), i64t2.into(), i64t2.into()], false), None),
    );
    funcs.insert(
        // u.recv_from (u: *UdpSocket, buf: *Buffer) -> i64 (bytes received, or -(status) on error).
        "udp_recv_from".to_string(),
        module.add_function("align_rt_udp_recv_from", i64t2.fn_type(&[ptr.into(), ptr.into()], false), None),
    );
    funcs.insert(
        // process.spawn (cmd_ptr, cmd_len: i64, args_ptr: *AlignStr, args_len: i64, out: **Child)
        // -> i32 status (owned child; 0 = ok). fork+execvp; a failed exec `_exit(127)`s in the child.
        "process_spawn".to_string(),
        module.add_function("align_rt_process_spawn", ctx.i32_type().fn_type(&[ptr.into(), i64t2.into(), ptr.into(), i64t2.into(), ptr.into()], false), None),
    );
    funcs.insert(
        // ch.wait (ch: *Child) -> i64 (exit code >= 0: WEXITSTATUS / 128+sig, or -(status) on error).
        "child_wait".to_string(),
        module.add_function("align_rt_child_wait", i64t2.fn_type(&[ptr.into()], false), None),
    );
    funcs.insert(
        // drop(ch) (ch: *Child) -> void; reap via blocking waitpid if not yet waited (no zombie).
        "child_free".to_string(),
        module.add_function("align_rt_child_free", ctx.void_type().fn_type(&[ptr.into()], false), None),
    );
    funcs.insert(
        // ch.kill (ch: *Child, sig: i64) -> i32 status (0 = ok; AL_INVALID for a bad sig / reaped child,
        // else the mapped errno — EPERM/ESRCH). libc kill(pid, sig); sig 0 = liveness probe.
        "child_kill".to_string(),
        module.add_function("align_rt_child_kill", ctx.i32_type().fn_type(&[ptr.into(), i64t2.into()], false), None),
    );
    funcs.insert(
        // process.exec (cmd_ptr, cmd_len: i64, args_ptr: *AlignStr, args_len: i64) -> i32 status.
        // execvp in the CURRENT process: on success it replaces the image and never returns, so this
        // returns only on failure (the mapped errno; AL_INVALID for a bad cmd/argv).
        "process_exec".to_string(),
        module.add_function("align_rt_process_exec", ctx.i32_type().fn_type(&[ptr.into(), i64t2.into(), ptr.into(), i64t2.into()], false), None),
    );
    funcs.insert(
        // fs.read_file_view (path_ptr, path_len, arena: *Arena, out: *{ptr,len}) -> i32 errno-status.
        "fs_read_file_view".to_string(),
        module.add_function("align_rt_fs_read_file_view", ctx.i32_type().fn_type(&[ptr.into(), i64t2.into(), ptr.into(), ptr.into()], false), None),
    );
    funcs.insert(
        // fs.read_bytes_view (path_ptr, path_len, arena: *Arena, out: *{ptr,len}) -> i32 errno-status.
        "fs_read_bytes_view".to_string(),
        module.add_function("align_rt_fs_read_bytes_view", ctx.i32_type().fn_type(&[ptr.into(), i64t2.into(), ptr.into(), ptr.into()], false), None),
    );
    funcs.insert(
        // drop(array<string>) (ptr, len) -> void; deep free (each element's buffer, then the header).
        "free_string_array".to_string(),
        module.add_function("align_rt_free_string_array", ctx.void_type().fn_type(&[ptr.into(), i64t2.into()], false), None),
    );
    // std.io / std.fs — reader / writer (own an fd) + buffer (owned bytes).
    funcs.insert(
        // fs.open (path_ptr, path_len, out: **Reader) -> i32 errno-status.
        "io_reader_open".to_string(),
        module.add_function("align_rt_io_reader_open", ctx.i32_type().fn_type(&[ptr.into(), i64t2.into(), ptr.into()], false), None),
    );
    funcs.insert(
        // io.stdin () -> *Reader (opaque handle).
        "io_reader_stdin".to_string(),
        module.add_function("align_rt_io_reader_stdin", ptr.fn_type(&[], false), None),
    );
    funcs.insert(
        // r.read(b) (r: *Reader, b: *Buffer) -> i64 (count, or -(status)).
        "io_reader_read".to_string(),
        module.add_function("align_rt_io_reader_read", i64t2.fn_type(&[ptr.into(), ptr.into()], false), None),
    );
    funcs.insert(
        // r.buffered() (r: *Reader) -> *Reader (same handle, now buffered).
        "io_reader_buffered".to_string(),
        module.add_function("align_rt_io_reader_buffered", ptr.fn_type(&[ptr.into()], false), None),
    );
    funcs.insert(
        // r.read_line(b) (r: *Reader, b: *Buffer) -> i64 (consumed incl. terminator, 0 = EOF, or -(status)).
        "io_reader_read_line".to_string(),
        module.add_function("align_rt_io_reader_read_line", i64t2.fn_type(&[ptr.into(), ptr.into()], false), None),
    );
    funcs.insert(
        // bytes.as_str() (ptr, len, out: *{ptr,len}) -> i32 errno-status (AL_INVALID on bad UTF-8).
        "bytes_as_str".to_string(),
        module.add_function("align_rt_bytes_as_str", ctx.i32_type().fn_type(&[ptr.into(), i64t2.into(), ptr.into()], false), None),
    );
    funcs.insert(
        // drop(r) (r: *Reader) -> void; close if owned.
        "io_reader_free".to_string(),
        module.add_function("align_rt_io_reader_free", ctx.void_type().fn_type(&[ptr.into()], false), None),
    );
    funcs.insert(
        // fs.create (path_ptr, path_len, out: **Writer) -> i32 errno-status.
        "io_writer_create".to_string(),
        module.add_function("align_rt_io_writer_create", ctx.i32_type().fn_type(&[ptr.into(), i64t2.into(), ptr.into()], false), None),
    );
    funcs.insert(
        // io.stdout / io.stderr / .buffered() (fd: i32, buffered: i32) -> *Writer (opaque handle).
        "io_writer_std".to_string(),
        module.add_function("align_rt_io_writer_std", ptr.fn_type(&[ctx.i32_type().into(), ctx.i32_type().into()], false), None),
    );
    funcs.insert(
        // w.write(s) (w: *Writer, ptr, len) -> i32 errno-status.
        "io_writer_write".to_string(),
        module.add_function("align_rt_io_writer_write", ctx.i32_type().fn_type(&[ptr.into(), ptr.into(), i64t2.into()], false), None),
    );
    funcs.insert(
        // w.write(builder) (w: *Writer, b: *Builder) -> i32 errno-status.
        "io_writer_write_builder".to_string(),
        module.add_function("align_rt_io_writer_write_builder", ctx.i32_type().fn_type(&[ptr.into(), ptr.into()], false), None),
    );
    funcs.insert(
        // w.flush() (w: *Writer) -> i32 errno-status.
        "io_writer_flush".to_string(),
        module.add_function("align_rt_io_writer_flush", ctx.i32_type().fn_type(&[ptr.into()], false), None),
    );
    funcs.insert(
        // drop(w) (w: *Writer) -> void; final flush + close if owned.
        "io_writer_free".to_string(),
        module.add_function("align_rt_io_writer_free", ctx.void_type().fn_type(&[ptr.into()], false), None),
    );
    funcs.insert(
        // io.copy(r, w) (r: *Reader, w: *Writer) -> i64 (bytes transferred, or -(status)).
        "io_copy".to_string(),
        module.add_function("align_rt_io_copy", i64t2.fn_type(&[ptr.into(), ptr.into()], false), None),
    );
    funcs.insert(
        // fs.create_rw (path_ptr, path_len, out: **RwFile) -> i32 errno-status.
        "io_file_create".to_string(),
        module.add_function("align_rt_io_file_create", ctx.i32_type().fn_type(&[ptr.into(), i64t2.into(), ptr.into()], false), None),
    );
    funcs.insert(
        // fs.open_rw (path_ptr, path_len, out: **RwFile) -> i32 errno-status.
        "io_file_open".to_string(),
        module.add_function("align_rt_io_file_open", ctx.i32_type().fn_type(&[ptr.into(), i64t2.into(), ptr.into()], false), None),
    );
    funcs.insert(
        // f.pread(b, off) (f: *RwFile, b: *Buffer, off: i64) -> i64 (count, or -(status)).
        "io_file_pread".to_string(),
        module.add_function("align_rt_io_file_pread", i64t2.fn_type(&[ptr.into(), ptr.into(), i64t2.into()], false), None),
    );
    funcs.insert(
        // f.pwrite(data, off) (f: *RwFile, ptr, len, off: i64) -> i64 (full count, or -(status)).
        "io_file_pwrite".to_string(),
        module.add_function("align_rt_io_file_pwrite", i64t2.fn_type(&[ptr.into(), ptr.into(), i64t2.into(), i64t2.into()], false), None),
    );
    funcs.insert(
        // f.len() (f: *RwFile) -> i64 (length, or -(status)).
        "io_file_len".to_string(),
        module.add_function("align_rt_io_file_len", i64t2.fn_type(&[ptr.into()], false), None),
    );
    funcs.insert(
        // drop(f) (f: *RwFile) -> void; close the fd.
        "io_file_free".to_string(),
        module.add_function("align_rt_io_file_free", ctx.void_type().fn_type(&[ptr.into()], false), None),
    );
    funcs.insert(
        // buffer(cap) (cap: i64) -> *Buffer (opaque handle).
        "buffer_new".to_string(),
        module.add_function("align_rt_buffer_new", ptr.fn_type(&[i64t2.into()], false), None),
    );
    funcs.insert(
        // b.bytes() (b: *Buffer, out: *{ptr,len}) -> void; a slice<u8> view.
        "buffer_bytes".to_string(),
        module.add_function("align_rt_buffer_bytes", ctx.void_type().fn_type(&[ptr.into(), ptr.into()], false), None),
    );
    funcs.insert(
        // b.len() (b: *Buffer) -> i64.
        "buffer_len".to_string(),
        module.add_function("align_rt_buffer_len", i64t2.fn_type(&[ptr.into()], false), None),
    );
    funcs.insert(
        // drop(b) (b: *Buffer) -> void; free.
        "buffer_free".to_string(),
        module.add_function("align_rt_buffer_free", ctx.void_type().fn_type(&[ptr.into()], false), None),
    );
    funcs.insert(
        // b.put_*(v) (b: *Buffer, bits: i64, width: i64, be: i32) -> void; append `width` bytes.
        "buffer_put".to_string(),
        module.add_function(
            "align_rt_buffer_put",
            ctx.void_type().fn_type(&[ptr.into(), i64t2.into(), i64t2.into(), ctx.i32_type().into()], false),
            None,
        ),
    );
    funcs.insert(
        // b.append(data) (b: *Buffer, ptr: *u8, len: i64) -> void; copy-append a byte blob.
        "buffer_append".to_string(),
        module.add_function("align_rt_buffer_append", ctx.void_type().fn_type(&[ptr.into(), ptr.into(), i64t2.into()], false), None),
    );
    // array_builder<T> (M12 A6): new/push/push_str/append/build + the two Drop frees.
    // array_builder(elem_size: i64) -> *ArrayBuilder (opaque handle).
    let array_builder_new =
        module.add_function("align_rt_array_builder_new", ptr.fn_type(&[i64t2.into()], false), None);
    mark_alloc_like(ctx, array_builder_new);
    funcs.insert("array_builder_new".to_string(), array_builder_new);
    funcs.insert(
        "array_builder_init_stack".to_string(),
        module.add_function(
            "align_rt_array_builder_init_stack",
            ptr.fn_type(&[ptr.into(), i64t2.into()], false),
            None,
        ),
    );
    funcs.insert(
        // b.push(v) (b: *ArrayBuilder, bits: i64) -> void; append one scalar element.
        "array_builder_push".to_string(),
        module.add_function("align_rt_array_builder_push", ctx.void_type().fn_type(&[ptr.into(), i64t2.into()], false), None),
    );
    funcs.insert(
        // b.push(s) (b: *ArrayBuilder, ptr: *u8, len: i64) -> void; append one moved-in string.
        "array_builder_push_str".to_string(),
        module.add_function("align_rt_array_builder_push_str", ctx.void_type().fn_type(&[ptr.into(), ptr.into(), i64t2.into()], false), None),
    );
    funcs.insert(
        // b.append(xs) (b: *ArrayBuilder, src: *u8, count: i64) -> void; bulk-append scalar elements.
        "array_builder_append".to_string(),
        module.add_function("align_rt_array_builder_append", ctx.void_type().fn_type(&[ptr.into(), ptr.into(), i64t2.into()], false), None),
    );
    funcs.insert(
        // b.build() (b: *ArrayBuilder) -> {ptr,len}; freeze into an owned array<T> (zero-copy).
        "array_builder_build".to_string(),
        module.add_function("align_rt_array_builder_build", slice_struct_type(ctx).fn_type(&[ptr.into()], false), None),
    );
    funcs.insert(
        "array_builder_build_stack".to_string(),
        module.add_function(
            "align_rt_array_builder_build_stack",
            slice_struct_type(ctx).fn_type(&[ptr.into()], false),
            None,
        ),
    );
    funcs.insert(
        // drop(b) scalar element (b: *ArrayBuilder) -> void; free storage + header.
        "array_builder_free".to_string(),
        module.add_function("align_rt_array_builder_free", ctx.void_type().fn_type(&[ptr.into()], false), None),
    );
    funcs.insert(
        "array_builder_free_stack".to_string(),
        module.add_function(
            "align_rt_array_builder_free_stack",
            ctx.void_type().fn_type(&[ptr.into()], false),
            None,
        ),
    );
    funcs.insert(
        // drop(b) string element (b: *ArrayBuilder) -> void; deep-free each string, then storage.
        "array_builder_free_strings".to_string(),
        module.add_function("align_rt_array_builder_free_strings", ctx.void_type().fn_type(&[ptr.into()], false), None),
    );
    funcs.insert(
        "array_builder_free_strings_stack".to_string(),
        module.add_function(
            "align_rt_array_builder_free_strings_stack",
            ctx.void_type().fn_type(&[ptr.into()], false),
            None,
        ),
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
        // json.scan one row (J5): (input, input_len, cursor: *i64, fields, n, out_row: *u8, out_size,
        // phf, phf_len, phf_seed) -> i32 status (0 = row / 1 = done / 2 = malformed). Reuses the
        // struct decode descriptor; `cursor` is the mutable byte offset into the input.
        "json_scan_next".to_string(),
        module.add_function(
            "align_rt_json_scan_next",
            ctx.i32_type().fn_type(
                &[ptr.into(), i64t2.into(), ptr.into(), ptr.into(), i64t2.into(), ptr.into(), i64t2.into(), ptr.into(), i64t2.into(), i64t2.into()],
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
    funcs.insert(
        "builder_finish_stack".to_string(),
        module.add_function(
            "align_rt_builder_finish_stack",
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
    // `std.encoding` — encode (byte view `{ptr,len}`) -> owned `string` `{ptr,len}`.
    for (key, sym) in [
        ("base64_encode", "align_rt_base64_encode"),
        ("base64url_encode", "align_rt_base64url_encode"),
        ("hex_encode", "align_rt_hex_encode"),
        ("percent_encode", "align_rt_percent_encode"),
        ("form_encode", "align_rt_form_encode"),
    ] {
        funcs.insert(
            key.to_string(),
            module.add_function(sym, slice_struct_type(ctx).fn_type(&[ptr.into(), i64t2.into()], false), None),
        );
    }
    // `std.encoding` — decode (`str` view `{ptr,len}`, out: *handle) -> i32 status (0 ok / AL_INVALID).
    for (key, sym) in [
        ("base64_decode", "align_rt_base64_decode"),
        ("base64url_decode", "align_rt_base64url_decode"),
        ("hex_decode", "align_rt_hex_decode"),
        ("percent_decode", "align_rt_percent_decode"),
        ("form_decode", "align_rt_form_decode"),
    ] {
        funcs.insert(
            key.to_string(),
            module.add_function(sym, ctx.i32_type().fn_type(&[ptr.into(), i64t2.into(), ptr.into()], false), None),
        );
    }
    funcs.insert(
        // encoding.utf8_valid (ptr, len) -> i32 (1 valid / 0 invalid).
        "utf8_valid".to_string(),
        module.add_function("align_rt_utf8_valid", ctx.i32_type().fn_type(&[ptr.into(), i64t2.into()], false), None),
    );
    // `std.crypto` (M11 Slice 1). constant_time_equal (a_ptr, a_len, b_ptr, b_len) -> i32 (1 equal /
    // 0 not; length is public, the equal-length compare is branchless). random (buf: *Buffer) -> void
    // (fills the buffer's full capacity from the OS CSPRNG; aborts on failure).
    funcs.insert(
        "crypto_ct_equal".to_string(),
        module.add_function(
            "align_rt_crypto_ct_equal",
            ctx.i32_type().fn_type(&[ptr.into(), i64t2.into(), ptr.into(), i64t2.into()], false),
            None,
        ),
    );
    funcs.insert(
        "crypto_random".to_string(),
        module.add_function("align_rt_crypto_random", ctx.void_type().fn_type(&[ptr.into()], false), None),
    );
    // `std.crypto` (M11 Slice 2). sha256 / sha512 (data view `{ptr,len}`) -> a fresh owned `array<u8>`
    // `{ptr,len}` (32 / 64 bytes; the digest, returned by value like `rng_sample`; the bound local
    // `Drop`-frees it). Both wrap libcrypto's `EVP_Q_digest`; an engine failure aborts in the runtime.
    funcs.insert(
        "crypto_sha256".to_string(),
        module.add_function("align_rt_crypto_sha256", slice_struct_type(ctx).fn_type(&[ptr.into(), i64t2.into()], false), None),
    );
    funcs.insert(
        "crypto_sha512".to_string(),
        module.add_function("align_rt_crypto_sha512", slice_struct_type(ctx).fn_type(&[ptr.into(), i64t2.into()], false), None),
    );
    // `std.crypto` (M11 Slice 3). hmac_sha256 (key view + data view) -> a fresh owned `array<u8>`
    // `{ptr,len}` (32-byte tag, returned by value like the digests). hkdf_sha256 (salt/ikm/info views
    // + i64 len, out: *handle) returns an i32 status, writing an owned `buffer` handle into `out`
    // (the `std.compress` status shape).
    funcs.insert(
        "crypto_hmac_sha256".to_string(),
        module.add_function(
            "align_rt_crypto_hmac_sha256",
            slice_struct_type(ctx).fn_type(&[ptr.into(), i64t2.into(), ptr.into(), i64t2.into()], false),
            None,
        ),
    );
    funcs.insert(
        "crypto_hkdf_sha256".to_string(),
        module.add_function(
            "align_rt_crypto_hkdf_sha256",
            ctx.i32_type().fn_type(
                &[ptr.into(), i64t2.into(), ptr.into(), i64t2.into(), ptr.into(), i64t2.into(), i64t2.into(), ptr.into()],
                false,
            ),
            None,
        ),
    );
    // `std.crypto` (M11 Slice 4) — AEAD. Each of the four `{aes_gcm,chacha20_poly1305}_{seal,open}`
    // entry points takes four byte views (key/nonce/input/aad, each `{ptr,len}`) + an out handle slot,
    // returns an i32 status, and writes an owned `buffer` handle (ciphertext||tag on seal, plaintext on
    // open) into `out` (the `std.compress`/hkdf status shape).
    for name in ["crypto_aes_gcm_seal", "crypto_aes_gcm_open", "crypto_chacha20_poly1305_seal", "crypto_chacha20_poly1305_open"] {
        funcs.insert(
            name.to_string(),
            module.add_function(
                &format!("align_rt_{name}"),
                ctx.i32_type().fn_type(
                    &[
                        ptr.into(), i64t2.into(), ptr.into(), i64t2.into(), ptr.into(), i64t2.into(), ptr.into(), i64t2.into(),
                        ptr.into(),
                    ],
                    false,
                ),
                None,
            ),
        );
    }
    // `std.crypto` (M11 Slice 5) — argon2id. `argon2id` takes two byte views (password/salt, each
    // `{ptr,len}`) + four i64 tuning knobs (m_cost/t_cost/parallelism/len) + an out handle slot,
    // returns an i32 status, and writes an owned `buffer` handle (the derived tag) into `out` (the
    // `std.compress`/hkdf status shape).
    funcs.insert(
        "crypto_argon2id".to_string(),
        module.add_function(
            "align_rt_crypto_argon2id",
            ctx.i32_type().fn_type(
                &[
                    ptr.into(), i64t2.into(), ptr.into(), i64t2.into(), i64t2.into(), i64t2.into(), i64t2.into(),
                    i64t2.into(), ptr.into(),
                ],
                false,
            ),
            None,
        ),
    );
    // `std.compress` — gzip via libz / zstd via libzstd. compress (data view `{ptr,len}`, i64 level,
    // out: *handle) and decompress (data view `{ptr,len}`, out: *handle) both return an i32 status
    // (0 ok / AL_INVALID / AL_CODE+n), writing an owned `buffer` handle into `out`. Both codecs share
    // the same ABI shape.
    funcs.insert(
        "compress_gzip_compress".to_string(),
        module.add_function(
            "align_rt_compress_gzip_compress",
            ctx.i32_type().fn_type(&[ptr.into(), i64t2.into(), i64t2.into(), ptr.into()], false),
            None,
        ),
    );
    funcs.insert(
        "compress_gzip_decompress".to_string(),
        module.add_function(
            "align_rt_compress_gzip_decompress",
            ctx.i32_type().fn_type(&[ptr.into(), i64t2.into(), ptr.into()], false),
            None,
        ),
    );
    funcs.insert(
        "compress_zstd_compress".to_string(),
        module.add_function(
            "align_rt_compress_zstd_compress",
            ctx.i32_type().fn_type(&[ptr.into(), i64t2.into(), i64t2.into(), ptr.into()], false),
            None,
        ),
    );
    funcs.insert(
        "compress_zstd_decompress".to_string(),
        module.add_function(
            "align_rt_compress_zstd_decompress",
            ctx.i32_type().fn_type(&[ptr.into(), i64t2.into(), ptr.into()], false),
            None,
        ),
    );
    // `std.rand` — the `rng` state is always passed by pointer to its `[4 x i64]` slot (mutated in
    // place). `seed_with(out, s)` / `seed_os(out)` initialize it; `next`/`range` advance + return an
    // i64; `shuffle`/`sample` take the slice `{ptr,len}` split into a raw pointer + length + element
    // size. `sample` returns a fresh owned `array<T>` `{ptr,len}`.
    funcs.insert(
        "rng_seed_with".to_string(),
        module.add_function("align_rt_rng_seed_with", ctx.void_type().fn_type(&[ptr.into(), i64t2.into()], false), None),
    );
    funcs.insert(
        "rng_seed_os".to_string(),
        module.add_function("align_rt_rng_seed_os", ctx.void_type().fn_type(&[ptr.into()], false), None),
    );
    funcs.insert(
        "rng_next".to_string(),
        module.add_function("align_rt_rng_next", i64t2.fn_type(&[ptr.into()], false), None),
    );
    funcs.insert(
        "rng_range".to_string(),
        module.add_function("align_rt_rng_range", i64t2.fn_type(&[ptr.into(), i64t2.into(), i64t2.into()], false), None),
    );
    funcs.insert(
        "rng_shuffle".to_string(),
        module.add_function("align_rt_rng_shuffle", ctx.void_type().fn_type(&[ptr.into(), ptr.into(), i64t2.into(), i64t2.into()], false), None),
    );
    funcs.insert(
        "rng_sample".to_string(),
        module.add_function(
            "align_rt_rng_sample",
            slice_struct_type(ctx).fn_type(&[ptr.into(), ptr.into(), i64t2.into(), i64t2.into(), i64t2.into()], false),
            None,
        ),
    );
    // `std.cli` — the command / parsed handles are opaque pointers. `command(name)` allocates one;
    // `flag_*` register into it (void); `parse(cmd, argv{ptr,len}, out)` -> i32 status; `get_*` read
    // a flag (i32/i64/`{ptr,len}` view); `usage` renders an owned `string` `{ptr,len}`; the two
    // `*_free` symbols drop the handles.
    funcs.insert(
        "cli_command".to_string(),
        module.add_function("align_rt_cli_command_new", ptr.fn_type(&[ptr.into(), i64t2.into()], false), None),
    );
    funcs.insert(
        "cli_flag_bool".to_string(),
        module.add_function("align_rt_cli_flag_bool", ctx.void_type().fn_type(&[ptr.into(), ptr.into(), i64t2.into()], false), None),
    );
    funcs.insert(
        "cli_flag_str".to_string(),
        module.add_function("align_rt_cli_flag_str", ctx.void_type().fn_type(&[ptr.into(), ptr.into(), i64t2.into(), ptr.into(), i64t2.into()], false), None),
    );
    funcs.insert(
        "cli_flag_i64".to_string(),
        module.add_function("align_rt_cli_flag_i64", ctx.void_type().fn_type(&[ptr.into(), ptr.into(), i64t2.into(), i64t2.into()], false), None),
    );
    funcs.insert(
        "cli_parse".to_string(),
        module.add_function("align_rt_cli_parse", ctx.i32_type().fn_type(&[ptr.into(), ptr.into(), i64t2.into(), ptr.into()], false), None),
    );
    funcs.insert(
        "cli_get_bool".to_string(),
        module.add_function("align_rt_cli_get_bool", ctx.i32_type().fn_type(&[ptr.into(), ptr.into(), i64t2.into()], false), None),
    );
    funcs.insert(
        "cli_get_i64".to_string(),
        module.add_function("align_rt_cli_get_i64", i64t2.fn_type(&[ptr.into(), ptr.into(), i64t2.into()], false), None),
    );
    funcs.insert(
        "cli_get_str".to_string(),
        module.add_function("align_rt_cli_get_str", slice_struct_type(ctx).fn_type(&[ptr.into(), ptr.into(), i64t2.into()], false), None),
    );
    funcs.insert(
        "cli_usage".to_string(),
        module.add_function("align_rt_cli_usage", slice_struct_type(ctx).fn_type(&[ptr.into()], false), None),
    );
    funcs.insert(
        "cli_command_free".to_string(),
        module.add_function("align_rt_cli_command_free", ctx.void_type().fn_type(&[ptr.into()], false), None),
    );
    funcs.insert(
        "cli_parsed_free".to_string(),
        module.add_function("align_rt_cli_parsed_free", ctx.void_type().fn_type(&[ptr.into()], false), None),
    );
    // `std.http` (Slice 1) — the request / response handles are opaque pointers. `request(method,url)`
    // allocates a request; `header`/`body` mutate it (void); `parse(data{ptr,len}, out)` -> i32 status
    // writes a response handle; `resp_status` -> i64; `resp_header(resp, name{ptr,len}, out)` -> i32
    // present-flag writes a `str` view; `resp_body` -> a `{ptr,len}` view; the two `*_free` drop the
    // handles. (`align_rt_http_serialize` is a runtime-only codec — Slice 2's client calls it — so it
    // is not declared here.)
    funcs.insert(
        "http_request".to_string(),
        module.add_function("align_rt_http_request_new", ptr.fn_type(&[ptr.into(), i64t2.into(), ptr.into(), i64t2.into()], false), None),
    );
    funcs.insert(
        "http_header".to_string(),
        module.add_function("align_rt_http_header", ctx.void_type().fn_type(&[ptr.into(), ptr.into(), i64t2.into(), ptr.into(), i64t2.into()], false), None),
    );
    funcs.insert(
        "http_body".to_string(),
        module.add_function("align_rt_http_body", ctx.void_type().fn_type(&[ptr.into(), ptr.into(), i64t2.into()], false), None),
    );
    funcs.insert(
        "http_parse".to_string(),
        module.add_function("align_rt_http_parse", ctx.i32_type().fn_type(&[ptr.into(), i64t2.into(), ptr.into()], false), None),
    );
    funcs.insert(
        "http_resp_status".to_string(),
        module.add_function("align_rt_http_resp_status", i64t2.fn_type(&[ptr.into()], false), None),
    );
    funcs.insert(
        "http_resp_header".to_string(),
        module.add_function("align_rt_http_resp_header", ctx.i32_type().fn_type(&[ptr.into(), ptr.into(), i64t2.into(), ptr.into()], false), None),
    );
    funcs.insert(
        "http_resp_body".to_string(),
        module.add_function("align_rt_http_resp_body", slice_struct_type(ctx).fn_type(&[ptr.into()], false), None),
    );
    funcs.insert(
        "http_request_free".to_string(),
        module.add_function("align_rt_http_request_free", ctx.void_type().fn_type(&[ptr.into()], false), None),
    );
    funcs.insert(
        "http_resp_free".to_string(),
        module.add_function("align_rt_http_resp_free", ctx.void_type().fn_type(&[ptr.into()], false), None),
    );
    // `std.http` (Slice 2) — the client is an opaque pointer. `client()` allocates one (no args);
    // `get(client, url{ptr,len}, out)` / `post(client, url{ptr,len}, body{ptr,len}, out)` /
    // `request(client, req, out)` each -> i32 status, writing an `http response` handle to `out`;
    // `client_free` drops the handle (and, from Slice 3, closes pooled conns).
    funcs.insert(
        "http_client_new".to_string(),
        module.add_function("align_rt_http_client_new", ptr.fn_type(&[], false), None),
    );
    funcs.insert(
        "http_client_get".to_string(),
        module.add_function("align_rt_http_client_get", ctx.i32_type().fn_type(&[ptr.into(), ptr.into(), i64t2.into(), ptr.into()], false), None),
    );
    funcs.insert(
        "http_client_post".to_string(),
        module.add_function(
            "align_rt_http_client_post",
            ctx.i32_type().fn_type(&[ptr.into(), ptr.into(), i64t2.into(), ptr.into(), i64t2.into(), ptr.into()], false),
            None,
        ),
    );
    funcs.insert(
        "http_client_request".to_string(),
        module.add_function("align_rt_http_client_request", ctx.i32_type().fn_type(&[ptr.into(), ptr.into(), ptr.into()], false), None),
    );
    funcs.insert(
        // cl.get_many (client, urls_ptr, urls_len, max_concurrency, out: *{ptr,len}) -> i32 status.
        // Writes an owned `array<response>` `{ptr,len}` header (buffer of response handles) into `out`.
        "http_get_many".to_string(),
        module.add_function(
            "align_rt_http_get_many",
            ctx.i32_type().fn_type(&[ptr.into(), ptr.into(), i64t2.into(), i64t2.into(), ptr.into()], false),
            None,
        ),
    );
    funcs.insert(
        // drop(array<response>) (ptr, len) -> void; deep free (each response handle, then the header).
        "free_response_array".to_string(),
        module.add_function("align_rt_free_response_array", ctx.void_type().fn_type(&[ptr.into(), i64t2.into()], false), None),
    );
    funcs.insert(
        "http_client_free".to_string(),
        module.add_function("align_rt_http_client_free", ctx.void_type().fn_type(&[ptr.into()], false), None),
    );
    // `std.http` (Slice 4) — the server primitive. All handles are opaque pointers. `serve(host{ptr,
    // len}, port, out)` / `accept(server, out)` -> i32 status, writing an owned handle to `out`;
    // `respond(ctx, rb)` -> i32 status (consumes both). The ctx getters return a `{ptr,len}` view
    // (method/path/body) or write one to `out` + return an i32 present flag (header). `response(status)`
    // allocates a builder; `rb_header`/`rb_body` mutate it; the three `*_free` fns drop the handles.
    funcs.insert(
        "http_serve".to_string(),
        module.add_function("align_rt_http_serve", ctx.i32_type().fn_type(&[ptr.into(), i64t2.into(), i64t2.into(), ptr.into()], false), None),
    );
    funcs.insert(
        "http_server_free".to_string(),
        module.add_function("align_rt_http_server_free", ctx.void_type().fn_type(&[ptr.into()], false), None),
    );
    funcs.insert(
        "http_accept".to_string(),
        module.add_function("align_rt_http_accept", ctx.i32_type().fn_type(&[ptr.into(), ptr.into()], false), None),
    );
    funcs.insert(
        "http_ctx_method".to_string(),
        module.add_function("align_rt_http_ctx_method", slice_struct_type(ctx).fn_type(&[ptr.into()], false), None),
    );
    funcs.insert(
        "http_ctx_path".to_string(),
        module.add_function("align_rt_http_ctx_path", slice_struct_type(ctx).fn_type(&[ptr.into()], false), None),
    );
    funcs.insert(
        "http_ctx_header".to_string(),
        module.add_function("align_rt_http_ctx_header", ctx.i32_type().fn_type(&[ptr.into(), ptr.into(), i64t2.into(), ptr.into()], false), None),
    );
    funcs.insert(
        "http_ctx_body".to_string(),
        module.add_function("align_rt_http_ctx_body", slice_struct_type(ctx).fn_type(&[ptr.into()], false), None),
    );
    funcs.insert(
        "http_ctx_free".to_string(),
        module.add_function("align_rt_http_ctx_free", ctx.void_type().fn_type(&[ptr.into()], false), None),
    );
    funcs.insert(
        "http_response_new".to_string(),
        module.add_function("align_rt_http_response_new", ptr.fn_type(&[i64t2.into()], false), None),
    );
    funcs.insert(
        "http_rb_header".to_string(),
        module.add_function("align_rt_http_rb_header", ctx.void_type().fn_type(&[ptr.into(), ptr.into(), i64t2.into(), ptr.into(), i64t2.into()], false), None),
    );
    funcs.insert(
        "http_rb_body".to_string(),
        module.add_function("align_rt_http_rb_body", ctx.void_type().fn_type(&[ptr.into(), ptr.into(), i64t2.into()], false), None),
    );
    funcs.insert(
        "http_response_free".to_string(),
        module.add_function("align_rt_http_response_free", ctx.void_type().fn_type(&[ptr.into()], false), None),
    );
    funcs.insert(
        "http_respond".to_string(),
        module.add_function("align_rt_http_respond", ctx.i32_type().fn_type(&[ptr.into(), ptr.into()], false), None),
    );
    // Streaming response (SSE/chunked): `respond_stream(ctx, rb, out) -> i32` (writes the head + framing,
    // lifts the fd into an owned `http_stream` at `out`); `stream_send(s, ptr, len) -> i32`;
    // `stream_finish(s) -> i32` (consumes `s`); `stream_free(s)` (Drop: close-only).
    funcs.insert(
        "http_respond_stream".to_string(),
        module.add_function("align_rt_http_respond_stream", ctx.i32_type().fn_type(&[ptr.into(), ptr.into(), ptr.into()], false), None),
    );
    funcs.insert(
        "http_stream_send".to_string(),
        module.add_function("align_rt_http_stream_send", ctx.i32_type().fn_type(&[ptr.into(), ptr.into(), i64t2.into()], false), None),
    );
    funcs.insert(
        "http_stream_finish".to_string(),
        module.add_function("align_rt_http_stream_finish", ctx.i32_type().fn_type(&[ptr.into()], false), None),
    );
    funcs.insert(
        "http_stream_free".to_string(),
        module.add_function("align_rt_http_stream_free", ctx.void_type().fn_type(&[ptr.into()], false), None),
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
    // `std.path` — `base`/`dir`/`ext(p)` return a borrowed sub-`str` `{ptr,len}` of `p`; `normalize(p)`
    // returns a freshly-allocated owned `string` `{ptr,len}`. Each is (ptr, len) -> {ptr,len}.
    for (key, sym) in [
        ("path_base", "align_rt_path_base"),
        ("path_dir", "align_rt_path_dir"),
        ("path_ext", "align_rt_path_ext"),
        ("path_normalize", "align_rt_path_normalize"),
    ] {
        funcs.insert(
            key.to_string(),
            module.add_function(sym, slice_struct_type(ctx).fn_type(&[ptr.into(), i64t2.into()], false), None),
        );
    }
    funcs.insert(
        // path.join (a_ptr, a_len, b_ptr, b_len) -> {ptr,len} owned string.
        "path_join".to_string(),
        module.add_function(
            "align_rt_path_join",
            slice_struct_type(ctx).fn_type(&[ptr.into(), i64t2.into(), ptr.into(), i64t2.into()], false),
            None,
        ),
    );
    funcs.insert(
        // env.get (name_ptr, name_len, out: *{ptr,len}) -> i32 present flag (1/0).
        "env_get".to_string(),
        module.add_function("align_rt_env_get", ctx.i32_type().fn_type(&[ptr.into(), i64t2.into(), ptr.into()], false), None),
    );
    funcs.insert(
        // env.set (name_ptr, name_len, val_ptr, val_len) -> i32 errno-status.
        "env_set".to_string(),
        module.add_function("align_rt_env_set", ctx.i32_type().fn_type(&[ptr.into(), i64t2.into(), ptr.into(), i64t2.into()], false), None),
    );
    funcs.insert(
        // time.now () -> i64 (UNIX-epoch ns, CLOCK_REALTIME).
        "time_now".to_string(),
        module.add_function("align_rt_time_now", i64t2.fn_type(&[], false), None),
    );
    funcs.insert(
        // time.instant () -> i64 (monotonic ns, CLOCK_MONOTONIC).
        "time_instant".to_string(),
        module.add_function("align_rt_time_instant", i64t2.fn_type(&[], false), None),
    );
    funcs.insert(
        // time.sleep (ns: i64) -> void.
        "time_sleep".to_string(),
        module.add_function("align_rt_time_sleep", ctx.void_type().fn_type(&[i64t2.into()], false), None),
    );
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
        "builder_into_string_stack".to_string(),
        module.add_function(
            "align_rt_builder_into_string_stack",
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
    funcs.insert(
        "builder_free_stack".to_string(),
        module.add_function(
            "align_rt_builder_free_stack",
            ctx.void_type().fn_type(&[ptr.into()], false),
            None,
        ),
    );
    // M13 Slice 5A — every `align_rt_*` declare now exists; hand-annotate the audited subset whose
    // contract is provable from the runtime body (LLVM can't see those Rust bodies and never inlines
    // the calls, so it infers nothing without this). Fail-safe: an unlisted symbol gets no attribute.
    // `rt_lto_skip_guarded` (the `--rt-lto` path) additionally withholds curation from the guarded
    // set — they are about to gain real bodies whose attributes LLVM infers (M14 Slice 2).
    apply_rt_contract_attrs(ctx, module, rt_lto_skip_guarded);
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
        mark_private_helper(thunk);
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
        mark_private_helper(thunk);
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
        mark_private_helper(tramp);
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

    // Pass 2: define bodies.
    for f in &program.fns {
        let builder = ctx.create_builder();
        let stack_headers = stack_header_plan(f);
        // Under debug info, give each function a DISubprogram (anchored to its first source line)
        // and attach it to the LLVM function, so its instructions can carry DILocations.
        let fn_line = debug_ctx.as_ref().map_or(0, |_| first_fn_line(f));
        let subprogram = debug_ctx.as_ref().map(|dc| {
            let sp = dc.dib.create_function(
                dc.scope,
                symbol_name(f),
                None,
                dc.file,
                fn_line,
                dc.subty,
                /* is_local_to_unit */ false,
                /* is_definition */ true,
                fn_line,
                DIFlags::ZERO,
                /* is_optimized */ true,
            );
            funcs[&f.name].set_subprogram(sp);
            sp
        });
        FnGen {
            ctx,
            module,
            builder: &builder,
            funcs: &funcs,
            extern_abi: &extern_abi,
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
            stack_header_slots: stack_headers.slots,
            stack_header_new_values: stack_headers.new_values,
            stack_header_load_values: stack_headers.load_values,
            stack_headers: HashMap::new(),
            stack_template_values: stack_headers.template_values,
            stack_template_headers: HashMap::new(),
            blocks: Vec::new(),
            alias_scopes: HashMap::new(),
            dibuilder: debug_ctx.as_ref().map(|dc| &dc.dib),
            subprogram,
            fn_line,
        }
        .emit_fn()?;
    }
    // Resolve debug metadata before it is read (verify / opt pipeline / print). `Drop` also
    // finalizes, but doing it explicitly keeps the ordering obvious.
    if let Some(dc) = &debug_ctx {
        dc.dib.finalize();
    }
    // A `Result`- or `Unit`-returning main needs a C `main` wrapper: `Result` maps Ok/Err to an
    // exit code (and, when `main(args: array<str>)`, marshals argv into the `array<str>`
    // argument — the argv form is Result-only, sema-enforced); `Unit` has no error to report, so
    // the wrapper just calls `align_main` and always returns a defined `0` (the bug this fixes —
    // previously a `()`-returning `main` WAS the C entry directly, declared `void`, and `ret void`
    // left the C ABI's i32 return register undefined; see `docs/open-questions.md` "Unit-returning
    // `fn main()` yields a nondeterministic exit code").
    if let Some(f) =
        program.fns.iter().find(|f| f.name == "main" && (matches!(f.ret, Ty::Result(..)) || f.ret == Ty::Unit))
    {
        emit_main_wrapper(ctx, module, funcs["main"], f.ret, !f.params.is_empty())?;
    }
    Ok(())
}

/// The function's fallback source line for debug info: the first non-zero statement line in program
/// order, or 1 if none was recorded (a synthetic function). Never 0 — a `DISubprogram` line of 0 is
/// legal but a nonzero fallback keeps every anchored location inside the function's line range.
fn first_fn_line(f: &Function) -> u32 {
    f.blocks
        .iter()
        .flat_map(|b| b.stmt_lines.iter())
        .find_map(|&(line, _)| (line != 0).then_some(line))
        .unwrap_or(1)
}

/// The LLVM symbol for a function: a `Result`- or `Unit`-returning `main` is emitted as
/// `align_main` (the C `main` is a generated wrapper that always returns a defined `i32`);
/// everything else keeps its name. (An `-> i32` `main` needs no wrapper — it already returns
/// the C ABI's type directly — so it keeps the `main` symbol and IS the C entry.)
fn symbol_name(f: &Function) -> &str {
    if f.name == "main" && (matches!(f.ret, Ty::Result(..)) || f.ret == Ty::Unit) {
        "align_main"
    } else {
        &f.name
    }
}

/// Emit the C `main` for a `Result<(), Error>`- or `Unit`-returning Align `main` (renamed
/// `align_main`, see [`symbol_name`]): call it, then materialize a **defined** `i32` exit code —
/// on `Err(code)` report the error and exit with `code`; on `Ok` or a plain `Unit` return, exit 0.
/// A `Unit` `align_main` is `void`, so there is no tag/payload to inspect; the wrapper's only job
/// for that case is to turn the void call into `ret i32 0` (never leave the ABI return register
/// undefined — the bug this function exists to close for the `Unit` case, `has_args` always
/// `false` there since sema restricts the `args: array<str>` form to a `Result`-returning `main`).
fn emit_main_wrapper<'c>(
    ctx: &'c Context,
    module: &Module<'c>,
    align_main: FunctionValue<'c>,
    ret: Ty,
    has_args: bool,
) -> Result<(), CodegenError> {
    if !matches!(ret, Ty::Result(_, _)) && ret != Ty::Unit {
        return Err(CodegenError::Lowering("main wrapper on a non-Result, non-Unit return".into()));
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

    let call = builder.build_call(align_main, &call_args, "r").map_err(lower)?;
    if ret == Ty::Unit {
        // `align_main` is `void(...)` — nothing to inspect, just materialize a defined `0`.
        builder.build_return(Some(&i32t.const_int(0, false))).map_err(lower)?;
        return Ok(());
    }
    let res = call
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
        // A `{ptr,len}` payload (an owned `string` in an Option/Result, slice 8a; also str/slice/
        // array views) lowers to the slice struct. A `json.doc` is a `{tape,node}` = `{ptr,i64}` too.
        Ty::Str | Ty::String | Ty::Slice(_) | Ty::Soa(_) | Ty::JsonDoc | Ty::JsonScanner(_) | Ty::DynArray(_) => slice_struct_type(ctx).into(),
        // An AoS struct array is a `{ptr,len}` view too; an SoA one would be a different
        // representation (column buffers), so match the layout — `Layout::Soa` (M6) makes this
        // arm go non-exhaustive (a compile error pointing exactly here).
        Ty::DynStructArray(_, Layout::Aos) | Ty::DynSliceArray(_) | Ty::DynResponseArray => slice_struct_type(ctx).into(),
        // `Task<R>` (④b) is a box in the task_group region — a pointer, like `box<T>`.
        Ty::Task(_) => ctx.ptr_type(AddressSpace::default()).into(),
        // A `reader`/`writer`/`buffer` / cli handle / `tcp_conn` payload is an opaque pointer.
        Ty::Reader | Ty::Writer | Ty::Buffer | Ty::CliCommand | Ty::CliParsed | Ty::TcpConn | Ty::TcpListener | Ty::UdpSocket | Ty::Child | Ty::File | Ty::HttpRequest | Ty::HttpResponse | Ty::HttpClient | Ty::HttpServer | Ty::HttpRequestCtx | Ty::ResponseBuilder | Ty::HttpStream => ctx.ptr_type(AddressSpace::default()).into(),
        // `vecN<T>` (M6) → the LLVM vector `<N x T>`.
        Ty::Vec(s, n) => vec_llvm_ty(ctx, scalar_to_ty(s), n),
        // A comparison `mask` (M6) → `<N x i1>` (one bool lane per vector lane; element-independent).
        Ty::Mask(_, n) => ctx.bool_type().vec_type(n).into(),
        // `rng` — the 256-bit Xoshiro256++ state as `[4 x i64]` (a Copy by-value aggregate).
        Ty::Rng => rng_llvm_type(ctx),
        _ => int_type(ctx, ty).into(),
    }
}

/// The LLVM type of an `rng` value — the Xoshiro256++ state, `[4 x i64]`. A Copy by-value
/// aggregate (passed/returned in memory per SysV, which LLVM handles); the runtime mutates it
/// through a pointer to its slot.
fn rng_llvm_type<'c>(ctx: &'c Context) -> BasicTypeEnum<'c> {
    ctx.i64_type().array_type(4).into()
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
/// The payload [`Scalar`] at aggregate field `idx` of an `Option`/`Result` (Some/Ok = field 1,
/// Err = field 2) — so a drop can pick the right destructor (`reader`/`writer` handles close their
/// fd; every other Move payload frees a `{ptr,len}` buffer).
fn payload_field_scalar(ty: Ty, idx: u32) -> Option<Scalar> {
    match (ty, idx) {
        (Ty::Option(s), 1) => Some(s),
        (Ty::Result(o, _), 1) => Some(o),
        (Ty::Result(_, e), 2) => Some(e),
        _ => None,
    }
}

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
        Ty::Box(_) | Ty::ArenaHandle | Ty::Builder | Ty::StrFinder | Ty::Writer | Ty::Reader | Ty::Buffer | Ty::ArrayBuilder(_) | Ty::TcpConn | Ty::TcpListener | Ty::UdpSocket | Ty::Child | Ty::File | Ty::Raw => ctx.ptr_type(AddressSpace::default()).into(),
        // A function value is a closure `{fn_ptr, env_ptr}` here too — matching `llvm_type`, so an
        // `Ty::Fn` in an ABI position (later: fn-typed parameters/returns) is not silently `i32`.
        Ty::Fn(_) => closure_struct_type(ctx).into(),
        Ty::Slice(_) | Ty::Soa(_) | Ty::JsonDoc | Ty::JsonScanner(_) | Ty::Str | Ty::String | Ty::DynArray(_) => slice_struct_type(ctx).into(),
        // AoS struct array = `{ptr,len}`; SoA (M6) differs → match the layout (forces revisit).
        Ty::DynStructArray(_, Layout::Aos) | Ty::DynSliceArray(_) | Ty::DynResponseArray => slice_struct_type(ctx).into(),
        _ => scalar_type(ctx, ty, sx, ex),
    }
}

// ── `extern "C"` by-value struct ABI (System V AMD64 only) ──────────────────────────────────────
//
// A `layout(C)` struct crosses the C boundary by value using the SysV AMD64 register convention
// (ABI §3.2.3). We reproduce *exactly* the coerced IR types clang emits — flattened `i64`/`double`
// arguments per eightbyte, an `{T0,T1}` aggregate return for two-register structs — so an Align call
// is binary-compatible with a clang/gcc-compiled callee (both lower these same IR types identically).
// This is SysV-AMD64-only; every other target is rejected in `build_module` (a wrong per-target
// register rule is the one FFI corner that silently miscompiles, so we never guess).
//
// Completeness within our field domain: a `layout(C)` struct's fields are integer/float scalars
// (`align_sema` enforces this), each naturally aligned, so no field straddles an eightbyte boundary
// and the only classes are INTEGER and SSE — never X87/COMPLEX_X87. A struct larger than two
// eightbytes (> 16 bytes) is MEMORY as a whole (no `__m256`/SSEUP in the domain), handled separately.

/// SysV class of one eightbyte of a register-passed `layout(C)` struct.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Eb {
    /// Passed/returned in a general-purpose register (integer scalar, or a mixed eightbyte —
    /// INTEGER+SSE merges to INTEGER).
    Integer,
    /// Passed/returned in an SSE (XMM) register (all overlapping fields are float).
    Sse,
}

impl Eb {
    /// The LLVM type standing in for one eightbyte: `i64` for a GP register, `double` for an SSE
    /// register. A wider-than-used integer (e.g. `i64` for an eightbyte holding one `i32`) is
    /// ABI-safe — both land in the same GP register, and the caller-side padded slot keeps the load
    /// in bounds — matching how clang's narrower coerce type occupies the identical register.
    fn llvm<'c>(self, ctx: &'c Context) -> BasicTypeEnum<'c> {
        match self {
            Eb::Integer => ctx.i64_type().into(),
            Eb::Sse => ctx.f64_type().into(),
        }
    }
}

/// The SysV AMD64 by-value plan for a register-passed `layout(C)` struct (≤ 16 bytes): its per-
/// eightbyte classes, its byte size, and its struct id (to reconstruct the value at a call site).
#[derive(Clone)]
struct StructAbi {
    /// One class per eightbyte, in ascending byte order (length 1 or 2).
    ebs: Vec<Eb>,
    /// The struct id, indexing the LLVM struct-type / struct-def tables.
    id: u32,
}

/// Classify a `layout(C)` struct for SysV AMD64 by-value passing. Returns `Some(abi)` for a register
/// struct (size ≤ 16 bytes) and `None` for a MEMORY struct (> 16 bytes — not register-passed;
/// rejected in FFI v1). `st`/`def` are the struct's LLVM type and definition; `td` gives real field
/// offsets, so the classification tracks the actual emitted layout. Only called for a `layout(C)`
/// struct on an x86-64 SysV target.
fn classify_struct_abi(
    id: u32,
    st: &StructType,
    def: &StructDef,
    td: &inkwell::targets::TargetData,
) -> Option<StructAbi> {
    let size = td.get_store_size(st);
    if size > 16 {
        return None; // MEMORY-class: not register-passed
    }
    // A zero-size (empty) struct has no C ABI representation; sema rejects it as an FFI type before
    // codegen, so it never reaches here — but stay total (no eightbytes to classify).
    if size == 0 {
        return Some(StructAbi { ebs: Vec::new(), id });
    }
    let eb_count = size.div_ceil(8) as usize; // 1 or 2 eightbytes
    let mut ebs: Vec<Option<Eb>> = vec![None; eb_count];
    // `layout(C)` keeps declaration order (identity physical map), so field `i` sits at physical
    // slot `i`; a naturally-aligned scalar ≤ 8 bytes lies wholly within one eightbyte.
    for (i, f) in def.fields.iter().enumerate() {
        let off = td.offset_of_element(st, i as u32).unwrap_or(0);
        let eb = (off / 8) as usize;
        let cls = if matches!(f.ty, Ty::Float(_)) { Eb::Sse } else { Eb::Integer };
        // Merge within an eightbyte: INTEGER dominates SSE (SSE only if every field is float).
        ebs[eb] = Some(match ebs[eb] {
            Some(Eb::Integer) => Eb::Integer,
            _ if cls == Eb::Integer => Eb::Integer,
            Some(Eb::Sse) | None => cls,
        });
    }
    // A pure-padding eightbyte cannot occur for a size-accounted struct; default to INTEGER (a
    // valid GP register) so the function is total.
    Some(StructAbi { ebs: ebs.into_iter().map(|c| c.unwrap_or(Eb::Integer)).collect(), id })
}

/// How one `extern "C"` parameter crosses the ABI boundary.
#[derive(Clone)]
enum ParamAbi {
    /// A scalar / `raw`: passed as its own value.
    Direct,
    /// A `str`/`slice` view: passed as its data pointer (`C char*`/`void*`); length travels
    /// separately via `s.len()`.
    ViewPtr,
    /// A `layout(C)` struct passed by value in registers: flattened to one `i64`/`double` argument
    /// per eightbyte.
    StructRegs(StructAbi),
    /// A `layout(C)` struct too large for registers (> 16 bytes, MEMORY class). Rejected in FFI v1
    /// — a `byval` pointer is semantically identical to the existing struct-by-pointer FFI, so we do
    /// not add a redundant second mechanism.
    StructMemory,
}

/// How an `extern "C"` return value crosses the ABI boundary.
#[derive(Clone)]
enum ReturnAbi {
    /// void / scalar / `raw`.
    Direct,
    /// A `layout(C)` struct returned by value in registers (≤ 16 bytes).
    StructRegs(StructAbi),
    /// A `layout(C)` struct returned via a hidden `sret` pointer (> 16 bytes, MEMORY class).
    /// Rejected in FFI v1 (deferred until a concrete C API needs a large by-value return).
    StructMemory,
}

/// The full SysV ABI plan for one `extern "C"` symbol.
#[derive(Clone)]
struct ExternAbi {
    params: Vec<ParamAbi>,
    ret: ReturnAbi,
}

/// The LLVM return type for a register-passed struct: a single scalar for a one-eightbyte struct,
/// an `{T0,T1}` aggregate for a two-eightbyte struct (matching clang: `i64` / `double` /
/// `{i64,i64}` / `{double,double}` / `{i64,double}` …). The aggregate's field classes drive LLVM's
/// two-register return assignment (GP vs XMM), so INTEGER→`i64` and SSE→`double` must be exact.
fn struct_ret_type<'c>(ctx: &'c Context, abi: &StructAbi) -> BasicTypeEnum<'c> {
    if abi.ebs.len() == 1 {
        abi.ebs[0].llvm(ctx)
    } else {
        let fields: Vec<BasicTypeEnum> = abi.ebs.iter().map(|e| e.llvm(ctx)).collect();
        ctx.struct_type(&fields, false).into()
    }
}

/// The SysV AMD64 argument-register budget: 6 general-purpose (RDI, RSI, RDX, RCX, R8, R9) and 8 SSE
/// (XMM0–XMM7).
const SYSV_INT_ARG_REGS: u32 = 6;
const SYSV_SSE_ARG_REGS: u32 = 8;

/// Enforce the SysV **all-or-nothing** rule for by-value struct *arguments*: a struct is passed in
/// registers only if *every* one of its eightbytes fits in the class registers still free after the
/// preceding arguments; otherwise the ABI puts the whole struct in memory via a `byval` pointer.
///
/// We do not implement that `byval` path — and, crucially, cannot fake it by flattening. A flattened
/// `{i64,i64}` argument at the exhaustion boundary makes LLVM assign one eightbyte to the last free
/// register and spill the other to the stack, whereas a clang-compiled callee reading a `byval`
/// argument expects the whole struct on the stack; the two disagree (verified: a `{i64,i64}` passed
/// after five `i64` args round-trips to garbage). So we **reject** any signature where a by-value
/// struct argument would fall to memory, rather than silently miscompile. In every *accepted* case
/// the struct fits in registers, and per-eightbyte flattening is byte-identical to clang's own
/// flattened parameter form, so the call is correct.
///
/// Only struct arguments consume the budget check: a scalar/pointer/view that itself spills to the
/// stack is lowered identically on both sides (a single stack slot), so it never diverges.
fn check_sysv_struct_args_fit(
    ext_name: &str,
    abi: &ExternAbi,
    param_tys: &[Ty],
    structs: &[StructDef],
) -> Result<(), CodegenError> {
    let mut gp = 0u32;
    let mut sse = 0u32;
    for (i, (pa, ty)) in abi.params.iter().zip(param_tys).enumerate() {
        match pa {
            ParamAbi::Direct => {
                if matches!(ty, Ty::Float(_)) {
                    sse += 1;
                } else {
                    gp += 1; // integer / `raw` pointer → a general-purpose register
                }
            }
            ParamAbi::ViewPtr => gp += 1, // a `str`/`slice` passes as one data pointer
            ParamAbi::StructRegs(sabi) => {
                let need_int = sabi.ebs.iter().filter(|e| matches!(e, Eb::Integer)).count() as u32;
                let need_sse = sabi.ebs.iter().filter(|e| matches!(e, Eb::Sse)).count() as u32;
                if gp + need_int > SYSV_INT_ARG_REGS || sse + need_sse > SYSV_SSE_ARG_REGS {
                    let sname = &structs[sabi.id as usize].name;
                    return Err(CodegenError::Lowering(format!(
                        "extern '{ext_name}': by-value struct '{sname}' (argument {n}) would be passed in memory — the preceding arguments exhaust the SysV class registers ({SYSV_INT_ARG_REGS} integer / {SYSV_SSE_ARG_REGS} SSE), so the struct falls to the MEMORY-class `byval` ABI, which is not supported in FFI v1. Reorder the parameters so the struct fits in registers, or pass it by pointer (`raw`).",
                        n = i + 1,
                    )));
                }
                gp += need_int;
                sse += need_sse;
            }
            // A > 16-byte MEMORY struct consumes no registers and is rejected separately (with a size
            // message) in the declaration loop.
            ParamAbi::StructMemory => {}
        }
    }
    Ok(())
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
fn field_abi_align(ty: Ty, structs: &[StructDef], enums: &[EnumDef]) -> u64 {
    match ty {
        Ty::Int(it) => (it.bits / 8).max(1) as u64,
        Ty::Float(ft) => (ft.bits / 8) as u64,
        Ty::Bool => 1,
        Ty::Char => 4,
        Ty::Unit => 4,
        Ty::Struct(id) | Ty::StructArray(id, _) => structs[id as usize]
            .fields
            .iter()
            .map(|f| field_abi_align(f.ty, structs, enums))
            .max()
            .unwrap_or(1),
        // A sum type lowers to `{ i32 tag, <payloads flattened> }` (`enum_types`), so its ABI
        // alignment is the max of the i32 tag (4) and every payload scalar's alignment (J1b: an enum
        // is a valid struct field). Must match sema's `enum_size_align`.
        Ty::Enum(id) => enums.get(id as usize).map_or(4, |e| {
            e.variants
                .iter()
                .flat_map(|v| v.payload.iter())
                .map(|&s| field_abi_align(scalar_to_ty(s), structs, enums))
                .fold(4, u64::max)
        }),
        Ty::Array(s, _) => scalar_bytes(s).clamp(1, 8),
        _ => 8,
    }
}

/// The logical→physical field-index map for a struct. A non-`layout(C)` struct is laid out in
/// **descending alignment** (Rust's default) to eliminate padding; ties keep declaration order
/// (a *stable* sort), so the layout is deterministic. A `layout(C)` struct keeps declaration order
/// (identity map) — its byte layout is the FFI/`raw`/json boundary and must not move. The returned
/// vector `m` satisfies `m[logical] = physical`; invert with [`physical_order`] to emit the body.
fn logical_to_physical(s: &StructDef, structs: &[StructDef], enums: &[EnumDef]) -> Vec<u32> {
    let n = s.fields.len();
    if s.c_repr {
        return (0..n as u32).collect();
    }
    // Physical order = logical indices sorted by descending alignment (stable → decl order on ties).
    let mut order: Vec<u32> = (0..n as u32).collect();
    order.sort_by_key(|&i| std::cmp::Reverse(field_abi_align(s.fields[i as usize].ty, structs, enums)));
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

/// Round `n` up to the next multiple of the power-of-two alignment `a` (branch-free; the `a <= 1`
/// guard avoids the `a - 1` underflow a stray `a == 0` would cause). The codegen dual of
/// `align_sema::align_up`.
fn align_up(n: u64, a: u64) -> u64 {
    if a <= 1 { n } else { (n + a - 1) & !(a - 1) }
}

/// Build and set the LLVM body of struct `s` (whose opaque type is `st`), the **one** place a struct's
/// LLVM layout is emitted. Fields are laid out in codegen's canonical physical order (`perm`): a
/// non-`layout(C)` struct is reordered by descending alignment to eliminate padding; a `layout(C)`
/// struct keeps declaration order. For an `align(N)` struct, an `[K x i8]` tail is appended so the
/// type's ABI **size** is rounded up to `N` — this is what gives a tight `[N x %S]` array an
/// over-aligned element *stride* (every element stays `align(N)`), exactly as C pads a struct's size
/// up to its alignment. The over-alignment itself is applied at the storage seam (`type_align`, the
/// alloca / global), never as a member alignment, so the aggregate type's ABI *alignment* stays
/// natural — the padding field is `align 1`. Shared by `emit_llvm` and the layout-parity test so the
/// two can never diverge.
fn set_struct_body<'c>(
    ctx: &'c Context,
    st: StructType<'c>,
    s: &StructDef,
    perm: &[u32],
    struct_types: &[StructType<'c>],
    enum_types: &[StructType<'c>],
    target_data: &inkwell::targets::TargetData,
) {
    // `abi_type` maps each field (floats to their float type, `str` to the `{ ptr, len }` view, a
    // nested struct to its (now-created) struct type, a sum-type field to its `enum_types` entry —
    // J1b). Fields are emitted in physical order: physical slot `p` holds the logical field whose map
    // entry is `p`. The enum types must already exist (created opaque/literal before this runs).
    let mut fields: Vec<BasicTypeEnum> =
        physical_order(perm).iter().map(|&li| abi_type(ctx, s.fields[li as usize].ty, struct_types, enum_types)).collect();
    if let Some(a) = s.align {
        // Measure the natural size (from an anonymous struct of the same fields), then pad the type
        // up to `round_up(natural_size, align)` so the array stride is over-aligned.
        let natural = target_data.get_abi_size(&ctx.struct_type(&fields, false));
        let padded = align_up(natural, a as u64);
        if padded > natural {
            fields.push(ctx.i8_type().array_type((padded - natural) as u32).into());
        }
    }
    st.set_body(&fields, false);
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
        Scalar::DynResponseArray => unreachable!("an owned response array is not a box/array payload"),
        // A `str` view is never a `box` payload (`box<str>` is rejected), but it *is* a valid
        // `array<str>` element — a `{ptr,len}` view, 16 bytes (the established str size, as in the
        // json field descriptor). Used to size a `group_by(.str_key)` output key buffer.
        Scalar::Str => 16,
        // A `slice<T>` view is a `{ptr,len}` — 16 bytes, like `Str`. Not a box/array payload today
        // (it only rides an `Option`/`Result`, e.g. `read_bytes_view`), but sizing it correctly (vs.
        // `unreachable!`) keeps this total should a slice element ever be sized here.
        Scalar::Slice(_) => 16,
        // A `json.doc` handle is a `{tape,node}` = `{ptr,i64}`, 16 bytes (like `Str`/`Slice`). Never a
        // box/array payload today (it only rides an `Option`/`Result`), but sized correctly for totality.
        Scalar::JsonDoc => 16,
        Scalar::Soa(_) => unreachable!("a soa view is not a box payload"),
        Scalar::Enum(_) => unreachable!("a sum type is not a box payload"),
        Scalar::Param(_) => unreachable!("a generic parameter is substituted before codegen"),
        Scalar::Reader | Scalar::Writer => unreachable!("a reader/writer handle is not a box/array payload"),
        Scalar::Buffer => unreachable!("a buffer handle is not a box/array payload"),
        Scalar::File => unreachable!("a file handle is not a box/array payload"),
        Scalar::CliParsed => unreachable!("a cli parsed handle is not a box/array payload"),
        Scalar::HttpResponse => unreachable!("an http response handle is not a box/array payload"),
        Scalar::HttpServer => unreachable!("an http_server handle is not a box/array payload"),
        Scalar::HttpRequestCtx => unreachable!("an http_request_ctx handle is not a box/array payload"),
        Scalar::HttpStream => unreachable!("an http_stream handle is not a box/array payload"),
        Scalar::TcpConn => unreachable!("a tcp_conn handle is not a box/array payload"),
        Scalar::TcpListener => unreachable!("a tcp_listener handle is not a box/array payload"),
        Scalar::UdpSocket => unreachable!("a udp_socket handle is not a box/array payload"),
        Scalar::Child => unreachable!("a child handle is not a box/array payload"),
    }
}

fn is_signed(ty: Ty) -> bool {
    matches!(ty, Ty::Int(IntTy { signed: true, .. }))
}

/// The null-safe runtime `*_free` for a bare Move **handle** type — a type laid out as a single
/// opaque pointer (`scalar_type` maps all of these to `ptr`) whose drop closes/frees that handle.
/// `None` for any non-handle type (a `{ptr,len}` buffer, an owned collection, a struct/enum/tuple —
/// each dropped by its own arm). Every listed `*_free` tolerates a null handle, so a moved-out /
/// zeroed slot drops harmlessly.
///
/// **One source of truth for two drop sites:** the standalone-local `Stmt::Drop` and the struct-field
/// drop (`drop_struct_fields`, F1②) both route through this, so a bare handle local and a handle
/// *field* free identically. The set here MUST equal the handle types `align_sema::is_field_ok`
/// admits as fields (`is_move_handle`) — a type allowed as a field but missing here would leak.
/// `Builder`/`StrFinder`/`ArrayBuilder` are Move but NOT here (distinct non-pointer drops); they are
/// correspondingly rejected as fields.
fn handle_free_fn(ty: Ty) -> Option<&'static str> {
    Some(match ty {
        Ty::Writer => "io_writer_free",
        Ty::Reader => "io_reader_free",
        Ty::Buffer => "buffer_free",
        Ty::File => "io_file_free",
        Ty::CliCommand => "cli_command_free",
        Ty::CliParsed => "cli_parsed_free",
        Ty::TcpConn => "tcp_conn_free",
        Ty::TcpListener => "tcp_listener_free",
        Ty::UdpSocket => "udp_socket_free",
        Ty::Child => "child_free",
        Ty::HttpRequest => "http_request_free",
        Ty::HttpResponse => "http_resp_free",
        Ty::HttpClient => "http_client_free",
        Ty::HttpServer => "http_server_free",
        Ty::HttpRequestCtx => "http_ctx_free",
        Ty::ResponseBuilder => "http_response_free",
        Ty::HttpStream => "http_stream_free",
        _ => return None,
    })
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

/// The field-descriptor table (+ PHF) `emit_desc_table` emits for one struct, without the store
/// size. A nested-struct field's `JsonSubTable` global bundles this with the store size; the
/// top-level `DecodeTable` adds the store size for the decode call.
struct DescTable<'c> {
    descs: inkwell::values::PointerValue<'c>,
    n_fields: u64,
    phf_ptr: inkwell::values::PointerValue<'c>,
    phf_len: u64,
    phf_seed: u64,
}

/// The canonical `wyhash`, seeded — the hash behind the compile-time perfect-hash field dispatch.
/// This and the runtime-side `json_phf_hash` (`align_runtime`) **both** call `align_hash::wyhash`,
/// so the slot a JSON key maps to is byte-identical on the two ends *by construction* — the shared
/// crate makes the codegen↔runtime PHF byte-match structural rather than a hand-kept invariant.
fn phf_hash(bytes: &[u8], seed: u64) -> u64 {
    align_hash::wyhash(bytes, seed)
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

/// Shared param/return ABI type mapping used by both [`declare_fn`] (defines a function) and
/// [`declare_imported_fn`] (declares an external cross-unit call target). Structs / struct-arrays /
/// tuples / enums pass and return by value as their aggregate LLVM type; everything else falls back
/// to `abi_type` (scalars + Option/Result/slice/str). A single shared helper means a new `Ty` variant
/// special-cased here is special-cased for *both* callers — a declare-vs-definition LLVM signature
/// divergence between them would be an ABI mismatch (ill-formed IR / memory corruption at the call
/// boundary), so the two match arms must never be free to drift apart.
fn abi_map_ty<'c>(
    ctx: &'c Context,
    ty: Ty,
    struct_types: &[StructType<'c>],
    enum_types: &[StructType<'c>],
    tuple_types: &[StructType<'c>],
) -> BasicTypeEnum<'c> {
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
}

// The type-table + `exports` parameters are each independently threaded through from `build_module`
// (no natural grouping struct exists yet for "the type tables"); splitting them into a bag-of-fields
// struct would obscure more than it clarifies for a single call site.
#[allow(clippy::too_many_arguments)]
fn declare_fn<'c>(
    ctx: &'c Context,
    module: &Module<'c>,
    f: &Function,
    symbol: &str,
    struct_types: &[StructType<'c>],
    enum_types: &[StructType<'c>],
    tuple_types: &[StructType<'c>],
    exports: &[String],
) -> FunctionValue<'c> {
    let map = |ty: Ty| -> BasicTypeEnum<'c> { abi_map_ty(ctx, ty, struct_types, enum_types, tuple_types) };
    let param_types: Vec<BasicMetadataTypeEnum> =
        f.params.iter().map(|s| map(f.slots[*s as usize]).into()).collect();
    let fn_ty = if f.ret == Ty::Unit {
        ctx.void_type().fn_type(&param_types, false)
    } else {
        map(f.ret).fn_type(&param_types, false)
    };
    let fv = module.add_function(symbol, fn_ty, None);
    mark_nounwind(ctx, fv);
    // Every Align program function is module-private (internal) EXCEPT:
    //  - the C entry: an `-> i32` `main` keeps the symbol name `main` and IS the C entry (`crt0`
    //    resolves it by name), so it must stay external — its LLVM return type is already the C
    //    ABI's `i32`, so no wrapper is needed. A `Result`- or `Unit`-returning `main` is emitted as
    //    `align_main` here (internal) instead, and its external C `main` wrapper — which always
    //    returns a defined `i32` (`0`, an `Err` exit code, or `0` for a plain `Unit` return; see
    //    `emit_main_wrapper`) — is generated separately. No other function is named `main`.
    //  - an explicit export root (`emit-obj`/`emit-llvm --export <name>`, M13 Codex-audit item
    //    1). Exporting a function makes it BOTH a linker-visible external symbol AND a DCE root in
    //    the same step (`external` linkage keeps LLVM's `globaldce`/`internalize`-style passes from
    //    ever considering it dead), so linkage and "what stays reachable" always agree.
    //
    // BOTH checks are keyed on `symbol` (the LLVM name), never `f.name` (the source name): for a
    // `Result`-returning `main`, `f.name == "main"` but `symbol == "align_main"` — if the export
    // check compared `f.name`, `--export main` would match it and skip internalizing `align_main`,
    // leaving it wrongly external (a real, one-line-fix regression caught in review). Keying on
    // `symbol` makes `--export main` compare against `"align_main"`, which never matches, so
    // `align_main` still internalizes and `--export main` stays the harmless no-op the CLI promises
    // (the C `main` wrapper was already external via the first check, unconditionally). Every
    // *other* function has `symbol == f.name` (only `main` is ever renamed), so this is
    // observationally identical to keying on `f.name` for the entire non-`main` case — the export
    // roots the driver validates (`align_driver::unknown_exports`, matched against `Function::name`)
    // still name exactly what the caller wrote.
    //  - a per-unit `pub` export (M15 S2): a non-entry `pub` user function keeps `external` linkage
    //    so a dependent unit's object can resolve the cross-unit call. `f.exportable` is set only by
    //    per-unit lowering; the whole-program path leaves it `false`, so the default object is
    //    byte-identical (every function but `main`/`--export` still internalizes).
    if symbol != "main" && !exports.iter().any(|e| e == symbol) && !f.exportable {
        mark_internal(fv);
    }
    fv
}

/// M15 S2: declare an imported `pub` function (a cross-unit call target defined in another unit's
/// object) as an external, bodyless LLVM `declare`. The signature is computed exactly as
/// [`declare_fn`] does for a definition (structs / struct-arrays / tuples / enums pass and return by
/// value as their aggregate type; scalars/views via `abi_type`), so the call type matches the
/// owning unit's definition and the linker binds them. Linkage stays external (an undefined symbol
/// cannot be internal); `nounwind` matches the Align contract every program function carries.
fn declare_imported_fn<'c>(
    ctx: &'c Context,
    module: &Module<'c>,
    imp: &align_sema::hir::ImportedFn,
    struct_types: &[StructType<'c>],
    enum_types: &[StructType<'c>],
    tuple_types: &[StructType<'c>],
) -> FunctionValue<'c> {
    let map = |ty: Ty| -> BasicTypeEnum<'c> { abi_map_ty(ctx, ty, struct_types, enum_types, tuple_types) };
    let param_types: Vec<BasicMetadataTypeEnum> = imp.params.iter().map(|&ty| map(ty).into()).collect();
    let fn_ty = if imp.ret == Ty::Unit {
        ctx.void_type().fn_type(&param_types, false)
    } else {
        map(imp.ret).fn_type(&param_types, false)
    };
    let fv = module.add_function(&imp.name, fn_ty, None);
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

/// M13 Slice 1 — link hygiene. Align compiles the whole program (entry file + every transitively
/// imported user module) into ONE LLVM module and ONE object file: `pub` is a sema-level *module*
/// visibility that is fully resolved by name-mangling before codegen, and there is no separate
/// compilation or C-ABI export of Align function bodies (see `align_sema` note near the `map_into`
/// `noalias` derivation). Therefore the only *program* definition the linker/runtime must resolve
/// by name is the C entry `main`, plus whatever the caller names as an explicit export root
/// (`emit-obj`/`emit-llvm --export`, M13 Codex-audit item 1) — e.g. a benchmark kernel or FFI
/// library with no `main`. Every other emitted definition is module-private, so we give it the
/// tightest linkage that still lets its address escape to the runtime when needed. This unlocks
/// LLVM IPO/DCE/inlining and `constmerge`. `external` is kept ONLY for `main`, export roots, and
/// the undefined `declare`s (runtime `align_rt_*` + `extern "C"` FFI) — an undefined symbol cannot
/// be internal.
///
/// `internal`: an Align *program* function body — module-local, but the name is kept (harmless).
fn mark_internal<'c>(f: FunctionValue<'c>) {
    f.set_linkage(Linkage::Internal);
}

/// `private`: a compiler-generated helper (fn-value / closure / spawn-trampoline / par_map thunk).
/// Stronger than `internal` — the symbol name itself is dropped. These are only ever reached
/// through a function pointer (handed to the runtime or an indirect call), never by symbol name, so
/// dropping the name is safe and maximizes what the optimizer may fold away.
fn mark_private_helper<'c>(f: FunctionValue<'c>) {
    f.set_linkage(Linkage::Private);
}

/// `private unnamed_addr`: a codegen-emitted constant global (string bytes, JSON field-descriptor
/// table, perfect-hash table). `private` hides the symbol — nothing references these by name, only
/// by the pointer we return. `unnamed_addr` declares the *address* is not significant (only the
/// contents are), which lets LLVM's `constmerge` fold byte-identical constants (e.g. two equal
/// string literals) into one. The caller sets `constant` where the data is immutable.
fn mark_private_unnamed_addr<'c>(g: inkwell::values::GlobalValue<'c>) {
    g.set_linkage(Linkage::Private);
    g.set_unnamed_address(inkwell::values::UnnamedAddress::Global);
}

/// Resolve a named enum attribute to its LLVM kind id, **failing loudly** if this LLVM version does
/// not recognize the name.
///
/// `Attribute::get_named_enum_kind_id` returns `0` for any name the linked LLVM does not know — a
/// renamed, removed, or mistyped attribute (LLVM 22, for instance, removed the `nocapture`
/// parameter attribute in favour of `captures(...)`, so `"nocapture"` now resolves to `0`). Kind
/// `0` is not a usable attribute: `create_enum_attribute(0, _)` silently emits the wrong thing
/// (inkwell's LLVM-22 printer renders it as the bare, un-reparseable `none` shorthand), which
/// *drops* the optimization contract the attribute was meant to carry (`noalias` / `readonly` /
/// `memory` / `captures`) — a silent miscompile.
///
/// Every `name` reaching here is a compiler-internal string literal (never user input), so an
/// unknown name is a compiler build defect against the linked LLVM: input-independent — it would
/// fail on *every* compilation, not for one particular program. That is an internal invariant
/// violation, handled the way the rest of codegen handles them (`unreachable!` / `expect`): a loud
/// panic naming the offending attribute, **not** a per-program [`CodegenError`] (which would falsely
/// blame the user's source).
fn enum_kind_id(name: &str) -> u32 {
    let kind = inkwell::attributes::Attribute::get_named_enum_kind_id(name);
    assert!(
        kind != 0,
        "codegen: LLVM does not recognize the enum attribute {name:?} (kind id 0) — it was renamed \
         or removed in this LLVM version; emitting it would silently drop the attribute's \
         optimization contract"
    );
    kind
}

/// Add a named zero-valued enum attribute at `loc`. Fails loudly (see [`enum_kind_id`]) if the
/// attribute name is unknown to the linked LLVM.
fn add_enum_attr<'c>(
    ctx: &'c Context,
    f: FunctionValue<'c>,
    loc: inkwell::attributes::AttributeLoc,
    name: &str,
) {
    f.add_attribute(loc, ctx.create_enum_attribute(enum_kind_id(name), 0));
}

/// Add a named enum attribute carrying an integer `value` at `loc` — the valued form for attributes
/// whose payload is a packed encoding (`memory(...)`, `captures(...)`). Same fail-loud kind-id
/// resolution as [`add_enum_attr`].
fn add_valued_enum_attr<'c>(
    ctx: &'c Context,
    f: FunctionValue<'c>,
    loc: inkwell::attributes::AttributeLoc,
    name: &str,
    value: u64,
) {
    f.add_attribute(loc, ctx.create_enum_attribute(enum_kind_id(name), value));
}

/// The `captures(none)` parameter-attribute payload. LLVM 22 replaced the old `nocapture` parameter
/// attribute with the richer `captures(...)` attribute (`llvm/Support/ModRef.h`): a `CaptureInfo` is
/// a pair of `CaptureComponents` (Other, Ret) encoded as `(u32(Other) << 4) | u32(Ret)`
/// (`CaptureInfo::toIntValue`). `captures(none)` = `CaptureInfo::none()` = both components
/// `CaptureComponents::None` (`== 0`), so its encoded value is `(0 << 4) | 0` = `0`. Verified against
/// the LLVM 22.1.8 headers (`/usr/lib/llvm-22/include/llvm/Support/ModRef.h`:
/// `enum class CaptureComponents { None = 0, ... }` and `uint32_t CaptureInfo::toIntValue()`), and
/// empirically: `LLVMCreateEnumAttribute(ctx, kind("captures"), 0)` prints `captures(none)`.
///
/// Emitting the modern attribute directly — rather than the removed `nocapture` name, which resolves
/// to kind id `0` and only *looked* correct because inkwell's LLVM-22 printer renders it as the
/// un-reparseable `none` shorthand — keeps the textual `emit-llvm | llvm-as-22` round-trip valid
/// (the printer now emits the canonical `captures(none)` spelling `llvm-as-22` accepts).
const CAPTURES_NONE: u64 = 0;

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
/// `malloc`; the `*_begin`, `builder_new`, and `array_builder_new` handle allocators = one
/// `Box::new`) — so it additionally gets `nofree`.
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

// ---------------------------------------------------------------------------------------------
// M13 Slice 5A — contract attributes for the opaque `align_rt_*` runtime declarations.
//
// LLVM cannot see the Rust bodies behind the `align_rt_*` declares in `align_runtime`, and these
// calls never inline, so its FunctionAttrs pass infers nothing for them: every runtime call is an
// opaque "reads+writes all memory, may not return, may unwind" barrier. `RT_CONTRACT_ATTRS`
// hand-annotates the small, individually-audited set whose contract is *provable from the runtime
// body* (each entry cites `crates/align_runtime/src/lib.rs` and its verified reasoning). A wrong
// attribute here is a silent miscompile, so the fail-safe default is NO attribute: any symbol
// absent from the table gets nothing. Applied once by `apply_rt_contract_attrs`, after every
// runtime declare exists (all `align_rt_*` are external declares — never Align definitions).
// ---------------------------------------------------------------------------------------------

/// LLVM 19's packed `MemoryEffects` bitmask (`llvm/IR/ModRef.h`): 2 bits per location —
/// `ArgMem = 0`, `InaccessibleMem = 1`, `Other = 2` — holding a `ModRefInfo` (`Ref = 1` reads,
/// `Mod = 2` writes). `memory(argmem: read)` = ArgMem:Ref only = `1 << (0 * 2)` = `1`, every other
/// location `NoModRef`. This encoding is version-sensitive (a location was added after LLVM 19;
/// re-verified to print canonically on LLVM 22 at the 2026-07-12 upgrade), so
/// `rt_contract_attrs_pin_encoding_and_curation` pins the emitted attribute's textual form —
/// an LLVM upgrade that shifts the bits fails that test loudly instead of silently miscompiling.
const MEM_ARGMEM_READ: u64 = 1;

/// The contract of one runtime declaration: which function-level valueless enum attributes it
/// carries, its LLVM `memory(...)` effect bitmask (if any), and which pointer parameters are
/// `readonly` + `captures(none)` (the function only reads through them and never stores/returns
/// them).
struct RtContract {
    fn_attrs: &'static [&'static str],
    memory: Option<u64>,
    read_ptr_params: &'static [u32],
}

/// Function-level attributes shared by the provably pure-finite reader fns: they always return
/// (a loop bounded by the input length — never abort, never spin), never free reachable memory, and
/// never synchronize with other threads (any internal atomic is `monotonic`-or-weaker, permitted
/// under `nosync`). Paired with `memory(argmem: read)` where the body touches ONLY argument memory.
const PURE_READ: &[&str] = &["willreturn", "nofree", "nosync"];

/// The contract for `sym` (a full `align_rt_*` symbol), or `None` (fail-safe: no attributes). Every
/// entry was read against the runtime body; the justification cites the function and why each
/// attribute holds. Curated conservatively — when a fn touched non-argument memory (a feature-detect
/// cache) `memory(...)` is deliberately withheld even though the fn is otherwise pure.
fn rt_contract(sym: &str) -> Option<RtContract> {
    // `hash64`/`hash128` (align_runtime `align_rt_hash64`/`_hash128`): `wyhash` over `safe_slice`d
    // argument bytes — pure arithmetic, no allocation, no global reads/writes (`WY_SEED`/`WY_SECRET`
    // are `const`, baked into code, not memory), always terminates, returns a scalar/`{u64,u64}` and
    // never stores the pointer. → `memory(argmem: read)` + pure-finite + `readonly captures(none)` on ptr.
    // `str_eq`/`str_cmp`/`eq_ignore_case`/`starts_with`/`ends_with`: slice compare (`==`/`.cmp()`/
    // `eq_ignore_ascii_case`) over `safe_slice`d argument bytes → `memcmp`-class, argument memory
    // only, no globals, no feature-detect, returns an `i32`/`i64`. Same treatment; both ptr params
    // (`0` and `2`) are `readonly captures(none)`.
    let memcmp_class = |params: &'static [u32]| RtContract {
        fn_attrs: PURE_READ,
        memory: Some(MEM_ARGMEM_READ),
        read_ptr_params: params,
    };
    // `utf8_valid` (`align_rt_utf8_valid` → `validate_utf8`) and the `memchr::memmem`-backed
    // `str_contains`/`str_find`/`str_rfind`: pure-finite readers, BUT their dispatch runs
    // `is_x86_feature_detected!` / memchr's runtime CPU-feature detection, which reads (and, on the
    // first call, writes) a process-global cache — non-argument memory. So `memory(...)` is WITHHELD
    // (an `argmem: read` / `read` claim would be a lie the first time). They keep the pure-finite
    // flags and `readonly captures(none)` params (all still true), just no memory-effects attribute.
    let feature_detect_reader = |params: &'static [u32]| RtContract {
        fn_attrs: PURE_READ,
        memory: None,
        read_ptr_params: params,
    };
    match sym {
        "align_rt_hash64" | "align_rt_hash128" => Some(memcmp_class(&[0])),
        "align_rt_str_eq"
        | "align_rt_str_cmp"
        | "align_rt_str_eq_ignore_case"
        | "align_rt_str_starts_with"
        | "align_rt_str_ends_with" => Some(memcmp_class(&[0, 2])),
        "align_rt_utf8_valid" => Some(feature_detect_reader(&[0])),
        "align_rt_str_contains" | "align_rt_str_find" | "align_rt_str_rfind" => {
            Some(feature_detect_reader(&[0, 2]))
        }
        // `str_finder_find` (doc-13 §6.6): searches a prepared plan. For a *multi-byte* needle the
        // vector/Two-Way setup and feature detection all happened in `str_finder_new`
        // (`is_available()` → `with_pair()` → `Searcher::new`), so `Finder::find` would touch only
        // argument memory. BUT for a **one-byte** needle `memchr 2.8.2` routes `Finder::find` through
        // `searcher_kind_one_byte` → `crate::memchr(byte, haystack)`, whose `unsafe_ifunc!` reads —
        // and on the first call writes — a process-global `static FN: AtomicPtr<()>` dispatch cache
        // (non-argument memory). That is the SAME feature-detect-at-find-time lie this file forbids
        // for the one-shot `str_contains`/`str_find`. So `memory(...)` is WITHHELD: `finder_find` gets
        // the `feature_detect_reader` contract (pure-finite flags + `readonly captures(none)` on both
        // pointers, no memory-effects attribute) — identical to `str_find`. The plan-reuse win is the
        // point of hoisting; the memory-effects upgrade was an optional extra and is falsified here.
        "align_rt_str_finder_find" => Some(feature_detect_reader(&[0, 1])),
        // The abort family (`align_rt_bounds_fail`/`len_mismatch_fail`/`range_fail`/`div_fail`/
        // `process_exit`/`process_abort`): each is `-> !` in the runtime — it prints then calls
        // `std::process::abort()` / `_exit` / `std::process::exit`, none of which return. MIR already
        // emits `unreachable` after the call, so `noreturn` is free hygiene (it lets LLVM drop the
        // fall-through and mark the path cold). No pointer params; no memory claim needed.
        "align_rt_bounds_fail"
        | "align_rt_len_mismatch_fail"
        | "align_rt_range_fail"
        | "align_rt_utf8_boundary_fail"
        | "align_rt_div_fail"
        | "align_rt_alloc_size_fail"
        | "align_rt_process_exit"
        | "align_rt_process_abort" => Some(RtContract {
            fn_attrs: &["noreturn"],
            memory: None,
            read_ptr_params: &[],
        }),
        _ => None,
    }
}

/// The `align_rt_*` symbols whose bodies the `--rt-lto` bitcode artifact defines (M14 Slice 2):
/// the `memcmp`-class fast-path string primitives the probe measured as an LTO win (`str_eq` 2.1×).
/// `str_cmp` is deliberately EXCLUDED (it regressed ~0.72× under post-link reoptimization);
/// `utf8_valid` is excluded from v1 (SIMD body + non-argument feature-detect memory, unmeasured).
/// This set is the single seam for two structural facts: (a) `apply_rt_contract_attrs` skips
/// hand-curating these declares when `--rt-lto` is on (they will gain real bodies whose attributes
/// LLVM infers — a stale curated attr shadowing a visible body is a latent miscompile), and (b)
/// `link_in_rt_lto` sheds exactly [`rt_contract`]'s attrs from each once its body is merged in.
const RT_LTO_GUARDED: &[&str] = &[
    "align_rt_str_eq",
    "align_rt_str_starts_with",
    "align_rt_str_ends_with",
    "align_rt_str_eq_ignore_case",
];

/// Whether `sym` is a `--rt-lto` guarded symbol (see [`RT_LTO_GUARDED`]).
fn is_rt_lto_guarded(sym: &str) -> bool {
    RT_LTO_GUARDED.contains(&sym)
}

/// Apply `RT_CONTRACT_ATTRS` to every runtime declaration in the module. Runs once, after all
/// `align_rt_*` declares are created. Filters to `align_rt_`-prefixed *declarations* (zero basic
/// blocks) so it can never touch an Align-generated definition; a symbol without a contract entry is
/// left untouched (the fail-safe default).
///
/// `skip_guarded` = the `--rt-lto` path: the guarded set ([`RT_LTO_GUARDED`]) is left UN-annotated
/// because [`link_in_rt_lto`] is about to give each a real body (curating a declare we then define
/// would just shadow LLVM's body-derived inference — the probe's `rt_contract` split). Off (`false`,
/// the flag-off default and the fallback re-annotate) → curate every contracted declare, today's
/// behavior. The decision is made by the caller AFTER probing the baked bitcode (probe-then-annotate),
/// so an unparseable artifact re-annotates correctly instead of silently dropping the contract.
fn apply_rt_contract_attrs<'c>(ctx: &'c Context, module: &Module<'c>, skip_guarded: bool) {
    for f in module.get_functions() {
        let name = f.get_name();
        let Ok(name) = name.to_str() else { continue };
        if !name.starts_with("align_rt_") || f.count_basic_blocks() != 0 {
            continue;
        }
        if skip_guarded && is_rt_lto_guarded(name) {
            continue;
        }
        let Some(c) = rt_contract(name) else { continue };
        for a in c.fn_attrs {
            add_enum_attr(ctx, f, inkwell::attributes::AttributeLoc::Function, a);
        }
        if let Some(mem) = c.memory {
            add_valued_enum_attr(ctx, f, inkwell::attributes::AttributeLoc::Function, "memory", mem);
        }
        for &p in c.read_ptr_params {
            add_enum_attr(ctx, f, inkwell::attributes::AttributeLoc::Param(p), "readonly");
            // `captures(none)`: the pointer's address/provenance never escapes (LLVM 22's `captures`
            // replaces the removed `nocapture` — see `CAPTURES_NONE`). Emitting the modern attribute
            // directly (value 0) keeps the `emit-llvm | llvm-as-22` textual round-trip valid.
            add_valued_enum_attr(
                ctx,
                f,
                inkwell::attributes::AttributeLoc::Param(p),
                "captures",
                CAPTURES_NONE,
            );
        }
    }
}

/// Strip exactly [`rt_contract`]`(sym)`'s hand-curated attributes from a now-body-carrying runtime
/// function `f` (the `--rt-lto` safety-net shed). Once a body is visible, LLVM's own FunctionAttrs
/// pass infers memory/`nofree`/etc. from the real code; a stale hand-curated attr shadowing that
/// body is a latent miscompile (the `rt_contract` split, M14 Slice 2). We remove ONLY this symbol's
/// curated attrs — never a blanket all-attr wipe, which would also strip rustc's body-derived attrs
/// and *weaken* the result. `remove_enum_attribute` is a no-op when the attribute is absent, so this
/// is safe even though `apply_rt_contract_attrs(skip_guarded=true)` already withheld them on the
/// declare (the merged definition could still carry them from a future annotate path).
fn shed_rt_contract_attrs<'c>(f: FunctionValue<'c>, c: &RtContract) {
    use inkwell::attributes::AttributeLoc;
    for a in c.fn_attrs {
        f.remove_enum_attribute(AttributeLoc::Function, enum_kind_id(a));
    }
    if c.memory.is_some() {
        f.remove_enum_attribute(AttributeLoc::Function, enum_kind_id("memory"));
    }
    for &p in c.read_ptr_params {
        f.remove_enum_attribute(AttributeLoc::Param(p), enum_kind_id("readonly"));
        f.remove_enum_attribute(AttributeLoc::Param(p), enum_kind_id("captures"));
    }
}

/// Parse the baked `--rt-lto` bitcode (`build.rs` → `str_prims.bc`, `include_bytes!`) into a module
/// in `ctx`. The probe half of "probe-then-annotate": the caller decides whether to skip curating
/// the guarded declares based on whether THIS succeeds, so an unparseable artifact never leaves the
/// contract silently dropped.
///
/// inkwell 0.9's `create_from_memory_range_copy` *asserts* a trailing nul byte and hands LLVM
/// `len - 1` bytes (the nul is treated as one-past-the-end) — a raw `include_bytes!` bitcode slice
/// has no such nul, so passing it directly silently drops the final bitcode byte ("Invalid bitcode
/// signature"). We copy the bitcode into a `Vec` with an appended nul so the constructor copies
/// exactly `bitcode.len()` real bytes.
fn parse_rt_lto_module<'c>(ctx: &'c Context, bitcode: &[u8]) -> Result<Module<'c>, String> {
    let mut with_nul = Vec::with_capacity(bitcode.len() + 1);
    with_nul.extend_from_slice(bitcode);
    with_nul.push(0);
    let buf = MemoryBuffer::create_from_memory_range_copy(&with_nul, "align_rt_lto");
    Module::parse_bitcode_from_buffer(&buf, ctx).map_err(|e| e.to_string())
}

/// Link the parsed `--rt-lto` runtime module into the program `module` in place, then normalize the
/// merged bodies (M14 Slice 2). Steps: (0) compare the parsed runtime module's datalayout against
/// the program's — a mismatch means blindly overwriting it (the old unconditional
/// `rt.set_data_layout`) would silently relayout the runtime's types/offsets to a target it was not
/// baked for, a latent miscompile; instead this falls back exactly like an unparseable artifact
/// (loud diagnostic, guarded declares re-curated, no merge — see [`probe_rt_lto`]); (1) match the
/// incoming module's triple to the program's (cosmetic — `link_in_module` does not check it) and the
/// datalayout too (now known equal); (2) `link_in_module` (the definitions replace the program's
/// external `align_rt_*` declares); (3) for every `align_rt_*` function that now has a body, shed its
/// `rt_contract` attrs ([`shed_rt_contract_attrs`]) and set it `internal` DIRECTLY (never the
/// internalize pass — the `{main} ∪ --export` roots model stays untouched, and no runtime symbol is
/// externally defined, so there is no duplicate-external vs the `.a` at final link); (4) `verify` the
/// merged module. Runs on the RAW module, BEFORE the single `run_opt_pipeline` — never a second opt
/// run (the probe's double-opt is what regressed `str_cmp`).
///
/// On the datalayout-mismatch fallback (step 0), no merge happens and this re-curates the guarded
/// declares itself (`apply_rt_contract_attrs(ctx, module, false)`) before returning `Ok(())` — the
/// caller passed `rt_lto_skip_guarded = true` to `build_module` on the strength of the bitcode merely
/// parsing, so without this the guarded declares would be left permanently un-curated.
fn link_in_rt_lto<'c>(ctx: &'c Context, module: &Module<'c>, rt: Module<'c>) -> Result<(), CodegenError> {
    // (0) A parsed-but-wrong-target artifact is the same class of compiler build defect as an
    // unparseable one — fail loud, fall back, never force a mismatched layout onto the runtime's IR.
    // Compared via a bool first (rather than holding the `Ref<DataLayout>` borrows across the branch
    // below) so `rt`'s data-layout `RefCell` is free again before `rt.set_data_layout` needs it
    // mutably on the match path.
    let want = module.get_data_layout();
    let matches = rt.get_data_layout().as_str() == want.as_str();
    if !matches {
        eprintln!(
            "alignc: --rt-lto disabled: baked runtime bitcode datalayout ({:?}) does not match the \
             program's ({:?}); falling back to the runtime staticlib. This is a compiler build \
             defect, not a problem with your program.",
            rt.get_data_layout().as_str(),
            want.as_str()
        );
        apply_rt_contract_attrs(ctx, module, false);
        return Ok(());
    }
    // (1) Match target so the linker never complains about a triple mismatch (cosmetic — datalayout
    // is already confirmed equal above).
    rt.set_triple(&module.get_triple());
    rt.set_data_layout(&want);
    // (2) Merge. On success `rt` is consumed; its `align_rt_*` definitions override the program's
    // external declares of the same name.
    module
        .link_in_module(rt)
        .map_err(|e| CodegenError::Target(format!("--rt-lto: linking runtime bitcode failed: {e}")))?;
    // (3) Normalize every runtime symbol that now carries a body.
    for f in module.get_functions() {
        let name = f.get_name();
        let Ok(name) = name.to_str() else { continue };
        if !name.starts_with("align_rt_") || f.count_basic_blocks() == 0 {
            continue;
        }
        if let Some(c) = rt_contract(name) {
            shed_rt_contract_attrs(f, &c);
        }
        mark_internal(f);
    }
    // (4) A merged module that does not verify is a compiler bug (our own baked bitcode), not a user
    // error — surface it loudly rather than emitting a broken object.
    module
        .verify()
        .map_err(|e| CodegenError::Target(format!("--rt-lto: merged module failed verification: {e}")))
}

/// Apply the size-optimization function attributes for the build `profile` to every Align-generated
/// *definition* in the module. `small` gets `optsize`; `tiny` gets `optsize` + `minsize` (`minsize`
/// implies `optsize`, but clang `-Oz` emits both explicitly, so we do too). All other profiles are a
/// no-op — the speed profiles carry no size attrs.
///
/// Filters to definitions (`count_basic_blocks() > 0`), so it never touches an `align_rt_*`
/// declaration: the target set is completely disjoint from [`apply_rt_contract_attrs`], which only
/// touches declarations. Called on the object path (after `build_module`, before the opt pipeline);
/// the diagnostic lenses never run it, keeping their IR shape profile-independent.
fn apply_size_attrs<'c>(ctx: &'c Context, module: &Module<'c>, profile: Profile) {
    let names: &[&str] = match profile {
        Profile::Small => &["optsize"],
        Profile::Tiny => &["optsize", "minsize"],
        _ => return,
    };
    for f in module.get_functions() {
        if f.count_basic_blocks() == 0 {
            continue; // a declaration (`align_rt_*`, an extern) — never a size-attr target
        }
        for &name in names {
            add_enum_attr(ctx, f, inkwell::attributes::AttributeLoc::Function, name);
        }
    }
}

struct FnGen<'c, 'a> {
    ctx: &'c Context,
    module: &'a Module<'c>,
    builder: &'a Builder<'c>,
    funcs: &'a HashMap<String, FunctionValue<'c>>,
    /// The SysV ABI plan for each `extern "C"` symbol — to coerce call arguments (view→data
    /// pointer, `layout(C)` struct→register slots) and reconstruct a by-value struct return.
    extern_abi: &'a HashMap<String, ExternAbi>,
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
    /// Conservative whole-MIR proof for builder headers whose pointer never leaves its defining
    /// function/local. Each selected local gets one reusable 64-byte entry alloca; new/load value
    /// maps choose the stack init and consuming runtime entry points.
    stack_header_slots: HashSet<Slot>,
    stack_header_new_values: HashMap<ValueId, Slot>,
    stack_header_load_values: HashMap<ValueId, Slot>,
    stack_headers: HashMap<Slot, inkwell::values::PointerValue<'c>>,
    /// Template builders are compiler-internal and unconditionally consumed before their result
    /// value is published, so each dynamic template expression gets its own reusable entry buffer.
    stack_template_values: HashSet<ValueId>,
    stack_template_headers: HashMap<ValueId, inkwell::values::PointerValue<'c>>,
    blocks: Vec<BasicBlock<'c>>,
    /// Per-`map_into`-loop scoped-`noalias` metadata, keyed by the MIR loop's scope id: the
    /// `(in_list, out_list)` scope lists (each a one-scope MDNode) built lazily on first use. The
    /// `in`/`out` scopes share a fresh disjoint domain per id, so the loop's source load
    /// (`!alias.scope in`, `!noalias out`) and `dst` store (`!alias.scope out`, `!noalias in`) are
    /// proven not to overlap. Globally unique per (function, id) so distinct loops never collide.
    alias_scopes: HashMap<u32, (inkwell::values::MetadataValue<'c>, inkwell::values::MetadataValue<'c>)>,
    /// Opt-in debug info (`explain-opt`): the module's `DebugInfoBuilder` and this function's
    /// `DISubprogram`, plus a fallback line. `None` in a normal build → [`FnGen::set_line`] is a
    /// no-op and no `DILocation`s are emitted.
    dibuilder: Option<&'a DebugInfoBuilder<'c>>,
    subprogram: Option<DISubprogram<'c>>,
    /// The function's fallback source line (first non-zero statement line, else 1) — set on every
    /// body instruction so a statement with no recorded line still carries a `DILocation` (LLVM's
    /// verifier requires one on inlinable calls in a function that has debug info).
    fn_line: u32,
}

fn is_builder_header_ty(ty: Ty) -> bool {
    matches!(ty, Ty::Builder | Ty::ArrayBuilder(_))
}

#[derive(Default)]
struct StackHeaderPlan {
    slots: HashSet<Slot>,
    new_values: HashMap<ValueId, Slot>,
    load_values: HashMap<ValueId, Slot>,
    template_values: HashSet<ValueId>,
}

/// Select only directly-bound builder locals whose header cannot escape. This deliberately rejects
/// aliases, user-call arguments/returns, lifted-lambda threading, and any derived builder-typed SSA
/// value. False negatives retain the boxed ABI; a false positive would return a stack pointer, so
/// the proof stays intentionally small and auditable.
fn stack_header_plan(f: &Function) -> StackHeaderPlan {
    let mut new_defs = HashMap::<ValueId, Ty>::new();
    let mut load_defs = HashMap::<ValueId, Slot>::new();
    let mut template_values = HashSet::<ValueId>::new();
    for block in &f.blocks {
        for stmt in &block.stmts {
            if let Stmt::Let(v, rv) = stmt {
                match rv {
                    Rvalue::BuilderNew { .. } | Rvalue::ArrayBuilderNew { .. } => {
                        new_defs.insert(*v, f.value_tys[*v as usize]);
                    }
                    Rvalue::Load(slot) if is_builder_header_ty(f.slots[*slot as usize]) => {
                        load_defs.insert(*v, *slot);
                    }
                    Rvalue::Template(..) => {
                        // The header exists only inside `gen_template`: no MIR operand can name it,
                        // and both arena/non-arena paths consume it before publishing the string.
                        template_values.insert(*v);
                    }
                    _ => {}
                }
            }
        }
    }

    let mut stores = HashMap::<Slot, Vec<ValueId>>::new();
    let mut bad = HashSet::<Slot>::new();
    for block in &f.blocks {
        for stmt in &block.stmts {
            if let Stmt::Store(slot, op) = stmt
                && is_builder_header_ty(f.slots[*slot as usize])
            {
                match op {
                    Operand::Value(v) if new_defs.get(v) == Some(&f.slots[*slot as usize]) => {
                        stores.entry(*slot).or_default().push(*v);
                    }
                    _ => {
                        bad.insert(*slot);
                    }
                }
            }
        }
    }

    let mut slots: HashSet<Slot> = stores.keys().copied().collect();
    slots.retain(|slot| !bad.contains(slot));
    // One freshly-created pointer may initialize exactly one local exactly once. Re-storing or
    // aliasing it rejects every destination rather than trying to prove mutually-exclusive lives.
    let mut owner = HashMap::<ValueId, Slot>::new();
    for (&slot, values) in &stores {
        for &value in values {
            if let Some(other) = owner.insert(value, slot) {
                bad.insert(slot);
                bad.insert(other);
            }
        }
    }

    fn reject_header_operand(
        op: &Operand,
        load_defs: &HashMap<ValueId, Slot>,
        new_owner: &HashMap<ValueId, Slot>,
        bad: &mut HashSet<Slot>,
    ) {
        if let Operand::Value(v) = op {
            if let Some(slot) = load_defs.get(v) {
                bad.insert(*slot);
            }
            if let Some(slot) = new_owner.get(v) {
                bad.insert(*slot);
            }
        }
    }
    fn allow_loaded(
        op: &Operand,
        load_defs: &HashMap<ValueId, Slot>,
        new_owner: &HashMap<ValueId, Slot>,
        allowed: &mut HashSet<ValueId>,
        bad: &mut HashSet<Slot>,
    ) {
        if let Operand::Value(v) = op {
            if load_defs.contains_key(v) {
                allowed.insert(*v);
            }
            // Audited runtime operations may use a pointer only after it has been loaded from its
            // owning slot; a direct use of the new SSA value would create a second live handle.
            if let Some(slot) = new_owner.get(v) {
                bad.insert(*slot);
            }
        }
    }
    let mut allowed_loads = HashSet::<ValueId>::new();

    for block in &f.blocks {
        for stmt in &block.stmts {
            match stmt {
                Stmt::Store(slot, Operand::Value(value)) if owner.get(value) == Some(slot) => {}
                Stmt::Store(_, op) => reject_header_operand(op, &load_defs, &owner, &mut bad),
                Stmt::Let(v, rv) => {
                    // Any builder-typed transform other than a direct new/load can carry a pointer
                    // across a call/branch/return boundary. Reject every candidate of that type.
                    if is_builder_header_ty(f.value_tys[*v as usize])
                        && !matches!(rv, Rvalue::BuilderNew { .. } | Rvalue::ArrayBuilderNew { .. } | Rvalue::Load(_))
                    {
                        let ty = f.value_tys[*v as usize];
                        for slot in stores.keys().copied().filter(|s| f.slots[*s as usize] == ty) {
                            bad.insert(slot);
                        }
                    }
                    match rv {
                        Rvalue::BuilderWriteStr(b, _)
                        | Rvalue::BuilderWriteInt(b, _)
                        | Rvalue::BuilderWriteBool(b, _)
                        | Rvalue::BuilderWriteChar(b, _)
                        | Rvalue::BuilderWriteFloat(b, _)
                        | Rvalue::BuilderToString(b) => {
                            allow_loaded(b, &load_defs, &owner, &mut allowed_loads, &mut bad)
                        }
                        Rvalue::BuilderWriteStrIntStr(b, _, _, _) => {
                            allow_loaded(b, &load_defs, &owner, &mut allowed_loads, &mut bad)
                        }
                        Rvalue::WriterWriteBuilder(_, b) | Rvalue::FsWriteFileBuilder { builder: b, .. } => {
                            allow_loaded(b, &load_defs, &owner, &mut allowed_loads, &mut bad);
                        }
                        Rvalue::ArrayBuilderPush { builder, .. }
                        | Rvalue::ArrayBuilderPushStr { builder, .. }
                        | Rvalue::ArrayBuilderAppend { builder, .. }
                        | Rvalue::ArrayBuilderBuild { builder } => {
                            allow_loaded(builder, &load_defs, &owner, &mut allowed_loads, &mut bad);
                        }
                        Rvalue::Call(_, args) => {
                            for op in args {
                                reject_header_operand(op, &load_defs, &owner, &mut bad);
                            }
                        }
                        // A hoisted `str_finder` plan (doc-13 §6.6) never holds a builder header; its
                        // needle/plan/haystack operands are `str` views / an opaque plan pointer.
                        // Audit them anyway (a header can never be one, so this only ever passes) so
                        // these do not hit the fail-closed wildcard and needlessly reject unrelated
                        // stack-header candidates in the same function.
                        Rvalue::StrFinderNew { needle } => {
                            reject_header_operand(needle, &load_defs, &owner, &mut bad);
                        }
                        Rvalue::StrFinderFind { plan, haystack } => {
                            reject_header_operand(plan, &load_defs, &owner, &mut bad);
                            reject_header_operand(haystack, &load_defs, &owner, &mut bad);
                        }
                        Rvalue::CallIndirect { callee, args, .. } => {
                            reject_header_operand(callee, &load_defs, &owner, &mut bad);
                            for op in args {
                                reject_header_operand(op, &load_defs, &owner, &mut bad);
                            }
                        }
                        Rvalue::Closure { captures, .. } => {
                            for op in captures {
                                reject_header_operand(op, &load_defs, &owner, &mut bad);
                            }
                        }
                        Rvalue::Use(op) => reject_header_operand(op, &load_defs, &owner, &mut bad),
                        Rvalue::BuilderNew { .. }
                        | Rvalue::ArrayBuilderNew { .. }
                        | Rvalue::Load(_)
                        | Rvalue::StrLit(_)
                        | Rvalue::ConstArray { .. }
                        | Rvalue::StrClone(_)
                        | Rvalue::SliceLen(_)
                        | Rvalue::Bin(..)
                        | Rvalue::Cast { .. } => {}
                        // This is intentionally fail-closed. A new or unaudited rvalue might retain
                        // a builder operand inside an aggregate even when its result is not itself
                        // builder-typed. Rejecting every candidate in the function is a harmless
                        // optimization false negative; silently accepting it could make a stack
                        // header escape. Add a narrower arm above only after auditing all operands.
                        _ => bad.extend(slots.iter().copied()),
                    }
                }
                _ => {}
            }
        }
        if let Term::Return(Some(op)) = &block.term {
            reject_header_operand(op, &load_defs, &owner, &mut bad);
        }
    }

    // A load not consumed by one of the audited non-retaining operations above is an unknown use.
    for (&value, &slot) in &load_defs {
        if slots.contains(&slot) && !allowed_loads.contains(&value) {
            bad.insert(slot);
        }
    }
    slots.retain(|slot| !bad.contains(slot));

    let mut plan = StackHeaderPlan { slots, template_values, ..StackHeaderPlan::default() };
    for (&slot, values) in &stores {
        if plan.slots.contains(&slot) {
            for &value in values {
                plan.new_values.insert(value, slot);
            }
        }
    }
    for (value, slot) in load_defs {
        if plan.slots.contains(&slot) {
            plan.load_values.insert(value, slot);
        }
    }
    plan
}

impl<'c, 'a> FnGen<'c, 'a> {
    fn err(&self, e: impl std::fmt::Display) -> CodegenError {
        CodegenError::Lowering(e.to_string())
    }

    fn stack_header_slot_for_operand(&self, op: &Operand) -> Option<Slot> {
        match op {
            Operand::Value(v) => self.stack_header_load_values.get(v).copied(),
            _ => None,
        }
    }

    /// The `(in_list, out_list)` scoped-`noalias` metadata lists for `map_into` loop `scope`, built
    /// once per id. `in`/`out` are two scopes sharing a fresh domain; each list is a one-scope
    /// MDNode. A scope node's operand[1] is its domain (`AliasScopeNode::getDomain` reads operand 1),
    /// so both scopes report the same domain and the AA can prove the `dst` store (`alias.scope=out`,
    /// `noalias=in`) never aliases the source load (`alias.scope=in`, `noalias=out`). Every node
    /// carries a globally-unique string (`fn_name.mapinto.id`) so the metadata uniquer keeps this
    /// loop's scopes distinct from every other loop's — required for the disjointness claim to stay
    /// sound if the function is later inlined next to another `map_into`.
    fn alias_scope_lists(&mut self, scope: u32) -> (inkwell::values::MetadataValue<'c>, inkwell::values::MetadataValue<'c>) {
        if let Some(v) = self.alias_scopes.get(&scope) {
            return *v;
        }
        let tag = format!("{}.mapinto.{scope}", self.f.name);
        let domain = self
            .ctx
            .metadata_node(&[self.ctx.metadata_string(&format!("align.domain.{tag}")).into()]);
        let in_scope = self.ctx.metadata_node(&[
            self.ctx.metadata_string(&format!("align.in.{tag}")).into(),
            domain.into(),
        ]);
        let out_scope = self.ctx.metadata_node(&[
            self.ctx.metadata_string(&format!("align.out.{tag}")).into(),
            domain.into(),
        ]);
        let in_list = self.ctx.metadata_node(&[in_scope.into()]);
        let out_list = self.ctx.metadata_node(&[out_scope.into()]);
        self.alias_scopes.insert(scope, (in_list, out_list));
        (in_list, out_list)
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

    /// Branch to the cold allocation-overflow abort when `invalid` is true, then continue codegen
    /// in a fresh success block. The runtime allocator ABI takes a signed i64 byte count, so every
    /// checked size operation must fit `0..=i64::MAX`, not merely the full unsigned i64 domain.
    fn guard_allocation_size(&self, invalid: inkwell::values::IntValue<'c>) -> Result<(), CodegenError> {
        let fail = self.ctx.append_basic_block(self.func, "alloc.size.fail");
        let ok = self.ctx.append_basic_block(self.func, "alloc.size.ok");
        self.builder.build_conditional_branch(invalid, fail, ok).map_err(|e| self.err(e))?;
        self.builder.position_at_end(fail);
        self.builder
            .build_call(self.funcs["alloc_size_fail"], &[], "")
            .map_err(|e| self.err(e))?;
        self.builder.build_unreachable().map_err(|e| self.err(e))?;
        self.builder.position_at_end(ok);
        Ok(())
    }

    /// Checked non-negative `a * b` for an allocator byte count. `umul.with.overflow` catches the
    /// full-width wrap; the signed-negative checks reject a negative logical count and a product
    /// above `i64::MAX`, which the signed allocator ABI would otherwise interpret as non-positive.
    fn checked_allocation_mul(
        &self,
        a: inkwell::values::IntValue<'c>,
        b: inkwell::values::IntValue<'c>,
        name: &str,
    ) -> Result<inkwell::values::IntValue<'c>, CodegenError> {
        let ty = a.get_type();
        let agg = self
            .call_overflow_intrinsic("llvm.umul.with.overflow", ty, a, b)?
            .into_struct_value();
        let product = self.builder.build_extract_value(agg, 0, name).map_err(|e| self.err(e))?.into_int_value();
        let overflow = self.builder.build_extract_value(agg, 1, "alloc.mul.overflow").map_err(|e| self.err(e))?.into_int_value();
        let a_negative = self.builder.build_int_compare(IntPredicate::SLT, a, ty.const_zero(), "alloc.count.negative").map_err(|e| self.err(e))?;
        let product_negative = self.builder.build_int_compare(IntPredicate::SLT, product, ty.const_zero(), "alloc.product.negative").map_err(|e| self.err(e))?;
        let invalid = self.builder.build_or(overflow, a_negative, "alloc.mul.invalid").map_err(|e| self.err(e))?;
        let invalid = self.builder.build_or(invalid, product_negative, "alloc.mul.invalid").map_err(|e| self.err(e))?;
        self.guard_allocation_size(invalid)?;
        Ok(product)
    }

    /// Checked addition for an allocator byte count, including the alignment bump. Inputs are
    /// already non-negative; reject both unsigned wrap and a result above the signed ABI maximum.
    fn checked_allocation_add(
        &self,
        a: inkwell::values::IntValue<'c>,
        b: inkwell::values::IntValue<'c>,
        name: &str,
    ) -> Result<inkwell::values::IntValue<'c>, CodegenError> {
        let ty = a.get_type();
        let agg = self
            .call_overflow_intrinsic("llvm.uadd.with.overflow", ty, a, b)?
            .into_struct_value();
        let sum = self.builder.build_extract_value(agg, 0, name).map_err(|e| self.err(e))?.into_int_value();
        let overflow = self.builder.build_extract_value(agg, 1, "alloc.add.overflow").map_err(|e| self.err(e))?.into_int_value();
        let sum_negative = self.builder.build_int_compare(IntPredicate::SLT, sum, ty.const_zero(), "alloc.sum.negative").map_err(|e| self.err(e))?;
        let invalid = self.builder.build_or(overflow, sum_negative, "alloc.add.invalid").map_err(|e| self.err(e))?;
        self.guard_allocation_size(invalid)?;
        Ok(sum)
    }

    /// Checked byte offset of column `field` while allocating a `soa<Struct>` buffer. This is the
    /// allocation-only counterpart of [`Self::soa_column_offset`]: every product, column end, and
    /// alignment bump must fit the signed allocator byte-size ABI before the buffer is created.
    fn soa_allocation_column_offset(
        &self,
        len: inkwell::values::IntValue<'c>,
        sizes: &[u64],
        field: usize,
    ) -> Result<inkwell::values::IntValue<'c>, CodegenError> {
        if field >= sizes.len() {
            return Err(self.err("soa allocation column index out of bounds"));
        }
        let ty = len.get_type();
        let mut off = ty.const_zero();
        for j in 1..=field {
            let adv = self.checked_allocation_mul(len, ty.const_int(sizes[j - 1], false), "coladv")?;
            let sum = self.checked_allocation_add(off, adv, "colend")?;
            let align = sizes[j];
            off = if align > 1 {
                let bumped = self.checked_allocation_add(sum, ty.const_int(align - 1, false), "colbump")?;
                self.builder.build_and(bumped, ty.const_int(!(align - 1), false), "colalign").map_err(|e| self.err(e))?
            } else {
                sum
            };
        }
        Ok(off)
    }

    /// Byte offset of column `field` within an already allocated `soa<Struct>` column-major buffer of `len` rows:
    /// `start_0 = 0`, `start_j = align_up(start_{j-1} + len*size_{j-1}, size_j)`. Each column is
    /// padded to the field's alignment (= its size for a primitive), so mixed-width columns stay
    /// naturally aligned for any `len`. Shared by [`Rvalue::IndexColumn`] and
    /// [`Stmt::StoreColumn`]; [`Self::soa_allocation_column_offset`] mirrors it with overflow checks
    /// for the allocation-size walk.
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

    /// An 8-byte-aligned entry-block scratch slot of `n` eightbytes (`[n x i64]`), used to coerce a
    /// `layout(C)` struct to/from its SysV register form. Sizing to a multiple of 8 keeps every
    /// per-eightbyte `i64`/`double` load in bounds even when the struct's last eightbyte is only
    /// partially occupied (the padding bytes are ABI-irrelevant).
    fn eightbyte_slot(&self, n: usize) -> Result<inkwell::values::PointerValue<'c>, CodegenError> {
        let arr = self.ctx.i64_type().array_type(n as u32);
        self.alloca_at_entry(arr.into(), "sysv_slot")
    }

    /// The address of eightbyte `i` within an [`FnGen::eightbyte_slot`] — `slot + i * 8` bytes, via
    /// an i64-strided GEP. In bounds by construction (`i < n`).
    fn eightbyte_ptr(&self, slot: inkwell::values::PointerValue<'c>, i: usize) -> Result<inkwell::values::PointerValue<'c>, CodegenError> {
        let idx = self.ctx.i64_type().const_int(i as u64, false);
        unsafe {
            self.builder
                .build_in_bounds_gep(self.ctx.i64_type(), slot, &[idx], "ebp")
                .map_err(|e| self.err(e))
        }
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
        mark_private_helper(thunk);
        let saved = self.builder.get_insert_block();
        // This thunk has no DISubprogram; emitting into it while the outer function's debug location
        // is active would attach a wrong-scope `!dbg` (a verifier error). Clear it while building
        // the thunk, then restore the outer function's fallback location afterward (a no-op without
        // debug info). The thunk needs no locations — the verifier's rule applies only to functions
        // that carry debug info.
        if self.dibuilder.is_some() {
            self.builder.unset_current_debug_location();
        }
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
        // Restore an active debug location for the outer function's subsequent instructions.
        self.set_line(self.fn_line, 0);
        Ok(thunk.as_global_value().as_pointer_value())
    }

    /// Set the builder's current debug location (opt-in). A no-op without debug info. `line == 0`
    /// (a statement with no recorded source line) falls back to the function line, so every body
    /// instruction carries a location — LLVM's verifier requires one on inlinable calls in a
    /// function that has debug info.
    fn set_line(&self, line: u32, col: u32) {
        if let (Some(dib), Some(sp)) = (self.dibuilder, self.subprogram) {
            let (l, c) = if line == 0 { (self.fn_line.max(1), 0) } else { (line, col) };
            let loc = dib.create_debug_location(self.ctx, l, c, sp.as_debug_info_scope(), None);
            self.builder.set_current_debug_location(loc);
        }
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
            // Usually the natural ABI alignment (a no-op vs LLVM's default); for a struct (or fixed
            // struct array) declared `align(N)` it returns `N`, so the stack slot is over-aligned —
            // together with the `set_struct_body` size padding, a `[N x %S]` array's elements all stay
            // over-aligned (`open-questions.md` "`align(N)`"). A per-slot binding override
            // (`align(N) data := [...]`) over-aligns a scalar-array slot the same way — the max of the
            // declared `N` and the natural alignment, so it can only ever *over*-align (never UB).
            let natural = self.type_align(*ty);
            let align = self.f.slot_align.get(i).copied().flatten().map_or(natural, |n| n.max(natural));
            let inst = ptr
                .as_instruction()
                .ok_or_else(|| self.err("alloca did not yield an instruction"))?;
            inst.set_alignment(align).map_err(|e| self.err(e))?;
            self.slots.insert(i as Slot, ptr);
        }
        // Runtime-owned payloads keep their existing representation. Only a proven-nonescaping
        // header uses caller storage: one conservative 64-byte/16-aligned buffer per local, reused
        // across reassignments after the previous header has been consumed or dropped.
        let mut header_slots: Vec<Slot> = self.stack_header_slots.iter().copied().collect();
        header_slots.sort_unstable(); // deterministic IR/object bytes
        for slot in header_slots {
            let ptr = self
                .builder
                .build_alloca(self.ctx.i8_type().array_type(64), &format!("header_{slot}"))
                .map_err(|e| self.err(e))?;
            let inst = ptr
                .as_instruction()
                .ok_or_else(|| self.err("header alloca did not yield an instruction"))?;
            inst.set_alignment(16).map_err(|e| self.err(e))?;
            self.stack_headers.insert(slot, ptr);
        }
        let mut template_values: Vec<ValueId> = self.stack_template_values.iter().copied().collect();
        template_values.sort_unstable();
        for value in template_values {
            let ptr = self
                .builder
                .build_alloca(self.ctx.i8_type().array_type(64), &format!("template_header_{value}"))
                .map_err(|e| self.err(e))?;
            let inst = ptr
                .as_instruction()
                .ok_or_else(|| self.err("template header alloca did not yield an instruction"))?;
            inst.set_alignment(16).map_err(|e| self.err(e))?;
            self.stack_template_headers.insert(value, ptr);
        }

        // Establish a fallback debug location (a no-op without debug info) so every body
        // instruction — including ones from statements with no recorded line — carries one.
        self.set_line(self.fn_line, 0);

        // Emit each block.
        for b in &self.f.blocks {
            let bb = self.blocks[b.id as usize];
            self.builder.position_at_end(bb);
            self.gen_block(b)?;
        }
        Ok(())
    }

    fn gen_block(&mut self, b: &Block) -> Result<(), CodegenError> {
        for (i, s) in b.stmts.iter().enumerate() {
            // Anchor this statement's instructions to its source line (opt-in debug info). Keep the
            // previous location for a statement with no recorded line (`(0, 0)`).
            if let Some(&(line, col)) = b.stmt_lines.get(i)
                && line != 0
            {
                self.set_line(line, col);
            }
            match s {
                Stmt::Let(v, rv) => {
                    let result_ty = self.f.value_tys[*v as usize];
                    if let Some(val) = self.gen_rvalue(*v, rv, result_ty)? {
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
                Stmt::StoreConstArray { slot, elems, elem } => {
                    // Pooled all-constant array binding (doc-13 §8.4, S3): materialize the folded
                    // elements once as a `private unnamed_addr constant [N x elem]` (the #514 rodata
                    // global) and copy it into the fixed `array<T>` slot with a single `llvm.memcpy`,
                    // in place of `n` element stores. For a read-only binding LLVM's MemCpyOpt then
                    // replaces the slot with the constant global directly, eliminating the alloca and
                    // the copy; a `mut` binding (excluded by sema) would keep the writable copy.
                    let dest = self.slots[slot];
                    let (src, _len) = self.const_array_global(elems, *elem);
                    let arr_ty = self.llvm_type(self.f.slots[*slot as usize]).into_array_type();
                    let size = self.target_data.get_store_size(&arr_ty);
                    let align = self.target_data.get_abi_alignment(&arr_ty);
                    self.builder
                        .build_memcpy(dest, align, src, align, self.ctx.i64_type().const_int(size, false))
                        .map_err(|e| self.err(e))?;
                }
                Stmt::DropElem(slot, idx, sid) => {
                    // Free the owned fields of element `idx` before it is overwritten (Slice 4b).
                    let ep = self.elem_ptr(*slot, idx)?;
                    self.drop_struct_fields(ep, *sid)?;
                }
                Stmt::DropElemField(slot, idx, path) => {
                    // Free one owned `string` leaf field of element `idx` before it is overwritten
                    // (4b) — `us[i].name` or a nested `us[i].addr.name`. The leaf field pointer is
                    // built the same way as the store (`elem_field_ptr`, a `[0,idx,*path]` GEP).
                    debug_assert!(matches!(self.f.slots[*slot as usize], Ty::StructArray(..)), "DropElemField on a non-struct-array slot");
                    let fp = self.elem_field_ptr(*slot, idx, path)?;
                    let agg = self.builder.build_load(slice_struct_type(self.ctx), fp, "dropelemfldv").map_err(|e| self.err(e))?.into_struct_value();
                    let ptr = self.builder.build_extract_value(agg, 0, "dropelemfldptr").map_err(|e| self.err(e))?;
                    self.builder.build_call(self.funcs["free"], &[ptr.into()], "").map_err(|e| self.err(e))?;
                }
                Stmt::StoreElemField(slot, idx, path, op) => {
                    let ep = self.elem_field_ptr(*slot, idx, path)?;
                    let val = self.operand(op);
                    self.builder.build_store(ep, val).map_err(|e| self.err(e))?;
                }
                Stmt::StoreElemFieldPtr { base, index, path, struct_id, value } => {
                    // `base[index].f0.f1.… <- value` for an owned dynamic `array<Struct>` view — the
                    // write dual of `Rvalue::IndexFieldPtr`: extract the buffer pointer from the
                    // `{ptr,len}` aggregate and GEP `%Struct, ptr, index, *pfield(path)` (one struct
                    // GEP level per path segment, each through the logical→physical map).
                    let agg = self.operand(base).into_struct_value();
                    let buf = self.builder.build_extract_value(agg, 0, "aosptr").map_err(|e| self.err(e))?.into_pointer_value();
                    let st = self.struct_types[*struct_id as usize];
                    let idx = self.operand(index).into_int_value();
                    // `[index]` reaches element `index` (stride `st`); each physical field index then
                    // descends one struct level to the leaf being written.
                    let mut indices = vec![idx];
                    for pidx in self.phys_field_indices(*struct_id, path) {
                        indices.push(self.ctx.i32_type().const_int(pidx as u64, false));
                    }
                    let ep = unsafe {
                        self.builder
                            .build_in_bounds_gep(st, buf, &indices, "aosfieldst")
                            .map_err(|e| self.err(e))?
                    };
                    let val = self.operand(value);
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
                Stmt::PtrStoreNoalias { ptr, index, value, scope } => {
                    // `dst[i] <- val` for a `map_into` loop: like `PtrStore`, plus the loop's `out`
                    // alias scope so the vectorizer knows it can't overlap the (`in`-scoped) source
                    // load. `alias.scope = {out}`, `noalias = {in}`.
                    let p = self.operand(ptr).into_pointer_value();
                    let idx = self.operand(index).into_int_value();
                    let val = self.operand(value);
                    let ep = unsafe {
                        self.builder
                            .build_in_bounds_gep(val.get_type(), p, &[idx], "ptrstore")
                            .map_err(|e| self.err(e))?
                    };
                    let st = self.builder.build_store(ep, val).map_err(|e| self.err(e))?;
                    let (in_list, out_list) = self.alias_scope_lists(*scope);
                    let scope_kind = self.ctx.get_kind_id("alias.scope");
                    let noalias_kind = self.ctx.get_kind_id("noalias");
                    st.set_metadata(out_list, scope_kind).map_err(|_| self.err("set alias.scope"))?;
                    st.set_metadata(in_list, noalias_kind).map_err(|_| self.err("set noalias"))?;
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
                    let z: BasicValueEnum = if matches!(ty, Ty::Builder | Ty::StrFinder | Ty::Writer | Ty::Reader | Ty::Buffer | Ty::ArrayBuilder(_) | Ty::CliCommand | Ty::CliParsed | Ty::TcpConn | Ty::TcpListener | Ty::UdpSocket | Ty::Child | Ty::File | Ty::HttpRequest | Ty::HttpResponse | Ty::HttpClient | Ty::HttpServer | Ty::HttpRequestCtx | Ty::ResponseBuilder | Ty::HttpStream) {
                        // A builder / writer / reader / buffer / cli / tcp_conn / tcp_listener / udp_socket handle slot holds a bare (nullable) handle pointer.
                        self.ctx.ptr_type(AddressSpace::default()).const_null().into()
                    } else if matches!(ty, Ty::StructArray(..)) {
                        // A fixed array of a Move struct: zero the whole `[N x %Struct]` so every
                        // element's owned fields read {null,0} until constructed — its per-element
                        // `Drop` then frees nulls on an unwritten element (no-op). (Slice 4a.)
                        self.llvm_type(ty).into_array_type().const_zero().into()
                    } else if payload_is_move(ty) || matches!(ty, Ty::Tuple(_) | Ty::Struct(_) | Ty::Enum(_) | Ty::DictEncoded(..)) {
                        // Zero the whole aggregate so each owned field/element reads {null,0}. A Move
                        // struct is zeroed wholesale here; its recursive `Drop` then frees nulls on an
                        // unconstructed / moved-out path (no-op) — see `drop_struct_fields`. A Move enum
                        // (J2) zeroes to tag 0 + null payloads, so its tag-switched `Drop` frees null
                        // (no-op) on an unconstructed / moved-out path — see `drop_enum`.
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
                    // Null one owned field of a struct slot after a partial field move: a `string`
                    // `{ptr,len}` field (`n := u.name`), or a **Move**-enum field whose payload a
                    // `match m.content { … }` moved out (J3). Zero the field's own type so the struct's
                    // recursive `Drop` frees null there — a `{ptr,len}` slice for a `string`, the whole
                    // `{ tag, payloads }` aggregate for an enum (tag → 0, every payload ptr null → the
                    // tag-switched `drop_enum` frees null on every arm).
                    let Ty::Struct(sid) = self.f.slots[*slot as usize] else {
                        unreachable!("NullStructField on a non-struct slot");
                    };
                    let field_ptr = self
                        .builder
                        .build_struct_gep(self.struct_types[sid as usize], self.slots[slot], self.pfield(sid, *idx), "nullstructfld")
                        .map_err(|e| self.err(e))?;
                    let zero: inkwell::values::BasicValueEnum = match self.structs[sid as usize].fields[*idx as usize].ty {
                        Ty::Enum(eid) => self.enum_types[eid as usize].const_zero().into(),
                        _ => slice_struct_type(self.ctx).const_zero().into(),
                    };
                    self.builder.build_store(field_ptr, zero).map_err(|e| self.err(e))?;
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
                        let free = if self.stack_header_slots.contains(slot) {
                            "builder_free_stack"
                        } else {
                            "builder_free"
                        };
                        self.builder
                            .build_call(self.funcs[free], &[p.into()], "")
                            .map_err(|e| self.err(e))?;
                    } else if ty == Ty::StrFinder {
                        // A hoisted repeated-needle plan (doc-13 §6.6): free the boxed searcher
                        // (null-safe — a never-initialised synthetic-owner slot reads null). The
                        // drop-flag machinery guards this so it runs exactly once on every exit path.
                        let p = self
                            .builder
                            .build_load(self.ctx.ptr_type(AddressSpace::default()), self.slots[slot], "dropf")
                            .map_err(|e| self.err(e))?;
                        self.builder
                            .build_call(self.funcs["str_finder_free"], &[p.into()], "")
                            .map_err(|e| self.err(e))?;
                    } else if let Ty::ArrayBuilder(elem) = ty {
                        // An unfrozen `array_builder<T>`: free its storage + header. A `string` element
                        // builder deep-frees each pushed-not-frozen string first (the same
                        // `free_string_array`-class helper); a scalar builder frees the flat storage.
                        // Both are null-safe (a moved-out / never-grown slot drops harmlessly — the
                        // slot was nulled at `build`'s move site).
                        let stack = self.stack_header_slots.contains(slot);
                        let free_fn = if elem == align_sema::Scalar::String && stack {
                            "array_builder_free_strings_stack"
                        } else if elem == align_sema::Scalar::String {
                            "array_builder_free_strings"
                        } else if stack {
                            "array_builder_free_stack"
                        } else {
                            "array_builder_free"
                        };
                        let p = self
                            .builder
                            .build_load(self.ctx.ptr_type(AddressSpace::default()), self.slots[slot], "dropab")
                            .map_err(|e| self.err(e))?;
                        self.builder
                            .build_call(self.funcs[free_fn], &[p.into()], "")
                            .map_err(|e| self.err(e))?;
                    } else if let Some(free_fn) = handle_free_fn(ty) {
                        // A bare Move **handle**: a writer flushes + closes; a reader closes; a buffer /
                        // cli / http handle frees; a tcp_conn / tcp_listener / udp_socket closes its
                        // socket fd; a `child` reaps its pid. Each runtime `*_free` is null-safe (a
                        // moved-out / never-initialised slot drops harmlessly). One source of truth
                        // with the struct-field drop (`handle_free_fn`).
                        let p = self
                            .builder
                            .build_load(self.ctx.ptr_type(AddressSpace::default()), self.slots[slot], "droph")
                            .map_err(|e| self.err(e))?;
                        self.builder
                            .build_call(self.funcs[free_fn], &[p.into()], "")
                            .map_err(|e| self.err(e))?;
                    } else if payload_is_move(ty) {
                        // An Option/Result owning a Move payload: free each owned payload field
                        // (null-safe — the inactive arm reads {null,0}/null, and a moved-out aggregate
                        // was nulled at the move site). A `reader`/`writer` payload is a bare handle
                        // pointer closed by its own `*_free`; every other Move payload is a `{ptr,len}`
                        // whose buffer pointer is `free`d.
                        let aty = self.llvm_type(ty).into_struct_type();
                        let agg = self
                            .builder
                            .build_load(aty, self.slots[slot], "drop")
                            .map_err(|e| self.err(e))?
                            .into_struct_value();
                        for idx in move_payload_fields(ty) {
                            let field = self
                                .builder
                                .build_extract_value(agg, idx, "droppl")
                                .map_err(|e| self.err(e))?;
                            match payload_field_scalar(ty, idx) {
                                Some(Scalar::Reader) | Some(Scalar::Writer) | Some(Scalar::Buffer) | Some(Scalar::File) | Some(Scalar::CliParsed) | Some(Scalar::TcpConn) | Some(Scalar::TcpListener) | Some(Scalar::UdpSocket) | Some(Scalar::Child) | Some(Scalar::HttpResponse) | Some(Scalar::HttpServer) | Some(Scalar::HttpRequestCtx) | Some(Scalar::HttpStream) => {
                                    // The field is the handle pointer itself; each `*_free` is null-safe
                                    // (the inactive arm / a moved-out aggregate reads a null handle).
                                    let free_fn = match payload_field_scalar(ty, idx) {
                                        Some(Scalar::Writer) => "io_writer_free",
                                        Some(Scalar::Reader) => "io_reader_free",
                                        Some(Scalar::Buffer) => "buffer_free",
                                        Some(Scalar::File) => "io_file_free",
                                        Some(Scalar::TcpConn) => "tcp_conn_free",
                                        Some(Scalar::TcpListener) => "tcp_listener_free",
                                        Some(Scalar::UdpSocket) => "udp_socket_free",
                                        Some(Scalar::Child) => "child_free",
                                        Some(Scalar::HttpResponse) => "http_resp_free",
                                        Some(Scalar::HttpServer) => "http_server_free",
                                        Some(Scalar::HttpRequestCtx) => "http_ctx_free",
                                        Some(Scalar::HttpStream) => "http_stream_free",
                                        _ => "cli_parsed_free",
                                    };
                                    self.builder
                                        .build_call(self.funcs[free_fn], &[field.into_pointer_value().into()], "")
                                        .map_err(|e| self.err(e))?;
                                }
                                Some(Scalar::DynArray(align_sema::PrimScalar::String)) => {
                                    // `Result<array<string>, Error>` (`fs.read_dir`): the field is a
                                    // `{ptr,len}` owned string-array — deep free (each element buffer,
                                    // then the header), null-safe.
                                    let sv = field.into_struct_value();
                                    let ptr = self.builder.build_extract_value(sv, 0, "dropplptr").map_err(|e| self.err(e))?;
                                    let len = self.builder.build_extract_value(sv, 1, "droppllen").map_err(|e| self.err(e))?;
                                    self.builder
                                        .build_call(self.funcs["free_string_array"], &[ptr.into(), len.into()], "")
                                        .map_err(|e| self.err(e))?;
                                }
                                _ => {
                                    let ptr = self.builder.build_extract_value(field.into_struct_value(), 0, "dropplptr").map_err(|e| self.err(e))?;
                                    self.builder
                                        .build_call(self.funcs["free"], &[ptr.into()], "")
                                        .map_err(|e| self.err(e))?;
                                }
                            }
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
                    } else if matches!(ty, Ty::DynArray(Scalar::String)) {
                        // An owned `array<string>` (`fs.read_dir`): each element owns its own buffer, so
                        // the `Drop` is a **deep** free — `align_rt_free_string_array(base, len)` frees
                        // every element buffer, then the header. Distinct from a scalar `array<T>` (one
                        // buffer, the else below). Null-safe (a moved-out `{null,0}` frees nothing).
                        let agg = self
                            .builder
                            .build_load(slice_struct_type(self.ctx), self.slots[slot], "dropstrarr")
                            .map_err(|e| self.err(e))?
                            .into_struct_value();
                        let ptr = self.builder.build_extract_value(agg, 0, "dropstrarrptr").map_err(|e| self.err(e))?;
                        let len = self.builder.build_extract_value(agg, 1, "dropstrarrlen").map_err(|e| self.err(e))?;
                        self.builder
                            .build_call(self.funcs["free_string_array"], &[ptr.into(), len.into()], "")
                            .map_err(|e| self.err(e))?;
                    } else if matches!(ty, Ty::DynResponseArray) {
                        // An owned `array<response>` (`cl.get_many`): each element is an owned `http
                        // response` handle, so the `Drop` is a **deep** free —
                        // `align_rt_free_response_array(base, len)` frees every handle, then the header.
                        // Null-safe (a moved-out `{null,0}` frees nothing). The response dual of the
                        // `array<string>` deep-free above.
                        let agg = self
                            .builder
                            .build_load(slice_struct_type(self.ctx), self.slots[slot], "droprsparr")
                            .map_err(|e| self.err(e))?
                            .into_struct_value();
                        let ptr = self.builder.build_extract_value(agg, 0, "droprsparrptr").map_err(|e| self.err(e))?;
                        let len = self.builder.build_extract_value(agg, 1, "droprsparrlen").map_err(|e| self.err(e))?;
                        self.builder
                            .build_call(self.funcs["free_response_array"], &[ptr.into(), len.into()], "")
                            .map_err(|e| self.err(e))?;
                    } else if let Ty::Enum(eid) = ty {
                        // A Move sum type (J2): tag-switched drop — free the live variant's owned
                        // `array<Struct>` payload buffer (null-safe on a moved-out / unconstructed slot).
                        self.drop_enum(self.slots[slot], eid)?;
                    } else if matches!(ty, Ty::DynStructArray(eid, _) if struct_is_move(eid, self.structs, self.enums)) {
                        // An owned `array<Move-struct>` standalone local (J3b) — e.g.
                        // `ms: array<Message> := json.decode(...)`. Deep-free each element's owned buffers
                        // then the AoS, via the same helper the struct-*field* drop uses (a flat free
                        // would leak every element's owned buffer). Null-safe (a moved-out `{null,0}`
                        // frees nothing and iterates 0 times).
                        let Ty::DynStructArray(eid, _) = ty else { unreachable!() };
                        self.deep_free_struct_array(self.slots[slot], eid)?;
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
                    // Free the buffer of an owned `{ptr, len}` value (an unbound temporary). An
                    // `array<string>` temporary would need the deep free, but none is produced today
                    // (`fs.read_dir` is always bound via `?`), so the shallow free stays correct here.
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
    fn gen_rvalue(&mut self, result_id: ValueId, rv: &Rvalue, result_ty: Ty) -> Result<Option<BasicValueEnum<'c>>, CodegenError> {
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
                let agg = self.builder.build_insert_value(sty.get_poison(), new_ptr, 0, "colptr").map_err(|e| self.err(e))?.into_struct_value();
                self.builder.build_insert_value(agg, len, 1, "collen").map_err(|e| self.err(e))?.into_struct_value().into()
            }
            Rvalue::OptionSome(op) => {
                let Ty::Option(s) = result_ty else {
                    return Err(self.err("Some result is not an Option"));
                };
                let oty = option_struct_type(self.ctx, s, self.struct_types, self.enum_types);
                let payload = self.operand(op);
                let tag = self.ctx.i8_type().const_int(1, false);
                // Start zeroed (not poison): an owned (Move) payload's drop frees the payload field
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
            Rvalue::IndexField(slot, idx, path) => {
                let ep = self.elem_field_ptr(*slot, idx, path)?;
                let ty = abi_type(self.ctx, result_ty, self.struct_types, self.enum_types);
                self.builder.build_load(ty, ep, "idxfld").map_err(|e| self.err(e))?
            }
            // Build `<n x elem>` via an insertelement chain over a poison vector (M6).
            Rvalue::MakeVec { elems, elem, n } => {
                let vty = vec_llvm_ty(self.ctx, *elem, *n).into_vector_type();
                let mut acc = vty.get_poison();
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
            // Reduce a mask to `bool` = true iff any lane is set (OR-fold), the vector div/rem guard.
            Rvalue::MaskAny { mask, n } => {
                let m = self.operand(mask).into_vector_value();
                self.horizontal_or(m, *n)?.into()
            }
            // `s.load(i)` — `<n x T>` load from `&buf[i]`. Default alignment is the element's (the GEP
            // yields only an element-aligned pointer, so the load must NOT assume the wider vector
            // alignment). `align = Some(N)` is a MIR-proven over-alignment (a whole borrow of an
            // `align(N)` binding at an N-aligned offset — `proven_vec_load_align`); use the larger of
            // it and the element alignment.
            Rvalue::VecLoad { slice, index, elem, n, align } => {
                let sv = self.operand(slice).into_struct_value();
                let buf = self.builder.build_extract_value(sv, 0, "vlbuf").map_err(|e| self.err(e))?.into_pointer_value();
                let index = self.operand(index).into_int_value();
                let elem_lt = scalar_type(self.ctx, *elem, self.struct_types, self.enum_types);
                let ep = unsafe {
                    self.builder.build_in_bounds_gep(elem_lt, buf, &[index], "vloadgep").map_err(|e| self.err(e))?
                };
                let vty = vec_llvm_ty(self.ctx, *elem, *n).into_vector_type();
                let loaded = self.builder.build_load(vty, ep, "vload").map_err(|e| self.err(e))?;
                let elem_align = self.type_align(*elem);
                let load_align = align.map_or(elem_align, |n| n.max(elem_align));
                loaded
                    .into_vector_value()
                    .as_instruction()
                    .ok_or_else(|| self.err("vector load is not an instruction"))?
                    .set_alignment(load_align)
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
                let mut acc = st.get_poison();
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
                // Build the tuple aggregate by inserting each element into a poison struct.
                let st = self.tuple_types[*tuple_id as usize];
                let mut agg = st.get_poison();
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
                    .build_insert_value(sty.get_poison(), ptr0, 0, "slcptr")
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
                let bytes = self.checked_allocation_mul(count_v, elem_bytes, "bytes")?;
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
                let bytes = self.checked_allocation_mul(count_v, elem_bytes, "bytes")?;
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
                let last_off = self.soa_allocation_column_offset(len_v, &sizes, last)?;
                let last_bytes = self.checked_allocation_mul(len_v, i64t.const_int(sizes[last], false), "lastcol")?;
                let total = self.checked_allocation_add(last_off, last_bytes, "soabytes")?;
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
                    .build_insert_value(sty.get_poison(), p, 0, "arrptr")
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
                    let mut spec_val = spec_ty.get_poison();
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
                let agg = self.builder.build_insert_value(ty.get_poison(), s, 0, "encsrc").map_err(|e| self.err(e))?.into_struct_value();
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
                    .build_insert_value(sty.get_poison(), out_buf, 0, "pmptr")
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
                    .build_insert_value(sty.get_poison(), ptr, 0, "strptr")
                    .map_err(|e| self.err(e))?
                    .into_struct_value();
                self.builder
                    .build_insert_value(agg, len, 1, "strlen")
                    .map_err(|e| self.err(e))?
                    .into_struct_value()
                    .into()
            }
            Rvalue::ConstArray { elems, elem } => {
                // Build the `{ &rodata, len }` slice view over the per-unit private constant global.
                let (ptr, len) = self.const_array_global(elems, *elem);
                let sty = slice_struct_type(self.ctx);
                let agg = self
                    .builder
                    .build_insert_value(sty.get_poison(), ptr, 0, "captr")
                    .map_err(|e| self.err(e))?
                    .into_struct_value();
                self.builder
                    .build_insert_value(agg, len, 1, "calen")
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
            Rvalue::StrFinderNew { needle } => {
                // Split the needle `{ptr,len}` and build the hoisted plan (doc-13 §6.6). Allocator-
                // class call (`align_rt_str_finder_new`); the returned handle is a bare `ptr`.
                let ne = self.operand(needle).into_struct_value();
                let np = self.builder.build_extract_value(ne, 0, "fnp").map_err(|e| self.err(e))?;
                let nl = self.builder.build_extract_value(ne, 1, "fnl").map_err(|e| self.err(e))?;
                self.builder
                    .build_call(self.funcs["str_finder_new"], &[np.into(), nl.into()], "finder")
                    .map_err(|e| self.err(e))?
                    .try_as_basic_value()
                    .basic()
                    .expect("str_finder_new returns a pointer")
            }
            Rvalue::StrFinderFind { plan, haystack } => {
                // Search `haystack` with a prepared plan: `align_rt_str_finder_find(plan, hptr, hlen)`
                // returns an `i64` index or `-1` (MIR compares `>= 0`). The plan handle is a bare
                // `ptr`; split the haystack `{ptr,len}` view. No CPU-feature detection here (it
                // happened in `finder_new`), so the declaration carries `memory(argmem: read)`.
                let plan_ptr = self.operand(plan);
                let ha = self.operand(haystack).into_struct_value();
                let hp = self.builder.build_extract_value(ha, 0, "fhp").map_err(|e| self.err(e))?;
                let hl = self.builder.build_extract_value(ha, 1, "fhl").map_err(|e| self.err(e))?;
                self.builder
                    .build_call(self.funcs["str_finder_find"], &[plan_ptr.into(), hp.into(), hl.into()], "finderfind")
                    .map_err(|e| self.err(e))?
                    .try_as_basic_value()
                    .basic()
                    .expect("str_finder_find returns i64")
            }
            Rvalue::BuilderNew { capacity } => {
                // Open a builder with a null arena: the finished `string` is heap-owned
                // (`into_string` copies into a fresh malloc'd buffer), not arena-tied. `capacity`
                // pre-sizes the backing buffer so appends don't reallocate (0 = default).
                let null = self.ctx.ptr_type(AddressSpace::default()).const_null();
                let cap = self.operand(capacity);
                if let Some(slot) = self.stack_header_new_values.get(&result_id).copied() {
                    let header = self.stack_headers[&slot];
                    return Ok(Some(
                        self.builder
                            .build_call(
                                self.funcs["builder_init_stack"],
                                &[header.into(), null.into(), cap.into()],
                                "builder.stack",
                            )
                            .map_err(|e| self.err(e))?
                            .try_as_basic_value()
                            .basic()
                            .expect("builder_init_stack returns a pointer"),
                    ));
                }
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
                // object is freed by the runtime, or consumed in caller stack storage when the
                // whole-MIR noescape proof selected that local.
                let b = self.operand(bld).into();
                let finish = if self.stack_header_slot_for_operand(bld).is_some() {
                    "builder_into_string_stack"
                } else {
                    "builder_into_string"
                };
                self.builder
                    .build_call(self.funcs[finish], &[b], "tostr")
                    .map_err(|e| self.err(e))?
                    .try_as_basic_value()
                    .basic()
                    .expect("builder_into_string returns a {ptr,len}")
            }
            Rvalue::Template(pieces, arena) => self.gen_template(result_id, pieces, arena.as_ref())?,
            // `schema` is a cache-invalidation fingerprint printed into the MIR; codegen rebuilds the
            // descriptor table directly from `struct_id`, so it does not read `schema` here.
            Rvalue::JsonDecode { struct_id, input, out, .. } => self.gen_json_decode(*struct_id, input, *out)?,
            Rvalue::JsonDecodeArray { elem, input, out } => self.gen_json_decode_array(*elem, input, *out)?,
            Rvalue::JsonDecodeScalar { scalar, input, out } => self.gen_json_decode_scalar(*scalar, input, *out)?,
            Rvalue::JsonDecodeStructArray { struct_id, input, out, .. } => self.gen_json_decode_struct_array(*struct_id, input, *out)?,
            Rvalue::JsonDecodeSoa { struct_id, input, out, arena, .. } => self.gen_json_decode_soa(*struct_id, input, *out, arena)?,
            Rvalue::JsonDecodeUnion { enum_id, input, out, .. } => self.gen_json_decode_union(*enum_id, input, *out)?,
            // json.doc (J4). `get`/`at` are void (the runtime writes the child handle into `out`).
            Rvalue::JsonDoc { input, arena, out } => self.gen_json_doc(input, arena, *out)?,
            Rvalue::JsonDocKind { doc } => self.gen_json_doc_kind(doc, result_ty)?,
            Rvalue::JsonDocGet { doc, key, out } => {
                self.gen_json_doc_get(doc, key, *out)?;
                return Ok(None);
            }
            Rvalue::JsonDocAt { doc, index, out } => {
                self.gen_json_doc_at(doc, index, *out)?;
                return Ok(None);
            }
            Rvalue::JsonDocAsStr { doc, out } => self.gen_json_doc_as_str(doc, *out)?,
            Rvalue::JsonDocAsScalar { scalar, doc, out } => self.gen_json_doc_as_scalar(*scalar, doc, *out)?,
            Rvalue::JsonDocLen { doc } => {
                let (tape, node) = self.split_doc(doc)?;
                self.builder
                    .build_call(self.funcs["json_doc_len"], &[tape.into(), node.into()], "jlen")
                    .map_err(|e| self.err(e))?
                    .try_as_basic_value().basic().expect("json_doc_len returns i64")
            }
            Rvalue::JsonDocKey { doc, index, out } => self.gen_json_doc_key(doc, index, *out)?,
            Rvalue::JsonDocElems { doc, arena, out } => {
                self.gen_json_doc_elems(doc, arena, *out)?;
                return Ok(None);
            }
            // json.scan (J5). The scanner value IS the input `{ptr,len}` view — no allocation.
            Rvalue::JsonScanNew { input } => self.operand(input),
            // One streaming step: decode the next object at `*cursor` into the `row` slot, return the
            // i32 status (0 = row / 1 = done / 2 = malformed).
            Rvalue::JsonScanNext { scanner, struct_id, cursor, row, .. } => self.gen_json_scan_next(*struct_id, scanner, *cursor, *row)?,
            Rvalue::FsReadFile { path, out } => self.gen_fs_read_file(path, *out)?,
            // fs.open / fs.create — write the handle into `out`, return an i32 errno-status.
            Rvalue::ReaderOpen { path, out } => self.gen_open_handle("io_reader_open", path, *out)?,
            Rvalue::WriterCreate { path, out } => self.gen_open_handle("io_writer_create", path, *out)?,
            // All A4 `file` rvalues (create_rw/open_rw + pread/pwrite/len) go through ONE
            // `#[inline(never)]` helper, so `gen_rvalue` gains a single tiny arm rather than five inline
            // bodies — `gen_rvalue` is depth-recursive (via operand materialization), so keeping its
            // frame flat preserves the expr-depth budget (the #296 lesson, mirroring MIR's dispatcher).
            Rvalue::FileCreateRw { .. } | Rvalue::FileOpenRw { .. }
            | Rvalue::FilePread { .. } | Rvalue::FilePwrite { .. } | Rvalue::FileLen { .. } => self.gen_file_rvalue(rv)?,
            Rvalue::ReaderStdin => self
                .builder
                .build_call(self.funcs["io_reader_stdin"], &[], "stdin")
                .map_err(|e| self.err(e))?
                .try_as_basic_value().basic().expect("io_reader_stdin returns a pointer"),
            Rvalue::WriterStd { fd, buffered } => {
                let fd = self.ctx.i32_type().const_int(*fd as u64, true);
                let buffered = self.ctx.i32_type().const_int(*buffered as u64, false);
                self.builder
                    .build_call(self.funcs["io_writer_std"], &[fd.into(), buffered.into()], "wstd")
                    .map_err(|e| self.err(e))?
                    .try_as_basic_value().basic().expect("io_writer_std returns a pointer")
            }
            Rvalue::ReaderRead(r, buf) => {
                let rp = self.operand(r).into();
                let bp = self.operand(buf).into();
                self.builder
                    .build_call(self.funcs["io_reader_read"], &[rp, bp], "read")
                    .map_err(|e| self.err(e))?
                    .try_as_basic_value().basic().expect("io_reader_read returns i64")
            }
            // The three A7 line-read rvalues (`.buffered()` / `.read_line()` / `.as_str()`) go
            // through ONE `#[inline(never)]` helper, so `gen_rvalue` gains a single tiny arm rather
            // than three inline bodies — `gen_rvalue` is depth-recursive (via operand
            // materialization), so keeping its frame flat preserves the expr-depth budget (the #296
            // lesson, mirroring the file / array_builder rvalue dispatchers).
            Rvalue::ReaderBuffered(_) | Rvalue::ReaderReadLine(..) | Rvalue::BytesAsStr { .. } => {
                return self.gen_reader_line_rvalue(rv);
            }
            Rvalue::IoCopy(r, w) => {
                let rp = self.operand(r).into();
                let wp = self.operand(w).into();
                self.builder
                    .build_call(self.funcs["io_copy"], &[rp, wp], "copy")
                    .map_err(|e| self.err(e))?
                    .try_as_basic_value().basic().expect("io_copy returns i64")
            }
            Rvalue::WriterWrite(w, s) => {
                let wp = self.operand(w).into();
                let agg = self.operand(s).into_struct_value();
                let ptr = self.builder.build_extract_value(agg, 0, "wptr").map_err(|e| self.err(e))?;
                let len = self.builder.build_extract_value(agg, 1, "wlen").map_err(|e| self.err(e))?;
                self.builder
                    .build_call(self.funcs["io_writer_write"], &[wp, ptr.into(), len.into()], "wr")
                    .map_err(|e| self.err(e))?
                    .try_as_basic_value().basic().expect("io_writer_write returns i32")
            }
            Rvalue::WriterWriteBuilder(w, bld) => {
                let wp = self.operand(w).into();
                let bp = self.operand(bld).into();
                self.builder
                    .build_call(self.funcs["io_writer_write_builder"], &[wp, bp], "wrb")
                    .map_err(|e| self.err(e))?
                    .try_as_basic_value().basic().expect("io_writer_write_builder returns i32")
            }
            Rvalue::WriterFlush(w) => {
                let wp = self.operand(w).into();
                self.builder
                    .build_call(self.funcs["io_writer_flush"], &[wp], "wflush")
                    .map_err(|e| self.err(e))?
                    .try_as_basic_value().basic().expect("io_writer_flush returns i32")
            }
            Rvalue::BufferNew(cap) => {
                let cap = self.operand(cap).into();
                self.builder
                    .build_call(self.funcs["buffer_new"], &[cap], "buf")
                    .map_err(|e| self.err(e))?
                    .try_as_basic_value().basic().expect("buffer_new returns a pointer")
            }
            Rvalue::BufferBytes(buf) => {
                // The runtime writes the `{ptr,len}` view into a stack slot; load it back.
                let bp = self.operand(buf).into();
                let slot = self.alloca_at_entry(slice_struct_type(self.ctx).into(), "bytesslot")?;
                self.builder
                    .build_call(self.funcs["buffer_bytes"], &[bp, slot.into()], "")
                    .map_err(|e| self.err(e))?;
                self.builder.build_load(slice_struct_type(self.ctx), slot, "bytes").map_err(|e| self.err(e))?
            }
            Rvalue::BufferLen(buf) => {
                let bp = self.operand(buf).into();
                self.builder
                    .build_call(self.funcs["buffer_len"], &[bp], "buflen")
                    .map_err(|e| self.err(e))?
                    .try_as_basic_value().basic().expect("buffer_len returns i64")
            }
            // `bytes.<scalar>_<le|be>(off)` — inline binary scalar read. The byte address is a plain
            // (non-`inbounds`) GEP into the bounds-checked `slice<u8>` view; the load is alignment-1
            // (the offset is arbitrary). A `be` read byte-swaps into host order (single-byte reads
            // carry `be:false`, so `bswap` never applies to `i8`). A float loads its bits then
            // bit-casts. Host little-endianness is assumed (x86-64 / aarch64), as elsewhere.
            Rvalue::BytesRead { bytes, offset, scalar, be } => {
                let (ptr, _len) = self.split_str(bytes)?;
                let ptr = ptr.into_pointer_value();
                let off = self.operand(offset).into_int_value();
                let addr = unsafe {
                    self.builder.build_gep(self.ctx.i8_type(), ptr, &[off], "byteaddr").map_err(|e| self.err(e))?
                };
                let is_float = matches!(scalar, Ty::Float(_));
                let load_int_ty = if is_float {
                    match scalar { Ty::Float(FloatTy { bits: 32 }) => self.ctx.i32_type(), _ => self.ctx.i64_type() }
                } else {
                    int_type(self.ctx, *scalar)
                };
                let loaded = self.builder.build_load(load_int_ty, addr, "rawbits").map_err(|e| self.err(e))?;
                loaded
                    .as_instruction_value()
                    .ok_or_else(|| self.err("bytes read load is not an instruction"))?
                    .set_alignment(1)
                    .map_err(|_| self.err("set bytes-read load alignment"))?;
                // llvm.bswap is only defined for widths > 8; sema never builds an 8-bit `_be`
                // read, but guard defensively so a future gap cannot become an LLVM crash.
                let bits = if *be && load_int_ty.get_bit_width() > 8 {
                    self.call_intrinsic("llvm.bswap", &[load_int_ty.into()], &[loaded.into()])?.into_int_value()
                } else {
                    loaded.into_int_value()
                };
                if is_float {
                    self.builder.build_bit_cast(bits, float_type(self.ctx, *scalar), "asfloat").map_err(|e| self.err(e))?
                } else {
                    bits.into()
                }
            }
            // `buf.put_<scalar>_<le|be>(v)` — reinterpret `v` to its raw i64 bits (a float bit-casts;
            // a narrower int zero-extends, so its low `width` bytes are the two's-complement value),
            // then hand (bits, width, be) to the runtime, which appends `width` bytes in order.
            Rvalue::BufferPut { buffer, value, scalar, be } => {
                let bp = self.operand(buffer).into();
                let width = match scalar {
                    Ty::Int(IntTy { bits, .. }) | Ty::Float(FloatTy { bits }) => u64::from(*bits) / 8,
                    _ => return Err(self.err("buffer put scalar must be a fixed-width int/float")),
                };
                let i64t = self.ctx.i64_type();
                let bits = if matches!(scalar, Ty::Float(_)) {
                    let fv = self.operand(value).into_float_value();
                    let int_bits = match scalar { Ty::Float(FloatTy { bits: 32 }) => self.ctx.i32_type(), _ => i64t };
                    let as_int = self.builder.build_bit_cast(fv, int_bits, "fbits").map_err(|e| self.err(e))?.into_int_value();
                    self.builder.build_int_z_extend_or_bit_cast(as_int, i64t, "bits64").map_err(|e| self.err(e))?
                } else {
                    let iv = self.operand(value).into_int_value();
                    self.builder.build_int_z_extend_or_bit_cast(iv, i64t, "bits64").map_err(|e| self.err(e))?
                };
                let width_c = i64t.const_int(width, false);
                let be_c = self.ctx.i32_type().const_int(u64::from(*be), false);
                self.builder
                    .build_call(self.funcs["buffer_put"], &[bp, bits.into(), width_c.into(), be_c.into()], "")
                    .map_err(|e| self.err(e))?;
                return Ok(None);
            }
            // `buf.append(data)` — copy the raw `slice<u8>` bytes onto the growable buffer.
            Rvalue::BufferAppend { buffer, data } => {
                let bp = self.operand(buffer).into();
                let (ptr, len) = self.split_str(data)?;
                self.builder
                    .build_call(self.funcs["buffer_append"], &[bp, ptr.into(), len.into()], "")
                    .map_err(|e| self.err(e))?;
                return Ok(None);
            }
            // `array_builder<T>` (M12 A6) new/push/push_str/append/build go through ONE
            // `#[inline(never)]` dispatcher, so `gen_rvalue` gains a single tiny arm rather than five
            // inline bodies (the #296 expr-depth lesson, mirroring the file-rvalue dispatcher).
            Rvalue::ArrayBuilderNew { .. } | Rvalue::ArrayBuilderPush { .. } | Rvalue::ArrayBuilderPushStr { .. }
            | Rvalue::ArrayBuilderAppend { .. } | Rvalue::ArrayBuilderBuild { .. } => {
                return self.gen_array_builder_rvalue(result_id, rv);
            }
            // fs.write_file — marshal the path `{ptr,len}` and the str/bytes data `{ptr,len}`, return
            // an i32 errno-status.
            Rvalue::FsWriteFile { path, data } => {
                let (p_ptr, p_len) = self.split_str(path)?;
                let (d_ptr, d_len) = self.split_str(data)?;
                self.builder
                    .build_call(self.funcs["fs_write_file"], &[p_ptr.into(), p_len.into(), d_ptr.into(), d_len.into()], "fwf")
                    .map_err(|e| self.err(e))?
                    .try_as_basic_value().basic().expect("fs_write_file returns i32")
            }
            Rvalue::FsWriteFileBuilder { path, builder } => {
                let (p_ptr, p_len) = self.split_str(path)?;
                let bp = self.operand(builder).into();
                self.builder
                    .build_call(self.funcs["fs_write_file_builder"], &[p_ptr.into(), p_len.into(), bp], "fwfb")
                    .map_err(|e| self.err(e))?
                    .try_as_basic_value().basic().expect("fs_write_file_builder returns i32")
            }
            Rvalue::FsExists { path } => {
                let (p_ptr, p_len) = self.split_str(path)?;
                self.builder
                    .build_call(self.funcs["fs_exists"], &[p_ptr.into(), p_len.into()], "fex")
                    .map_err(|e| self.err(e))?
                    .try_as_basic_value().basic().expect("fs_exists returns i32")
            }
            Rvalue::FsRemove { path } => {
                let (p_ptr, p_len) = self.split_str(path)?;
                self.builder
                    .build_call(self.funcs["fs_remove"], &[p_ptr.into(), p_len.into()], "frm")
                    .map_err(|e| self.err(e))?
                    .try_as_basic_value().basic().expect("fs_remove returns i32")
            }
            // fs.read_dir — write the owned array<string> `{ptr,len}` into `out`, return i32 status.
            Rvalue::FsReadDir { path, out } => {
                let out_ptr = self.slots[out];
                self.builder.build_store(out_ptr, slice_struct_type(self.ctx).const_zero()).map_err(|e| self.err(e))?;
                let (p_ptr, p_len) = self.split_str(path)?;
                self.builder
                    .build_call(self.funcs["fs_read_dir"], &[p_ptr.into(), p_len.into(), out_ptr.into()], "frd")
                    .map_err(|e| self.err(e))?
                    .try_as_basic_value().basic().expect("fs_read_dir returns i32")
            }
            // dns.resolve — write the owned array<string> `{ptr,len}` into `out`, return i32 status.
            Rvalue::DnsResolve { host, out } => {
                let out_ptr = self.slots[out];
                self.builder.build_store(out_ptr, slice_struct_type(self.ctx).const_zero()).map_err(|e| self.err(e))?;
                let (h_ptr, h_len) = self.split_str(host)?;
                self.builder
                    .build_call(self.funcs["dns_resolve"], &[h_ptr.into(), h_len.into(), out_ptr.into()], "dnsr")
                    .map_err(|e| self.err(e))?
                    .try_as_basic_value().basic().expect("dns_resolve returns i32")
            }
            // tcp.connect — write the owned tcp_conn handle pointer into `out`, return i32 status.
            Rvalue::TcpConnect { host, port, out } => {
                let out_ptr = self.slots[out];
                self.builder.build_store(out_ptr, self.ctx.ptr_type(AddressSpace::default()).const_null()).map_err(|e| self.err(e))?;
                let (h_ptr, h_len) = self.split_str(host)?;
                let port_v = self.operand(port).into();
                self.builder
                    .build_call(self.funcs["tcp_connect"], &[h_ptr.into(), h_len.into(), port_v, out_ptr.into()], "tconn")
                    .map_err(|e| self.err(e))?
                    .try_as_basic_value().basic().expect("tcp_connect returns i32")
            }
            // c.reader() / c.writer() — borrow an M9 reader/writer over the conn's fd (owns_fd:false).
            Rvalue::ConnReader(c) => {
                let cp = self.operand(c).into();
                self.builder
                    .build_call(self.funcs["tcp_conn_reader"], &[cp], "creader")
                    .map_err(|e| self.err(e))?
                    .try_as_basic_value().basic().expect("tcp_conn_reader returns a pointer")
            }
            Rvalue::ConnWriter(c) => {
                let cp = self.operand(c).into();
                self.builder
                    .build_call(self.funcs["tcp_conn_writer"], &[cp], "cwriter")
                    .map_err(|e| self.err(e))?
                    .try_as_basic_value().basic().expect("tcp_conn_writer returns a pointer")
            }
            // tcp.listen — write the owned tcp_listener handle pointer into `out`, return i32 status.
            Rvalue::TcpListen { host, port, out } => {
                let out_ptr = self.slots[out];
                self.builder.build_store(out_ptr, self.ctx.ptr_type(AddressSpace::default()).const_null()).map_err(|e| self.err(e))?;
                let (h_ptr, h_len) = self.split_str(host)?;
                let port_v = self.operand(port).into();
                self.builder
                    .build_call(self.funcs["tcp_listen"], &[h_ptr.into(), h_len.into(), port_v, out_ptr.into()], "tlisten")
                    .map_err(|e| self.err(e))?
                    .try_as_basic_value().basic().expect("tcp_listen returns i32")
            }
            // l.accept — write the owned accepted tcp_conn handle pointer into `out`, return i32 status.
            Rvalue::TcpAccept { listener, out } => {
                let out_ptr = self.slots[out];
                self.builder.build_store(out_ptr, self.ctx.ptr_type(AddressSpace::default()).const_null()).map_err(|e| self.err(e))?;
                let lp = self.operand(listener).into();
                self.builder
                    .build_call(self.funcs["tcp_accept"], &[lp, out_ptr.into()], "taccept")
                    .map_err(|e| self.err(e))?
                    .try_as_basic_value().basic().expect("tcp_accept returns i32")
            }
            // udp.bind — write the owned udp_socket handle pointer into `out`, return i32 status.
            Rvalue::UdpBind { host, port, out } => {
                let out_ptr = self.slots[out];
                self.builder.build_store(out_ptr, self.ctx.ptr_type(AddressSpace::default()).const_null()).map_err(|e| self.err(e))?;
                let (h_ptr, h_len) = self.split_str(host)?;
                let port_v = self.operand(port).into();
                self.builder
                    .build_call(self.funcs["udp_bind"], &[h_ptr.into(), h_len.into(), port_v, out_ptr.into()], "ubind")
                    .map_err(|e| self.err(e))?
                    .try_as_basic_value().basic().expect("udp_bind returns i32")
            }
            // u.send_to — sendto the byte view `data` to host/port from the socket's fd; return i64
            // (bytes sent, or -(status)). `data` is a {ptr,len} byte view (str/string/slice<u8>).
            Rvalue::UdpSendTo { sock, data, host, port } => {
                let sp = self.operand(sock).into();
                let (d_ptr, d_len) = self.split_str(data)?;
                let (h_ptr, h_len) = self.split_str(host)?;
                let port_v = self.operand(port).into();
                self.builder
                    .build_call(self.funcs["udp_send_to"], &[sp, d_ptr.into(), d_len.into(), h_ptr.into(), h_len.into(), port_v], "usend")
                    .map_err(|e| self.err(e))?
                    .try_as_basic_value().basic().expect("udp_send_to returns i64")
            }
            // u.recv_from — block for one datagram into the buffer; return i64 (bytes received, or
            // -(status)).
            Rvalue::UdpRecvFrom { sock, buffer } => {
                let sp = self.operand(sock).into();
                let bp = self.operand(buffer).into();
                self.builder
                    .build_call(self.funcs["udp_recv_from"], &[sp, bp], "urecv")
                    .map_err(|e| self.err(e))?
                    .try_as_basic_value().basic().expect("udp_recv_from returns i64")
            }
            // process.spawn — fork+execvp; write the owned child handle into `out`, return i32 status.
            // `cmd` is a {ptr,len} str view (the lookup path); `args` is a {ptr,len} of str views (the
            // child's full argv, marshalled to C strings by the runtime).
            Rvalue::ProcessSpawn { cmd, args, out } => {
                let out_ptr = self.slots[out];
                self.builder
                    .build_store(out_ptr, self.ctx.ptr_type(AddressSpace::default()).const_null())
                    .map_err(|e| self.err(e))?;
                let (c_ptr, c_len) = self.split_str(cmd)?;
                let (a_ptr, a_len) = self.split_str(args)?;
                self.builder
                    .build_call(self.funcs["process_spawn"], &[c_ptr.into(), c_len.into(), a_ptr.into(), a_len.into(), out_ptr.into()], "spawn")
                    .map_err(|e| self.err(e))?
                    .try_as_basic_value().basic().expect("process_spawn returns i32")
            }
            // ch.wait — waitpid the child (marking it reaped through the pointer); return i64 (exit
            // code >= 0, or -(status) on a double-wait / waitpid error).
            Rvalue::ChildWait { child } => {
                let cp = self.operand(child).into();
                self.builder
                    .build_call(self.funcs["child_wait"], &[cp], "cwait")
                    .map_err(|e| self.err(e))?
                    .try_as_basic_value().basic().expect("child_wait returns i64")
            }
            // ch.kill — libc kill(pid, sig); return i32 status (0 = ok, AL_INVALID for a bad sig /
            // reaped child, else the mapped errno). `child` is a *Child pointer, `sig` an i64.
            Rvalue::ChildKill { child, sig } => {
                let cp = self.operand(child).into();
                let sv = self.operand(sig).into();
                self.builder
                    .build_call(self.funcs["child_kill"], &[cp, sv], "ckill")
                    .map_err(|e| self.err(e))?
                    .try_as_basic_value().basic().expect("child_kill returns i32")
            }
            // process.exec — execvp in the current process; on success it never returns, so this only
            // returns an i32 status on failure. `cmd` is a {ptr,len} str view (the lookup path); `args`
            // is a {ptr,len} of str views (the new image's full argv, marshalled to C strings).
            Rvalue::ProcessExec { cmd, args } => {
                let (c_ptr, c_len) = self.split_str(cmd)?;
                let (a_ptr, a_len) = self.split_str(args)?;
                self.builder
                    .build_call(self.funcs["process_exec"], &[c_ptr.into(), c_len.into(), a_ptr.into(), a_len.into()], "pexec")
                    .map_err(|e| self.err(e))?
                    .try_as_basic_value().basic().expect("process_exec returns i32")
            }
            // fs.read_file_view — mmap into the arena, write the str view `{ptr,len}` into `out`,
            // return i32 status.
            Rvalue::FsReadFileView { path, arena, out } => {
                let out_ptr = self.slots[out];
                self.builder.build_store(out_ptr, slice_struct_type(self.ctx).const_zero()).map_err(|e| self.err(e))?;
                let (p_ptr, p_len) = self.split_str(path)?;
                let ah = self.operand(arena).into();
                self.builder
                    .build_call(self.funcs["fs_read_file_view"], &[p_ptr.into(), p_len.into(), ah, out_ptr.into()], "frfv")
                    .map_err(|e| self.err(e))?
                    .try_as_basic_value().basic().expect("fs_read_file_view returns i32")
            }
            // fs.read_bytes_view — the binary sibling: same mmap-into-arena call, writing the
            // `slice<u8>` view `{ptr,len}` into `out` (identical layout to the `str` view), return i32.
            Rvalue::FsReadBytesView { path, arena, out } => {
                let out_ptr = self.slots[out];
                self.builder.build_store(out_ptr, slice_struct_type(self.ctx).const_zero()).map_err(|e| self.err(e))?;
                let (p_ptr, p_len) = self.split_str(path)?;
                let ah = self.operand(arena).into();
                self.builder
                    .build_call(self.funcs["fs_read_bytes_view"], &[p_ptr.into(), p_len.into(), ah, out_ptr.into()], "frbv")
                    .map_err(|e| self.err(e))?
                    .try_as_basic_value().basic().expect("fs_read_bytes_view returns i32")
            }
            // std.path — join/normalize return an owned `{ptr,len}`; base/dir/ext a borrowed view.
            Rvalue::PathJoin { a, b } => {
                let (ap, al) = self.split_str(a)?;
                let (bp, bl) = self.split_str(b)?;
                self.builder
                    .build_call(self.funcs["path_join"], &[ap.into(), al.into(), bp.into(), bl.into()], "pjoin")
                    .map_err(|e| self.err(e))?
                    .try_as_basic_value().basic().expect("path_join returns a {ptr,len}")
            }
            Rvalue::PathComponent { kind, path } => {
                let fk = match kind {
                    align_sema::hir::PathComponentKind::Base => "path_base",
                    align_sema::hir::PathComponentKind::Dir => "path_dir",
                    align_sema::hir::PathComponentKind::Ext => "path_ext",
                };
                let (pp, pl) = self.split_str(path)?;
                self.builder
                    .build_call(self.funcs[fk], &[pp.into(), pl.into()], "pcomp")
                    .map_err(|e| self.err(e))?
                    .try_as_basic_value().basic().expect("path component returns a {ptr,len}")
            }
            Rvalue::PathNormalize { path } => {
                let (pp, pl) = self.split_str(path)?;
                self.builder
                    .build_call(self.funcs["path_normalize"], &[pp.into(), pl.into()], "pnorm")
                    .map_err(|e| self.err(e))?
                    .try_as_basic_value().basic().expect("path_normalize returns a {ptr,len}")
            }
            // std.encoding — encode returns an owned `{ptr,len}` string; decode writes a `buffer`
            // handle into `out` and returns an i32 status; utf8_valid returns an i32 (1/0).
            Rvalue::EncodingEncode { kind, data } => {
                let fk = match kind {
                    align_sema::hir::EncodingKind::Base64 => "base64_encode",
                    align_sema::hir::EncodingKind::Base64Url => "base64url_encode",
                    align_sema::hir::EncodingKind::Hex => "hex_encode",
                    align_sema::hir::EncodingKind::Percent => "percent_encode",
                    align_sema::hir::EncodingKind::Form => "form_encode",
                };
                let (dp, dl) = self.split_str(data)?;
                self.builder
                    .build_call(self.funcs[fk], &[dp.into(), dl.into()], "enc")
                    .map_err(|e| self.err(e))?
                    .try_as_basic_value().basic().expect("encode returns a {ptr,len}")
            }
            Rvalue::EncodingDecode { kind, input, out } => {
                let fk = match kind {
                    align_sema::hir::EncodingKind::Base64 => "base64_decode",
                    align_sema::hir::EncodingKind::Base64Url => "base64url_decode",
                    align_sema::hir::EncodingKind::Hex => "hex_decode",
                    align_sema::hir::EncodingKind::Percent => "percent_decode",
                    align_sema::hir::EncodingKind::Form => "form_decode",
                };
                let out_ptr = self.slots[out];
                self.builder
                    .build_store(out_ptr, self.ctx.ptr_type(AddressSpace::default()).const_null())
                    .map_err(|e| self.err(e))?;
                let (ip, il) = self.split_str(input)?;
                self.builder
                    .build_call(self.funcs[fk], &[ip.into(), il.into(), out_ptr.into()], "dec")
                    .map_err(|e| self.err(e))?
                    .try_as_basic_value().basic().expect("decode returns i32 status")
            }
            Rvalue::Utf8Valid { data } => {
                let (dp, dl) = self.split_str(data)?;
                self.builder
                    .build_call(self.funcs["utf8_valid"], &[dp.into(), dl.into()], "u8v")
                    .map_err(|e| self.err(e))?
                    .try_as_basic_value().basic().expect("utf8_valid returns i32")
            }
            // std.crypto — constant_time_equal splits both byte views to `{ptr,len}` and returns an
            // i32 (1/0); random passes the `buffer` handle pointer and returns void (fills in place).
            Rvalue::CryptoCtEqual { a, b } => {
                let (ap, al) = self.split_str(a)?;
                let (bp, bl) = self.split_str(b)?;
                self.builder
                    .build_call(self.funcs["crypto_ct_equal"], &[ap.into(), al.into(), bp.into(), bl.into()], "cteq")
                    .map_err(|e| self.err(e))?
                    .try_as_basic_value().basic().expect("crypto_ct_equal returns i32")
            }
            Rvalue::CryptoRandom { out } => {
                let op = self.operand(out).into();
                self.builder
                    .build_call(self.funcs["crypto_random"], &[op], "")
                    .map_err(|e| self.err(e))?;
                return Ok(None);
            }
            // std.crypto — sha256/sha512 split the data byte view to `{ptr,len}` and return a fresh
            // owned `array<u8>` `{ptr,len}` (the digest), by value like `rng_sample`.
            Rvalue::CryptoHash { algo, data } => {
                let (dp, dl) = self.split_str(data)?;
                let f = match algo {
                    align_sema::hir::HashAlgo::Sha256 => self.funcs["crypto_sha256"],
                    align_sema::hir::HashAlgo::Sha512 => self.funcs["crypto_sha512"],
                };
                self.builder
                    .build_call(f, &[dp.into(), dl.into()], "digest")
                    .map_err(|e| self.err(e))?
                    .try_as_basic_value().basic().expect("crypto sha returns a {ptr,len}")
            }
            // std.crypto — hmac_sha256 splits the key + data byte views to `{ptr,len}` and returns a
            // fresh owned `array<u8>` `{ptr,len}` (the 32-byte tag), by value like the digests.
            Rvalue::CryptoHmac { key, data } => {
                let (kp, kl) = self.split_str(key)?;
                let (dp, dl) = self.split_str(data)?;
                self.builder
                    .build_call(self.funcs["crypto_hmac_sha256"], &[kp.into(), kl.into(), dp.into(), dl.into()], "hmac")
                    .map_err(|e| self.err(e))?
                    .try_as_basic_value().basic().expect("crypto hmac returns a {ptr,len}")
            }
            // std.crypto — hkdf_sha256 splits the salt/ikm/info byte views to `{ptr,len}`; the out
            // handle slot is caller-zeroed (so the Err path frees nothing); the runtime returns an i32
            // status.
            Rvalue::CryptoHkdf { salt, ikm, info, len, out } => {
                let out_ptr = self.slots[out];
                self.builder
                    .build_store(out_ptr, self.ctx.ptr_type(AddressSpace::default()).const_null())
                    .map_err(|e| self.err(e))?;
                let (sp, sl) = self.split_str(salt)?;
                let (ip, il) = self.split_str(ikm)?;
                let (np, nl) = self.split_str(info)?;
                let lv = self.operand(len);
                self.builder
                    .build_call(
                        self.funcs["crypto_hkdf_sha256"],
                        &[sp.into(), sl.into(), ip.into(), il.into(), np.into(), nl.into(), lv.into(), out_ptr.into()],
                        "hkdf",
                    )
                    .map_err(|e| self.err(e))?
                    .try_as_basic_value().basic().expect("hkdf_sha256 returns i32 status")
            }
            // std.crypto (Slice 4) — AEAD. The four byte views (key/nonce/input/aad) split to
            // `{ptr,len}`; the out handle slot is caller-zeroed (so the Err path frees nothing); the
            // runtime entry point is selected from the (cipher, dir) pair. Returns an i32 status.
            Rvalue::CryptoAead { cipher, dir, key, nonce, input, aad, out } => {
                use align_sema::hir::{AeadCipher, AeadDir};
                let fk = match (cipher, dir) {
                    (AeadCipher::Aes256Gcm, AeadDir::Seal) => "crypto_aes_gcm_seal",
                    (AeadCipher::Aes256Gcm, AeadDir::Open) => "crypto_aes_gcm_open",
                    (AeadCipher::ChaCha20Poly1305, AeadDir::Seal) => "crypto_chacha20_poly1305_seal",
                    (AeadCipher::ChaCha20Poly1305, AeadDir::Open) => "crypto_chacha20_poly1305_open",
                };
                let out_ptr = self.slots[out];
                self.builder
                    .build_store(out_ptr, self.ctx.ptr_type(AddressSpace::default()).const_null())
                    .map_err(|e| self.err(e))?;
                let (kp, kl) = self.split_str(key)?;
                let (np, nl) = self.split_str(nonce)?;
                let (ip, il) = self.split_str(input)?;
                let (ap, al) = self.split_str(aad)?;
                self.builder
                    .build_call(
                        self.funcs[fk],
                        &[kp.into(), kl.into(), np.into(), nl.into(), ip.into(), il.into(), ap.into(), al.into(), out_ptr.into()],
                        "aead",
                    )
                    .map_err(|e| self.err(e))?
                    .try_as_basic_value().basic().expect("crypto aead returns i32 status")
            }
            // std.crypto (Slice 5) — argon2id. The password/salt byte views split to `{ptr,len}`; the
            // four i64 tuning knobs pass as scalars; the out handle slot is caller-zeroed (so the Err
            // path frees nothing). The runtime validates the public bounds and returns an i32 status.
            Rvalue::CryptoArgon2(a) => {
                let out_ptr = self.slots[&a.out];
                self.builder
                    .build_store(out_ptr, self.ctx.ptr_type(AddressSpace::default()).const_null())
                    .map_err(|e| self.err(e))?;
                let (pp, pl) = self.split_str(&a.password)?;
                let (sp, sl) = self.split_str(&a.salt)?;
                let m = self.operand(&a.m_cost);
                let t = self.operand(&a.t_cost);
                let p = self.operand(&a.parallelism);
                let l = self.operand(&a.len);
                self.builder
                    .build_call(
                        self.funcs["crypto_argon2id"],
                        &[pp.into(), pl.into(), sp.into(), sl.into(), m.into(), t.into(), p.into(), l.into(), out_ptr.into()],
                        "argon2id",
                    )
                    .map_err(|e| self.err(e))?
                    .try_as_basic_value().basic().expect("crypto argon2id returns i32 status")
            }
            // std.compress — gzip via libz / zstd via libzstd. The data view splits to `{ptr,len}`;
            // the out handle slot is caller-zeroed (so the Err path frees nothing); the runtime
            // returns an i32 status.
            Rvalue::CompressCompress { kind, data, level, out } => {
                let fk = match kind {
                    align_sema::hir::CompressKind::Gzip => "compress_gzip_compress",
                    align_sema::hir::CompressKind::Zstd => "compress_zstd_compress",
                };
                let out_ptr = self.slots[out];
                self.builder
                    .build_store(out_ptr, self.ctx.ptr_type(AddressSpace::default()).const_null())
                    .map_err(|e| self.err(e))?;
                let (dp, dl) = self.split_str(data)?;
                let lv = self.operand(level);
                self.builder
                    .build_call(self.funcs[fk], &[dp.into(), dl.into(), lv.into(), out_ptr.into()], "gzc")
                    .map_err(|e| self.err(e))?
                    .try_as_basic_value().basic().expect("gzip_compress returns i32 status")
            }
            Rvalue::CompressDecompress { kind, data, out } => {
                let fk = match kind {
                    align_sema::hir::CompressKind::Gzip => "compress_gzip_decompress",
                    align_sema::hir::CompressKind::Zstd => "compress_zstd_decompress",
                };
                let out_ptr = self.slots[out];
                self.builder
                    .build_store(out_ptr, self.ctx.ptr_type(AddressSpace::default()).const_null())
                    .map_err(|e| self.err(e))?;
                let (dp, dl) = self.split_str(data)?;
                self.builder
                    .build_call(self.funcs[fk], &[dp.into(), dl.into(), out_ptr.into()], "gzd")
                    .map_err(|e| self.err(e))?
                    .try_as_basic_value().basic().expect("gzip_decompress returns i32 status")
            }
            // std.rand — the rng state is passed by pointer to its slot (mutated in place).
            // `seed*` writes into `out`; `next`/`range` advance and return an i64; `shuffle`
            // rearranges a slice in place; `sample` returns a fresh owned `array<T>` `{ptr,len}`.
            Rvalue::RandSeed { seed, out } => {
                let out_ptr = self.slots[out];
                match seed {
                    Some(s) => {
                        let sv = self.operand(s);
                        self.builder
                            .build_call(self.funcs["rng_seed_with"], &[out_ptr.into(), sv.into()], "")
                            .map_err(|e| self.err(e))?;
                    }
                    None => {
                        self.builder
                            .build_call(self.funcs["rng_seed_os"], &[out_ptr.into()], "")
                            .map_err(|e| self.err(e))?;
                    }
                }
                return Ok(None);
            }
            Rvalue::RandNext { rng } => {
                let rng_ptr = self.slots[rng];
                self.builder
                    .build_call(self.funcs["rng_next"], &[rng_ptr.into()], "rnext")
                    .map_err(|e| self.err(e))?
                    .try_as_basic_value().basic().expect("rng_next returns i64")
            }
            Rvalue::RandRange { rng, lo, hi } => {
                let rng_ptr = self.slots[rng];
                let lo = self.operand(lo);
                let hi = self.operand(hi);
                self.builder
                    .build_call(self.funcs["rng_range"], &[rng_ptr.into(), lo.into(), hi.into()], "rrange")
                    .map_err(|e| self.err(e))?
                    .try_as_basic_value().basic().expect("rng_range returns i64")
            }
            Rvalue::RandShuffle { rng, xs, elem } => {
                let rng_ptr = self.slots[rng];
                let (xp, xl) = self.split_str(xs)?;
                let esz = scalar_bytes(align_sema::ty_to_scalar(*elem).expect("shuffle element is a scalar"));
                let esz = self.ctx.i64_type().const_int(esz, false);
                self.builder
                    .build_call(self.funcs["rng_shuffle"], &[rng_ptr.into(), xp.into(), xl.into(), esz.into()], "")
                    .map_err(|e| self.err(e))?;
                return Ok(None);
            }
            Rvalue::RandSample { rng, xs, k, elem } => {
                let rng_ptr = self.slots[rng];
                let (xp, xl) = self.split_str(xs)?;
                let kv = self.operand(k);
                let esz = scalar_bytes(align_sema::ty_to_scalar(*elem).expect("sample element is a scalar"));
                let esz = self.ctx.i64_type().const_int(esz, false);
                self.builder
                    .build_call(self.funcs["rng_sample"], &[rng_ptr.into(), xp.into(), xl.into(), kv.into(), esz.into()], "rsample")
                    .map_err(|e| self.err(e))?
                    .try_as_basic_value().basic().expect("rng_sample returns a {ptr,len}")
            }
            // std.cli — the command / parsed handles are opaque pointers passed by value; `command`
            // allocates one; `flag_*` register (void); `parse` writes a handle into `out` + returns an
            // i32 status; `get_*` read a flag; `usage` returns an owned `{ptr,len}` string.
            Rvalue::CliCommand { name } => {
                let (np, nl) = self.split_str(name)?;
                self.builder
                    .build_call(self.funcs["cli_command"], &[np.into(), nl.into()], "clicmd")
                    .map_err(|e| self.err(e))?
                    .try_as_basic_value().basic().expect("cli_command returns a handle pointer")
            }
            Rvalue::CliFlag { cmd, kind, name, default } => {
                let c = self.operand(cmd).into_pointer_value();
                let (np, nl) = self.split_str(name)?;
                match kind {
                    align_sema::hir::CliFlagKind::Bool => {
                        self.builder
                            .build_call(self.funcs["cli_flag_bool"], &[c.into(), np.into(), nl.into()], "")
                            .map_err(|e| self.err(e))?;
                    }
                    align_sema::hir::CliFlagKind::Str => {
                        let d = default.as_ref().expect("flag_str carries a str default");
                        let (dp, dl) = self.split_str(d)?;
                        self.builder
                            .build_call(self.funcs["cli_flag_str"], &[c.into(), np.into(), nl.into(), dp.into(), dl.into()], "")
                            .map_err(|e| self.err(e))?;
                    }
                    align_sema::hir::CliFlagKind::I64 => {
                        let d = default.as_ref().expect("flag_i64 carries an i64 default");
                        let dv = self.operand(d);
                        self.builder
                            .build_call(self.funcs["cli_flag_i64"], &[c.into(), np.into(), nl.into(), dv.into()], "")
                            .map_err(|e| self.err(e))?;
                    }
                }
                return Ok(None);
            }
            Rvalue::CliParse { cmd, args, out } => {
                let c = self.operand(cmd).into_pointer_value();
                let out_ptr = self.slots[out];
                self.builder
                    .build_store(out_ptr, self.ctx.ptr_type(AddressSpace::default()).const_null())
                    .map_err(|e| self.err(e))?;
                let (ap, al) = self.split_str(args)?;
                self.builder
                    .build_call(self.funcs["cli_parse"], &[c.into(), ap.into(), al.into(), out_ptr.into()], "cliparse")
                    .map_err(|e| self.err(e))?
                    .try_as_basic_value().basic().expect("cli_parse returns i32 status")
            }
            Rvalue::CliGetBool { parsed, name } => {
                let p = self.operand(parsed).into_pointer_value();
                let (np, nl) = self.split_str(name)?;
                self.builder
                    .build_call(self.funcs["cli_get_bool"], &[p.into(), np.into(), nl.into()], "cligetb")
                    .map_err(|e| self.err(e))?
                    .try_as_basic_value().basic().expect("cli_get_bool returns i32")
            }
            Rvalue::CliGetI64 { parsed, name } => {
                let p = self.operand(parsed).into_pointer_value();
                let (np, nl) = self.split_str(name)?;
                self.builder
                    .build_call(self.funcs["cli_get_i64"], &[p.into(), np.into(), nl.into()], "cligeti")
                    .map_err(|e| self.err(e))?
                    .try_as_basic_value().basic().expect("cli_get_i64 returns i64")
            }
            Rvalue::CliGetStr { parsed, name } => {
                let p = self.operand(parsed).into_pointer_value();
                let (np, nl) = self.split_str(name)?;
                self.builder
                    .build_call(self.funcs["cli_get_str"], &[p.into(), np.into(), nl.into()], "cligets")
                    .map_err(|e| self.err(e))?
                    .try_as_basic_value().basic().expect("cli_get_str returns a {ptr,len}")
            }
            Rvalue::CliUsage { cmd } => {
                let c = self.operand(cmd).into_pointer_value();
                self.builder
                    .build_call(self.funcs["cli_usage"], &[c.into()], "cliusage")
                    .map_err(|e| self.err(e))?
                    .try_as_basic_value().basic().expect("cli_usage returns a {ptr,len}")
            }
            // std.http — the request / response handles are opaque pointers passed by value.
            Rvalue::HttpRequest { method, url } => {
                let (mp, ml) = self.split_str(method)?;
                let (up, ul) = self.split_str(url)?;
                self.builder
                    .build_call(self.funcs["http_request"], &[mp.into(), ml.into(), up.into(), ul.into()], "httpreq")
                    .map_err(|e| self.err(e))?
                    .try_as_basic_value().basic().expect("http_request returns a handle pointer")
            }
            Rvalue::HttpHeader { req, name, value } => {
                let r = self.operand(req).into_pointer_value();
                let (np, nl) = self.split_str(name)?;
                let (vp, vl) = self.split_str(value)?;
                self.builder
                    .build_call(self.funcs["http_header"], &[r.into(), np.into(), nl.into(), vp.into(), vl.into()], "")
                    .map_err(|e| self.err(e))?;
                return Ok(None);
            }
            Rvalue::HttpBody { req, data } => {
                let r = self.operand(req).into_pointer_value();
                let (dp, dl) = self.split_str(data)?;
                self.builder
                    .build_call(self.funcs["http_body"], &[r.into(), dp.into(), dl.into()], "")
                    .map_err(|e| self.err(e))?;
                return Ok(None);
            }
            Rvalue::HttpParse { data, out } => {
                let out_ptr = self.slots[out];
                self.builder
                    .build_store(out_ptr, self.ctx.ptr_type(AddressSpace::default()).const_null())
                    .map_err(|e| self.err(e))?;
                let (dp, dl) = self.split_str(data)?;
                self.builder
                    .build_call(self.funcs["http_parse"], &[dp.into(), dl.into(), out_ptr.into()], "httpparse")
                    .map_err(|e| self.err(e))?
                    .try_as_basic_value().basic().expect("http_parse returns i32 status")
            }
            Rvalue::HttpRespStatus { resp } => {
                let p = self.operand(resp).into_pointer_value();
                self.builder
                    .build_call(self.funcs["http_resp_status"], &[p.into()], "httpstatus")
                    .map_err(|e| self.err(e))?
                    .try_as_basic_value().basic().expect("http_resp_status returns i64")
            }
            Rvalue::HttpRespHeader { resp, name, out } => {
                let p = self.operand(resp).into_pointer_value();
                let out_ptr = self.slots[out];
                self.builder.build_store(out_ptr, slice_struct_type(self.ctx).const_zero()).map_err(|e| self.err(e))?;
                let (np, nl) = self.split_str(name)?;
                self.builder
                    .build_call(self.funcs["http_resp_header"], &[p.into(), np.into(), nl.into(), out_ptr.into()], "httphdr")
                    .map_err(|e| self.err(e))?
                    .try_as_basic_value().basic().expect("http_resp_header returns i32 present flag")
            }
            Rvalue::HttpRespBody { resp } => {
                let p = self.operand(resp).into_pointer_value();
                self.builder
                    .build_call(self.funcs["http_resp_body"], &[p.into()], "httpbody")
                    .map_err(|e| self.err(e))?
                    .try_as_basic_value().basic().expect("http_resp_body returns a {ptr,len}")
            }
            // std.http (Slice 2) client — each request writes an owned `http response` handle into
            // `out` and returns an i32 status. Null `out` first (the Err branch reads null → nothing to
            // free), matching `http_parse`.
            Rvalue::HttpClient => self
                .builder
                .build_call(self.funcs["http_client_new"], &[], "httpclient")
                .map_err(|e| self.err(e))?
                .try_as_basic_value().basic().expect("http_client_new returns a handle pointer"),
            Rvalue::HttpClientGet { client, url, out } => {
                let c = self.operand(client).into_pointer_value();
                let out_ptr = self.slots[out];
                self.builder.build_store(out_ptr, self.ctx.ptr_type(AddressSpace::default()).const_null()).map_err(|e| self.err(e))?;
                let (up, ul) = self.split_str(url)?;
                self.builder
                    .build_call(self.funcs["http_client_get"], &[c.into(), up.into(), ul.into(), out_ptr.into()], "httpget")
                    .map_err(|e| self.err(e))?
                    .try_as_basic_value().basic().expect("http_client_get returns i32 status")
            }
            Rvalue::HttpClientPost { client, url, body, out } => {
                let c = self.operand(client).into_pointer_value();
                let out_ptr = self.slots[out];
                self.builder.build_store(out_ptr, self.ctx.ptr_type(AddressSpace::default()).const_null()).map_err(|e| self.err(e))?;
                let (up, ul) = self.split_str(url)?;
                let (bp, bl) = self.split_str(body)?;
                self.builder
                    .build_call(self.funcs["http_client_post"], &[c.into(), up.into(), ul.into(), bp.into(), bl.into(), out_ptr.into()], "httppost")
                    .map_err(|e| self.err(e))?
                    .try_as_basic_value().basic().expect("http_client_post returns i32 status")
            }
            Rvalue::HttpClientRequest { client, req, out } => {
                let c = self.operand(client).into_pointer_value();
                let r = self.operand(req).into_pointer_value();
                let out_ptr = self.slots[out];
                self.builder.build_store(out_ptr, self.ctx.ptr_type(AddressSpace::default()).const_null()).map_err(|e| self.err(e))?;
                self.builder
                    .build_call(self.funcs["http_client_request"], &[c.into(), r.into(), out_ptr.into()], "httpreqcall")
                    .map_err(|e| self.err(e))?
                    .try_as_basic_value().basic().expect("http_client_request returns i32 status")
            }
            // cl.get_many — the runtime writes an owned `array<response>` `{ptr,len}` header into `out`
            // and returns an i32 status (0 = ok; else the lowest-index error). Zero the out slot first
            // (the Err branch reads `{null,0}` → nothing to free), like `fs.read_dir`. `urls` is a
            // `slice<str>` (split to ptr+len); `max_concurrency` is an i64.
            Rvalue::HttpGetMany { client, urls, max_concurrency, out } => {
                let c = self.operand(client).into_pointer_value();
                let out_ptr = self.slots[out];
                self.builder.build_store(out_ptr, slice_struct_type(self.ctx).const_zero()).map_err(|e| self.err(e))?;
                let (up, ul) = self.split_str(urls)?;
                let mc = self.operand(max_concurrency).into();
                self.builder
                    .build_call(self.funcs["http_get_many"], &[c.into(), up.into(), ul.into(), mc, out_ptr.into()], "httpgetmany")
                    .map_err(|e| self.err(e))?
                    .try_as_basic_value().basic().expect("http_get_many returns i32 status")
            }
            // std.http (Slice 4) server — `serve`/`accept` write an owned handle into `out` + return an
            // i32 status (null `out` first, like the client); `respond` returns i32 (no out); the ctx
            // getters return a `{ptr,len}` view (or write one to `out` for `header`); `response` allocates
            // a builder; `rb_header`/`rb_body` are void; a header/body/status is passed by value.
            Rvalue::HttpServe { host, port, out } => {
                let (hp, hl) = self.split_str(host)?;
                let port_v = self.operand(port);
                let out_ptr = self.slots[out];
                self.builder.build_store(out_ptr, self.ctx.ptr_type(AddressSpace::default()).const_null()).map_err(|e| self.err(e))?;
                self.builder
                    .build_call(self.funcs["http_serve"], &[hp.into(), hl.into(), port_v.into(), out_ptr.into()], "httpserve")
                    .map_err(|e| self.err(e))?
                    .try_as_basic_value().basic().expect("http_serve returns i32 status")
            }
            Rvalue::HttpAccept { server, out } => {
                let s = self.operand(server).into_pointer_value();
                let out_ptr = self.slots[out];
                self.builder.build_store(out_ptr, self.ctx.ptr_type(AddressSpace::default()).const_null()).map_err(|e| self.err(e))?;
                self.builder
                    .build_call(self.funcs["http_accept"], &[s.into(), out_ptr.into()], "httpaccept")
                    .map_err(|e| self.err(e))?
                    .try_as_basic_value().basic().expect("http_accept returns i32 status")
            }
            Rvalue::HttpCtxMethod { ctx } => {
                let p = self.operand(ctx).into_pointer_value();
                self.builder
                    .build_call(self.funcs["http_ctx_method"], &[p.into()], "httpmethod")
                    .map_err(|e| self.err(e))?
                    .try_as_basic_value().basic().expect("http_ctx_method returns a {ptr,len}")
            }
            Rvalue::HttpCtxPath { ctx } => {
                let p = self.operand(ctx).into_pointer_value();
                self.builder
                    .build_call(self.funcs["http_ctx_path"], &[p.into()], "httppath")
                    .map_err(|e| self.err(e))?
                    .try_as_basic_value().basic().expect("http_ctx_path returns a {ptr,len}")
            }
            Rvalue::HttpCtxHeader { ctx, name, out } => {
                let p = self.operand(ctx).into_pointer_value();
                let out_ptr = self.slots[out];
                self.builder.build_store(out_ptr, slice_struct_type(self.ctx).const_zero()).map_err(|e| self.err(e))?;
                let (np, nl) = self.split_str(name)?;
                self.builder
                    .build_call(self.funcs["http_ctx_header"], &[p.into(), np.into(), nl.into(), out_ptr.into()], "httpctxhdr")
                    .map_err(|e| self.err(e))?
                    .try_as_basic_value().basic().expect("http_ctx_header returns i32 present flag")
            }
            Rvalue::HttpCtxBody { ctx } => {
                let p = self.operand(ctx).into_pointer_value();
                self.builder
                    .build_call(self.funcs["http_ctx_body"], &[p.into()], "httpctxbody")
                    .map_err(|e| self.err(e))?
                    .try_as_basic_value().basic().expect("http_ctx_body returns a {ptr,len}")
            }
            Rvalue::HttpResponseBuilder { status } => {
                let status_v = self.operand(status);
                self.builder
                    .build_call(self.funcs["http_response_new"], &[status_v.into()], "httprb")
                    .map_err(|e| self.err(e))?
                    .try_as_basic_value().basic().expect("http_response_new returns a handle pointer")
            }
            Rvalue::HttpRbHeader { rb, name, value } => {
                let r = self.operand(rb).into_pointer_value();
                let (np, nl) = self.split_str(name)?;
                let (vp, vl) = self.split_str(value)?;
                self.builder
                    .build_call(self.funcs["http_rb_header"], &[r.into(), np.into(), nl.into(), vp.into(), vl.into()], "")
                    .map_err(|e| self.err(e))?;
                return Ok(None);
            }
            Rvalue::HttpRbBody { rb, data } => {
                let r = self.operand(rb).into_pointer_value();
                let (dp, dl) = self.split_str(data)?;
                self.builder
                    .build_call(self.funcs["http_rb_body"], &[r.into(), dp.into(), dl.into()], "")
                    .map_err(|e| self.err(e))?;
                return Ok(None);
            }
            Rvalue::HttpRespond { ctx, rb } => {
                let c = self.operand(ctx).into_pointer_value();
                let r = self.operand(rb).into_pointer_value();
                self.builder
                    .build_call(self.funcs["http_respond"], &[c.into(), r.into()], "httprespond")
                    .map_err(|e| self.err(e))?
                    .try_as_basic_value().basic().expect("http_respond returns i32 status")
            }
            Rvalue::HttpRespondStream { ctx, rb, out } => {
                let c = self.operand(ctx).into_pointer_value();
                let r = self.operand(rb).into_pointer_value();
                let out_ptr = self.slots[out];
                self.builder.build_store(out_ptr, self.ctx.ptr_type(AddressSpace::default()).const_null()).map_err(|e| self.err(e))?;
                self.builder
                    .build_call(self.funcs["http_respond_stream"], &[c.into(), r.into(), out_ptr.into()], "httprespondstream")
                    .map_err(|e| self.err(e))?
                    .try_as_basic_value().basic().expect("http_respond_stream returns i32 status")
            }
            Rvalue::HttpStreamSend { stream, chunk } => {
                let s = self.operand(stream).into_pointer_value();
                let (dp, dl) = self.split_str(chunk)?;
                self.builder
                    .build_call(self.funcs["http_stream_send"], &[s.into(), dp.into(), dl.into()], "httpstreamsend")
                    .map_err(|e| self.err(e))?
                    .try_as_basic_value().basic().expect("http_stream_send returns i32 status")
            }
            Rvalue::HttpStreamFinish { stream } => {
                let s = self.operand(stream).into_pointer_value();
                self.builder
                    .build_call(self.funcs["http_stream_finish"], &[s.into()], "httpstreamfinish")
                    .map_err(|e| self.err(e))?
                    .try_as_basic_value().basic().expect("http_stream_finish returns i32 status")
            }
            // env.get — write the owned value {ptr,len} into `out`, return an i32 present flag.
            Rvalue::EnvGet { name, out } => {
                let out_ptr = self.slots[out];
                self.builder.build_store(out_ptr, slice_struct_type(self.ctx).const_zero()).map_err(|e| self.err(e))?;
                let (np, nl) = self.split_str(name)?;
                self.builder
                    .build_call(self.funcs["env_get"], &[np.into(), nl.into(), out_ptr.into()], "envget")
                    .map_err(|e| self.err(e))?
                    .try_as_basic_value().basic().expect("env_get returns i32")
            }
            Rvalue::EnvSet { name, value } => {
                let (np, nl) = self.split_str(name)?;
                let (vp, vl) = self.split_str(value)?;
                self.builder
                    .build_call(self.funcs["env_set"], &[np.into(), nl.into(), vp.into(), vl.into()], "envset")
                    .map_err(|e| self.err(e))?
                    .try_as_basic_value().basic().expect("env_set returns i32")
            }
            Rvalue::TimeNow => self
                .builder
                .build_call(self.funcs["time_now"], &[], "now")
                .map_err(|e| self.err(e))?
                .try_as_basic_value().basic().expect("time_now returns i64"),
            Rvalue::TimeInstant => self
                .builder
                .build_call(self.funcs["time_instant"], &[], "instant")
                .map_err(|e| self.err(e))?
                .try_as_basic_value().basic().expect("time_instant returns i64"),
            Rvalue::TimeSleep { ns } => {
                let n = self.operand(ns).into();
                self.builder
                    .build_call(self.funcs["time_sleep"], &[n], "")
                    .map_err(|e| self.err(e))?;
                return Ok(None);
            }
            Rvalue::MakeError { enum_id, tag, code } => {
                // Build the builtin `Error` aggregate `{ i32 tag, i32 code }` from runtime operands.
                let sty = self.enum_types[*enum_id as usize];
                let t = self.operand(tag);
                let c = self.operand(code);
                let agg = self
                    .builder
                    .build_insert_value(sty.const_zero(), t, 0, "etag")
                    .map_err(|e| self.err(e))?;
                self.builder
                    .build_insert_value(agg, c, 1, "ecode")
                    .map_err(|e| self.err(e))?
                    .into_struct_value()
                    .into()
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
            Rvalue::SliceIndexNoalias { slice, index, scope } => {
                // Like `SliceIndex`, plus the `map_into` loop's `in` alias scope so the vectorizer
                // knows this source load can't overlap the (`out`-scoped) `dst` store.
                // `alias.scope = {in}`, `noalias = {out}`.
                let agg = self.operand(slice).into_struct_value();
                let ptr = self.builder.build_extract_value(agg, 0, "ptr").map_err(|e| self.err(e))?.into_pointer_value();
                let ty = scalar_type(self.ctx, result_ty, self.struct_types, self.enum_types);
                let idx = self.operand(index).into_int_value();
                let ep = unsafe {
                    self.builder
                        .build_in_bounds_gep(ty, ptr, &[idx], "slcidx")
                        .map_err(|e| self.err(e))?
                };
                let load = self.builder.build_load(ty, ep, "slcload").map_err(|e| self.err(e))?;
                let (in_list, out_list) = self.alias_scope_lists(*scope);
                let scope_kind = self.ctx.get_kind_id("alias.scope");
                let noalias_kind = self.ctx.get_kind_id("noalias");
                let inst = load
                    .as_instruction_value()
                    .ok_or_else(|| self.err("slice load is not an instruction"))?;
                inst.set_metadata(in_list, scope_kind).map_err(|_| self.err("set alias.scope"))?;
                inst.set_metadata(out_list, noalias_kind).map_err(|_| self.err("set noalias"))?;
                load
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
                    .build_insert_value(sty.get_poison(), newptr, 0, "subvptr")
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
                // A foreign call coerces each argument to its SysV form: a `str`/`slice` view → its
                // data pointer; a `layout(C)` struct → one `i64`/`double` per eightbyte; everything
                // else passes as its value. A non-extern call passes every argument directly.
                let argv: Vec<inkwell::values::BasicMetadataValueEnum> = match self.extern_abi.get(name) {
                    Some(abi) => {
                        let mut v: Vec<inkwell::values::BasicMetadataValueEnum> = Vec::with_capacity(args.len());
                        for (o, pa) in args.iter().zip(&abi.params) {
                            let val = self.operand(o);
                            match pa {
                                ParamAbi::Direct => v.push(val.into()),
                                ParamAbi::ViewPtr => {
                                    let ptr = self
                                        .builder
                                        .build_extract_value(val.into_struct_value(), 0, "ffiptr")
                                        .map_err(|e| self.err(e))?;
                                    v.push(ptr.into());
                                }
                                ParamAbi::StructRegs(sabi) => {
                                    // Store the struct into a padded (eightbyte-multiple) slot, then
                                    // load one `i64`/`double` per eightbyte — the SysV register form.
                                    // The padded slot keeps every 8-byte load in bounds even when the
                                    // last eightbyte is only partially occupied.
                                    let slot = self.eightbyte_slot(sabi.ebs.len())?;
                                    self.builder.build_store(slot, val.into_struct_value()).map_err(|e| self.err(e))?;
                                    for (i, &eb) in sabi.ebs.iter().enumerate() {
                                        let p = self.eightbyte_ptr(slot, i)?;
                                        let lv = self.builder.build_load(eb.llvm(self.ctx), p, "eb").map_err(|e| self.err(e))?;
                                        v.push(lv.into());
                                    }
                                }
                                // `StructMemory` params were rejected at declaration time.
                                ParamAbi::StructMemory => {
                                    return Err(self.err(format!("extern '{name}': by-value MEMORY-class struct argument is unsupported")));
                                }
                            }
                        }
                        v
                    }
                    None => args.iter().map(|o| self.operand(o).into()).collect(),
                };
                let cs = self
                    .builder
                    .build_call(callee, &argv, "call")
                    .map_err(|e| self.err(e))?;
                // Reconstruct a by-value struct return from its register form.
                if let Some(ExternAbi { ret: ReturnAbi::StructRegs(sabi), .. }) = self.extern_abi.get(name) {
                    let rv = cs
                        .try_as_basic_value()
                        .basic()
                        .ok_or_else(|| self.err(format!("extern '{name}' returns a struct by value but produced no value")))?;
                    let slot = self.eightbyte_slot(sabi.ebs.len())?;
                    self.builder.build_store(slot, rv).map_err(|e| self.err(e))?;
                    let sty = self.struct_types[sabi.id as usize];
                    let sv = self.builder.build_load(sty, slot, "ffiret").map_err(|e| self.err(e))?;
                    return Ok(Some(sv));
                }
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
                let mut argv: Vec<inkwell::values::BasicMetadataValueEnum> = vec![env.into()];
                argv.extend(args.iter().map(|o| inkwell::values::BasicMetadataValueEnum::from(self.operand(o))));
                if *ret_ty == Ty::Unit {
                    // Align `()` functions use LLVM `void`, including their env-ABI fn-value
                    // thunks. Calling such a thunk through an `i32` signature is an ABI mismatch
                    // that opaque pointers cannot verify, so keep the indirect call Unit-aware just
                    // like the spawn trampoline above.
                    let fn_ty = self.ctx.void_type().fn_type(&param_meta, false);
                    self.builder
                        .build_indirect_call(fn_ty, fn_ptr, &argv, "")
                        .map_err(|e| self.err(e))?;
                    return Ok(None);
                }
                let fn_ty = self.llvm_type(*ret_ty).fn_type(&param_meta, false);
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
            Ty::Box(_) | Ty::ArenaHandle | Ty::Builder | Ty::StrFinder | Ty::Writer | Ty::Reader | Ty::Buffer | Ty::ArrayBuilder(_) | Ty::TcpConn | Ty::TcpListener | Ty::UdpSocket | Ty::Child | Ty::File | Ty::Raw => self.ctx.ptr_type(AddressSpace::default()).into(),
            Ty::Fn(_) => closure_struct_type(self.ctx).into(),
            Ty::Array(s, n) => scalar_type(self.ctx, scalar_to_ty(s), self.struct_types, self.enum_types).array_type(n).into(),
            Ty::StructArray(id, n) => self.struct_types[id as usize].array_type(n).into(),
            Ty::Slice(_) | Ty::Soa(_) | Ty::Str | Ty::String | Ty::DynArray(_) => slice_struct_type(self.ctx).into(),
            // AoS struct array = `{ptr,len}`; SoA (M6) differs → match the layout (forces revisit).
            Ty::DynStructArray(_, Layout::Aos) | Ty::DynSliceArray(_) | Ty::DynResponseArray => slice_struct_type(self.ctx).into(),
            Ty::DictEncoded(..) => dictenc_struct_type(self.ctx).into(),
            // `rng` — the Xoshiro256++ state, `[4 x i64]` (a Copy by-value aggregate).
            Ty::Rng => rng_llvm_type(self.ctx),
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
    /// A struct (or fixed struct array) declared `align(N)` returns `N`; everything else returns the
    /// natural ABI alignment LLVM derives from the type. This over-aligns the *storage* (alloca /
    /// global); the matching *size* padding for a tight array stride lives in `set_struct_body`
    /// (`open-questions.md` "`align(N)`"). Routing all alignment through here keeps it one place.
    fn type_align(&self, ty: Ty) -> u32 {
        let custom = match ty {
            // A struct value, and a fixed AoS array of it (`[N x %Struct]`, whose alignment is the
            // element's), take the struct's declared alignment — together with the element size
            // padding (`set_struct_body`), every array element stays over-aligned. A `DynStructArray`
            // slot holds a `{ptr,len}` view, not the struct — its element-buffer over-alignment is a
            // separate heap/runtime concern (still deferred), so the slot itself stays naturally aligned.
            Ty::Struct(id) | Ty::StructArray(id, _) => self.structs[id as usize].align,
            _ => None,
        };
        // `align(N)` only ever *over*-aligns: take the max of the declared and the natural ABI
        // alignment, so a too-small `align(N)` can never under-align a value (which would be UB).
        let natural = self.target_data.get_abi_alignment(&self.llvm_type(ty));
        custom.map_or(natural, |c| c.max(natural))
    }

    /// `&slot[index].f0.f1.…` — GEP `[0, index, *pfield(path)]` into a `[N x %Struct]` alloca. The
    /// field `path` (length ≥ 1) walks the (nested) element struct; each level's logical index is
    /// mapped to its physical slot via [`Self::phys_field_indices`] (correct under field reordering).
    fn elem_field_ptr(&self, slot: Slot, idx: &Operand, path: &[u32]) -> Result<inkwell::values::PointerValue<'c>, CodegenError> {
        let arr_ty = self.llvm_type(self.f.slots[slot as usize]);
        let zero = self.ctx.i64_type().const_zero();
        let index = self.operand(idx).into_int_value();
        let sid = self.array_elem_struct_id(slot);
        // `[0, index]` reaches element `index`; each physical field index descends one struct level.
        let mut indices = vec![zero, index];
        for pidx in self.phys_field_indices(sid, path) {
            indices.push(self.ctx.i32_type().const_int(pidx as u64, false));
        }
        unsafe {
            self.builder
                .build_in_bounds_gep(arr_ty, self.slots[&slot], &indices, "elemfield")
                .map_err(|e| self.err(e))
        }
    }

    /// Map a logical field `path` (length ≥ 1) through a chain of nested structs rooted at
    /// `struct_id` to the sequence of **physical** (reordered, `pfield`) field indices — one per
    /// path segment. Each non-final field must be a `Struct` (sema's nested-access walk guarantees
    /// it); used to build a multi-index element-field GEP for both fixed (`[N x %Struct]`) and
    /// dynamic (`{ptr,len}` buffer) struct arrays.
    fn phys_field_indices(&self, struct_id: u32, path: &[u32]) -> Vec<u32> {
        let mut sid = struct_id;
        let mut out = Vec::with_capacity(path.len());
        for (k, &logical) in path.iter().enumerate() {
            out.push(self.pfield(sid, logical));
            if k + 1 < path.len() {
                sid = match self.structs[sid as usize].fields[logical as usize].ty {
                    Ty::Struct(nid) => nid,
                    other => unreachable!("nested element-field path through non-struct {other:?}"),
                };
            }
        }
        out
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
                Ty::Struct(nid) if struct_is_move(nid, self.structs, self.enums) => {
                    let fp = self.builder.build_struct_gep(st, base, pi, "dropnest").map_err(|e| self.err(e))?;
                    self.drop_struct_fields(fp, nid)?;
                }
                // A Move sum-type field (J3) — an owned `array<T>` payload variant makes the enclosing
                // struct Move (`ty_owns_buffer_rec`'s enum arm). Tag-switch and free the live variant's
                // owned buffer via `drop_enum` (a non-Move enum owns nothing → not a Move struct field →
                // never reaches here). `DropFlagInit` zeroes the aggregate, so a moved-out / unconstructed
                // enum field reads tag 0 and frees `null` — null-safe, single-free every path.
                Ty::Enum(eid) if enum_is_move(eid, self.enums) => {
                    let fp = self.builder.build_struct_gep(st, base, pi, "dropenumfld").map_err(|e| self.err(e))?;
                    self.drop_enum(fp, eid)?;
                }
                // An owned `array<Move-struct>` field (J3b) — the `Chat { messages: array<Message> }`
                // shape, where each element owns a buffer (a `string`/owned-array field, or a Move-enum
                // field like `Message`'s `content`). Deep-free each element then the AoS (`free(null)` is
                // a no-op for an empty array) via the shared helper.
                Ty::DynStructArray(eid, _) if struct_is_move(eid, self.structs, self.enums) => {
                    let fp = self.builder.build_struct_gep(st, base, pi, "dropdeeparr").map_err(|e| self.err(e))?;
                    self.deep_free_struct_array(fp, eid)?;
                }
                // An owned `array<T>` field (REST-gateway runway Slice C) with a **non-owned** element —
                // free its single heap buffer (field 0 of the `{ptr,len}`; `free(null)` is a no-op for an
                // empty array). A scalar / `str`-view / plain-data-struct element owns nothing, so this is
                // one flat free — no per-element deep free — and `array<Struct>`'s `str` fields are
                // borrowed views into the input, not freed here. (A Move-struct element is deep-freed by
                // the arm above.)
                Ty::DynArray(_) | Ty::DynStructArray(..) | Ty::DynSliceArray(_) => {
                    let fp = self.builder.build_struct_gep(st, base, pi, "droparr").map_err(|e| self.err(e))?;
                    let agg = self
                        .builder
                        .build_load(slice_struct_type(self.ctx), fp, "droparrv")
                        .map_err(|e| self.err(e))?
                        .into_struct_value();
                    let ptr = self.builder.build_extract_value(agg, 0, "droparrptr").map_err(|e| self.err(e))?;
                    self.builder.build_call(self.funcs["free"], &[ptr.into()], "").map_err(|e| self.err(e))?;
                }
                // A nested Move-struct *array* field — drop each element (defensive: struct fields
                // reject array types today — `is_field_ok` — so this is unreachable, but keeping the
                // owned case here means a future array-valued field can't silently fail-open and leak).
                Ty::StructArray(eid, n) if struct_is_move(eid, self.structs, self.enums) => {
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
                // A Move **handle** field (F1②): a bare pointer handle — `http_request_ctx`, `file`,
                // a reader/writer/buffer, a socket, an http request/response/client/server/stream, a
                // cli command/parsed. Load the pointer and call its null-safe `*_free`, exactly like a
                // standalone handle local's `Stmt::Drop` (shared `handle_free_fn` — one source of
                // truth). A moved-out / zeroed field reads a null handle, so the free is a no-op —
                // the resource is closed at most once. `is_field_ok` admits exactly this handle set,
                // so no allowed field type reaches the `_` arm below and silently leaks.
                ty if handle_free_fn(ty).is_some() => {
                    let free_fn = handle_free_fn(ty).expect("guarded by the arm pattern");
                    let fp = self.builder.build_struct_gep(st, base, pi, "drophandle").map_err(|e| self.err(e))?;
                    let p = self
                        .builder
                        .build_load(self.ctx.ptr_type(AddressSpace::default()), fp, "drophandlev")
                        .map_err(|e| self.err(e))?;
                    self.builder.build_call(self.funcs[free_fn], &[p.into()], "").map_err(|e| self.err(e))?;
                }
                _ => {}
            }
        }
        Ok(())
    }

    /// Deep-free an owned `array<Move-struct>` (J3b) whose `{ptr,len}` aggregate lives at `slice_ptr`:
    /// loop over the `len` elements, recursively `drop_struct_fields` each (freeing its own owned
    /// fields — a `string`/owned-array/Move-enum field, transitively), then free the AoS buffer itself.
    /// A flat free alone would leak every element's owned buffer. `drop_struct_fields` may append basic
    /// blocks (a Move-enum element's `drop_enum`), so the loop back-edge branches from the block current
    /// *after* the recursive call (`get_insert_block`). An empty array (len 0 / null ptr) skips the loop
    /// and frees null. Shared by the struct-field drop (`drop_struct_fields`) and the standalone-local
    /// drop (`Stmt::Drop`), so a bare `array<Move-struct>` local and an `array<Move-struct>` field free
    /// identically.
    fn deep_free_struct_array(&self, slice_ptr: inkwell::values::PointerValue<'c>, eid: u32) -> Result<(), CodegenError> {
        let agg = self
            .builder
            .build_load(slice_struct_type(self.ctx), slice_ptr, "dropdeeparrv")
            .map_err(|e| self.err(e))?
            .into_struct_value();
        let ptr = self.builder.build_extract_value(agg, 0, "dropdeeparrptr").map_err(|e| self.err(e))?.into_pointer_value();
        let len = self.builder.build_extract_value(agg, 1, "dropdeeparrlen").map_err(|e| self.err(e))?.into_int_value();
        let elem_ty = self.struct_types[eid as usize];
        let i64t = self.ctx.i64_type();
        let head = self.ctx.append_basic_block(self.func, "dropdeep.head");
        let body = self.ctx.append_basic_block(self.func, "dropdeep.body");
        let done = self.ctx.append_basic_block(self.func, "dropdeep.done");
        let pred = self.builder.get_insert_block().ok_or_else(|| self.err("no insert block"))?;
        self.builder.build_unconditional_branch(head).map_err(|e| self.err(e))?;
        // head: i = phi [0, pred], [i+1, after-body]; branch to body while i < len.
        self.builder.position_at_end(head);
        let phi = self.builder.build_phi(i64t, "dropdeep.i").map_err(|e| self.err(e))?;
        phi.add_incoming(&[(&i64t.const_zero(), pred)]);
        let i_cur = phi.as_basic_value().into_int_value();
        let cond = self
            .builder
            .build_int_compare(inkwell::IntPredicate::ULT, i_cur, len, "dropdeep.cmp")
            .map_err(|e| self.err(e))?;
        self.builder.build_conditional_branch(cond, body, done).map_err(|e| self.err(e))?;
        // body: deep-free element i's owned fields, then i+1 and loop back.
        self.builder.position_at_end(body);
        let ep = unsafe {
            self.builder.build_in_bounds_gep(elem_ty, ptr, &[i_cur], "dropdeep.ep").map_err(|e| self.err(e))?
        };
        self.drop_struct_fields(ep, eid)?;
        let after = self.builder.get_insert_block().ok_or_else(|| self.err("no insert block"))?;
        let inext = self.builder.build_int_add(i_cur, i64t.const_int(1, false), "dropdeep.inext").map_err(|e| self.err(e))?;
        phi.add_incoming(&[(&inext, after)]);
        self.builder.build_unconditional_branch(head).map_err(|e| self.err(e))?;
        // done: free the AoS buffer (null-safe for an empty array).
        self.builder.position_at_end(done);
        self.builder.build_call(self.funcs["free"], &[ptr.into()], "").map_err(|e| self.err(e))?;
        Ok(())
    }

    /// Tag-switched drop of a **Move** sum type (J2): load the i32 tag, switch on it, and for each
    /// variant carrying an owned payload free that payload's buffer. A variant whose payload is a
    /// scalar / `str` / plain-struct owns nothing and falls through (the `else` continue block). Every
    /// owned enum payload today is an owned `array<T>` — a `{ptr,len}` freed by its field-0 pointer,
    /// one flat free (the element is non-owned, pass 0c). Null-safe: an unconstructed / moved-out enum
    /// was zeroed by `DropFlagInit`, so the tag reads 0 and a variant-0 owned payload frees `null`.
    /// `base` is the pointer to the in-memory enum aggregate (`{ i32 tag, payloads… }`).
    fn drop_enum(&self, base: inkwell::values::PointerValue<'c>, enum_id: u32) -> Result<(), CodegenError> {
        let ety = self.enum_types[enum_id as usize];
        // (variant tag, LLVM field indices of its owned payloads) for every variant that owns a buffer.
        // A variant's payload `k` lives at flat field index `field_base + k` (`field_base` includes the
        // tag slot — see `MakeEnum`). Snapshot into owned `Vec`s so no borrow of `self.enums` is held
        // across the builder calls below.
        let owned: Vec<(u64, Vec<u32>)> = self.enums[enum_id as usize]
            .variants
            .iter()
            .enumerate()
            .filter_map(|(vi, v)| {
                let fields: Vec<u32> = v
                    .payload
                    .iter()
                    .enumerate()
                    .filter(|(_, s)| s.is_move())
                    .map(|(k, _)| v.field_base + k as u32)
                    .collect();
                (!fields.is_empty()).then_some((vi as u64, fields))
            })
            .collect();
        if owned.is_empty() {
            return Ok(()); // a non-Move enum reached here only defensively — nothing to free.
        }
        let tag_ptr = self.builder.build_struct_gep(ety, base, 0, "droptag").map_err(|e| self.err(e))?;
        let tag = self
            .builder
            .build_load(self.ctx.i32_type(), tag_ptr, "droptagv")
            .map_err(|e| self.err(e))?
            .into_int_value();
        let cont = self.ctx.append_basic_block(self.func, "drop.enum.cont");
        let cases: Vec<_> = owned
            .iter()
            .map(|(vi, _)| (self.ctx.i32_type().const_int(*vi, false), self.ctx.append_basic_block(self.func, "drop.enum.v")))
            .collect();
        self.builder.build_switch(tag, cont, &cases).map_err(|e| self.err(e))?;
        for ((_, fields), (_, bb)) in owned.iter().zip(cases.iter()) {
            self.builder.position_at_end(*bb);
            for &fi in fields {
                let fp = self.builder.build_struct_gep(ety, base, fi, "dropev").map_err(|e| self.err(e))?;
                let agg = self
                    .builder
                    .build_load(slice_struct_type(self.ctx), fp, "dropevv")
                    .map_err(|e| self.err(e))?
                    .into_struct_value();
                let ptr = self.builder.build_extract_value(agg, 0, "dropevptr").map_err(|e| self.err(e))?;
                self.builder.build_call(self.funcs["free"], &[ptr.into()], "").map_err(|e| self.err(e))?;
            }
            self.builder.build_unconditional_branch(cont).map_err(|e| self.err(e))?;
        }
        self.builder.position_at_end(cont);
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
            // `==`/`!=` use the length-fast-path `str_eq`; the four ordering ops use the
            // byte-lexicographic `str_cmp` (`Ord(str)`).
            return match op {
                BinOp::Eq | BinOp::Ne => self.gen_str_eq(op, a, b),
                _ => self.gen_str_cmp(op, a, b),
            };
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
        mark_private_unnamed_addr(g);
        (g.as_pointer_value(), self.ctx.i64_type().const_int(s.len() as u64, false))
    }

    /// Materialize an aggregate constant's folded elements as a private read-only global and return
    /// its `(&elements, len)` — the array-literal analogue of [`Self::str_global`]. A `str`-element
    /// array lays out as `[N x {ptr,len}]` (each element a constant view into the string pool); any
    /// other scalar as `[N x elem]`. `unnamed_addr` lets the linker merge identical constants, so this
    /// does no interning of its own.
    fn const_array_global(&self, elems: &[ConstElem], elem: Ty) -> (inkwell::values::PointerValue<'c>, IntValue<'c>) {
        let len = self.ctx.i64_type().const_int(elems.len() as u64, false);
        let arr: inkwell::values::ArrayValue<'c> = match elem {
            Ty::Str => {
                let sty = slice_struct_type(self.ctx);
                let vals: Vec<_> = elems
                    .iter()
                    .map(|e| {
                        let ConstElem::Str(s) = e else { unreachable!("str-element array holds Str elements") };
                        let (ptr, l) = self.str_global(s);
                        sty.const_named_struct(&[ptr.into(), l.into()])
                    })
                    .collect();
                sty.const_array(&vals)
            }
            Ty::Float(_) => {
                let ft = float_type(self.ctx, elem);
                let vals: Vec<_> = elems
                    .iter()
                    .map(|e| {
                        let ConstElem::Float(v) = e else { unreachable!("float-element array holds Float elements") };
                        ft.const_float(*v)
                    })
                    .collect();
                ft.const_array(&vals)
            }
            // Int / Bool / Char — all lower to an LLVM integer of the element width.
            _ => {
                let it = int_type(self.ctx, elem);
                let signed = is_signed(elem);
                let vals: Vec<_> = elems
                    .iter()
                    .map(|e| match e {
                        ConstElem::Int(v) => it.const_int(*v as u64, signed),
                        ConstElem::Bool(b) => it.const_int(*b as u64, false),
                        ConstElem::Char(c) => it.const_int(*c as u64, false),
                        _ => unreachable!("scalar-element array holds int/bool/char elements"),
                    })
                    .collect();
                it.const_array(&vals)
            }
        };
        let g = self.module.add_global(arr.get_type(), None, "const_arr");
        g.set_initializer(&arr);
        g.set_constant(true);
        mark_private_unnamed_addr(g);
        (g.as_pointer_value(), len)
    }

    /// `template "..."` → in-place builder init, a write per piece, then in-place finish → str.
    /// `json.decode` into struct `struct_id`: zero the out slot, build a field-descriptor
    /// table `[{ name_ptr, name_len: i64, tag: i32, offset: i64 }]` (tag = byte width for
    /// ints, 0 for bool; offset from the target layout), and call the runtime parser. Returns
    /// the i32 status.
    /// The LLVM type of one `json.decode` field descriptor: `{ name_ptr, name_len: i64, tag: i32,
    /// offset: i64, sub: ptr, opt_tag: i64 }` — matching the runtime `JsonField` `#[repr(C)]`. `sub`
    /// is null except for a nested-struct payload (kind 4); `opt_tag` is `-1` for a required field or
    /// the `Option` tag byte offset for an optional (`Option<T>`) field.
    fn json_desc_ty(&self) -> inkwell::types::StructType<'c> {
        let ptr_ty = self.ctx.ptr_type(AddressSpace::default());
        let i64t = self.ctx.i64_type();
        let i32t = self.ctx.i32_type();
        self.ctx
            .struct_type(&[ptr_ty.into(), i64t.into(), i32t.into(), i64t.into(), ptr_ty.into(), i64t.into()], false)
    }

    /// The `(tag, sub)` descriptor pair for a decodeable **payload** type — the `(signed<<16)|
    /// (kind<<8)|width` tag and the nested sub-table pointer (non-null only for a `Struct` payload).
    /// Shared by a direct field and an `Option<T>` field's payload so both encode the payload
    /// identically. `null` is the null pointer constant for non-struct payloads.
    fn json_payload_tag_sub(&mut self, ty: Ty, null: inkwell::values::PointerValue<'c>) -> (u64, inkwell::values::PointerValue<'c>) {
        match ty {
            Ty::Int(it) => (((it.signed as u64) << 16) | (it.bits / 8) as u64, null),
            Ty::Bool => ((1 << 8) | 1, null),
            Ty::Float(ft) => ((2 << 8) | (ft.bits / 8) as u64, null),
            Ty::Str => ((3 << 8) | 16, null),
            Ty::Struct(nested_id) => (4 << 8, self.emit_json_subtable(nested_id)),
            // `array<Struct>` field (kind 5, REST-gateway runway Slice C): width 16 (the field's own
            // `{ptr,len}` slot); `sub` is the ELEMENT struct's schema (store_size = element stride),
            // which the runtime uses to decode the JSON array into an owned AoS. The nested `str`
            // element fields stay borrowed views into the input.
            Ty::DynStructArray(eid, _) => ((5 << 8) | 16, self.emit_json_subtable(eid)),
            // `array<scalar>` field (kind 7, JSON completeness T1b): the field's own `{ptr,len}` slot is
            // width 16 (low byte); the ELEMENT scalar (int/float/bool) is packed into the upper bits so
            // one tag carries both — elem-signed bit 16, elem-kind (0=int/1=bool/2=float) bits 20-23,
            // elem-width bits 24-27. `sub` is null (scalars need no sub-schema). The element's own
            // (kind,width,sign) come from the scalar-field encoding, relocated here.
            Ty::DynArray(s @ (Scalar::Int(_) | Scalar::Float(_) | Scalar::Bool)) => {
                let (etag, _) = self.json_payload_tag_sub(scalar_to_ty(s), null);
                let elem_kind = (etag >> 8) & 0xff;
                let elem_width = etag & 0xff;
                let elem_signed = (etag >> 16) & 1;
                ((7 << 8) | 16 | (elem_signed << 16) | (elem_kind << 20) | (elem_width << 24), null)
            }
            // A shape-directed union (`enum`) field (kind 6, JSON completeness J1b-2b): `sub` is a
            // `JsonUnion` (not a `JsonSubTable`) — the runtime `field_width`/`write_value`/encode arms
            // reinterpret it by the kind. The width byte is unused (the size comes from
            // `JsonUnion.store_size`, like a nested struct's kind-4 `sub.store_size`).
            Ty::Enum(eid) => (6 << 8, self.emit_json_union(eid)),
            _ => unreachable!("json.decode payload is int/float/bool/str/nested-struct/array<struct>/enum-union (sema-checked)"),
        }
    }

    /// The byte offset of an `Option<s>`'s payload within its `{ i8 tag, payload }` LLVM layout
    /// (`option_struct_type` element 1) — where the decoder writes the `Some` value.
    fn option_payload_offset(&self, s: Scalar) -> u64 {
        let opt_ty = option_struct_type(self.ctx, s, self.struct_types, self.enum_types);
        self.target_data.offset_of_element(&opt_ty, 1).expect("Option payload is element 1")
    }

    /// The LLVM type of a nested-struct field's sub-schema, matching the runtime `JsonSubTable`
    /// `#[repr(C)]`: `{ descs: ptr, n_fields: i64, store_size: i64, phf: ptr, phf_len: i64,
    /// phf_seed: i64 }`.
    fn json_subtable_ty(&self) -> inkwell::types::StructType<'c> {
        let ptr_ty = self.ctx.ptr_type(AddressSpace::default());
        let i64t = self.ctx.i64_type();
        self.ctx
            .struct_type(&[ptr_ty.into(), i64t.into(), i64t.into(), ptr_ty.into(), i64t.into(), i64t.into()], false)
    }

    /// Emit the constant `{name_ptr, name_len, tag, offset, sub}` field-descriptor table (+ its
    /// perfect-hash slot table) for decoding struct `struct_id`, returning the table base, field
    /// count, and PHF. Recurses to emit a [`json_subtable_ty`] global for each nested-struct field
    /// (kind 4) — the struct graph is acyclic (sema's `struct_acyclic` rejects self-reference), so
    /// the recursion terminates. The table is a private constant global (no per-call alloca → safe
    /// inside a loop). Shared by single-struct and `array<Struct>` decode (MMv2 slice 8d).
    fn emit_desc_table(&mut self, struct_id: u32) -> DescTable<'c> {
        let ptr_ty = self.ctx.ptr_type(AddressSpace::default());
        let i64t = self.ctx.i64_type();
        let i32t = self.ctx.i32_type();
        let desc_ty = self.json_desc_ty();
        let null = ptr_ty.const_null();
        let fields = self.structs[struct_id as usize].fields.clone();
        let descs: Vec<inkwell::values::StructValue> = fields
            .iter()
            .enumerate()
            .map(|(i, f)| {
                let (name_ptr, name_len) = self.str_global(&f.name);
                // tag = (signed << 16) | (kind << 8) | byte-width. kind: 0 = int, 1 = bool,
                // 2 = float, 3 = str, 4 = nested struct. Bit 16 is the int sign flag (only meaningful
                // for kind 0). A `str` field is a `{ptr,len}` view; a nested-struct field carries kind
                // 4 with `sub` pointing to its sub-schema. The descriptor lists fields by *name* in
                // logical order, but the byte offset is the physical slot (fields are
                // alignment-reordered for non-`layout(C)` structs); `field_byte_offset` maps
                // logical→physical.
                let field_off = self.field_byte_offset(struct_id, i as u32);
                // An `Option<T>` field describes its **payload** (tag/sub), with `offset` pointing at
                // the payload slot inside the `Option` (`{ i8 tag, payload }`) and `opt_tag` = the
                // field's own byte offset (the tag byte). A required field is `opt_tag = -1` with
                // `offset` = the field itself.
                let (tag, sub_ptr, offset, opt_tag): (u64, inkwell::values::PointerValue, u64, i64) = match f.ty {
                    Ty::Option(s) => {
                        let (tag, sub_ptr) = self.json_payload_tag_sub(scalar_to_ty(s), null);
                        (tag, sub_ptr, field_off + self.option_payload_offset(s), field_off as i64)
                    }
                    other => {
                        let (tag, sub_ptr) = self.json_payload_tag_sub(other, null);
                        (tag, sub_ptr, field_off, -1)
                    }
                };
                desc_ty.const_named_struct(&[
                    name_ptr.into(),
                    name_len.into(),
                    i32t.const_int(tag, false).into(),
                    i64t.const_int(offset, false).into(),
                    sub_ptr.into(),
                    i64t.const_int(opt_tag as u64, true).into(),
                ])
            })
            .collect();
        let table_val = desc_ty.const_array(&descs);
        let table = self.module.add_global(table_val.get_type(), None, "jfields");
        table.set_initializer(&table_val);
        table.set_constant(true);
        mark_private_unnamed_addr(table);

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
                mark_private_unnamed_addr(g);
                (g.as_pointer_value(), slots.len() as u64, seed)
            }
            None => (ptr_ty.const_null(), 0, 0),
        };

        DescTable { descs: table.as_pointer_value(), n_fields: fields.len() as u64, phf_ptr, phf_len, phf_seed }
    }

    /// Emit the `JsonSubTable` global for a nested-struct field of type `struct_id` (its descriptor
    /// table, store size, and PHF), returning a pointer to it for the parent field's `sub` slot.
    fn emit_json_subtable(&mut self, struct_id: u32) -> inkwell::values::PointerValue<'c> {
        let inner = self.emit_desc_table(struct_id); // recurse — acyclic, so it terminates
        let store_size = self.target_data.get_store_size(&self.struct_types[struct_id as usize]);
        let i64t = self.ctx.i64_type();
        let subtable_ty = self.json_subtable_ty();
        let val = subtable_ty.const_named_struct(&[
            inner.descs.into(),
            i64t.const_int(inner.n_fields, false).into(),
            i64t.const_int(store_size, false).into(),
            inner.phf_ptr.into(),
            i64t.const_int(inner.phf_len, false).into(),
            i64t.const_int(inner.phf_seed, false).into(),
        ]);
        let g = self.module.add_global(subtable_ty, None, "jsub");
        g.set_initializer(&val);
        g.set_constant(true);
        mark_private_unnamed_addr(g);
        g.as_pointer_value()
    }

    /// Emit the field-descriptor table for decoding struct `struct_id`, wrapping [`emit_desc_table`]
    /// with the struct's store size. The table is a private constant global (safe inside a loop).
    fn decode_field_table(&mut self, struct_id: u32) -> DecodeTable<'c> {
        let sty = self.struct_types[struct_id as usize];
        let t = self.emit_desc_table(struct_id);
        DecodeTable {
            descs: t.descs,
            n_fields: t.n_fields,
            store_size: self.target_data.get_store_size(&sty),
            phf_ptr: t.phf_ptr,
            phf_len: t.phf_len,
            phf_seed: t.phf_seed,
        }
    }

    /// The LLVM type of a shape-directed union descriptor, matching the runtime `JsonUnion`
    /// `#[repr(C)]`: `{ arms: ptr, class_to_arm: ptr, enum_tags: ptr, n_arms: i64, store_size: i64 }`.
    fn json_union_ty(&self) -> inkwell::types::StructType<'c> {
        let ptr_ty = self.ctx.ptr_type(AddressSpace::default());
        let i64t = self.ctx.i64_type();
        self.ctx.struct_type(&[ptr_ty.into(), ptr_ty.into(), ptr_ty.into(), i64t.into(), i64t.into()], false)
    }

    /// Emit the constant [`JsonUnion`] descriptor for shape-directed decode/encode of sum type
    /// `enum_id` (JSON completeness J1b): one payload arm ([`JsonField`]) per variant, a `class_to_arm`
    /// table (shape class → arm index, `-1` if absent), and an `enum_tags` table (arm → variant index
    /// / enum tag). The payload's byte offset within the enum `{ i32 tag, payloads… }` comes from the
    /// LLVM layout (`offset_of_element(ety, 1 + field_base)`), so it stays the exact dual of the
    /// enum's codegen layout. Returns a pointer to the union global (a private constant → safe in a
    /// loop). Sema (`check_union_decodable`) guarantees each variant has one payload and the shape
    /// classes are pairwise distinct.
    fn emit_json_union(&mut self, enum_id: u32) -> inkwell::values::PointerValue<'c> {
        let ptr_ty = self.ctx.ptr_type(AddressSpace::default());
        let i64t = self.ctx.i64_type();
        let i32t = self.ctx.i32_type();
        let desc_ty = self.json_desc_ty();
        let null = ptr_ty.const_null();
        let ety = self.enum_types[enum_id as usize];
        let variants = self.enums[enum_id as usize].variants.clone();

        // One arm per variant + the shape-class → arm and arm → enum-tag tables.
        let mut arms: Vec<inkwell::values::StructValue> = Vec::with_capacity(variants.len());
        let mut class_to_arm = [-1i32; align_sema::JSON_SHAPE_CLASSES];
        let mut enum_tags: Vec<inkwell::values::IntValue> = Vec::with_capacity(variants.len());
        for (tag_idx, v) in variants.iter().enumerate() {
            // Sema guarantees exactly one payload per union variant; a missing one is a compiler bug.
            let payload = *v.payload.first().expect("union variant carries exactly one payload");
            let pty = scalar_to_ty(payload);
            let (tag, sub_ptr) = self.json_payload_tag_sub(pty, null);
            // The payload sits at enum LLVM element `field_base` (`field_base` is 1-based — it already
            // accounts for the i32 tag at element 0; `MakeEnum` stores the payload at `field_base + j`,
            // and a union variant has a single payload, j = 0).
            let off = self.target_data.offset_of_element(&ety, v.field_base).expect("valid enum payload offset");
            let arm_idx = arms.len() as i32;
            arms.push(desc_ty.const_named_struct(&[
                null.into(),                                    // name_ptr (unused for a union arm)
                i64t.const_zero().into(),                       // name_len
                i32t.const_int(tag, false).into(),              // packed payload kind/width/sign
                i64t.const_int(off, false).into(),              // payload byte offset in the enum
                sub_ptr.into(),                                 // object payload sub-schema (else null)
                i64t.const_int((-1i64) as u64, true).into(),    // opt_tag = -1 (unused)
            ]));
            enum_tags.push(i32t.const_int(tag_idx as u64, false));
            if let Some(cls) = align_sema::union_shape_class(payload) {
                class_to_arm[cls as usize] = arm_idx;
            }
        }

        let arms_val = desc_ty.const_array(&arms);
        let arms_g = self.module.add_global(arms_val.get_type(), None, "junion_arms");
        arms_g.set_initializer(&arms_val);
        arms_g.set_constant(true);
        mark_private_unnamed_addr(arms_g);

        let cls_val = i32t.const_array(&class_to_arm.iter().map(|&c| i32t.const_int(c as u64, true)).collect::<Vec<_>>());
        let cls_g = self.module.add_global(cls_val.get_type(), None, "junion_class");
        cls_g.set_initializer(&cls_val);
        cls_g.set_constant(true);
        mark_private_unnamed_addr(cls_g);

        let tags_val = i32t.const_array(&enum_tags);
        let tags_g = self.module.add_global(tags_val.get_type(), None, "junion_tags");
        tags_g.set_initializer(&tags_val);
        tags_g.set_constant(true);
        mark_private_unnamed_addr(tags_g);

        let store_size = self.target_data.get_store_size(&ety);
        let union_ty = self.json_union_ty();
        let val = union_ty.const_named_struct(&[
            arms_g.as_pointer_value().into(),
            cls_g.as_pointer_value().into(),
            tags_g.as_pointer_value().into(),
            i64t.const_int(variants.len() as u64, false).into(),
            i64t.const_int(store_size, false).into(),
        ]);
        let g = self.module.add_global(union_ty, None, "junion");
        g.set_initializer(&val);
        g.set_constant(true);
        mark_private_unnamed_addr(g);
        g.as_pointer_value()
    }

    /// `json.decode` into a shape-directed union (`enum`) target (JSON completeness J1b): zero the out
    /// enum, then call the runtime union decoder with the emitted [`JsonUnion`] descriptor. Returns
    /// the i32 status.
    fn gen_json_decode_union(&mut self, enum_id: u32, input: &Operand, out: Slot) -> Result<BasicValueEnum<'c>, CodegenError> {
        let ety = self.enum_types[enum_id as usize];
        let out_ptr = self.slots[&out];
        // Zero the enum so an unset payload/tag reads as 0 (a failed decode leaves it zeroed).
        self.builder.build_store(out_ptr, ety.const_zero()).map_err(|e| self.err(e))?;

        let agg = self.operand(input).into_struct_value();
        let in_ptr = self.builder.build_extract_value(agg, 0, "jin_p").map_err(|e| self.err(e))?;
        let in_len = self.builder.build_extract_value(agg, 1, "jin_l").map_err(|e| self.err(e))?;

        let union_desc = self.emit_json_union(enum_id);
        let cs = self
            .builder
            .build_call(
                self.funcs["json_decode_union"],
                &[in_ptr.into(), in_len.into(), union_desc.into(), out_ptr.into()],
                "jdecu",
            )
            .map_err(|e| self.err(e))?;
        Ok(cs.try_as_basic_value().basic().expect("json_decode_union returns i32"))
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

    /// `json.doc(input)` (J4): zero the out `{tape,node}` slot, then call the runtime parser with the
    /// input `{ptr,len}` and the enclosing arena handle. Returns the i32 status (0 = ok).
    fn gen_json_doc(&mut self, input: &Operand, arena: &Operand, out: Slot) -> Result<BasicValueEnum<'c>, CodegenError> {
        let out_ptr = self.slots[&out];
        self.builder.build_store(out_ptr, slice_struct_type(self.ctx).const_zero()).map_err(|e| self.err(e))?;
        let (in_ptr, in_len) = self.split_str(input)?;
        let ah = self.operand(arena);
        let cs = self
            .builder
            .build_call(self.funcs["json_doc_parse"], &[in_ptr.into(), in_len.into(), ah.into(), out_ptr.into()], "jdoc")
            .map_err(|e| self.err(e))?;
        Ok(cs.try_as_basic_value().basic().expect("json_doc_parse returns i32"))
    }

    /// Split a `json.doc` handle operand into `(tape_ptr, node_i64)` — the same `{ptr,i64}` layout as
    /// a `str`/slice, so `split_str` does exactly the right extraction.
    fn split_doc(&mut self, op: &Operand) -> Result<(BasicValueEnum<'c>, BasicValueEnum<'c>), CodegenError> {
        self.split_str(op)
    }

    /// `d.kind()` (J4): call the runtime for the i32 `json.kind` tag, then wrap it into the tag-only
    /// enum aggregate `{ i32 }` (like [`Rvalue::MakeError`] but a single field).
    fn gen_json_doc_kind(&mut self, doc: &Operand, result_ty: Ty) -> Result<BasicValueEnum<'c>, CodegenError> {
        let (tape, node) = self.split_doc(doc)?;
        let tag = self
            .builder
            .build_call(self.funcs["json_doc_kind"], &[tape.into(), node.into()], "jkind")
            .map_err(|e| self.err(e))?
            .try_as_basic_value()
            .basic()
            .expect("json_doc_kind returns i32");
        let Ty::Enum(id) = result_ty else { unreachable!("d.kind() has an enum result type") };
        let sty = self.enum_types[id as usize];
        Ok(self
            .builder
            .build_insert_value(sty.const_zero(), tag, 0, "jkindtag")
            .map_err(|e| self.err(e))?
            .into_struct_value()
            .into())
    }

    /// `d.get(key)` (J4): zero the out `{tape,node}` slot, then call the void runtime navigator, which
    /// writes the child handle (`Missing` if absent). The result is loaded from `out` by the caller.
    fn gen_json_doc_get(&mut self, doc: &Operand, key: &Operand, out: Slot) -> Result<(), CodegenError> {
        let out_ptr = self.slots[&out];
        self.builder.build_store(out_ptr, slice_struct_type(self.ctx).const_zero()).map_err(|e| self.err(e))?;
        let (tape, node) = self.split_doc(doc)?;
        let (kp, kl) = self.split_str(key)?;
        self.builder
            .build_call(self.funcs["json_doc_get"], &[tape.into(), node.into(), kp.into(), kl.into(), out_ptr.into()], "")
            .map_err(|e| self.err(e))?;
        Ok(())
    }

    /// `d.key(index)` (J4 slice 2): zero the out `{ptr,len}` slot, then call the runtime accessor, which
    /// writes the index-th member key (a `str` view) into `out` and returns an i32 present flag.
    fn gen_json_doc_key(&mut self, doc: &Operand, index: &Operand, out: Slot) -> Result<BasicValueEnum<'c>, CodegenError> {
        let out_ptr = self.slots[&out];
        self.builder.build_store(out_ptr, slice_struct_type(self.ctx).const_zero()).map_err(|e| self.err(e))?;
        let (tape, node) = self.split_doc(doc)?;
        let idx = self.operand(index);
        Ok(self
            .builder
            .build_call(self.funcs["json_doc_key"], &[tape.into(), node.into(), idx.into(), out_ptr.into()], "jkey")
            .map_err(|e| self.err(e))?
            .try_as_basic_value()
            .basic()
            .expect("json_doc_key returns i32 present flag"))
    }

    /// `d.elems()` (J4 slice 3): zero the out `{ptr,len}` slot, then call the void runtime materializer,
    /// which bump-allocates the level's handle buffer in `arena` and writes the `slice<json.doc>` header.
    fn gen_json_doc_elems(&mut self, doc: &Operand, arena: &Operand, out: Slot) -> Result<(), CodegenError> {
        let out_ptr = self.slots[&out];
        self.builder.build_store(out_ptr, slice_struct_type(self.ctx).const_zero()).map_err(|e| self.err(e))?;
        let (tape, node) = self.split_doc(doc)?;
        let ah = self.operand(arena);
        self.builder
            .build_call(self.funcs["json_doc_elems"], &[tape.into(), node.into(), ah.into(), out_ptr.into()], "")
            .map_err(|e| self.err(e))?;
        Ok(())
    }

    /// `d.at(index)` (J4): the array-index sibling of [`Self::gen_json_doc_get`].
    fn gen_json_doc_at(&mut self, doc: &Operand, index: &Operand, out: Slot) -> Result<(), CodegenError> {
        let out_ptr = self.slots[&out];
        self.builder.build_store(out_ptr, slice_struct_type(self.ctx).const_zero()).map_err(|e| self.err(e))?;
        let (tape, node) = self.split_doc(doc)?;
        let idx = self.operand(index);
        self.builder
            .build_call(self.funcs["json_doc_at"], &[tape.into(), node.into(), idx.into(), out_ptr.into()], "")
            .map_err(|e| self.err(e))?;
        Ok(())
    }

    /// `d.as_str()` (J4): zero the out `{ptr,len}` slot, then call the runtime accessor, which writes a
    /// `str` view into `out` on a JSON string and returns an i32 present flag.
    fn gen_json_doc_as_str(&mut self, doc: &Operand, out: Slot) -> Result<BasicValueEnum<'c>, CodegenError> {
        let out_ptr = self.slots[&out];
        self.builder.build_store(out_ptr, slice_struct_type(self.ctx).const_zero()).map_err(|e| self.err(e))?;
        let (tape, node) = self.split_doc(doc)?;
        Ok(self
            .builder
            .build_call(self.funcs["json_doc_as_str"], &[tape.into(), node.into(), out_ptr.into()], "jasstr")
            .map_err(|e| self.err(e))?
            .try_as_basic_value()
            .basic()
            .expect("json_doc_as_str returns i32 present flag"))
    }

    /// `d.as_i64()` / `d.as_f64()` / `d.as_bool()` (J4): zero the out scalar slot, then call the leaf
    /// accessor for `scalar`, which writes the value into `out` and returns an i32 present flag.
    fn gen_json_doc_as_scalar(&mut self, scalar: Ty, doc: &Operand, out: Slot) -> Result<BasicValueEnum<'c>, CodegenError> {
        // The out slot is read by the caller only on the `Some` (present) branch, and the runtime
        // writes it exactly then — so it never needs zeroing (unlike the view slots above, whose
        // `Drop`/consumers could otherwise see stale bytes).
        let out_ptr = self.slots[&out];
        let (tape, node) = self.split_doc(doc)?;
        let sym = match scalar {
            Ty::Int(_) => "json_doc_as_i64",
            Ty::Float(_) => "json_doc_as_f64",
            Ty::Bool => "json_doc_as_bool",
            _ => unreachable!("json.doc leaf accessor scalar is i64/f64/bool"),
        };
        Ok(self
            .builder
            .build_call(self.funcs[sym], &[tape.into(), node.into(), out_ptr.into()], "jasscalar")
            .map_err(|e| self.err(e))?
            .try_as_basic_value()
            .basic()
            .expect("json.doc leaf accessor returns i32 present flag"))
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

    /// One streaming step of a `json.scan(view)` fused terminal (J5): decode the next JSON object at
    /// `*cursor` in the scanner's input view into the `row` struct slot, advancing `*cursor`. Reuses
    /// the same decode descriptor table as [`gen_json_decode_struct_array`] (the row IS one element),
    /// passing the cursor + row slot pointers. Returns the i32 status (0 = row / 1 = done / 2 =
    /// malformed). The runtime zeroes the row before decoding, so `Option`/default fields are correct.
    fn gen_json_scan_next(&mut self, struct_id: u32, scanner: &Operand, cursor: Slot, row: Slot) -> Result<BasicValueEnum<'c>, CodegenError> {
        // The scanner is a `{ptr,len}` input view (str-ABI); split it into the input pointer + length.
        let (in_ptr, in_len) = self.split_str(scanner)?;
        let cursor_ptr = self.slots[&cursor];
        let row_ptr = self.slots[&row];
        let i64t = self.ctx.i64_type();
        let t = self.decode_field_table(struct_id);
        let n = i64t.const_int(t.n_fields, false);
        let out_size = i64t.const_int(t.store_size, false);
        let phf_len = i64t.const_int(t.phf_len, false);
        let phf_seed = i64t.const_int(t.phf_seed, false);
        let cs = self
            .builder
            .build_call(
                self.funcs["json_scan_next"],
                &[in_ptr.into(), in_len.into(), cursor_ptr.into(), t.descs.into(), n.into(), row_ptr.into(), out_size.into(), t.phf_ptr.into(), phf_len.into(), phf_seed.into()],
                "jscannext",
            )
            .map_err(|e| self.err(e))?;
        Ok(cs.try_as_basic_value().basic().expect("json_scan_next returns i32"))
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

    /// `json.decode(input)` into a bare **scalar** (T1b): zero the out scalar slot, then call the
    /// runtime scalar parser with the input `{ptr,len}` and the element tag (same encoding as an array
    /// element / a scalar field). The runtime writes the parsed scalar to `out` and returns the i32
    /// status (0 = ok); a trailing-garbage / type-mismatch input is a non-zero status → `Err`.
    fn gen_json_decode_scalar(&mut self, scalar: Ty, input: &Operand, out: Slot) -> Result<BasicValueEnum<'c>, CodegenError> {
        let out_ptr = self.slots[&out];
        // Zero the scalar slot so a failed decode leaves a defined value (the Err path ignores it).
        let sty = self.llvm_type(scalar);
        self.builder.build_store(out_ptr, sty.const_zero()).map_err(|e| self.err(e))?;
        let agg = self.operand(input).into_struct_value();
        let in_ptr = self.builder.build_extract_value(agg, 0, "jsin_p").map_err(|e| self.err(e))?;
        let in_len = self.builder.build_extract_value(agg, 1, "jsin_l").map_err(|e| self.err(e))?;
        let tag: u64 = match scalar {
            Ty::Int(it) => ((it.signed as u64) << 16) | (it.bits / 8) as u64,
            Ty::Bool => (1 << 8) | 1,
            Ty::Float(ft) => (2 << 8) | (ft.bits / 8) as u64,
            _ => unreachable!("json.decode scalar target is int/float/bool (sema-checked)"),
        };
        let tag_v = self.ctx.i32_type().const_int(tag, false);
        let cs = self
            .builder
            .build_call(
                self.funcs["json_decode_scalar"],
                &[in_ptr.into(), in_len.into(), tag_v.into(), out_ptr.into()],
                "jdecs",
            )
            .map_err(|e| self.err(e))?;
        Ok(cs.try_as_basic_value().basic().expect("json_decode_scalar returns i32"))
    }

    /// `fs.read_file(path)`: zero the out `{ptr,len}` slot, then call the runtime reader with the
    /// path `{ptr,len}`. The runtime writes the owned `string` (heap buffer freed by `Drop`) to
    /// `out`. Returns the i32 status (0 = ok).
    /// Split a `str`/`bytes` `{ptr,len}` operand into its `(data_ptr, len)` components — the marshal
    /// for every runtime call that takes a view as a `ptr`+`len` pair (`fs.write_file`, `fs.exists`,
    /// `fs.remove`, `fs.read_dir`, `fs.read_file_view` paths).
    fn split_str(&mut self, op: &Operand) -> Result<(BasicValueEnum<'c>, BasicValueEnum<'c>), CodegenError> {
        let agg = self.operand(op).into_struct_value();
        let ptr = self.builder.build_extract_value(agg, 0, "sv_ptr").map_err(|e| self.err(e))?;
        let len = self.builder.build_extract_value(agg, 1, "sv_len").map_err(|e| self.err(e))?;
        Ok((ptr, len))
    }

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

    /// The three A7 line-read rvalues off `gen_rvalue`'s hot path (the #296 expr-depth lesson): each
    /// yields a value, so all return `Ok(Some(..))`.
    #[inline(never)]
    fn gen_reader_line_rvalue(&mut self, rv: &Rvalue) -> Result<Option<BasicValueEnum<'c>>, CodegenError> {
        let v = match rv {
            // r.buffered() — upgrade the reader in place, return the (same) handle pointer.
            Rvalue::ReaderBuffered(r) => {
                let rp = self.operand(r).into();
                self.builder
                    .build_call(self.funcs["io_reader_buffered"], &[rp], "buffered")
                    .map_err(|e| self.err(e))?
                    .try_as_basic_value().basic().expect("io_reader_buffered returns a pointer")
            }
            // r.read_line(b) — fill the buffer with the stripped line body, return i64 consumed-or-status.
            Rvalue::ReaderReadLine(r, buf) => {
                let rp = self.operand(r).into();
                let bp = self.operand(buf).into();
                self.builder
                    .build_call(self.funcs["io_reader_read_line"], &[rp, bp], "readline")
                    .map_err(|e| self.err(e))?
                    .try_as_basic_value().basic().expect("io_reader_read_line returns i64")
            }
            // bytes.as_str() — validate UTF-8, write the `str` view `{ptr,len}` into `out`, return i32.
            Rvalue::BytesAsStr { bytes, out } => {
                let out_ptr = self.slots[out];
                self.builder.build_store(out_ptr, slice_struct_type(self.ctx).const_zero()).map_err(|e| self.err(e))?;
                let (b_ptr, b_len) = self.split_str(bytes)?;
                self.builder
                    .build_call(self.funcs["bytes_as_str"], &[b_ptr.into(), b_len.into(), out_ptr.into()], "asstr")
                    .map_err(|e| self.err(e))?
                    .try_as_basic_value().basic().expect("bytes_as_str returns i32")
            }
            _ => return Err(self.err("gen_reader_line_rvalue on a non-A7 op")),
        };
        Ok(Some(v))
    }

    /// All A4 `file` rvalues (create_rw/open_rw + pread/pwrite/len). `#[inline(never)]` so the
    /// depth-recursive `gen_rvalue` gains one small arm, not five inline bodies (the #296 expr-depth
    /// frame lesson). Constructors mirror `fs.open` (write the handle into `out`, return i32 status);
    /// the methods call the runtime with the borrowed file + operands and return the i64 count-or-status.
    #[inline(never)]
    fn gen_file_rvalue(&mut self, rv: &Rvalue) -> Result<BasicValueEnum<'c>, CodegenError> {
        Ok(match rv {
            Rvalue::FileCreateRw { path, out } => self.gen_open_handle("io_file_create", path, *out)?,
            Rvalue::FileOpenRw { path, out } => self.gen_open_handle("io_file_open", path, *out)?,
            // f.pread(b, off) — the runtime fills the buffer window at `off`, returns i64 count-or-status.
            Rvalue::FilePread { file, buffer, offset } => {
                let fp = self.operand(file).into();
                let bp = self.operand(buffer).into();
                let off = self.operand(offset).into();
                self.builder
                    .build_call(self.funcs["io_file_pread"], &[fp, bp, off], "pread")
                    .map_err(|e| self.err(e))?
                    .try_as_basic_value().basic().expect("io_file_pread returns i64")
            }
            // f.pwrite(data, off) — split the `bytes` operand into ptr+len, return i64 count-or-status.
            Rvalue::FilePwrite { file, data, offset } => {
                let fp = self.operand(file).into();
                let agg = self.operand(data).into_struct_value();
                let ptr = self.builder.build_extract_value(agg, 0, "pwptr").map_err(|e| self.err(e))?;
                let len = self.builder.build_extract_value(agg, 1, "pwlen").map_err(|e| self.err(e))?;
                let off = self.operand(offset).into();
                self.builder
                    .build_call(self.funcs["io_file_pwrite"], &[fp, ptr.into(), len.into(), off], "pwrite")
                    .map_err(|e| self.err(e))?
                    .try_as_basic_value().basic().expect("io_file_pwrite returns i64")
            }
            // f.len() — a live fstat, returns i64 length-or-status.
            Rvalue::FileLen { file } => {
                let fp = self.operand(file).into();
                self.builder
                    .build_call(self.funcs["io_file_len"], &[fp], "flen")
                    .map_err(|e| self.err(e))?
                    .try_as_basic_value().basic().expect("io_file_len returns i64")
            }
            _ => unreachable!("gen_file_rvalue on a non-file rvalue"),
        })
    }

    /// `fs.open` / `fs.create`: zero the out handle slot (so a failed open leaves null — its `Drop`
    /// is a null-safe no-op), then call the runtime opener (`func`) with the path `{ptr,len}` and the
    /// out slot. Returns the i32 errno-status (0 = ok).
    fn gen_open_handle(&mut self, func: &str, path: &Operand, out: Slot) -> Result<BasicValueEnum<'c>, CodegenError> {
        let out_ptr = self.slots[&out];
        self.builder
            .build_store(out_ptr, self.ctx.ptr_type(AddressSpace::default()).const_null())
            .map_err(|e| self.err(e))?;
        let agg = self.operand(path).into_struct_value();
        let p_ptr = self.builder.build_extract_value(agg, 0, "path_p").map_err(|e| self.err(e))?;
        let p_len = self.builder.build_extract_value(agg, 1, "path_l").map_err(|e| self.err(e))?;
        let cs = self
            .builder
            .build_call(self.funcs[func], &[p_ptr.into(), p_len.into(), out_ptr.into()], "open")
            .map_err(|e| self.err(e))?;
        Ok(cs.try_as_basic_value().basic().expect("open returns i32 status"))
    }

    fn gen_template(
        &mut self,
        result_id: ValueId,
        pieces: &[align_mir::TemplatePiece],
        arena: Option<&Operand>,
    ) -> Result<BasicValueEnum<'c>, CodegenError> {
        // Pass the enclosing arena handle, or null for an individually owned finish retained by a
        // synthetic MIR string owner.
        let arena_ptr = match arena {
            Some(op) => self.operand(op),
            None => self.ctx.ptr_type(AddressSpace::default()).const_null().into(),
        };
        // A template/json.encode builder uses the default capacity (0) — static-part presizing is a
        // separate future opt; the user-facing `builder(capacity)` is the capacity surface.
        let zero = self.ctx.i64_type().const_zero();
        let header = self.stack_template_headers[&result_id];
        let bptr = self
            .builder
            .build_call(
                self.funcs["builder_init_stack"],
                &[header.into(), arena_ptr.into(), zero.into()],
                "b.stack",
            )
            .map_err(|e| self.err(e))?
            .try_as_basic_value()
            .basic()
            .expect("builder_init_stack returns a pointer");
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
                // `json.encode` `Option<T>` field: when `Some`, append `"name":<payload>,`; else
                // nothing. The payload is rendered per its scalar kind exactly like the plain holes
                // above (int/float/bool raw, str JSON-escaped), with a trailing comma that a later
                // [`PopComma`] strips if this was the last present field.
                align_mir::TemplatePiece::OptionField { opt, name } => {
                    let Ty::Option(s) = self.f.operand_ty(opt) else {
                        return Err(self.err("json.encode OptionField piece is not an Option"));
                    };
                    let agg = self.operand(opt).into_struct_value();
                    let tag = self.builder.build_extract_value(agg, 0, "otag").map_err(|e| self.err(e))?.into_int_value();
                    let payload = self.builder.build_extract_value(agg, 1, "opay").map_err(|e| self.err(e))?;
                    let is_some = self
                        .builder
                        .build_int_compare(IntPredicate::NE, tag, tag.get_type().const_zero(), "issome")
                        .map_err(|e| self.err(e))?;
                    let func = self
                        .builder
                        .get_insert_block()
                        .and_then(|b| b.get_parent())
                        .ok_or_else(|| self.err("no enclosing function for OptionField"))?;
                    let some_bb = self.ctx.append_basic_block(func, "opt.some");
                    let cont_bb = self.ctx.append_basic_block(func, "opt.cont");
                    self.builder.build_conditional_branch(is_some, some_bb, cont_bb).map_err(|e| self.err(e))?;
                    self.builder.position_at_end(some_bb);
                    // `"name":` prefix.
                    let (pptr, plen) = self.str_global(&format!("\"{name}\":"));
                    self.builder
                        .build_call(self.funcs["builder_write"], &[bptr.into(), pptr.into(), plen.into()], "")
                        .map_err(|e| self.err(e))?;
                    // Render the payload by its scalar kind (mirrors the holes above).
                    match scalar_to_ty(s) {
                        Ty::Str => {
                            let pa = payload.into_struct_value();
                            let sptr = self.builder.build_extract_value(pa, 0, "osp").map_err(|e| self.err(e))?;
                            let slen = self.builder.build_extract_value(pa, 1, "osl").map_err(|e| self.err(e))?;
                            self.builder
                                .build_call(self.funcs["builder_write_json_str"], &[bptr.into(), sptr.into(), slen.into()], "")
                                .map_err(|e| self.err(e))?;
                        }
                        Ty::Bool => {
                            let v = payload.into_int_value();
                            let wide = self.builder.build_int_z_extend(v, self.ctx.i32_type(), "bext").map_err(|e| self.err(e))?;
                            self.builder
                                .build_call(self.funcs["builder_write_bool"], &[bptr.into(), wide.into()], "")
                                .map_err(|e| self.err(e))?;
                        }
                        fty @ Ty::Float(_) => {
                            let v = payload.into_float_value();
                            let callee = if fty == Ty::Float(FloatTy { bits: 32 }) { "builder_write_f32" } else { "builder_write_f64" };
                            self.builder.build_call(self.funcs[callee], &[bptr.into(), v.into()], "").map_err(|e| self.err(e))?;
                        }
                        ity => {
                            let v = payload.into_int_value();
                            let wide = if v.get_type().get_bit_width() < 64 {
                                if is_signed(ity) {
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
                    }
                    // Trailing comma (stripped by `PopComma` if this is the last present field).
                    let (cptr, clen) = self.str_global(",");
                    self.builder
                        .build_call(self.funcs["builder_write"], &[bptr.into(), cptr.into(), clen.into()], "")
                        .map_err(|e| self.err(e))?;
                    self.builder.build_unconditional_branch(cont_bb).map_err(|e| self.err(e))?;
                    self.builder.position_at_end(cont_bb);
                }
                // `json.encode` of an `Option<struct>` field (T1b): when `Some`, write `"name":`, render
                // the payload struct via the runtime descriptor-driven encoder (the same schema decode
                // uses), then a trailing comma; when `None`, emit nothing. The payload struct is stored
                // to an entry alloca so the encoder can read it by field offset.
                align_mir::TemplatePiece::OptionStructField { opt, name, struct_id, .. } => {
                    let agg = self.operand(opt).into_struct_value();
                    let tag = self.builder.build_extract_value(agg, 0, "ostag").map_err(|e| self.err(e))?.into_int_value();
                    let payload = self.builder.build_extract_value(agg, 1, "ospay").map_err(|e| self.err(e))?;
                    let is_some = self
                        .builder
                        .build_int_compare(IntPredicate::NE, tag, tag.get_type().const_zero(), "osissome")
                        .map_err(|e| self.err(e))?;
                    let func = self
                        .builder
                        .get_insert_block()
                        .and_then(|b| b.get_parent())
                        .ok_or_else(|| self.err("no enclosing function for OptionStructField"))?;
                    let some_bb = self.ctx.append_basic_block(func, "optstruct.some");
                    let cont_bb = self.ctx.append_basic_block(func, "optstruct.cont");
                    self.builder.build_conditional_branch(is_some, some_bb, cont_bb).map_err(|e| self.err(e))?;
                    self.builder.position_at_end(some_bb);
                    let (pptr, plen) = self.str_global(&format!("\"{name}\":"));
                    self.builder
                        .build_call(self.funcs["builder_write"], &[bptr.into(), pptr.into(), plen.into()], "")
                        .map_err(|e| self.err(e))?;
                    // Store the payload struct to an entry alloca and hand its pointer + descriptor table
                    // to the runtime object encoder (a hoisted slot so an encode in a loop stays flat).
                    let sty = self.struct_types[*struct_id as usize];
                    let slot = self.alloca_at_entry(sty.into(), "ostruct_v")?;
                    self.builder.build_store(slot, payload).map_err(|e| self.err(e))?;
                    let t = self.emit_desc_table(*struct_id);
                    let n = i64t.const_int(t.n_fields, false);
                    self.builder
                        .build_call(self.funcs["json_encode_object"], &[bptr.into(), slot.into(), t.descs.into(), n.into()], "")
                        .map_err(|e| self.err(e))?;
                    let (cptr, clen) = self.str_global(",");
                    self.builder
                        .build_call(self.funcs["builder_write"], &[bptr.into(), cptr.into(), clen.into()], "")
                        .map_err(|e| self.err(e))?;
                    self.builder.build_unconditional_branch(cont_bb).map_err(|e| self.err(e))?;
                    self.builder.position_at_end(cont_bb);
                }
                align_mir::TemplatePiece::PopComma => {
                    self.builder
                        .build_call(self.funcs["builder_pop_comma"], &[bptr.into()], "")
                        .map_err(|e| self.err(e))?;
                }
                // `json.encode` of an `array<Struct>` field: hand the owned AoS `{ptr,len}` and the
                // element schema (the same descriptor table decode uses) to the runtime encoder, which
                // loops the elements emitting `[{...},...]` (a dynamic length can't unroll statically).
                align_mir::TemplatePiece::StructArrayField { array, struct_id } => {
                    let agg = self.operand(array).into_struct_value();
                    let ptr = self.builder.build_extract_value(agg, 0, "sap").map_err(|e| self.err(e))?;
                    let len = self.builder.build_extract_value(agg, 1, "sal").map_err(|e| self.err(e))?;
                    let t = self.emit_desc_table(*struct_id);
                    let n = i64t.const_int(t.n_fields, false);
                    let esz = i64t.const_int(self.target_data.get_store_size(&self.struct_types[*struct_id as usize]), false);
                    self.builder
                        .build_call(
                            self.funcs["json_encode_struct_array"],
                            &[bptr.into(), ptr.into(), len.into(), t.descs.into(), n.into(), esz.into()],
                            "",
                        )
                        .map_err(|e| self.err(e))?;
                }
                // `json.encode` of an `array<scalar>` field (T1b): hand the owned buffer `{ptr,len}` and
                // the element scalar tag (`(kind<<8)|width|(signed<<16)`, computed from the element type)
                // to the runtime encoder, which loops emitting `[e0,e1,…]` (dynamic length can't unroll).
                align_mir::TemplatePiece::ScalarArrayField { array, elem } => {
                    let agg = self.operand(array).into_struct_value();
                    let ptr = self.builder.build_extract_value(agg, 0, "scap").map_err(|e| self.err(e))?;
                    let len = self.builder.build_extract_value(agg, 1, "scal").map_err(|e| self.err(e))?;
                    let null = self.ctx.ptr_type(AddressSpace::default()).const_null();
                    let (etag, _) = self.json_payload_tag_sub(scalar_to_ty(*elem), null);
                    let etag = self.ctx.i32_type().const_int(etag, false);
                    self.builder
                        .build_call(self.funcs["json_encode_scalar_array"], &[bptr.into(), ptr.into(), len.into(), etag.into()], "")
                        .map_err(|e| self.err(e))?;
                }
                // `json.encode` of a shape-directed union: materialize the enum value in memory (the
                // runtime encoder reads the tag + live payload at byte offsets), then emit its bare
                // payload via the descriptor-driven union encoder.
                align_mir::TemplatePiece::UnionValue { value, enum_id, .. } => {
                    let ety = self.enum_types[*enum_id as usize];
                    let v = self.operand(value);
                    // Hoist the scratch slot to the entry block so an `encode` inside a loop does not
                    // grow the stack per iteration (the shared `alloca_at_entry` discipline).
                    let slot = self.alloca_at_entry(ety.into(), "junion_v")?;
                    self.builder.build_store(slot, v).map_err(|e| self.err(e))?;
                    let union_desc = self.emit_json_union(*enum_id);
                    self.builder
                        .build_call(self.funcs["json_encode_union"], &[bptr.into(), slot.into(), union_desc.into()], "")
                        .map_err(|e| self.err(e))?;
                }
            }
        }
        let finish = if arena.is_some() { "builder_finish_stack" } else { "builder_into_string_stack" };
        Ok(self
            .builder
            .build_call(self.funcs[finish], &[bptr.into()], "s")
            .map_err(|e| self.err(e))?
            .try_as_basic_value()
            .basic()
            .expect("builder finish returns a string descriptor"))
    }

    /// All `array_builder<T>` (M12 A6) rvalues (new/push/push_str/append/build). `#[inline(never)]`
    /// so `gen_rvalue` stays flat (the #296 expr-depth lesson, mirroring `gen_file_rvalue`). Returns
    /// `Some` for the value-producing ops (new/build) and `None` for the void growth ops.
    #[inline(never)]
    fn gen_array_builder_rvalue(
        &mut self,
        result_id: ValueId,
        rv: &Rvalue,
    ) -> Result<Option<BasicValueEnum<'c>>, CodegenError> {
        match rv {
            // `array_builder<T>()` — open an empty typed builder sized to the element stride.
            Rvalue::ArrayBuilderNew { elem_size } => {
                let es = self.ctx.i64_type().const_int(*elem_size as u64, false);
                if let Some(slot) = self.stack_header_new_values.get(&result_id).copied() {
                    let header = self.stack_headers[&slot];
                    let v = self
                        .builder
                        .build_call(
                            self.funcs["array_builder_init_stack"],
                            &[header.into(), es.into()],
                            "ab.stack",
                        )
                        .map_err(|e| self.err(e))?
                        .try_as_basic_value()
                        .basic()
                        .expect("array_builder_init_stack returns a pointer");
                    return Ok(Some(v));
                }
                let v = self
                    .builder
                    .build_call(self.funcs["array_builder_new"], &[es.into()], "ab")
                    .map_err(|e| self.err(e))?
                    .try_as_basic_value().basic().expect("array_builder_new returns a pointer");
                Ok(Some(v))
            }
            // `b.push(v)` (Copy scalar) — reinterpret `v` to its raw i64 bits (a float bit-casts; a
            // narrower int/bool/char zero-extends, so its low `elem_size` bytes are its value), then
            // hand the bits to the runtime, which writes the element's `elem_size` low bytes.
            Rvalue::ArrayBuilderPush { builder, value, scalar } => {
                let bp = self.operand(builder).into();
                let i64t = self.ctx.i64_type();
                let bits = if matches!(scalar, Ty::Float(_)) {
                    let fv = self.operand(value).into_float_value();
                    let int_bits = match scalar { Ty::Float(FloatTy { bits: 32 }) => self.ctx.i32_type(), _ => i64t };
                    let as_int = self.builder.build_bit_cast(fv, int_bits, "fbits").map_err(|e| self.err(e))?.into_int_value();
                    self.builder.build_int_z_extend_or_bit_cast(as_int, i64t, "bits64").map_err(|e| self.err(e))?
                } else {
                    let iv = self.operand(value).into_int_value();
                    self.builder.build_int_z_extend_or_bit_cast(iv, i64t, "bits64").map_err(|e| self.err(e))?
                };
                self.builder
                    .build_call(self.funcs["array_builder_push"], &[bp, bits.into()], "")
                    .map_err(|e| self.err(e))?;
                Ok(None)
            }
            // `b.push(s)` (string element) — split the moved-in `string` `{ptr,len}` and hand it to the
            // runtime, which stores it as one element (its source slot was nulled at the move site).
            Rvalue::ArrayBuilderPushStr { builder, value } => {
                let bp = self.operand(builder).into();
                let agg = self.operand(value).into_struct_value();
                let ptr = self.builder.build_extract_value(agg, 0, "sptr").map_err(|e| self.err(e))?;
                let len = self.builder.build_extract_value(agg, 1, "slen").map_err(|e| self.err(e))?;
                self.builder
                    .build_call(self.funcs["array_builder_push_str"], &[bp, ptr.into(), len.into()], "")
                    .map_err(|e| self.err(e))?;
                Ok(None)
            }
            // `b.append(xs)` — hand the `slice<T>` `{ptr, count}` to the runtime, which bulk-copies
            // `count` elements at the builder's stored stride.
            Rvalue::ArrayBuilderAppend { builder, data } => {
                let bp = self.operand(builder).into();
                let (ptr, count) = self.split_str(data)?;
                self.builder
                    .build_call(self.funcs["array_builder_append"], &[bp, ptr.into(), count.into()], "")
                    .map_err(|e| self.err(e))?;
                Ok(None)
            }
            // `b.build()` — freeze into an owned `array<T>` `{ptr,len}` (zero-copy); the runtime hands
            // off the storage and frees only the builder header.
            Rvalue::ArrayBuilderBuild { builder } => {
                let bp = self.operand(builder).into();
                let build = if self.stack_header_slot_for_operand(builder).is_some() {
                    "array_builder_build_stack"
                } else {
                    "array_builder_build"
                };
                let v = self
                    .builder
                    .build_call(self.funcs[build], &[bp], "abbuild")
                    .map_err(|e| self.err(e))?
                    .try_as_basic_value().basic().expect("array_builder_build returns a {ptr,len}");
                Ok(Some(v))
            }
            _ => unreachable!("gen_array_builder_rvalue on a non-array_builder rvalue"),
        }
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

    /// `str < str` / `<=` / `>` / `>=` (`Ord(str)`) via `align_rt_str_cmp`, which returns -1/0/1
    /// (byte-lexicographic). The operator becomes a signed compare of that result against 0 —
    /// `a < b` ⇔ `cmp < 0`, `a <= b` ⇔ `cmp <= 0`, etc. Also backs `sort`'s `str`-key comparator
    /// (the sort loop lowers a `BinOp::Gt` on `str` operands, routed here by `gen_bin`).
    fn gen_str_cmp(&mut self, op: BinOp, a: &Operand, b: &Operand) -> Result<BasicValueEnum<'c>, CodegenError> {
        let sa = self.operand(a).into_struct_value();
        let sb = self.operand(b).into_struct_value();
        let ext = |b: &Builder<'c>, v: inkwell::values::StructValue<'c>, i, n| {
            b.build_extract_value(v, i, n)
        };
        let pa = ext(self.builder, sa, 0, "pa").map_err(|e| self.err(e))?;
        let la = ext(self.builder, sa, 1, "la").map_err(|e| self.err(e))?;
        let pb = ext(self.builder, sb, 0, "pb").map_err(|e| self.err(e))?;
        let lb = ext(self.builder, sb, 1, "lb").map_err(|e| self.err(e))?;
        let cmp = self
            .builder
            .build_call(self.funcs["str_cmp"], &[pa.into(), la.into(), pb.into(), lb.into()], "strcmp")
            .map_err(|e| self.err(e))?
            .try_as_basic_value()
            .basic()
            .expect("str_cmp returns i32")
            .into_int_value();
        let zero = self.ctx.i32_type().const_zero();
        let pred = match op {
            BinOp::Lt => IntPredicate::SLT,
            BinOp::Le => IntPredicate::SLE,
            BinOp::Gt => IntPredicate::SGT,
            BinOp::Ge => IntPredicate::SGE,
            _ => return Err(self.err("gen_str_cmp expects an ordering operator")),
        };
        let v = self.builder.build_int_compare(pred, cmp, zero, "strord").map_err(|e| self.err(e))?;
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
        let poison = vty.get_poison();
        let init = self.builder.build_insert_element(poison, scalar, self.ctx.i32_type().const_zero(), "splat_init").map_err(|e| self.err(e))?;
        let mask = self.ctx.i32_type().vec_type(n).const_zero();
        self.builder.build_shuffle_vector(init, poison, mask, "splat").map_err(|e| self.err(e))
    }

    /// Sum the `n` lanes of a vector into the element scalar (M6 reductions — `sum_where`, `dot`).
    /// An extract-and-add chain; the optimizer turns it into a hardware reduction.
    fn horizontal_sum(&self, v: inkwell::values::VectorValue<'c>, is_float: bool, n: u32) -> Result<BasicValueEnum<'c>, CodegenError> {
        // A vector type always has width ≥ 1 (`vecN` is 2/4/8/16) — guard the lane-0 extract below.
        assert!(n > 0, "vector width must be at least 1");
        if is_float {
            let start = v.get_type().get_element_type().into_float_type().const_zero();
            self.call_intrinsic("llvm.vector.reduce.fadd", &[v.get_type().into()], &[start.into(), v.into()])
        } else {
            self.call_intrinsic("llvm.vector.reduce.add", &[v.get_type().into()], &[v.into()])
        }
    }

    /// Reduce a `<N x i1>` mask to a scalar `i1` = true iff **any** lane is set (an OR-fold of the
    /// lanes). Powers the vector `/`/`%` divisor guard (`any(divisor == 0)` → abort). Matches the
    /// hand-folded style of `horizontal_sum`/`horizontal_minmax`.
    fn horizontal_or(&self, v: inkwell::values::VectorValue<'c>, n: u32) -> Result<IntValue<'c>, CodegenError> {
        assert!(n > 0, "vector width must be at least 1");
        Ok(self.call_intrinsic("llvm.vector.reduce.or", &[v.get_type().into()], &[v.into()])?.into_int_value())
    }

    /// Fold the `n` lanes of a vector into the element scalar with the scalar min/max intrinsic
    /// (M6 `v.min()`/`v.max()`) — the same `llvm.{s,u}{min,max}` / `llvm.{minimum,maximum}` as the
    /// `core.math` scalar `a.min(b)`/`a.max(b)`, so the reduction matches that semantics exactly.
    fn horizontal_minmax(&self, v: inkwell::values::VectorValue<'c>, elem: Ty, n: u32, max: bool) -> Result<BasicValueEnum<'c>, CodegenError> {
        assert!(n > 0, "vector width must be at least 1");
        let name = if matches!(elem, Ty::Float(_)) {
            if max { "llvm.vector.reduce.fmaximum" } else { "llvm.vector.reduce.fminimum" }
        } else if is_signed(elem) {
            if max { "llvm.vector.reduce.smax" } else { "llvm.vector.reduce.smin" }
        } else {
            if max { "llvm.vector.reduce.umax" } else { "llvm.vector.reduce.umin" }
        };
        self.call_intrinsic(name, &[v.get_type().into()], &[v.into()])
    }

    fn gen_vec_bin(&mut self, op: BinOp, a: &Operand, b: &Operand, elem: Ty, n: u32) -> Result<BasicValueEnum<'c>, CodegenError> {
        let l = self.operand_as_vector(a, elem, n)?;
        let r = self.operand_as_vector(b, elem, n)?;
        let bld = self.builder;
        // sema restricts vector ops to elementwise arithmetic `+`/`-`/`*`/`/`/`%`; guard here too so
        // any future sema hole is a clean codegen error, never a panic. The `/`/`%` divisor guard
        // (any zero lane aborts, signed `INT_MIN/-1` lane wraps) is emitted in MIR (`lower_vec_div`),
        // so the raw `sdiv`/`udiv`/`srem`/`urem` below is already fed a UB-free divisor.
        if !matches!(op, BinOp::Add | BinOp::Sub | BinOp::Mul | BinOp::Div | BinOp::Rem) {
            return Err(self.err("unsupported vector operator (only + - * / % are lowered)"));
        }
        let v = if matches!(elem, Ty::Float(_)) {
            match op {
                BinOp::Sub => bld.build_float_sub(l, r, "vfsub"),
                BinOp::Mul => bld.build_float_mul(l, r, "vfmul"),
                BinOp::Div => bld.build_float_div(l, r, "vfdiv"),
                BinOp::Rem => bld.build_float_rem(l, r, "vfrem"),
                _ => bld.build_float_add(l, r, "vfadd"),
            }
        } else {
            let signed = is_signed(elem);
            match op {
                BinOp::Sub => bld.build_int_sub(l, r, "vsub"),
                BinOp::Mul => bld.build_int_mul(l, r, "vmul"),
                BinOp::Div if signed => bld.build_int_signed_div(l, r, "vsdiv"),
                BinOp::Div => bld.build_int_unsigned_div(l, r, "vudiv"),
                BinOp::Rem if signed => bld.build_int_signed_rem(l, r, "vsrem"),
                BinOp::Rem => bld.build_int_unsigned_rem(l, r, "vurem"),
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

/// Run the LLVM middle-end optimization pipeline over `module` in place. Without this, only the
/// backend codegen passes run — the lifted-lambda calls, fused `map`/`reduce`/`where` loops, and
/// bounds checks are left un-inlined and un-vectorized. The `default<O2>` pipeline inlines the
/// per-element calls, hoists invariants (LICM), and runs the loop / SLP vectorizers, so the
/// data-oriented core actually lowers to SIMD. Purely additive — no IR is generated differently.
///
/// Shared by object emission (`write_object`, which threads the build [`Profile`]'s pipeline) and the
/// optimized-IR lens (`emit_llvm_ir(.., optimized = true)` / `collect_opt_remarks`, both pinned to the
/// release `"default<O2>"` view — those are diagnostic lenses, not builds, so they stay at the one
/// canonical "what release does" pipeline regardless of `--profile`; Slice 4).
fn run_opt_pipeline(module: &Module, tm: &TargetMachine, pipeline: &str) -> Result<(), CodegenError> {
    module
        .run_passes(pipeline, tm, PassBuilderOptions::create())
        .map_err(|e| CodegenError::Target(format!("optimization pipeline: {e}")))
}

/// Run `pipeline` (the build [`Profile`]'s stock `default<O*>` string) then emit the object. The
/// profile's *linker* choices (strip) are applied later, by the driver at link time.
fn write_object(module: &Module, out: &Path, tm: &TargetMachine, pipeline: &str) -> Result<(), CodegenError> {
    run_opt_pipeline(module, tm, pipeline)?;
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
        emit_llvm_ir(&lower_program(&hir), &BuildTarget::Baseline, false, &[], None).unwrap()
    }

    // -- Build profiles reach the backend (Codex audit item 3) ------------------------------------

    #[test]
    fn codegen_opt_level_maps_each_profile() {
        // The codegen (machine-code) opt level — a separate dimension from `pipeline()`. Swapping
        // any single entry fails. `small`/`tiny` stay at `Default` on purpose (clang parity: size
        // lives in the `optsize`/`minsize` attrs + `default<Os|Oz>` pipeline, not the codegen level).
        assert_eq!(Profile::Dev.codegen_opt_level(), OptimizationLevel::None);
        assert_eq!(Profile::Release.codegen_opt_level(), OptimizationLevel::Default);
        assert_eq!(Profile::Fast.codegen_opt_level(), OptimizationLevel::Aggressive);
        assert_eq!(Profile::Small.codegen_opt_level(), OptimizationLevel::Default);
        assert_eq!(Profile::Tiny.codegen_opt_level(), OptimizationLevel::Default);
    }

    /// Build a module for `profile`, run the size-attr sweep, and report `(name, is_decl, optsize,
    /// minsize)` for every function — enough to pin the sweep exactly (definitions vs declarations,
    /// optsize vs minsize).
    fn size_attr_probe(src: &str, profile: Profile) -> Vec<(String, bool, bool, bool)> {
        let mut d = Diagnostics::new();
        let toks = tokenize(0, src, &mut d);
        let f = parse_file(toks, &mut d);
        let hir = check_file(&f, &mut d);
        assert!(!d.has_errors());
        let program = lower_program(&hir);

        let ctx = Context::create();
        let module = ctx.create_module("align");
        let tm = create_target_machine(&BuildTarget::Baseline, OptimizationLevel::Default).unwrap();
        build_module(&ctx, &module, &program, &tm, None, &[], false).unwrap();
        apply_size_attrs(&ctx, &module, profile);

        let optsize = inkwell::attributes::Attribute::get_named_enum_kind_id("optsize");
        let minsize = inkwell::attributes::Attribute::get_named_enum_kind_id("minsize");
        let loc = inkwell::attributes::AttributeLoc::Function;
        module
            .get_functions()
            .map(|f| {
                let name = f.get_name().to_str().unwrap().to_string();
                let is_decl = f.count_basic_blocks() == 0;
                let has_optsize = f.get_enum_attribute(loc, optsize).is_some();
                let has_minsize = f.get_enum_attribute(loc, minsize).is_some();
                (name, is_decl, has_optsize, has_minsize)
            })
            .collect()
    }

    #[test]
    fn size_attrs_sweep_hits_definitions_only_per_profile() {
        // A definition (`helper`, `main`) plus the always-declared `align_rt_*` prelude — so both
        // sides of the definition/declaration split are present in the module.
        let src = "fn helper(x: i64) -> i64 = x + 1\n\
                   fn main() -> i32 {\n  print(helper(41))\n  return 0\n}\n";

        // Tiny: every definition carries BOTH optsize and minsize; every declaration stays bare.
        let tiny = size_attr_probe(src, Profile::Tiny);
        let mut saw_def = false;
        let mut saw_rt_decl = false;
        for (name, is_decl, optsize, minsize) in &tiny {
            if *is_decl {
                if name.starts_with("align_rt_") {
                    saw_rt_decl = true;
                }
                assert!(!optsize && !minsize, "tiny: declaration {name} must stay attr-free");
            } else {
                assert!(*optsize && *minsize, "tiny: definition {name} must carry optsize+minsize");
                saw_def = true;
            }
        }
        assert!(saw_def, "test needs at least one definition to be meaningful");
        assert!(saw_rt_decl, "test needs at least one align_rt_ declaration to be meaningful");

        // Small: definitions get optsize only, never minsize; declarations stay bare.
        for (name, is_decl, optsize, minsize) in size_attr_probe(src, Profile::Small) {
            if is_decl {
                assert!(!optsize && !minsize, "small: declaration {name} must stay attr-free");
            } else {
                assert!(optsize && !minsize, "small: definition {name} must carry optsize only");
            }
        }

        // The speed profiles are a no-op sweep — nothing gains a size attr anywhere.
        for profile in [Profile::Dev, Profile::Release, Profile::Fast] {
            for (name, _is_decl, optsize, minsize) in size_attr_probe(src, profile) {
                assert!(!optsize && !minsize, "{}: {name} must have no size attrs", profile.name());
            }
        }
    }

    fn allocation_case_ir(
        rv: Rvalue,
        value_ty: Ty,
        structs: Vec<StructDef>,
        optimized: bool,
        count_arg: bool,
    ) -> String {
        let program = Program {
            fns: vec![Function {
                name: if count_arg { "allocation_probe" } else { "main" }.to_string(),
                params: if count_arg { vec![0] } else { vec![] },
                ret: Ty::Int(IntTy { bits: 32, signed: true }),
                slots: if count_arg { vec![Ty::Int(IntTy { bits: 64, signed: true })] } else { vec![] },
                slot_align: if count_arg { vec![None] } else { vec![] },
                value_tys: vec![value_ty],
                blocks: vec![Block {
                    id: 0,
                    stmts: vec![Stmt::Let(0, rv)],
                    stmt_lines: vec![(0, 0)],
                    term: Term::Return(Some(Operand::Const(Const::Int(
                        0,
                        Ty::Int(IntTy { bits: 32, signed: true }),
                    )))),
                }],
                entry: 0,
                exportable: false,
            }],
            externs: vec![],
            imported_fns: vec![],
            link_libs: vec![],
            structs,
            enums: vec![],
            tuples: vec![],
        };
        emit_llvm_ir(&program, &BuildTarget::Baseline, optimized, &[], None).unwrap()
    }

    fn function_body<'a>(ir: &'a str, name: &str) -> &'a str {
        ir.find(&format!(" @{name}("))
            .map(|start| &ir[start..])
            .and_then(|tail| tail.split_once("{\n").map(|(_, body)| body))
            .and_then(|body| body.split("\n}").next())
            .unwrap_or_else(|| panic!("{name} body not found"))
    }

    fn arena_allocation_case_ir() -> String {
        let i64_ty = Ty::Int(IntTy { bits: 64, signed: true });
        let program = Program {
            fns: vec![Function {
                name: "arena_allocation_probe".to_string(),
                params: vec![0],
                ret: Ty::Int(IntTy { bits: 32, signed: true }),
                slots: vec![i64_ty],
                slot_align: vec![None],
                value_tys: vec![Ty::ArenaHandle, Ty::Box(Scalar::Str)],
                blocks: vec![Block {
                    id: 0,
                    stmts: vec![
                        Stmt::Let(0, Rvalue::ArenaBegin),
                        Stmt::Let(
                            1,
                            Rvalue::ArenaAlloc { handle: Operand::Value(0), count: Operand::Arg(0), elem: Ty::Str },
                        ),
                    ],
                    stmt_lines: vec![(0, 0), (0, 0)],
                    term: Term::Return(Some(Operand::Const(Const::Int(
                        0,
                        Ty::Int(IntTy { bits: 32, signed: true }),
                    )))),
                }],
                entry: 0,
                exportable: false,
            }],
            externs: vec![],
            imported_fns: vec![],
            link_libs: vec![],
            structs: vec![],
            enums: vec![],
            tuples: vec![],
        };
        emit_llvm_ir(&program, &BuildTarget::Baseline, false, &[], None).unwrap()
    }

    fn soa_allocation_case_ir(len: Operand, row: StructDef, optimized: bool) -> String {
        let i64_ty = Ty::Int(IntTy { bits: 64, signed: true });
        let dynamic = matches!(len, Operand::Arg(0));
        let program = Program {
            fns: vec![Function {
                name: if dynamic { "soa_allocation_probe" } else { "main" }.to_string(),
                params: if dynamic { vec![0] } else { vec![] },
                ret: Ty::Int(IntTy { bits: 32, signed: true }),
                slots: if dynamic { vec![i64_ty] } else { vec![] },
                slot_align: if dynamic { vec![None] } else { vec![] },
                value_tys: vec![Ty::ArenaHandle, Ty::Box(Scalar::Int(IntTy { bits: 8, signed: false }))],
                blocks: vec![Block {
                    id: 0,
                    stmts: vec![
                        Stmt::Let(0, Rvalue::ArenaBegin),
                        Stmt::Let(
                            1,
                            Rvalue::SoaAlloc {
                                handle: Operand::Value(0),
                                len,
                                struct_id: 0,
                            },
                        ),
                    ],
                    stmt_lines: vec![(0, 0), (0, 0)],
                    term: Term::Return(Some(Operand::Const(Const::Int(
                        0,
                        Ty::Int(IntTy { bits: 32, signed: true }),
                    )))),
                }],
                entry: 0,
                exportable: false,
            }],
            externs: vec![],
            imported_fns: vec![],
            link_libs: vec![],
            structs: vec![row],
            enums: vec![],
            tuples: vec![],
        };
        emit_llvm_ir(&program, &BuildTarget::Baseline, optimized, &[], None).unwrap()
    }

    #[test]
    fn dynamic_allocation_sizes_are_checked_before_allocator_calls() {
        let i64_ty = Ty::Int(IntTy { bits: 64, signed: true });
        let dynamic = allocation_case_ir(
            Rvalue::HeapAllocBuf { count: Operand::Arg(0), elem: Ty::Str },
            Ty::Box(Scalar::Str),
            vec![],
            false,
            true,
        );
        assert!(dynamic.contains("@llvm.umul.with.overflow.i64"), "missing checked multiply:\n{dynamic}");
        assert!(dynamic.contains("icmp slt i64 %0, 0"), "missing negative-count guard:\n{dynamic}");
        assert!(dynamic.contains("@align_rt_alloc_size_fail"), "missing cold allocation failure:\n{dynamic}");

        let arena = arena_allocation_case_ir();
        let arena_body = function_body(&arena, "arena_allocation_probe");
        assert!(arena_body.contains("@llvm.umul.with.overflow.i64"), "arena allocation lacks checked multiply:\n{arena_body}");
        assert!(arena_body.contains("@align_rt_alloc_size_fail"), "arena allocation lacks overflow failure:\n{arena_body}");

        let fit = i64::MAX as i128 / 16;
        let fit_ir = allocation_case_ir(
            Rvalue::HeapAllocBuf { count: Operand::Const(Const::Int(fit, i64_ty)), elem: Ty::Str },
            Ty::Box(Scalar::Str),
            vec![],
            true,
            false,
        );
        let fit_body = function_body(&fit_ir, "main");
        assert!(fit_body.contains("@align_rt_alloc(i64 9223372036854775792)"), "largest fitting str allocation changed:\n{fit_body}");
        assert!(!fit_body.contains("@align_rt_alloc_size_fail"), "largest fitting count must not fail:\n{fit_body}");

        let over_ir = allocation_case_ir(
            Rvalue::HeapAllocBuf { count: Operand::Const(Const::Int(fit + 1, i64_ty)), elem: Ty::Str },
            Ty::Box(Scalar::Str),
            vec![],
            true,
            false,
        );
        assert!(function_body(&over_ir, "main").contains("@align_rt_alloc_size_fail"), "one-over-limit must fail:\n{over_ir}");

        for count in [-1i128, 0] {
            let out = allocation_case_ir(
                Rvalue::HeapAllocBuf { count: Operand::Const(Const::Int(count, i64_ty)), elem: i64_ty },
                Ty::Box(Scalar::Int(IntTy { bits: 64, signed: true })),
                vec![],
                true,
                false,
            );
            let body = function_body(&out, "main");
            if count < 0 {
                assert!(body.contains("@align_rt_alloc_size_fail"), "negative count must fail:\n{body}");
            } else {
                assert!(!body.contains("@align_rt_alloc_size_fail"), "zero count must remain legal:\n{body}");
            }
        }

        let row = StructDef {
            name: "Row".to_string(),
            fields: vec![
                align_sema::hir::FieldDef { name: "tiny".to_string(), ty: Ty::Int(IntTy { bits: 8, signed: false }) },
                align_sema::hir::FieldDef { name: "wide".to_string(), ty: i64_ty },
            ],
            align: None,
            c_repr: false,
        };
        let soa_dynamic = soa_allocation_case_ir(Operand::Arg(0), row.clone(), false);
        let soa_dynamic_body = function_body(&soa_dynamic, "soa_allocation_probe");
        assert_eq!(soa_dynamic_body.matches("@llvm.umul.with.overflow.i64").count(), 2, "SoA allocation must check both column products:\n{soa_dynamic_body}");
        assert_eq!(soa_dynamic_body.matches("@llvm.uadd.with.overflow.i64").count(), 3, "SoA allocation must check column end, alignment bump, and total size:\n{soa_dynamic_body}");

        let soa = soa_allocation_case_ir(
            Operand::Const(Const::Int(i64::MAX as i128, i64_ty)),
            row,
            true,
        );
        assert!(function_body(&soa, "main").contains("@align_rt_alloc_size_fail"), "SoA alignment bump overflow must fail:\n{soa}");
    }

    /// Layout parity: the sema `(size, align)` computation (`align_sema::ty_size_align` /
    /// `struct_size_align`, which the huge-struct-copy lint trusts) must equal the **real** LLVM ABI
    /// size/alignment of the struct as codegen lays it out (descending-alignment field order via
    /// `logical_to_physical`). This pins the two independent hand-written layout computations
    /// (`field_abi_align` here vs `ty_size_align` in sema) against LLVM ground truth, so any future
    /// drift — or a new wider-aligned field type added to `is_field_ok` without updating both — fails
    /// loudly. Covers every valid struct-field type, mixed widths that force a reorder, `str`/`string`
    /// views, nested structs, and `layout(C)` (declaration order preserved). `align(N)` over-aligned
    /// structs are **included**: the over-alignment pads the type's *size* up (the tight-array-stride
    /// fix) but leaves the aggregate's own ABI *alignment* natural — so sema (which reports the natural
    /// alignment and the padded size) must still equal LLVM's `(abi_size, abi_alignment)` of the
    /// size-padded type. This pins the padding math on both sides.
    #[test]
    fn sema_and_codegen_struct_layout_agree() {
        fn i(bits: u8, signed: bool) -> Ty {
            Ty::Int(IntTy { bits, signed })
        }
        fn f(bits: u8) -> Ty {
            Ty::Float(FloatTy { bits })
        }
        // `Option<T>` field: `{ i8 tag, T }`. Pins the option-field layout dual (ty_size_align ↔
        // option_struct_type) across scalar / str / nested-struct payloads and reorder cases.
        fn opt(ty: Ty) -> Ty {
            Ty::Option(align_sema::ty_to_scalar(ty).expect("option payload is a scalar"))
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
        // An `align(N)` over-aligned struct (never `layout(C)`; over-alignment composes with either
        // order but the point here is the size padding).
        fn adef(name: &str, align: u32, fields: &[Ty]) -> StructDef {
            StructDef { align: Some(align), ..sdef(name, false, fields) }
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
            // `align(N)` over-aligned structs: the type's size is padded up to N (tight array stride),
            // its natural ABI alignment is unchanged.
            adef("A64", 64, &[i(64, true), i(64, true)]),   // nat (16,8) → size 64, align 8
            adef("A16", 16, &[i(32, true)]),                // nat (4,4)  → size 16, align 4
            adef("APage", 4096, &[i(64, true)]),            // nat (8,8)  → size 4096, align 8
            adef("A4", 4, &[i(64, true)]),                  // N <= natural: a no-op (size 8, align 8)
            adef("A32mix", 32, &[i(8, true), i(64, true), i(8, true)]), // reorder + pad (nat 24 → 32)
            // `layout(C)` composed with `align(N)` (the FFI case): decl order preserved, size padded.
            StructDef { align: Some(32), ..sdef("A32C", true, &[i(8, true), i(64, true), i(8, true)]) },
            // `Option<T>` fields (REST-gateway runway Slice B): every payload kind + a reorder case.
            sdef("Oi64", false, &[opt(i(64, true))]),                     // { i8, i64 } → (16, 8)
            sdef("Obool", false, &[opt(Ty::Bool)]),                      // { i8, i8 }  → (2, 1)
            sdef("Ostr", false, &[opt(Ty::Str)]),                        // { i8, {ptr,len} } → (24, 8)
            sdef("Ostruct", false, &[opt(Ty::Struct(2))]),               // { i8, Pair } → (12, 4)
            sdef("OMix", false, &[Ty::Bool, opt(i(64, true)), opt(Ty::Str), i(8, true)]), // reorder
            StructDef { align: None, c_repr: true, ..sdef("OStrC", true, &[Ty::Bool, opt(Ty::Str)]) }, // layout(C) + option
            // `array<T>` fields (REST-gateway runway Slice C): an owned `{ptr,len}` (16, 8) — pins the
            // array-field layout dual (a `array<Struct>` / `array<scalar>` field is a heap handle).
            sdef("ArrStruct", false, &[i(64, true), Ty::DynStructArray(2, Layout::Aos)]), // { i64, {ptr,len} }
            sdef("ArrScalar", false, &[Ty::Bool, Ty::DynArray(align_sema::Scalar::Int(IntTy { bits: 64, signed: true }))]),
            // Sum-type (`enum`) fields (JSON completeness J1b): an enum lowers to `{ i32 tag, payloads
            // flattened }`, so its field alignment/size must agree between sema (`enum_size_align`) and
            // LLVM. `EScalar`/`EStr`/`EObj` (enum ids 0/1/2 below) cover scalar-only (align 4),
            // `str`-view (align 8), and object (struct payload) shapes, plus a reorder (`SEnum1`).
            sdef("SEnumScalar", false, &[Ty::Enum(0)]),                  // { i32, i32, i8 } → (12, 4)
            sdef("SEnum1", false, &[Ty::Enum(0), i(64, true)]),          // reorder: i64 then enum
            sdef("SEnumStr", false, &[Ty::Enum(1)]),                     // { i32, {ptr,len} , i64 }
            sdef("SEnumObj", false, &[Ty::Bool, Ty::Enum(2)]),           // reorder: enum then bool
        ];

        // Sum-type layouts referenced by the `SEnum*` fields above. Built exactly as codegen builds
        // `enum_types`: a literal `{ i32 tag, <every variant's payload flattened in variant order> }`.
        fn sc_int(bits: u8, signed: bool) -> align_sema::Scalar {
            align_sema::Scalar::Int(IntTy { bits, signed })
        }
        fn edef(name: &str, variants: &[&[align_sema::Scalar]]) -> align_sema::hir::EnumDef {
            let mut field_base = 1u32;
            let variants = variants
                .iter()
                .enumerate()
                .map(|(k, &payload)| {
                    let v = align_sema::hir::EnumVariant { name: format!("V{k}"), payload: payload.to_vec(), field_base };
                    field_base += payload.len() as u32;
                    v
                })
                .collect();
            align_sema::hir::EnumDef { name: name.to_string(), variants }
        }
        let enums = vec![
            edef("EScalar", &[&[sc_int(32, true)], &[align_sema::Scalar::Bool]]), // { i32, i32, i8 }
            edef("EStr", &[&[align_sema::Scalar::Str], &[sc_int(64, true)]]),     // { i32, {ptr,len}, i64 }
            edef("EObj", &[&[align_sema::Scalar::Struct(2)], &[sc_int(32, true)]]), // { i32, Pair, i32 }
        ];

        let ctx = Context::create();
        let tm = create_target_machine(&BuildTarget::Baseline, OptimizationLevel::Default).expect("target machine");
        let td = tm.get_target_data();

        // Build the LLVM struct types exactly as `codegen` does (opaque, then body via the shared
        // `set_struct_body` — the same size-padding path production uses). Enum types are built first
        // (as literal `{ i32, payloads }` structs) so a struct field of enum type resolves.
        let struct_types: Vec<StructType> = structs.iter().map(|s| ctx.opaque_struct_type(&s.name)).collect();
        let enum_types: Vec<StructType> = enums
            .iter()
            .map(|e| {
                let mut fields: Vec<BasicTypeEnum> = vec![ctx.i32_type().into()];
                for v in &e.variants {
                    for &s in &v.payload {
                        fields.push(scalar_type(&ctx, scalar_to_ty(s), &struct_types, &[]));
                    }
                }
                ctx.struct_type(&fields, false)
            })
            .collect();
        for (s, st) in structs.iter().zip(&struct_types) {
            let perm = logical_to_physical(s, &structs, &enums);
            set_struct_body(&ctx, *st, s, &perm, &struct_types, &enum_types, &td);
        }

        for (id, s) in structs.iter().enumerate() {
            let st = struct_types[id];
            let llvm = (td.get_abi_size(&st), td.get_abi_alignment(&st) as u64);
            let sema = align_sema::struct_abi_layout(id as u32, &structs, &enums);
            assert_eq!(sema, llvm, "layout parity mismatch on `{}` (sema {sema:?} vs LLVM {llvm:?})", s.name);
        }
    }

    #[test]
    fn align_functions_are_marked_nounwind() {
        // Align functions never unwind, so codegen marks them `nounwind` (drops exception edges /
        // unwind tables, enables more inlining). Every Align-defined function carries it...
        let out = ir("fn sq(x: i64) -> i64 = x * x\nfn main() -> i32 = sq(7) as i32\n");
        // `sq` is a non-exported program fn → `internal` (M13 Slice 1); `main` is the C entry →
        // external (no linkage word). Both still carry the `#0` nounwind attribute group.
        assert!(out.contains("define internal i64 @sq(i64 %0) #0"));
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
            "align_rt_array_builder_new",
            "align_rt_par_map", // fresh output buffer
        ] {
            assert!(out.contains(&format!("declare noalias ptr @{sym}")), "want noalias on {sym}:\n{out}");
        }
        // Resolve a runtime declare's attribute group and test it for a substring. (Scoped to the
        // named symbol — a module-wide `out.contains` would now also catch the M13 Slice 5A pure-fn
        // attributes like `willreturn` on `align_rt_hash64`.)
        let group_has = |sym: &str, needle: &str| -> bool {
            let decl = out.lines().find(|l| l.contains(&format!("@{sym}("))).expect("decl present");
            let n = decl.rsplit('#').next().and_then(|s| s.trim().parse::<u32>().ok());
            match n {
                None => false, // no attribute group at all
                Some(n) => out
                    .lines()
                    .find(|l| l.starts_with(&format!("attributes #{n} = ")))
                    .is_some_and(|l| l.contains(needle)),
            }
        };
        // Single-shot allocators get `nofree` + `nounwind` — but deliberately NOT `willreturn` (they
        // `abort` on OOM, so asserting they always return would be a miscompile).
        for sym in [
            "align_rt_alloc",
            "align_rt_arena_begin",
            "align_rt_tg_begin",
            "align_rt_builder_new",
            "align_rt_array_builder_new",
        ] {
            assert!(group_has(sym, "nofree"), "want nofree on single-shot allocator {sym}:\n{out}");
            assert!(group_has(sym, "nounwind"), "want nounwind on allocator {sym}:\n{out}");
        }
        for sym in [
            "align_rt_alloc",
            "align_rt_arena_alloc",
            "align_rt_tg_alloc",
            "align_rt_builder_new",
            "align_rt_array_builder_new",
        ] {
            assert!(!group_has(sym, "willreturn"), "{sym} can abort — must NOT claim willreturn:\n{out}");
        }
        // The **bump** allocators must NOT carry `nofree`: growing the region `Vec::push`es the
        // chunk list, which can reallocate (free) memory allocated before the call.
        assert!(!group_has("align_rt_arena_alloc", "nofree"), "bump alloc must not be nofree:\n{out}");
        assert!(!group_has("align_rt_tg_alloc", "nofree"), "bump alloc must not be nofree:\n{out}");
    }

    #[test]
    fn proven_nonescaping_builder_headers_use_stack_storage() {
        let out = ir(
            "fn main() -> i32 {\n\
               b := builder()\n\
               b.write(\"x\")\n\
               s := b.to_string()\n\
               mut a: array_builder<i64> := array_builder()\n\
               a.push(7)\n\
               xs := a.build()\n\
               return (s.len() + xs.len()) as i32\n\
             }\n",
        );
        assert!(out.contains("call ptr @align_rt_builder_init_stack("), "local builder should use stack init:\n{out}");
        assert!(
            out.contains("call { ptr, i64 } @align_rt_builder_into_string_stack("),
            "local builder should consume its stack header:\n{out}"
        );
        assert!(
            out.contains("call ptr @align_rt_array_builder_init_stack("),
            "local array_builder should use stack init:\n{out}"
        );
        assert!(
            out.contains("call { ptr, i64 } @align_rt_array_builder_build_stack("),
            "local array_builder should consume its stack header:\n{out}"
        );
        assert!(!out.contains("call ptr @align_rt_builder_new("), "local builder must not box its header:\n{out}");
        assert!(
            !out.contains("call ptr @align_rt_array_builder_new("),
            "local array_builder must not box its header:\n{out}"
        );
        assert!(out.contains("alloca [64 x i8], align 16"), "want aligned caller header storage:\n{out}");
    }

    #[test]
    fn unaudited_rvalue_wrapping_builder_rejects_stack_header() {
        // Build deliberately invalid future-shaped MIR: sema currently forbids Builder inside an
        // Option, but a newly-added aggregate rvalue must fail closed here until its operands are
        // audited. This pins the safety property independently of today's surface type limits.
        let i64_ty = Ty::Int(IntTy { bits: 64, signed: true });
        let f = Function {
            name: "future_wrapper".into(),
            params: vec![],
            ret: Ty::Unit,
            slots: vec![Ty::Builder],
            slot_align: vec![None],
            value_tys: vec![Ty::Builder, Ty::Builder, Ty::Unit],
            blocks: vec![Block {
                id: 0,
                stmts: vec![
                    Stmt::Let(0, Rvalue::BuilderNew { capacity: Operand::Const(Const::Int(0, i64_ty)) }),
                    Stmt::Store(0, Operand::Value(0)),
                    Stmt::Let(1, Rvalue::Load(0)),
                    Stmt::Let(2, Rvalue::OptionSome(Operand::Value(1))),
                ],
                stmt_lines: vec![(0, 0); 4],
                term: Term::Return(None),
            }],
            entry: 0,
            exportable: false,
        };
        assert!(stack_header_plan(&f).slots.is_empty(), "an unaudited wrapper must retain the boxed ABI");
    }

    #[test]
    fn direct_chunks_len_and_index_do_not_materialize_headers() {
        let out = ir(
            "fn main() -> i32 {\n\
               xs := [1, 2, 3, 4, 5]\n\
               a := xs.chunks(2).len()\n\
               b := xs.chunks(2)[2].len()\n\
               return (a + b) as i32\n\
             }\n",
        );
        let main = function_body(&out, "main");
        assert!(!main.contains("@align_rt_chunks"), "direct consumers must not call the materializer:\n{main}");
        assert!(!main.contains("@align_rt_alloc"), "direct consumers must not allocate chunk headers:\n{main}");
        assert!(main.contains("sdiv i64"), "chunk count must be computed from source_len/n:\n{main}");
        assert!(
            main.contains("getelementptr inbounds i64"),
            "direct index must compute one source sub-view:\n{main}"
        );

        let stored = ir(
            "fn main() -> i32 {\n\
               xs := [1, 2, 3, 4, 5]\n\
               cs := xs.chunks(2)\n\
               return cs.len() as i32\n\
             }\n",
        );
        assert!(
            function_body(&stored, "main").contains("call { ptr, i64 } @align_rt_chunks("),
            "a stored chunks value must retain its owned materialized representation:\n{stored}"
        );
    }

    #[test]
    fn escaping_array_builder_keeps_boxed_header() {
        let out = ir(
            "fn make() -> array_builder<i64> {\n\
               return array_builder()\n\
             }\n\
             fn main() -> i32 {\n\
               mut b := make()\n\
               b.push(1)\n\
               return b.build().len() as i32\n\
             }\n",
        );
        assert!(out.contains("call ptr @align_rt_array_builder_new("), "escaping return must keep boxed header:\n{out}");
        assert!(
            !out.contains("call ptr @align_rt_array_builder_init_stack("),
            "escaping return must not use caller-owned stack storage:\n{out}"
        );
    }

    #[test]
    fn array_builder_crossing_user_call_stays_boxed() {
        let out = ir(
            "fn pass(b: array_builder<i64>) -> array_builder<i64> {\n\
               return b\n\
             }\n\
             fn main() -> i32 {\n\
               mut b: array_builder<i64> := array_builder()\n\
               b.push(1)\n\
               mut c := pass(b)\n\
               c.push(2)\n\
               return c.build().len() as i32\n\
             }\n",
        );
        assert!(out.contains("call ptr @align_rt_array_builder_new("), "call-crossing header must be boxed:\n{out}");
        assert!(
            !out.contains("call ptr @align_rt_array_builder_init_stack("),
            "a header passed to user code must not point into its caller's stack:\n{out}"
        );
    }

    #[test]
    fn stack_header_reassignment_reuses_storage_after_drop() {
        let out = ir(
            "fn main() -> i32 {\n\
               mut b := builder()\n\
               b.write(\"old\")\n\
               b = builder()\n\
               b.write(\"new\")\n\
               return 0\n\
             }\n",
        );
        assert_eq!(
            out.matches("alloca [64 x i8], align 16").count(),
            1,
            "one local must reuse one header buffer:\n{out}"
        );
        assert_eq!(
            out.matches("call ptr @align_rt_builder_init_stack(").count(),
            2,
            "both assignments must initialize the reusable header:\n{out}"
        );
        assert!(
            out.matches("call void @align_rt_builder_free_stack(").count() >= 2,
            "the old value and final unfinished value must both be dropped in place:\n{out}"
        );
        assert!(!out.contains("call ptr @align_rt_builder_new("), "reassignment must not fall back to Box:\n{out}");
    }

    #[test]
    fn unfinished_stack_headers_use_element_aware_drop() {
        let out = ir(
            "fn main() -> i32 {\n\
               b := builder()\n\
               b.write(\"text\")\n\
               mut nums: array_builder<i64> := array_builder()\n\
               nums.push(1)\n\
               mut names: array_builder<string> := array_builder()\n\
               names.push(\"owned\".clone())\n\
               return 0\n\
             }\n",
        );
        assert!(out.contains("call void @align_rt_builder_free_stack("), "unfinished builder must drop in place:\n{out}");
        assert!(
            out.contains("call void @align_rt_array_builder_free_stack("),
            "unfinished scalar array builder must shallow-free its payload:\n{out}"
        );
        assert!(
            out.contains("call void @align_rt_array_builder_free_strings_stack("),
            "unfinished string array builder must deep-free its payload:\n{out}"
        );
        assert!(!out.contains("call void @align_rt_builder_free("), "stack builder must not free caller storage:\n{out}");
        assert!(
            !out.contains("call void @align_rt_array_builder_free_strings("),
            "stack string array builder must not free caller storage as a Box:\n{out}"
        );
    }

    #[test]
    fn internal_template_builders_always_use_stack_headers() {
        let out = ir(
            "fn main() -> i32 {\n\
               n := 7\n\
               heap_view := template \"heap={n}\"\n\
               arena {\n\
                 arena_view := template \"arena={n}\"\n\
                 print(arena_view)\n\
               }\n\
               return heap_view.len() as i32\n\
             }\n",
        );
        assert_eq!(
            out.matches("alloca [64 x i8], align 16").count(),
            2,
            "each dynamic template expression needs one reusable entry buffer:\n{out}"
        );
        assert_eq!(
            out.matches("call ptr @align_rt_builder_init_stack(").count(),
            2,
            "both internal template builders must initialize in place:\n{out}"
        );
        assert!(
            out.contains("call { ptr, i64 } @align_rt_builder_into_string_stack("),
            "arena-free template must consume its in-place header into the synthetic owner:\n{out}"
        );
        assert!(
            out.contains("call { ptr, i64 } @align_rt_builder_finish_stack("),
            "arena template must consume its in-place header into arena storage:\n{out}"
        );
        assert!(!out.contains("call ptr @align_rt_builder_new("), "internal template headers never need a Box:\n{out}");
    }

    #[test]
    fn rt_contract_attrs_pin_encoding_and_curation() {
        // Every runtime builtin is declared unconditionally, so a trivial program emits them all.
        let out = ir("fn main() -> i32 = 0\n");

        // (1) The `memory(...)` encoding pin. `MEM_ARGMEM_READ` (raw value 1) is the `MemoryEffects`
        // bitmask; a version bump can shift the bits. Assert it prints in canonical textual form so
        // an LLVM upgrade that changes the encoding fails HERE (loud), not silently. hash64 is the
        // clean pure-finite reader: `memory(argmem: read)` + the pure-finite flags + `readonly` +
        // `captures(none)` on its byte-pointer param.
        //
        // NOTE (Codex audit item 9, 2026-07-13): the no-capture contract is now emitted as the modern
        // `captures(none)` attribute (LLVM 22's replacement for the removed `nocapture`), via the
        // `captures` kind id + value 0 (`CAPTURES_NONE`) — NOT the old `nocapture` name (which
        // resolves to kind id 0 on LLVM 22 and only printed as the bare, un-reparseable `none`
        // shorthand). This both fixes the `emit-llvm | llvm-as-22` textual round-trip (proven by the
        // `emitted_ir_round_trips_through_llvm_as` gate in `align_driver`) and keeps the same
        // optimization contract the A8 gate (`vectorize_shapes.rs`
        // `a8_hash64_loop_invariant_hoist_enables_vectorization`) depends on. The canonical printed
        // order is `ptr readonly captures(none)` (LLVM sorts param attrs by kind id: readonly=53 <
        // captures=92). The `memory` and pure-finite spellings are unchanged.
        assert!(
            out.contains("declare i64 @align_rt_hash64(ptr readonly captures(none), i64)"),
            "want readonly + captures(none) on hash64's ptr param:\n{out}"
        );
        let hash_attrs = attr_group_of(&out, "align_rt_hash64");
        assert!(hash_attrs.contains("memory(argmem: read)"), "hash64 memory encoding drifted:\n{hash_attrs}");
        for a in ["willreturn", "nofree", "nosync"] {
            assert!(hash_attrs.contains(a), "hash64 must carry {a}:\n{hash_attrs}");
        }
        // hash128 shares the treatment.
        assert!(attr_group_of(&out, "align_rt_hash128").contains("memory(argmem: read)"));

        // (2) The str compare/order family: same `memory(argmem: read)` + `readonly captures(none)` on
        // BOTH pointer operands (params 0 and 2).
        assert!(
            out.contains(
                "declare i32 @align_rt_str_cmp(ptr readonly captures(none), i64, ptr readonly captures(none), i64)"
            ),
            "want readonly + captures(none) on both str_cmp operands:\n{out}"
        );
        assert!(attr_group_of(&out, "align_rt_str_cmp").contains("memory(argmem: read)"));

        // (3) The feature-detect readers (utf8_valid, memchr-backed str_find): pure-finite flags +
        // `readonly captures(none)` params, but memory is WITHHELD (their dispatch reads/writes a
        // global CPU-feature cache — non-argument memory).
        let u = attr_group_of(&out, "align_rt_utf8_valid");
        assert!(u.contains("willreturn") && u.contains("nofree"), "utf8_valid keeps pure-finite flags:\n{u}");
        assert!(!u.contains("memory("), "utf8_valid must NOT claim a memory effect (feature-detect cache):\n{u}");
        assert!(
            out.contains("declare i32 @align_rt_utf8_valid(ptr readonly captures(none), i64)"),
            "want readonly + captures(none) on utf8_valid's ptr:\n{out}"
        );
        let sf = attr_group_of(&out, "align_rt_str_find");
        assert!(!sf.contains("memory("), "str_find (memchr dispatch cache) must not claim a memory effect:\n{sf}");

        // (3b) The hoisted repeated-needle plan (doc-13 §6.6). LIKE the one-shot `str_find`,
        // `str_finder_find` keeps the pure-finite flags + `readonly captures(none)` on BOTH pointers
        // (the plan at param 0, the haystack at param 1) but WITHHOLDS `memory(...)`: a one-byte
        // needle routes `Finder::find` through `crate::memchr`'s `unsafe_ifunc!` global dispatch
        // cache (non-argument memory), so an `argmem: read` claim would be a lie. A regression that
        // re-adds a memory-effects attribute here fails loudly.
        assert!(
            out.contains(
                "declare i64 @align_rt_str_finder_find(ptr readonly captures(none), ptr readonly captures(none), i64)"
            ),
            "want readonly + captures(none) on both str_finder_find pointers:\n{out}"
        );
        let ff = attr_group_of(&out, "align_rt_str_finder_find");
        assert!(
            !ff.contains("memory("),
            "str_finder_find must NOT claim a memory effect (one-byte needle hits memchr's dispatch cache):\n{ff}"
        );
        for a in ["willreturn", "nofree", "nosync"] {
            assert!(ff.contains(a), "str_finder_find must carry {a}:\n{ff}");
        }
        // `finder_new` is allocator-class: `noalias` return + `nounwind` + `nofree`, but NOT
        // `willreturn` (a `Box` allocation aborts on OOM). Same treatment as `array_builder_new`.
        assert!(
            out.contains("declare noalias ptr @align_rt_str_finder_new("),
            "finder_new must return noalias (allocator-class):\n{out}"
        );
        let fnew = attr_group_of(&out, "align_rt_str_finder_new");
        assert!(fnew.contains("nofree") && fnew.contains("nounwind"), "finder_new must be nofree+nounwind:\n{fnew}");
        assert!(!fnew.contains("willreturn"), "finder_new must NOT be willreturn (OOM aborts):\n{fnew}");
        // `finder_free` is a bare null-safe deallocator declare (free fns take no curated attrs).
        assert!(
            out.contains("declare void @align_rt_str_finder_free(ptr)\n"),
            "finder_free must be an attribute-free declare:\n{out}"
        );

        // (4) The abort family: `noreturn`, nothing else. Never `willreturn` (they diverge).
        for sym in [
            "align_rt_bounds_fail",
            "align_rt_range_fail",
            "align_rt_utf8_boundary_fail",
            "align_rt_div_fail",
            "align_rt_alloc_size_fail",
            "align_rt_process_exit",
            "align_rt_process_abort",
        ] {
            let g = attr_group_of(&out, sym);
            assert!(g.contains("noreturn"), "{sym} must be noreturn:\n{g}");
            assert!(!g.contains("willreturn"), "{sym} diverges — must NOT be willreturn:\n{g}");
        }

        // (5) Fail-safe default: a runtime symbol with no contract entry gets NO added attribute.
        // `align_rt_print_i64` is impure (writes stdout) and unlisted → a bare declare.
        assert!(
            out.contains("declare void @align_rt_print_i64(i64)\n"),
            "an unlisted runtime declare must stay attribute-free:\n{out}"
        );
    }

    /// Semantic pin (Codex audit item 9): assert the no-capture contract as an *attribute-kind
    /// query*, not a textual string match — the adoption record's "prefer semantic attr assertions
    /// over full-declaration string pins where practical". The pointer param of a memcmp-class reader
    /// must carry BOTH `readonly` and the modern `captures` attribute, and the `captures` payload must
    /// be exactly `captures(none)` (encoded value 0 = `CAPTURES_NONE` = `CaptureInfo::none()`), so the
    /// attribute is proven present and correct regardless of how LLVM chooses to *print* it.
    #[test]
    fn rt_contract_captures_none_is_present_by_kind_id() {
        let mut d = Diagnostics::new();
        let toks = tokenize(0, "fn main() -> i32 = 0\n", &mut d);
        let f = parse_file(toks, &mut d);
        let hir = check_file(&f, &mut d);
        assert!(!d.has_errors());
        let program = lower_program(&hir);

        let ctx = Context::create();
        let module = ctx.create_module("align");
        let tm = create_target_machine(&BuildTarget::Baseline, OptimizationLevel::Default).unwrap();
        build_module(&ctx, &module, &program, &tm, None, &[], false).unwrap();

        let captures = inkwell::attributes::Attribute::get_named_enum_kind_id("captures");
        let readonly = inkwell::attributes::Attribute::get_named_enum_kind_id("readonly");
        assert_ne!(captures, 0, "LLVM must recognize the `captures` attribute");
        // Document the exact LLVM-22 regression this hardening fixes: the removed `nocapture` name
        // resolves to kind id 0 (an unusable no-op), which is why we emit `captures` instead.
        assert_eq!(
            inkwell::attributes::Attribute::get_named_enum_kind_id("nocapture"),
            0,
            "LLVM 22 removed `nocapture` (kind id 0) — the reason item 9 emits `captures` directly"
        );

        let hash64 = module.get_function("align_rt_hash64").expect("hash64 declared");
        let p0 = inkwell::attributes::AttributeLoc::Param(0);
        assert!(hash64.get_enum_attribute(p0, readonly).is_some(), "hash64 ptr param must be readonly");
        let cap = hash64.get_enum_attribute(p0, captures).expect("hash64 ptr param must carry captures");
        assert_eq!(
            cap.get_enum_value(),
            CAPTURES_NONE,
            "the captures payload must be captures(none) (encoded value 0)"
        );
    }

    /// Fail-loud pin (Codex audit item 9): resolving an attribute name LLVM does not recognize must
    /// panic, never silently no-op. A bogus name is version-robust — it resolves to kind id 0 on
    /// every LLVM — so this proves `enum_kind_id` (the shared gate under `add_enum_attr` /
    /// `add_valued_enum_attr`) rejects the whole class of renamed/removed/typo'd attributes.
    #[test]
    #[should_panic(expected = "does not recognize the enum attribute")]
    fn enum_kind_id_panics_on_unknown_attribute() {
        let _ = enum_kind_id("definitely_not_a_real_llvm_attribute");
    }

    #[test]
    fn allocas_live_only_in_entry_blocks() {
        // M13 Slice 5B pin. Every `alloca` codegen emits must sit in its function's ENTRY block
        // (via `alloca_at_entry` / the entry-positioned SysV slot path) — a mid-function alloca in a
        // loop would be a stack leak and defeats mem2reg. This program has multiple non-entry blocks
        // (a counted loop + a bounds-fail branch) and stack slots, so it exercises the invariant.
        let src = "fn run(xs: slice<i64>) -> i64 = xs.map(dbl).sum()\n\
             fn dbl(x: i64) -> i64 = x * 2\n\
             fn main(args: array<str>) -> Result<(), Error> {\n  \
               a := [1, 2, 3, 4, 5, 6, 7, 8]\n  \
               s : slice<i64> := a[0..args.len()]\n  \
               print(run(s))\n  \
               return Ok(())\n\
             }\n";
        let out = ir(src);
        // Walk each `define`; within it, the entry block is everything before the SECOND label line
        // (`name:`). Assert no `alloca` appears at or after that second label.
        for func in out.split("\ndefine ").skip(1) {
            let body = func.split("\n}\n").next().unwrap_or(func);
            let mut labels = 0usize;
            let mut past_entry = false;
            for line in body.lines() {
                let t = line.trim_end();
                // A basic-block label line looks like `bb1:` / `entry:` — a single token ending in `:`.
                if t.ends_with(':') && !t.contains(' ') && !t.contains('=') {
                    labels += 1;
                    if labels >= 2 {
                        past_entry = true;
                    }
                }
                if past_entry {
                    assert!(
                        !line.contains(" = alloca "),
                        "alloca outside the entry block:\n{line}\nin:\n{body}"
                    );
                }
            }
        }
    }

    #[test]
    fn bool_and_tag_storage_forms_are_pinned() {
        // M13 Slice 5B pin — the canonical storage widths (verified 2026-07-11, this codegen):
        //   * `bool` is `i1` in BOTH SSA and its stack slot (never widened to `i8`).
        //   * `Result<T,E>` / `Option<T>` lower to a tagged struct with an `i8` discriminant.
        //   * a general user sum type lowers to a tagged struct with an `i32` discriminant.
        // A future codegen change to any of these fails here (they are load-bearing for ABI + layout).
        let bool_ir = ir("fn main() -> i32 {\n  b : bool := true\n  if b { return 1 }\n  return 0\n}\n");
        assert!(bool_ir.contains("alloca i1"), "bool slot must be i1:\n{bool_ir}");
        assert!(bool_ir.contains("store i1"), "bool store must be i1:\n{bool_ir}");
        assert!(bool_ir.contains("br i1"), "bool branch must be on i1:\n{bool_ir}");
        assert!(!bool_ir.contains("alloca i8"), "bool must not be widened to an i8 slot:\n{bool_ir}");

        // `main() -> Result<(), Error>` returns the tagged Result struct — its tag is `i8`.
        let res_ir = ir("fn main() -> Result<(), Error> {\n  return Ok(())\n}\n");
        assert!(
            res_ir.contains("{ i8,") || res_ir.contains("{ i8 }"),
            "Result must carry an i8 tag:\n{res_ir}"
        );

        // A user sum type lowers to a non-union tagged struct with an i32 tag.
        let sum_ir = ir("Shape { Circle(i64), Square(i64) }\n\
             fn area(s: Shape) -> i64 = match s {\n    Circle(r) => r,\n    Square(w) => w,\n  }\n\
             fn main() -> i64 = area(Shape.Circle(3))\n");
        assert!(sum_ir.contains("{ i32,"), "a user sum type must carry an i32 tag:\n{sum_ir}");
    }

    /// The textual attribute group `{ ... }` attached to the declaration of `@sym`, resolved through
    /// its `#N` reference. Empty string if the declare carries no attribute group.
    fn attr_group_of(ir: &str, sym: &str) -> String {
        let decl = ir.lines().find(|l| l.contains(&format!("@{sym}("))).unwrap_or_else(|| panic!("declare for {sym} not found:\n{ir}"));
        let Some(n) = decl.rsplit('#').next().and_then(|s| s.trim().parse::<u32>().ok()) else {
            return String::new();
        };
        ir.lines()
            .find(|l| l.starts_with(&format!("attributes #{n} = ")))
            .unwrap_or("")
            .to_string()
    }

    #[test]
    fn phf_hash_is_pinned() {
        // Pins the hash of a known input. Codegen (`phf_hash`), runtime (`json_phf_hash`) and the
        // shared `align_hash::phf_pinned_vector` all assert this same value — since all three call
        // the one `align_hash::wyhash`, the byte-match is structural; this canary just guards against
        // an accidental algorithm/seed edit slipping past.
        assert_eq!(phf_hash(b"score", 0), 0x1300_a50c_fadb_78d9);
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
    fn unit_fn_value_uses_void_indirect_call_abi() {
        let text = ir("fn noop() {}\nfn main() -> i32 {\n  f := noop\n  f()\n  return 0\n}\n");
        assert!(
            text.contains("define private void @\"noop$fnval\"(ptr"),
            "Unit thunk must return void:\n{text}"
        );
        assert!(
            text.contains("call void %cf(ptr %ce)"),
            "Unit fn value must be called through a void signature:\n{text}"
        );
        assert!(
            !text.contains("call i32 %cf(ptr %ce)"),
            "Unit fn value must not be called through an i32 signature:\n{text}"
        );
    }

    #[test]
    fn fib_emits_calls_and_branch() {
        let src = "fn fib(n: i64) -> i64 {\n  if n < 2 { return n }\n  return fib(n - 1) + fib(n - 2)\n}\n";
        let text = ir(src);
        assert!(text.contains("define internal i64 @fib(i64"), "got:\n{text}");
        assert!(text.contains("call i64 @fib"), "expected recursive calls:\n{text}");
        assert!(text.contains("icmp slt"), "expected signed comparison:\n{text}");
    }
}
