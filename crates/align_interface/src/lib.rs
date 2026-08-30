//! M15 separate compilation — Slice 1a (producer side): the per-unit **interface summary**, its
//! canonical serialization, and its interface / implementation hashes.
//!
//! A *unit* is one module (one `.align` file). Given a checked whole program, [`build_summaries`]
//! extracts ONE [`InterfaceSummary`] per unit: the exported (`pub`) signatures, full exported type
//! definitions, exported consts, the per-`pub`-fn effect bit, generic `pub` template bodies, and the
//! unit's capability set. It then computes two independent fingerprints (`docs/impl/07-roadmap.md`
//! M15 S1; `docs/impl/10-cache-first-optimization.md` §6.4):
//!
//! * `interface_hash` — over the canonical **interface surface** (signatures + type defs + consts +
//!   effect bits + generic template bodies). Consumers depend on THIS hash only, so a private-body
//!   edit that does not change the surface leaves it unchanged (the headline incrementality win).
//! * `impl_hash` — over the unit's own implementation. The per-unit producer fingerprints the
//!   complete structural MIR program consumed by codegen (functions, type tables, declarations,
//!   linkage, and alignment); consumers do not depend on it.
//!
//! ## Honest S1a compromises (recorded)
//!
//! * **Whole-program summary compatibility.** The legacy multi-module producer partitions function
//!   MIR by owning unit because its one program is not a codegen unit. The shipped per-unit producer
//!   replaces its selected unit's hash with [`codegen_impl_hash`], over the exact MIR program passed
//!   to codegen. The object cache only consumes the latter.
//! * **Effect of a generic `pub` fn.** The whole-program purity analysis runs over the *monomorphized*
//!   concrete functions, so a generic template has no concrete effect entry. Its body ships in the
//!   interface (C++-template-like) and the consumer recomputes the effect on instantiation, so the
//!   summary records [`Effect::Unknown`] for a generic fn (the fail-closed reservation value).
//! * **Effect fail-closed default.** A non-generic `pub` fn missing from the effect map is recorded
//!   [`Effect::Impure`] (never optimistically Pure) — the fail-closed rule.
//! * **Hash strength.** `interface_hash`/`impl_hash` are 128-bit non-cryptographic (see [`hash`]).
//!   Upgrade to a strong digest at the CAS boundary in S3.
//! * **Capabilities.** Attributed per unit by matching each MIR function to the unit that owns its
//!   base name (a monomorph / lifted thunk shares its template's unit). A MIR function matching no
//!   unit base falls back to the entry unit (conservative — the entry unit always links). Stored as
//!   data; **not** folded into `interface_hash` (capabilities are a link-summary concern, doc-10 §6.4).
//!
//! ## Interface self-containment (S1b, ENFORCED)
//!
//! Sema rejects a `pub` item whose signature references a NON-`pub` type — a `pub fn`'s params /
//! return, a `pub` struct's fields, and a `pub` sum type's payloads may name only `pub` types (a
//! `pub const`'s type is scalar / `str`-only, so it can never name a user type). This is the M15
//! completeness invariant: a private type reachable from the public interface would be named in a
//! summary WITHOUT its definition, so its layout change could not flip the unit's `interface_hash`
//! (a stale-object miscompile once summaries are consumed). With the rejection in place, every type
//! named in an interface summary is `pub` and therefore carried with its full definition — the
//! interface is self-contained. Enforced in `align_sema` (Pass 0a-2); see
//! `crates/align_driver/tests/pub_exposure.rs`.

mod codec;
mod hash;
mod owned_json;
pub mod static_artifact;

pub use codec::{
    deserialize, deserialize_for_target, encode_interface_surface, serialize, DecodeError,
    FORMAT_VERSION,
};
pub use hash::{Hash128, Hash128Stream};
pub use owned_json::{
    OwnedJsonGraphInterfaceEntry, OwnedJsonObjectFormat, OwnedJsonTarget,
    encode_owned_json_graph_descriptor, encode_owned_json_graph_envelope,
};
pub use static_artifact::{
    decode_static_artifact, decode_static_command, decode_static_query, encode_static_command,
    encode_static_query, static_artifact_digest, static_options_hash, BindRetention, BindingEntry, CanonicalContract,
    CanonicalDefinition, CanonicalDefinitionBody, CanonicalDefinitionKind, CanonicalField,
    CanonicalType, CanonicalVariant, CheckPolicy, CheckedColumnMeta, CheckedMetadata,
    CheckedParameterMeta, CheckedQueryEvidence, DeclaredColumnMeta, DeclaredParameterMeta,
    DecodedSpanEntry, Driver, DriverEntry, DriverRestriction, MetaNullability, MetaStatementClass,
    ParameterOccurrence, QueryMetaPlan, RewriteEntry, Span, SqlSourceIdentity, StaticArtifact,
    StaticArtifactError, StaticCommandArtifact, StaticOption, StaticOptionOwner, StaticOptionValue,
    StaticQueryArtifact, VerificationState, BINDER_ABI_VERSION, DECODER_ABI_VERSION,
    REWRITE_FORMAT_VERSION, STATIC_ARTIFACT_FORMAT_VERSION,
};

use std::collections::{HashMap, HashSet};

pub use align_ast::ParamMode;
pub use align_sema::hir::{ReturnBorrowSummary, ReturnRegionSummary};

/// The three-valued effect bit of a `pub` fn (mirrors [`align_sema::FnEffect`]): `Pure` = provably no
/// observable side effect; `Impure` = transitively performs I/O; `Unknown` = the analysis cannot prove
/// it Pure (an unknown-effect indirect call, or a generic template whose effect is derived on
/// instantiation). Both `Impure` and `Unknown` fail closed at a `par_map`/parallel boundary.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Effect {
    Pure,
    Impure,
    Unknown,
}

impl From<align_sema::FnEffect> for Effect {
    fn from(e: align_sema::FnEffect) -> Effect {
        match e {
            align_sema::FnEffect::Pure => Effect::Pure,
            align_sema::FnEffect::Impure => Effect::Impure,
            align_sema::FnEffect::Unknown => Effect::Unknown,
        }
    }
}

/// A span-free, id-free type reference in an interface. Types are recorded by **name** (source-level
/// paths, module-namespaced), never by process-local interner id, so the encoding is canonical across
/// runs (`docs/impl/10` §6.4: no process-local ids).
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum IType {
    /// A named type, optionally with generic arguments: `i64`, `Option<i32>`, `other.Point`.
    /// `path` is the dotted source path (`()` for unit); `args` are its generic arguments.
    Named { path: String, args: Vec<IType> },
    /// An anonymous tuple type `(T, U, ...)`.
    Tuple(Vec<IType>),
    /// A function-value type `fn(params) -> ret`.
    Fn {
        params: Vec<IParam>,
        ret: Box<IType>,
        return_borrow: ReturnBorrowSummary,
        return_region: ReturnRegionSummary,
        return_cleanup: align_sema::hir::ReturnCleanupAbi,
    },
}

/// One parameter of a `pub` signature. **Names are intentionally excluded** (Align calls are
/// positional — renaming a parameter is not an interface change); only the parameter mode and type
/// are ABI-relevant.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct IParam {
    pub mode: ParamMode,
    pub ty: IType,
}

/// A generic type parameter declaration (`T` or `T: Ord`).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ITypeParam {
    pub name: String,
    pub bound: Option<String>,
}

/// An exported (`pub`) function signature.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct IFnSig {
    /// The source-level (bare) function name — the key consumers reference.
    pub name: String,
    pub type_params: Vec<ITypeParam>,
    pub params: Vec<IParam>,
    pub ret: IType,
    pub return_borrow: ReturnBorrowSummary,
    pub return_region: ReturnRegionSummary,
    pub return_cleanup: align_sema::hir::ReturnCleanupAbi,
    /// The 3-valued effect bit (part of the interface — flipping Pure→Impure is an interface change).
    pub effect: Effect,
    /// Canonical parameter roots whose contained views may be transferred to parallel workers.
    pub parallel_transfer_params: Vec<u32>,
    /// Whether the producer source has the exact top-level `unsafe {}` body shape required of a
    /// resource Drop hook. This is semantic validation metadata, not an importable hook path.
    pub resource_hook_body: bool,
    /// For a generic `pub` template: the declaration's source text (the body is part of the
    /// interface, C++-template-like — editing it invalidates consumers). `None` for a non-generic fn
    /// (whose body lives in the implementation, not the interface).
    pub generic_body: Option<String>,
}

/// An exported (`pub`) struct definition. Field order is preserved (it is the layout).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct IStructDef {
    pub name: String,
    pub type_params: Vec<ITypeParam>,
    /// `(field name, field type)` in declaration (= layout) order.
    pub fields: Vec<(String, IType)>,
    /// A declared over-alignment in bytes (`align(N)`), or `None` for natural alignment.
    pub align: Option<u32>,
    /// `layout(C)` — a stable, C-compatible flat layout.
    pub c_repr: bool,
    /// For a generic `pub` template: the declaration's source text; `None` otherwise.
    pub generic_body: Option<String>,
}

/// An exported (`pub`) sum-type definition. Variant order is preserved (it is the tag order).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct IEnumDef {
    pub name: String,
    pub type_params: Vec<ITypeParam>,
    /// `(variant name, positional payload types)` in declaration (= tag) order.
    pub variants: Vec<(String, Vec<IType>)>,
    /// For a generic `pub` template: the declaration's source text; `None` otherwise.
    pub generic_body: Option<String>,
}

/// An exported nominal native-resource definition. The internal source hook is intentionally not
/// part of the consumer API; cleanup links through the producer-owned support thunk.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct IResourceDef {
    pub name: String,
    pub type_params: Vec<ITypeParam>,
    pub generic_arity: u32,
    pub representation_version: u32,
    pub drop_thunk: String,
    pub drop_abi_fingerprint: [u8; 16],
}

/// An exported (`pub`) compile-time constant.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct IConst {
    pub name: String,
    /// The declared type annotation, if written (`NAME: i32 := ...`).
    pub ty: Option<IType>,
    /// The value's source text (editing it is an interface change).
    pub value_src: String,
}

/// One unit's complete interface summary plus its two fingerprints.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct InterfaceSummary {
    /// The unit's module path (`main` for the entry module, else the imported name, e.g. `geom`).
    pub unit: String,
    /// Exported functions, **sorted by name** (a `pub` fn set — order is not semantic).
    pub fns: Vec<IFnSig>,
    /// Exported structs, sorted by name.
    pub structs: Vec<IStructDef>,
    /// Target-bound descriptors for every exported, concrete direct-owned JSON record, in the same
    /// name order as the selected subset of `structs`.
    pub owned_json_graphs: Vec<OwnedJsonGraphInterfaceEntry>,
    /// Exported sum types, sorted by name.
    pub enums: Vec<IEnumDef>,
    /// Exported native resources, sorted by nominal name.
    pub resources: Vec<IResourceDef>,
    /// Exported consts, sorted by name.
    pub consts: Vec<IConst>,
    /// The unit's capability set (gated external libraries its code needs), sorted. Link-summary
    /// data; NOT folded into `interface_hash`.
    pub capabilities: Vec<String>,
    /// Hash of the canonical interface surface (signatures + type defs + consts + effect bits +
    /// generic template bodies). Consumers depend on this ONLY.
    pub interface_hash: Hash128,
    /// Hash of the unit's implementation. Per-unit compilation hashes the complete structural MIR
    /// codegen input; the legacy whole-program summary producer partitions function MIR by unit.
    pub impl_hash: Hash128,
}

/// The codegen name of a function, matching `align_sema::mangle_fn`: plain in the entry module,
/// `module$fn` elsewhere. (Replicated rather than exported from sema — a two-line, load-bearing
/// convention; a drift is caught by the capability-attribution tests, which round-trip through it.)
fn mangle(module: &str, is_entry: bool, name: &str) -> String {
    if is_entry {
        name.to_string()
    } else {
        format!("{module}${name}")
    }
}

/// A dotted source path (`other.Point` → `"other.Point"`, `i64` → `"i64"`).
fn path_to_string(p: &align_ast::Path) -> String {
    p.segments.iter().map(|s| s.name.as_str()).collect::<Vec<_>>().join(".")
}

/// A UTF-8-safe source slice; empty string on any out-of-range / non-boundary index (never panics —
/// spans are token boundaries, but a malformed input must not crash the producer).
fn safe_slice(src: &str, span: align_span::Span) -> String {
    src.get(span.lo as usize..span.hi as usize).unwrap_or("").to_string()
}

fn convert_type(t: &align_ast::Type) -> IType {
    match t {
        align_ast::Type::Named { path, args, .. } => {
            IType::Named { path: path_to_string(path), args: args.iter().map(convert_type).collect() }
        }
        align_ast::Type::Tuple { elems, .. } => IType::Tuple(elems.iter().map(convert_type).collect()),
        align_ast::Type::Fn { params, ret, .. } => IType::Fn {
            params: params
                .iter()
                .map(|p| IParam {
                    mode: p.mode,
                    ty: convert_type(&p.ty),
                })
                .collect(),
            ret: Box::new(convert_type(ret)),
            return_borrow: ReturnBorrowSummary::None,
            return_region: ReturnRegionSummary::None,
            return_cleanup: align_sema::hir::ReturnCleanupAbi::None,
        },
    }
}

/// The unit type sentinel (`()`), matching the AST's `Named` unit path — used for an omitted return.
fn unit_type() -> IType {
    IType::Named { path: "()".to_string(), args: Vec::new() }
}

fn convert_ret(ret: &Option<align_ast::Type>) -> IType {
    match ret {
        Some(t) => convert_type(t),
        None => unit_type(),
    }
}

fn resolved_owned_json_type(
    ty: align_sema::Ty,
    structs: &[align_sema::hir::StructDef],
) -> IType {
    let named = |path: String, args: Vec<IType>| IType::Named { path, args };
    match ty {
        align_sema::Ty::Int(value) => named(value.name(), Vec::new()),
        align_sema::Ty::Bool => named("bool".to_string(), Vec::new()),
        align_sema::Ty::String => named("string".to_string(), Vec::new()),
        align_sema::Ty::Struct(id) => structs
            .get(id as usize)
            .map(|definition| named(definition.name.clone(), Vec::new()))
            .unwrap_or_else(|| named("__unknown_owned_json_record".to_string(), Vec::new())),
        align_sema::Ty::Option(payload) => named(
            "Option".to_string(),
            vec![resolved_owned_json_type(align_sema::scalar_to_ty(payload), structs)],
        ),
        align_sema::Ty::DynArray(element) => named(
            "array".to_string(),
            vec![resolved_owned_json_type(align_sema::scalar_to_ty(element), structs)],
        ),
        align_sema::Ty::DynStructArray(id, align_sema::Layout::Aos) => named(
            "array".to_string(),
            vec![resolved_owned_json_type(align_sema::Ty::Struct(id), structs)],
        ),
        _ => named("__unsupported_owned_json_type".to_string(), Vec::new()),
    }
}

fn resolved_owned_json_structs(program: &align_sema::hir::Program) -> Vec<IStructDef> {
    program
        .structs
        .iter()
        .map(|definition| IStructDef {
            name: definition.name.clone(),
            type_params: Vec::new(),
            fields: definition
                .fields
                .iter()
                .map(|field| {
                    (
                        field.name.clone(),
                        resolved_owned_json_type(field.ty, &program.structs),
                    )
                })
                .collect(),
            align: definition.align,
            c_repr: definition.c_repr,
            generic_body: None,
        })
        .collect()
}

fn apply_function_cleanup_metadata(
    interface: &mut IType,
    resolved: align_sema::Ty,
    program: &align_sema::hir::Program,
) {
    match (interface, resolved) {
        (
            IType::Fn {
                params,
                ret,
                return_cleanup,
                ..
            },
            align_sema::Ty::Fn(id),
        ) => {
            let Some(definition) = program.fn_types.get(id as usize) else {
                return;
            };
            *return_cleanup = definition.return_cleanup;
            for (parameter, &(_, scalar)) in params.iter_mut().zip(&definition.params) {
                apply_function_cleanup_metadata(
                    &mut parameter.ty,
                    align_sema::scalar_to_ty(scalar),
                    program,
                );
            }
            apply_function_cleanup_metadata(ret, definition.ret, program);
        }
        (IType::Tuple(elements), align_sema::Ty::Tuple(id)) => {
            if let Some(definition) = program.tuples.get(id as usize) {
                for (element, &scalar) in elements.iter_mut().zip(&definition.elems) {
                    apply_function_cleanup_metadata(
                        element,
                        align_sema::scalar_to_ty(scalar),
                        program,
                    );
                }
            }
        }
        (IType::Named { args, .. }, resolved) => {
            let actuals: Vec<align_sema::Ty> = match resolved {
                align_sema::Ty::Option(value)
                | align_sema::Ty::Box(value)
                | align_sema::Ty::Slice(value)
                | align_sema::Ty::DynArray(value)
                | align_sema::Ty::Task(value)
                | align_sema::Ty::Array(value, _)
                | align_sema::Ty::Vec(value, _)
                | align_sema::Ty::Mask(value, _) => {
                    vec![align_sema::scalar_to_ty(value)]
                }
                align_sema::Ty::ArrayBuilder(value) => {
                    vec![align_sema::scalar_to_ty(value)]
                }
                ty @ (align_sema::Ty::VecArrayBuilder(..)
                | align_sema::Ty::MaskArrayBuilder(..)
                | align_sema::Ty::FixedArrayBuilder(..)
                | align_sema::Ty::FixedStructArrayBuilder(..)) => vec![ty
                    .array_builder_element()
                    .expect("matched aggregate builder")
                    .ty()],
                ty @ (align_sema::Ty::DynVecArray(..)
                | align_sema::Ty::DynMaskArray(..)
                | align_sema::Ty::DynFixedArray(..)
                | align_sema::Ty::DynFixedStructArray(..)) => vec![ty
                    .dyn_aggregate_array_element()
                    .expect("matched aggregate array")
                    .ty()],
                align_sema::Ty::Result(ok, err) => vec![
                    align_sema::scalar_to_ty(ok),
                    align_sema::scalar_to_ty(err),
                ],
                _ => Vec::new(),
            };
            for (argument, actual) in args.iter_mut().zip(actuals) {
                apply_function_cleanup_metadata(argument, actual, program);
            }
        }
        _ => {}
    }
}

fn convert_type_params(tps: &[align_ast::TypeParam]) -> Vec<ITypeParam> {
    tps.iter()
        .map(|tp| ITypeParam { name: tp.name.name.clone(), bound: tp.bound.as_ref().map(|b| b.name.clone()) })
        .collect()
}

fn is_pub(vis: align_ast::Vis) -> bool {
    matches!(vis, align_ast::Vis::Pub)
}

/// Build one [`InterfaceSummary`] per unit from a checked whole program.
///
/// * `modules` — the units, exactly as passed to [`align_sema::check_program`] (the AST is the source
///   of visibility / `out` markers / generics / consts, none of which survive into the checked
///   whole-program HIR).
/// * `program` — the checked whole-program HIR (the source of the per-fn effect bits).
/// * `mir` — the whole-program MIR (the source of truth for capability classification).
/// * `sources` — each unit's full source text, keyed by module path (for generic template bodies and
///   const values). A missing entry degrades those fields to empty (never panics).
pub fn build_summaries(
    modules: &[align_sema::Module],
    program: &align_sema::hir::Program,
    mir: &align_mir::Program,
    sources: &HashMap<String, String>,
    target: &OwnedJsonTarget,
) -> Result<Vec<InterfaceSummary>, String> {
    build_summaries_with_effects(
        modules,
        program,
        mir,
        sources,
        &HashMap::new(),
        target,
    )
}

/// Fingerprint the complete MIR program passed to one object-codegen invocation.
///
/// The human-readable MIR printer intentionally omits backend inputs such as struct/enum/tuple
/// tables, declarations, linkage, and slot alignment. Hashing only that view allowed a cache hit to
/// skip a cold-path codegen failure after one of those omitted inputs changed. The structural
/// rendering covers the complete [`align_mir::Program`]; the cache key separately namespaces the
/// compiler build and schema, so this representation need not be stable across compiler versions.
pub fn codegen_impl_hash(mir: &align_mir::Program) -> Hash128 {
    Hash128::of(align_mir::print::codegen_input_to_string(mir).as_bytes())
}

/// Like [`build_summaries`], but folds `external_effects` (M15 S1b: the effect bits of imported
/// non-generic `pub` functions whose bodies are not in `program`) into the per-unit effect
/// classification, so a unit's own `pub` fn that transitively calls an impure imported fn is recorded
/// impure in its summary. The whole-program producer path passes an empty map (all callees are in
/// `program`); the per-unit producer passes the union of its transitive dependencies' effect bits.
pub fn build_summaries_with_effects(
    modules: &[align_sema::Module],
    program: &align_sema::hir::Program,
    mir: &align_mir::Program,
    sources: &HashMap<String, String>,
    external_effects: &HashMap<String, align_sema::FnEffect>,
    target: &OwnedJsonTarget,
) -> Result<Vec<InterfaceSummary>, String> {
    let effects: HashMap<String, Effect> = align_sema::fn_effects(program, external_effects)
        .into_iter()
        .map(|(k, v)| (k, v.into()))
        .collect();
    let return_provenance: HashMap<
        &str,
        (&ReturnBorrowSummary, &ReturnRegionSummary, align_sema::hir::ReturnCleanupAbi),
    > = program
        .fns
        .iter()
        .map(|function| {
            (
                function.name.as_str(),
                (&function.return_borrow, &function.return_region, function.return_cleanup),
            )
        })
        .chain(program.imported_fns.iter().map(|function| {
            (
                function.name.as_str(),
                (&function.return_borrow, &function.return_region, function.return_cleanup),
            )
        }))
        .collect();
    let parallel_transfer: HashMap<&str, Vec<u32>> = program
        .fns
        .iter()
        .map(|function| {
            let params = match &function.parallel_transfer {
                ReturnBorrowSummary::None => Vec::new(),
                ReturnBorrowSummary::Roots { params, .. } => params.clone(),
            };
            (function.name.as_str(), params)
        })
        .chain(program.imported_fns.iter().map(|function| {
            (
                function.name.as_str(),
                function.parallel_transfer_params.clone(),
            )
        }))
        .collect();
    let caps_by_unit = partition_capabilities(modules, mir);
    let impl_hash_by_unit = partition_impl_hashes(modules, mir);

    let mut summaries = Vec::with_capacity(modules.len());
    for m in modules {
        let empty = String::new();
        let src = sources.get(&m.path).unwrap_or(&empty);

        let mut fns: Vec<IFnSig> = Vec::new();
        let mut structs: Vec<IStructDef> = Vec::new();
        let mut enums: Vec<IEnumDef> = Vec::new();
        let mut resources: Vec<IResourceDef> = Vec::new();
        let mut consts: Vec<IConst> = Vec::new();

        for item in &m.file.items {
            // Exhaustive over `align_ast::Item` on purpose (no `_` catch-all): a new variant must be
            // triaged here explicitly rather than silently dropped from the interface surface.
            match item {
                align_ast::Item::Fn(fd) => {
                    if is_pub(fd.vis) {
                        let is_generic = !fd.type_params.is_empty();
                        let effect = if is_generic {
                            // A generic template's effect is derived by the consumer on instantiation;
                            // its body ships in `generic_body`. Reserve Unknown.
                            Effect::Unknown
                        } else {
                            let canonical = mangle(&m.path, m.is_entry, &fd.name.name);
                            // Fail-closed: a non-generic pub fn missing from the effect map is Impure.
                            effects.get(&canonical).copied().unwrap_or(Effect::Impure)
                        };
                        let canonical = mangle(&m.path, m.is_entry, &fd.name.name);
                        let (
                            return_borrow,
                            return_region,
                            return_cleanup,
                            parallel_transfer_params,
                        ) = if is_generic {
                            (
                                ReturnBorrowSummary::None,
                                ReturnRegionSummary::None,
                                align_sema::hir::ReturnCleanupAbi::None,
                                Vec::new(),
                            )
                        } else {
                            return_provenance
                                .get(canonical.as_str())
                                .map(|(borrow, region, cleanup)| {
                                    (
                                        (*borrow).clone(),
                                        (*region).clone(),
                                        *cleanup,
                                        parallel_transfer
                                            .get(canonical.as_str())
                                            .cloned()
                                            .unwrap_or_default(),
                                    )
                                })
                                .unwrap_or((
                                    ReturnBorrowSummary::None,
                                    ReturnRegionSummary::None,
                                    align_sema::hir::ReturnCleanupAbi::None,
                                    Vec::new(),
                                ))
                        };
                        let mut params = fd
                            .params
                            .iter()
                            .map(|parameter| IParam {
                                mode: parameter.mode,
                                ty: convert_type(&parameter.ty),
                            })
                            .collect::<Vec<_>>();
                        let mut ret = convert_ret(&fd.ret);
                        if !is_generic
                            && let Some(function) =
                                program.fns.iter().find(|function| function.name == canonical)
                        {
                            for (parameter, local) in params.iter_mut().zip(&function.params) {
                                if let Some(local) = function.locals.get(*local as usize) {
                                    apply_function_cleanup_metadata(
                                        &mut parameter.ty,
                                        local.ty,
                                        program,
                                    );
                                }
                            }
                            apply_function_cleanup_metadata(&mut ret, function.ret, program);
                        }
                        fns.push(IFnSig {
                            name: fd.name.name.clone(),
                            type_params: convert_type_params(&fd.type_params),
                            params,
                            ret,
                            return_borrow,
                            return_region,
                            return_cleanup,
                            effect,
                            parallel_transfer_params,
                            resource_hook_body: align_sema::resource_hook_has_unsafe_body(&fd.body),
                            generic_body: is_generic.then(|| safe_slice(src, fd.span)),
                        });
                    }
                    // Non-pub fns are module-private: not part of the exported interface surface.
                }
                align_ast::Item::Struct(sd) => {
                    if is_pub(sd.vis) {
                        let is_generic = !sd.type_params.is_empty();
                        let mut fields = sd
                            .fields
                            .iter()
                            .map(|f| (f.name.name.clone(), convert_type(&f.ty)))
                            .collect::<Vec<_>>();
                        if !is_generic {
                            let canonical = mangle(&m.path, m.is_entry, &sd.name.name);
                            if let Some(definition) = program
                                .structs
                                .iter()
                                .find(|definition| definition.source_name == canonical)
                            {
                                for ((_, interface), resolved) in
                                    fields.iter_mut().zip(&definition.fields)
                                {
                                    apply_function_cleanup_metadata(
                                        interface,
                                        resolved.ty,
                                        program,
                                    );
                                }
                            }
                        }
                        structs.push(IStructDef {
                            name: sd.name.name.clone(),
                            type_params: convert_type_params(&sd.type_params),
                            fields,
                            align: sd.align,
                            c_repr: sd.c_repr,
                            generic_body: is_generic.then(|| safe_slice(src, sd.span)),
                        });
                    }
                    // Non-pub structs are module-private: not part of the exported interface surface.
                }
                align_ast::Item::Enum(ed) => {
                    if is_pub(ed.vis) {
                        let is_generic = !ed.type_params.is_empty();
                        let mut variants = ed
                            .variants
                            .iter()
                            .map(|v| {
                                (
                                    v.name.name.clone(),
                                    v.payload.iter().map(convert_type).collect::<Vec<_>>(),
                                )
                            })
                            .collect::<Vec<_>>();
                        if !is_generic {
                            let canonical = mangle(&m.path, m.is_entry, &ed.name.name);
                            if let Some(definition) = program
                                .enums
                                .iter()
                                .find(|definition| definition.source_name == canonical)
                            {
                                for ((_, interface_payload), resolved_variant) in
                                    variants.iter_mut().zip(&definition.variants)
                                {
                                    for (interface, &resolved) in
                                        interface_payload.iter_mut().zip(&resolved_variant.payload)
                                    {
                                        apply_function_cleanup_metadata(
                                            interface,
                                            align_sema::scalar_to_ty(resolved),
                                            program,
                                        );
                                    }
                                }
                            }
                        }
                        enums.push(IEnumDef {
                            name: ed.name.name.clone(),
                            type_params: convert_type_params(&ed.type_params),
                            variants,
                            generic_body: is_generic.then(|| safe_slice(src, ed.span)),
                        });
                    }
                    // Non-pub enums are module-private: not part of the exported interface surface.
                }
                align_ast::Item::Resource(resource) => {
                    if is_pub(resource.vis) {
                        let canonical = mangle(&m.path, m.is_entry, &resource.name.name);
                        let resolved = program.resources.iter().find(|definition| {
                            definition.source_name == canonical || definition.name == canonical
                        });
                        resources.push(IResourceDef {
                            name: resource.name.name.clone(),
                            type_params: convert_type_params(&resource.type_params),
                            generic_arity: resource.type_params.len() as u32,
                            representation_version: resolved
                                .map_or(1, |definition| definition.representation_version),
                            drop_thunk: resolved.map_or_else(
                                || format!("__align_resource_drop${canonical}"),
                                |definition| definition.drop_thunk.clone(),
                            ),
                            drop_abi_fingerprint: resolved.map_or(
                                *b"align-res-drop-1",
                                |definition| definition.drop_abi_fingerprint,
                            ),
                        });
                    }
                }
                align_ast::Item::Const(cd) => {
                    if is_pub(cd.vis) {
                        consts.push(IConst {
                            name: cd.name.name.clone(),
                            ty: cd.ty.as_ref().map(convert_type),
                            value_src: safe_slice(src, cd.value.span),
                        });
                    }
                    // Non-pub consts are module-private: not part of the exported interface surface.
                }
                align_ast::Item::Extern(..) => {}
                // extern fns are import-only (a bodyless FFI declaration bound to a C symbol), never
                // part of a unit's exported interface. (An `extern "C"` import is a link/impl concern;
                // exporting a body via `extern "C"` is explicitly out of M15.)
            }
        }

        // Canonicalize: exported item lists are sets — sort by name so the encoding is independent of
        // declaration order. (Field / variant / param order stays as-is — it is semantic.)
        fns.sort_by(|a, b| a.name.cmp(&b.name));
        structs.sort_by(|a, b| a.name.cmp(&b.name));
        enums.sort_by(|a, b| a.name.cmp(&b.name));
        resources.sort_by(|a, b| a.name.cmp(&b.name));
        consts.sort_by(|a, b| a.name.cmp(&b.name));

        let resolved_structs = resolved_owned_json_structs(program);
        let resolved_roots = structs
            .iter()
            .filter(|definition| definition.type_params.is_empty())
            .map(|definition| {
                let canonical = mangle(&m.path, m.is_entry, &definition.name);
                let root = program
                    .structs
                    .iter()
                    .position(|candidate| candidate.source_name == canonical)
                    .ok_or_else(|| {
                        format!(
                            "exported owned JSON record '{}' has no resolved definition",
                            definition.name
                        )
                    })?;
                Ok((definition.name.clone(), root))
            })
            .collect::<Result<Vec<_>, String>>()?;
        let owned_json_graphs = owned_json::entries_for_resolved_structs(
            &resolved_structs,
            &resolved_roots,
            target,
        )
            .ok_or_else(|| format!("unit '{}' has an unencodable owned JSON record", m.path))?;

        let mut capabilities = caps_by_unit.get(&m.path).cloned().unwrap_or_default();
        capabilities.sort();
        capabilities.dedup();

        // Assemble without hashes, compute them, then fill them in.
        let mut summary = InterfaceSummary {
            unit: m.path.clone(),
            fns,
            structs,
            owned_json_graphs,
            enums,
            resources,
            consts,
            capabilities,
            interface_hash: Hash128 { lo: 0, hi: 0 },
            impl_hash: Hash128 { lo: 0, hi: 0 },
        };
        summary.interface_hash = Hash128::of(&codec::encode_interface_surface(&summary));
        // Legacy whole-program summary hash: attribute stable-printed function MIR to each source
        // unit. The per-unit driver replaces this value with `codegen_impl_hash` over the exact
        // structural MIR program that feeds object codegen; only that stronger value reaches S3.
        // A unit whose whole-program MIR cannot be separated falls back to source bytes.
        summary.impl_hash =
            impl_hash_by_unit.get(&m.path).copied().unwrap_or_else(|| Hash128::of(src.as_bytes()));
        summaries.push(summary);
    }
    Ok(summaries)
}

/// Attribute each MIR function's capabilities to the unit that owns its base name, unioning per unit.
/// A monomorph (`base$i64`) / lifted thunk (`base$lambda0`) shares its template's unit via the base
/// prefix; longest-base-match disambiguates a `foo` vs `foo$bar` base pair. A function matching no
/// unit base falls back to the entry unit (conservative — the entry unit always links).
fn partition_capabilities(
    modules: &[align_sema::Module],
    mir: &align_mir::Program,
) -> HashMap<String, Vec<String>> {
    // base canonical fn name -> owning unit path.
    let mut base_to_unit: HashMap<String, String> = HashMap::new();
    let mut entry_unit: Option<String> = None;
    for m in modules {
        if m.is_entry {
            entry_unit = Some(m.path.clone());
        }
        for item in &m.file.items {
            if let align_ast::Item::Fn(fd) = item {
                base_to_unit.insert(mangle(&m.path, m.is_entry, &fd.name.name), m.path.clone());
            }
        }
    }

    let owning_unit = |fn_name: &str| -> Option<&String> {
        let mut best: Option<(&String, usize)> = None;
        for (base, unit) in &base_to_unit {
            let matches = fn_name == base
                || (fn_name.len() > base.len()
                    && fn_name.starts_with(base.as_str())
                    && fn_name.as_bytes()[base.len()] == b'$');
            if matches && best.is_none_or(|(_, len)| base.len() > len) {
                best = Some((unit, base.len()));
            }
        }
        best.map(|(u, _)| u)
    };

    let mut caps_by_unit: HashMap<String, Vec<String>> = HashMap::new();
    for f in &mir.fns {
        let caps = align_mir::function_capabilities(
            f,
            &mir.structs,
            &mir.tuples,
            &mir.enums,
            &mir.tagged_types,
        );
        if caps.is_empty() {
            continue;
        }
        let unit = owning_unit(f.name.as_str()).or(entry_unit.as_ref());
        let Some(unit) = unit else { continue };
        let bucket = caps_by_unit.entry(unit.clone()).or_default();
        for cap in caps {
            let name = format!("{cap:?}");
            if !bucket.contains(&name) {
                bucket.push(name);
            }
        }
    }
    caps_by_unit
}

/// The legacy whole-program `impl_hash` partition: split `mir.fns` into the unit that owns each
/// function (same longest-base-match rule as [`partition_capabilities`] — a monomorph / lifted thunk
/// / C-`main` wrapper shares its base's unit; an unowned function falls to the entry unit), then hash each
/// unit's functions' stable, location-free MIR text (names sorted so the encoding is
/// declaration-order-independent). A body edit changes that unit's printed MIR and so its
/// `impl_hash`; a pure comment/whitespace edit that lowers identically does not. Consumers never key
/// on `impl_hash` (only on `interface_hash`). Before the object cache is consulted, per-unit
/// compilation replaces this compatibility and inspection value with [`codegen_impl_hash`] over the
/// complete codegen input.
fn partition_impl_hashes(
    modules: &[align_sema::Module],
    mir: &align_mir::Program,
) -> HashMap<String, Hash128> {
    let mut base_to_unit: HashMap<String, String> = HashMap::new();
    let mut entry_unit: Option<String> = None;
    for m in modules {
        if m.is_entry {
            entry_unit = Some(m.path.clone());
        }
        for item in &m.file.items {
            if let align_ast::Item::Fn(fd) = item {
                base_to_unit.insert(mangle(&m.path, m.is_entry, &fd.name.name), m.path.clone());
            }
        }
    }
    let owning_unit = |fn_name: &str| -> Option<&String> {
        let mut best: Option<(&String, usize)> = None;
        for (base, unit) in &base_to_unit {
            let matches = fn_name == base
                || (fn_name.len() > base.len()
                    && fn_name.starts_with(base.as_str())
                    && fn_name.as_bytes()[base.len()] == b'$');
            if matches && best.is_none_or(|(_, len)| base.len() > len) {
                best = Some((unit, base.len()));
            }
        }
        best.map(|(u, _)| u)
    };

    // Bucket each MIR function's index by owning unit.
    let mut fns_by_unit: HashMap<String, Vec<usize>> = HashMap::new();
    for (i, f) in mir.fns.iter().enumerate() {
        let Some(unit) = owning_unit(f.name.as_str()).or(entry_unit.as_ref()) else { continue };
        fns_by_unit.entry(unit.clone()).or_default().push(i);
    }

    let mut out: HashMap<String, Hash128> = HashMap::new();
    for (unit, mut idxs) in fns_by_unit {
        idxs.sort_by(|&a, &b| mir.fns[a].name.cmp(&mir.fns[b].name));
        // Stable, location-free text of just this unit's functions, printed by reference one
        // function at a time (`function_to_string` prints types by id, never through a program's
        // type tables, so it needs no `Program` wrapper — and so no cloning every `Function` into a
        // temporary one just to print it). Concatenated in the same sorted, declaration-order-
        // independent order the old whole-`Program` printing established.
        let mut text = String::new();
        for &i in &idxs {
            text.push_str(&align_mir::print::function_to_string(&mir.fns[i]));
            text.push('\n');
        }
        out.insert(unit, Hash128::of(text.as_bytes()));
    }
    out
}

// ---- M15 S1b: reconstruct a dependency's public surface as source (consumer-side per-unit sema) ----
//
// The seam between the whole-program checker and per-unit checking is deliberately narrow: rather
// than a second resolver over summaries, an imported unit's `InterfaceSummary` is rendered back to
// Align source and re-parsed by the EXISTING parser into an `ast::File`, then fed to
// `align_sema::check_program_with_interface_facts` as an interface-only `Module`. Every
// table-building and resolution pass in sema is thus reused unchanged — one resolution code path.
// Generic templates and const values are stored as source text in the summary (they MUST be
// re-parsed regardless), so render-to-source unifies the whole reconstruction into a single
// `parse_file` call in the driver.

/// Render a UTF-8 type reference back to source. Every summary type is `Named`/`Tuple`/`Fn` (see
/// [`convert_type`]); a named type with args is `path<a, b>`, the unit type is its sentinel `()`.
fn render_itype(t: &IType) -> String {
    match t {
        IType::Named { path, args } => {
            if args.is_empty() {
                path.clone()
            } else {
                let a = args.iter().map(render_itype).collect::<Vec<_>>().join(", ");
                format!("{path}<{a}>")
            }
        }
        IType::Tuple(elems) => {
            let e = elems.iter().map(render_itype).collect::<Vec<_>>().join(", ");
            format!("({e})")
        }
        IType::Fn {
            params,
            ret,
            ..
        } => {
            let p = params
                .iter()
                .map(|p| {
                    let mode = match p.mode {
                        ParamMode::ByValue => "",
                        ParamMode::Out => "out ",
                        ParamMode::Borrow => "borrow ",
                        ParamMode::BorrowMut => "borrow mut ",
                    };
                    format!("{mode}{}", render_itype(&p.ty))
                })
                .collect::<Vec<_>>()
                .join(", ");
            format!("fn({p}) -> {}", render_itype(ret))
        }
    }
}

/// The unit-type sentinel produced by [`unit_type`] — an omitted `-> ()` return.
fn is_unit_itype(t: &IType) -> bool {
    matches!(t, IType::Named { path, args } if path == "()" && args.is_empty())
}

/// `<T, U: Ord>` for a generic declaration; empty for a non-generic one.
fn render_type_params(tps: &[ITypeParam]) -> String {
    if tps.is_empty() {
        return String::new();
    }
    let inner = tps
        .iter()
        .map(|t| match &t.bound {
            Some(b) => format!("{}: {}", t.name, b),
            None => t.name.clone(),
        })
        .collect::<Vec<_>>()
        .join(", ");
    format!("<{inner}>")
}

fn render_struct(s: &IStructDef) -> String {
    // `pub` first, then one canonical attribute order. The producer's stored generic fragment
    // starts at the type name, so these structured fields are the only source of layout prefixes.
    let mut out = String::from("pub ");
    if let Some(a) = s.align {
        out.push_str(&format!("align({a}) "));
    }
    if s.c_repr {
        out.push_str("layout(C) ");
    }
    out.push_str(&s.name);
    out.push_str(&render_type_params(&s.type_params));
    out.push_str(" {\n");
    for (name, ty) in &s.fields {
        out.push_str(&format!("    {name}: {},\n", render_itype(ty)));
    }
    out.push_str("}\n");
    out
}

fn render_enum(e: &IEnumDef) -> String {
    let mut out = String::from("pub ");
    out.push_str(&e.name);
    out.push_str(&render_type_params(&e.type_params));
    out.push_str(" {\n");
    for (name, payload) in &e.variants {
        if payload.is_empty() {
            out.push_str(&format!("    {name},\n"));
        } else {
            let ps = payload.iter().map(render_itype).collect::<Vec<_>>().join(", ");
            out.push_str(&format!("    {name}({ps}),\n"));
        }
    }
    out.push_str("}\n");
    out
}

/// Semantic compatibility failures found after the canonical interface codec has decoded a
/// structurally valid summary.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ImportCompatibilityError {
    DuplicateLocalType(String),
    DuplicateTypeParameter(String),
    TypeParameterShadowsLocalType(String),
    TypeParameterWithArguments(String),
    InvalidTypeArity {
        name: String,
        expected: usize,
        actual: usize,
    },
    UnresolvedBareType(String),
    GenericCLayoutUnsupported(String),
    GenericBodySyntax(String),
    GenericBodyMismatch(String),
    ResourceArityMismatch(String),
    ResourceRepresentationVersion {
        name: String,
        version: u32,
    },
    ResourceDropThunk(String),
    ResourceDropAbi(String),
    BorrowParamRegion,
    ReturnSummaryOnNonBorrowingType,
    ReturnSummaryRootCannotBorrow(u32),
    ReturnSummaryCaptureRoot,
    ReturnSummaryDisagreement,
    ReturnSummaryOnUnsupportedSignature,
    ReturnSummaryGenerativeCapabilityGraph,
    ParallelTransferRootsNonCanonical,
    ReturnCleanupMismatch,
}

impl std::fmt::Display for ImportCompatibilityError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ImportCompatibilityError::DuplicateLocalType(name) => {
                write!(f, "interface contains duplicate or ambiguous local type `{name}`")
            }
            ImportCompatibilityError::DuplicateTypeParameter(name) => {
                write!(f, "interface contains duplicate type parameter `{name}`")
            }
            ImportCompatibilityError::TypeParameterShadowsLocalType(name) => {
                write!(
                    f,
                    "interface type parameter `{name}` shadows a declared local type"
                )
            }
            ImportCompatibilityError::TypeParameterWithArguments(name) => {
                write!(f, "interface type parameter `{name}` cannot take type arguments")
            }
            ImportCompatibilityError::InvalidTypeArity {
                name,
                expected,
                actual,
            } => {
                write!(
                    f,
                    "interface type `{name}` expects {expected} type arguments, got {actual}"
                )
            }
            ImportCompatibilityError::UnresolvedBareType(name) => {
                write!(f, "interface contains unresolved bare type `{name}`")
            }
            ImportCompatibilityError::GenericCLayoutUnsupported(name) => {
                write!(
                    f,
                    "generic interface struct `{name}` cannot use `layout(C)` before concrete instantiation"
                )
            }
            ImportCompatibilityError::GenericBodySyntax(name) => {
                write!(
                    f,
                    "generic interface declaration `{name}` is not one valid declaration fragment"
                )
            }
            ImportCompatibilityError::GenericBodyMismatch(name) => {
                write!(
                    f,
                    "generic interface declaration `{name}` disagrees with its structured record"
                )
            }
            ImportCompatibilityError::ResourceArityMismatch(name) => {
                write!(f, "interface resource `{name}` has inconsistent generic arity")
            }
            ImportCompatibilityError::ResourceRepresentationVersion { name, version } => {
                write!(
                    f,
                    "interface resource `{name}` uses unsupported representation version {version}"
                )
            }
            ImportCompatibilityError::ResourceDropThunk(name) => {
                write!(f, "interface resource `{name}` has a noncanonical Drop thunk")
            }
            ImportCompatibilityError::ResourceDropAbi(name) => {
                write!(f, "interface resource `{name}` has an unsupported Drop ABI fingerprint")
            }
            ImportCompatibilityError::BorrowParamRegion => {
                write!(f, "interface borrows a region capability instead of passing it by value")
            }
            ImportCompatibilityError::ReturnSummaryOnNonBorrowingType => {
                write!(
                    f,
                    "interface return provenance is present on a return type that cannot borrow"
                )
            }
            ImportCompatibilityError::ReturnSummaryRootCannotBorrow(index) => {
                write!(
                    f,
                    "interface return provenance parameter root {index} names a type that cannot supply a borrow"
                )
            }
            ImportCompatibilityError::ReturnSummaryCaptureRoot => {
                write!(
                    f,
                    "an exported interface signature cannot contain a capture-root return summary"
                )
            }
            ImportCompatibilityError::ReturnSummaryDisagreement => {
                write!(
                    f,
                    "return-borrow and return-region summaries disagree in the L2b-a1 interface"
                )
            }
            ImportCompatibilityError::ReturnSummaryOnUnsupportedSignature => {
                write!(
                    f,
                    "return provenance is not enabled on generic or nested function-value signatures"
                )
            }
            ImportCompatibilityError::ReturnSummaryGenerativeCapabilityGraph => {
                write!(
                    f,
                    "return provenance capability validation found a generative recursive type graph"
                )
            }
            ImportCompatibilityError::ParallelTransferRootsNonCanonical => {
                write!(f, "interface parallel-transfer roots are not strictly increasing")
            }
            ImportCompatibilityError::ReturnCleanupMismatch => {
                write!(f, "interface return-cleanup metadata disagrees with its return type")
            }
        }
    }
}

impl std::error::Error for ImportCompatibilityError {}

#[derive(Clone, Copy)]
enum LocalDefinition<'a> {
    Struct(&'a IStructDef),
    Enum(&'a IEnumDef),
    Resource(&'a IResourceDef),
}

impl<'a> LocalDefinition<'a> {
    fn type_params(self) -> &'a [ITypeParam] {
        match self {
            LocalDefinition::Struct(definition) => &definition.type_params,
            LocalDefinition::Enum(definition) => &definition.type_params,
            LocalDefinition::Resource(definition) => &definition.type_params,
        }
    }

    fn values(self) -> Vec<&'a IType> {
        match self {
            LocalDefinition::Struct(definition) => {
                definition.fields.iter().map(|(_, ty)| ty).collect()
            }
            LocalDefinition::Enum(definition) => definition
                .variants
                .iter()
                .flat_map(|(_, payload)| payload)
                .collect(),
            LocalDefinition::Resource(_) => Vec::new(),
        }
    }
}

struct LocalDefinitionIndex<'a> {
    unit: &'a str,
    definitions: Vec<LocalDefinition<'a>>,
    by_name: HashMap<&'a str, usize>,
    param_offsets: Vec<usize>,
    total_params: usize,
}

impl<'a> LocalDefinitionIndex<'a> {
    fn new(summary: &'a InterfaceSummary) -> Result<Self, ImportCompatibilityError> {
        let mut definitions = Vec::with_capacity(
            summary.structs.len() + summary.enums.len() + summary.resources.len(),
        );
        let mut by_name = HashMap::new();
        for definition in &summary.structs {
            if by_name
                .insert(definition.name.as_str(), definitions.len())
                .is_some()
            {
                return Err(ImportCompatibilityError::DuplicateLocalType(
                    definition.name.clone(),
                ));
            }
            definitions.push(LocalDefinition::Struct(definition));
        }
        for definition in &summary.enums {
            if by_name
                .insert(definition.name.as_str(), definitions.len())
                .is_some()
            {
                return Err(ImportCompatibilityError::DuplicateLocalType(
                    definition.name.clone(),
                ));
            }
            definitions.push(LocalDefinition::Enum(definition));
        }
        for definition in &summary.resources {
            if by_name
                .insert(definition.name.as_str(), definitions.len())
                .is_some()
            {
                return Err(ImportCompatibilityError::DuplicateLocalType(
                    definition.name.clone(),
                ));
            }
            definitions.push(LocalDefinition::Resource(definition));
        }
        let mut total_params = 0usize;
        let mut param_offsets = Vec::with_capacity(definitions.len());
        for definition in &definitions {
            param_offsets.push(total_params);
            total_params += definition.type_params().len();
        }
        Ok(Self {
            unit: &summary.unit,
            definitions,
            by_name,
            param_offsets,
            total_params,
        })
    }

    fn local(&self, path: &str) -> Option<usize> {
        if let Some(name) = path.strip_prefix(self.unit).and_then(|rest| rest.strip_prefix('.')) {
            return self.by_name.get(name).copied();
        }
        if path.contains('.') {
            return None;
        }
        self.by_name.get(path).copied()
    }

    fn is_missing_qualified_local(&self, path: &str) -> bool {
        path.strip_prefix(self.unit)
            .and_then(|rest| rest.strip_prefix('.'))
            .is_some_and(|name| !name.contains('.') && !self.by_name.contains_key(name))
    }

    fn total_params(&self) -> usize {
        self.total_params
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum BuiltinCapability {
    BorrowLeaf,
    Transparent,
    Opaque,
}

/// Every builtin type spelling an interface can carry, with its argument arity and capability.
/// Kept as data (not match arms) so a test can sweep the exact set against `align_sema`'s
/// ownership bridge in both directions: the two describe one language, and a spelling that only
/// one of them knows is how a valid interface starts failing import validation.
const BUILTIN_CAPABILITIES: &[(&str, usize, BuiltinCapability)] = &[
    ("str", 0, BuiltinCapability::BorrowLeaf),
    ("region", 0, BuiltinCapability::BorrowLeaf),
    ("reader", 0, BuiltinCapability::BorrowLeaf),
    ("writer", 0, BuiltinCapability::BorrowLeaf),
    ("http_read_stream", 0, BuiltinCapability::BorrowLeaf),
    ("http_sse_stream", 0, BuiltinCapability::BorrowLeaf),
    ("http_sse_event", 0, BuiltinCapability::BorrowLeaf),
    ("http.http_sse_event", 0, BuiltinCapability::BorrowLeaf),
    ("http_headers", 0, BuiltinCapability::BorrowLeaf),
    ("json.doc", 0, BuiltinCapability::BorrowLeaf),
    ("slice", 1, BuiltinCapability::BorrowLeaf),
    ("soa", 1, BuiltinCapability::BorrowLeaf),
    ("json.scanner", 1, BuiltinCapability::BorrowLeaf),
    ("resource_ref", 1, BuiltinCapability::BorrowLeaf),
    ("Option", 1, BuiltinCapability::Transparent),
    ("array", 1, BuiltinCapability::Transparent),
    ("Result", 2, BuiltinCapability::Transparent),
    ("box", 1, BuiltinCapability::Opaque),
    ("array_builder", 1, BuiltinCapability::Opaque),
    ("buffer", 0, BuiltinCapability::Opaque),
    ("file", 0, BuiltinCapability::Opaque),
    ("rng", 0, BuiltinCapability::Opaque),
    ("regex", 0, BuiltinCapability::Opaque),
    ("captures", 0, BuiltinCapability::Opaque),
    ("tcp_conn", 0, BuiltinCapability::Opaque),
    ("tcp_listener", 0, BuiltinCapability::Opaque),
    ("udp_socket", 0, BuiltinCapability::Opaque),
    ("child", 0, BuiltinCapability::Opaque),
    ("run_bytes", 0, BuiltinCapability::Opaque),
    ("http_request_ctx", 0, BuiltinCapability::Opaque),
    ("response_builder", 0, BuiltinCapability::Opaque),
    ("http_stream", 0, BuiltinCapability::Opaque),
    ("json.kind", 0, BuiltinCapability::Opaque),
    ("Error", 0, BuiltinCapability::Opaque),
    ("core.Error", 0, BuiltinCapability::Opaque),
    ("argon2_params", 0, BuiltinCapability::Opaque),
    ("crypto.argon2_params", 0, BuiltinCapability::Opaque),
    ("regex_match", 0, BuiltinCapability::Opaque),
    ("regex.regex_match", 0, BuiltinCapability::Opaque),
    ("()", 0, BuiltinCapability::Opaque),
    ("bool", 0, BuiltinCapability::Opaque),
    ("char", 0, BuiltinCapability::Opaque),
    ("string", 0, BuiltinCapability::Opaque),
    ("i8", 0, BuiltinCapability::Opaque),
    ("i16", 0, BuiltinCapability::Opaque),
    ("i32", 0, BuiltinCapability::Opaque),
    ("i64", 0, BuiltinCapability::Opaque),
    ("u8", 0, BuiltinCapability::Opaque),
    ("u16", 0, BuiltinCapability::Opaque),
    ("u32", 0, BuiltinCapability::Opaque),
    ("u64", 0, BuiltinCapability::Opaque),
    ("f32", 0, BuiltinCapability::Opaque),
    ("f64", 0, BuiltinCapability::Opaque),
    ("raw", 0, BuiltinCapability::Opaque),
];

fn builtin_capability(path: &str) -> Option<(usize, BuiltinCapability)> {
    if let Some((_, arity, capability)) = BUILTIN_CAPABILITIES
        .iter()
        .find(|(spelling, _, _)| *spelling == path)
    {
        return Some((*arity, *capability));
    }
    let vector = path
        .strip_prefix("vec")
        .or_else(|| path.strip_prefix("mask"))
        .is_some_and(|width| matches!(width, "2" | "4" | "8" | "16"));
    vector.then_some((1, BuiltinCapability::Opaque))
}

fn bare_nominal_alias_prefers_local(path: &str) -> bool {
    matches!(path, "Error" | "argon2_params" | "regex_match")
}

fn builtin_capability_after_local(
    index: &LocalDefinitionIndex<'_>,
    path: &str,
) -> Option<(usize, BuiltinCapability)> {
    if bare_nominal_alias_prefers_local(path) && index.local(path).is_some() {
        None
    } else {
        builtin_capability(path)
    }
}

#[derive(Clone, PartialEq, Eq)]
struct BorrowFacts {
    intrinsic: bool,
    params: Vec<bool>,
}

impl BorrowFacts {
    fn empty(param_count: usize) -> Self {
        Self {
            intrinsic: false,
            params: vec![false; param_count],
        }
    }

    fn union(&mut self, other: &Self) {
        self.intrinsic |= other.intrinsic;
        for (current, incoming) in self.params.iter_mut().zip(&other.params) {
            *current |= *incoming;
        }
    }
}

struct CapabilityAnalysis<'a> {
    index: LocalDefinitionIndex<'a>,
    borrow: Vec<BorrowFacts>,
    ownership: Vec<OwnershipFacts>,
    growth: Vec<Vec<bool>>,
}

#[derive(Clone, PartialEq, Eq)]
struct OwnershipFacts {
    intrinsic: bool,
    unknown: bool,
    params: Vec<bool>,
}

impl OwnershipFacts {
    fn empty(param_count: usize) -> Self {
        Self {
            intrinsic: false,
            unknown: false,
            params: vec![false; param_count],
        }
    }

    fn union(&mut self, other: &Self) {
        self.intrinsic |= other.intrinsic;
        self.unknown |= other.unknown;
        for (current, incoming) in self.params.iter_mut().zip(&other.params) {
            *current |= *incoming;
        }
    }
}

impl<'a> CapabilityAnalysis<'a> {
    fn new(index: LocalDefinitionIndex<'a>) -> Result<Self, ImportCompatibilityError> {
        let borrow = index
            .definitions
            .iter()
            .map(|definition| BorrowFacts::empty(definition.type_params().len()))
            .collect();
        let growth = index
            .definitions
            .iter()
            .map(|definition| vec![true; definition.type_params().len()])
            .collect();
        let ownership = index
            .definitions
            .iter()
            .map(|definition| OwnershipFacts::empty(definition.type_params().len()))
            .collect();
        let mut analysis = Self {
            index,
            borrow,
            ownership,
            growth,
        };
        analysis.solve_borrow();
        analysis.solve_ownership();
        analysis.solve_growth();
        analysis.reject_generative_cycles()?;
        Ok(analysis)
    }

    fn eval_ownership(
        &self,
        ty: &IType,
        type_params: &[ITypeParam],
        summaries: &[OwnershipFacts],
    ) -> OwnershipFacts {
        let mut result = OwnershipFacts::empty(type_params.len());
        let mut work = vec![ty];
        while let Some(current) = work.pop() {
            match current {
                IType::Tuple(elements) => work.extend(elements),
                IType::Fn { .. } => {}
                IType::Named { path, args } => {
                    if args.is_empty()
                        && let Some(index) = type_params
                            .iter()
                            .position(|parameter| parameter.name == *path)
                    {
                        result.params[index] = true;
                        continue;
                    }
                    if matches!(path.as_str(), "Option" | "Result") {
                        work.extend(args);
                        continue;
                    }
                    // Whether a builtin owns droppable storage is sema's `needs_drop_flag`, reached
                    // through its spelling bridge. A hand-written name table here was a second model
                    // of the very bit this analysis validates, so a new droppable builtin surface
                    // type would have rejected every valid interface that returns it.
                    if let Some(owns_droppable) =
                        align_sema::builtin_spelling_needs_return_cleanup(path)
                    {
                        result.intrinsic |= owns_droppable;
                        continue;
                    }
                    if builtin_capability_after_local(&self.index, path).is_some() {
                        continue;
                    }
                    if let Some(index) = self.index.local(path) {
                        let summary = &summaries[index];
                        result.intrinsic |= summary.intrinsic;
                        result.unknown |= summary.unknown;
                        for (position, dependent) in summary.params.iter().copied().enumerate() {
                            if dependent
                                && let Some(argument) = args.get(position)
                            {
                                work.push(argument);
                            }
                        }
                    } else if path.contains('.') {
                        result.unknown = true;
                    }
                }
            }
        }
        result
    }

    fn solve_ownership(&mut self) {
        loop {
            let mut changed = false;
            for index in 0..self.index.definitions.len() {
                let definition = self.index.definitions[index];
                let mut next = OwnershipFacts::empty(definition.type_params().len());
                if matches!(definition, LocalDefinition::Resource(_)) {
                    next.intrinsic = true;
                }
                for value in definition.values() {
                    next.union(&self.eval_ownership(
                        value,
                        definition.type_params(),
                        &self.ownership,
                    ));
                }
                if next != self.ownership[index] {
                    self.ownership[index] = next;
                    changed = true;
                }
            }
            if !changed {
                break;
            }
        }
    }

    fn return_cleanup(&self, ty: &IType, type_params: &[ITypeParam]) -> Option<align_sema::hir::ReturnCleanupAbi> {
        let facts = self.eval_ownership(ty, type_params, &self.ownership);
        if facts.unknown || facts.params.iter().any(|dependent| *dependent) {
            None
        } else if facts.intrinsic {
            Some(align_sema::hir::ReturnCleanupAbi::DynamicBit)
        } else {
            Some(align_sema::hir::ReturnCleanupAbi::None)
        }
    }

    fn eval_borrow(
        &self,
        ty: &IType,
        type_params: &[ITypeParam],
        summaries: &[BorrowFacts],
    ) -> BorrowFacts {
        let mut result = BorrowFacts::empty(type_params.len());
        let mut work = vec![ty];
        while let Some(current) = work.pop() {
            match current {
                IType::Tuple(elements) => work.extend(elements),
                IType::Fn { .. } => result.intrinsic = true,
                IType::Named { path, args } => {
                    if args.is_empty()
                        && let Some(index) = type_params
                            .iter()
                            .position(|parameter| parameter.name == *path)
                    {
                        result.params[index] = true;
                        continue;
                    }
                    if let Some((_, capability)) =
                        builtin_capability_after_local(&self.index, path)
                    {
                        match capability {
                            BuiltinCapability::BorrowLeaf => result.intrinsic = true,
                            BuiltinCapability::Transparent => work.extend(args),
                            BuiltinCapability::Opaque => {}
                        }
                        continue;
                    }
                    if let Some(index) = self.index.local(path) {
                        let summary = &summaries[index];
                        result.intrinsic |= summary.intrinsic;
                        for (position, dependent) in summary.params.iter().copied().enumerate() {
                            if dependent
                                && let Some(argument) = args.get(position)
                            {
                                work.push(argument);
                            }
                        }
                    } else if path.contains('.') {
                        result.intrinsic = true;
                    }
                }
            }
        }
        result
    }

    fn eval_growth(
        &self,
        ty: &IType,
        type_params: &[ITypeParam],
        summaries: &[Vec<bool>],
    ) -> Vec<bool> {
        let mut result = vec![false; type_params.len()];
        let mut work = vec![ty];
        while let Some(current) = work.pop() {
            match current {
                IType::Tuple(elements) => work.extend(elements),
                IType::Fn { .. } => {}
                IType::Named { path, args } => {
                    if args.is_empty()
                        && let Some(index) = type_params
                            .iter()
                            .position(|parameter| parameter.name == *path)
                    {
                        result[index] = true;
                        continue;
                    }
                    if let Some((_, capability)) =
                        builtin_capability_after_local(&self.index, path)
                    {
                        if capability == BuiltinCapability::Transparent {
                            work.extend(args);
                        }
                        continue;
                    }
                    if let Some(index) = self.index.local(path) {
                        for (position, exposed) in summaries[index].iter().copied().enumerate() {
                            if exposed
                                && let Some(argument) = args.get(position)
                            {
                                work.push(argument);
                            }
                        }
                    }
                }
            }
        }
        result
    }

    fn solve_borrow(&mut self) {
        loop {
            let mut changed = false;
            for index in 0..self.index.definitions.len() {
                let definition = self.index.definitions[index];
                let mut next = self.borrow[index].clone();
                if matches!(definition, LocalDefinition::Resource(_)) {
                    // A resource may carry one inferred parent generation even though its public
                    // representation is always one pointer. Treat it as a borrow-capable leaf so
                    // imported return provenance cannot be discarded by interface validation.
                    next.intrinsic = true;
                }
                for value in definition.values() {
                    next.union(&self.eval_borrow(
                        value,
                        definition.type_params(),
                        &self.borrow,
                    ));
                }
                if next != self.borrow[index] {
                    self.borrow[index] = next;
                    changed = true;
                }
            }
            if !changed {
                break;
            }
        }
    }

    fn solve_growth(&mut self) {
        loop {
            let mut changed = false;
            for index in 0..self.index.definitions.len() {
                let definition = self.index.definitions[index];
                let mut next = vec![false; definition.type_params().len()];
                for value in definition.values() {
                    let facts = self.eval_growth(
                        value,
                        definition.type_params(),
                        &self.growth,
                    );
                    for (current, incoming) in next.iter_mut().zip(facts) {
                        *current |= incoming;
                    }
                }
                if next != self.growth[index] {
                    self.growth[index] = next;
                    changed = true;
                }
            }
            if !changed {
                break;
            }
        }
    }

    fn add_occurrence_edges(
        &self,
        source: usize,
        target: usize,
        target_param: usize,
        actual: &IType,
        edges: &mut Vec<(usize, usize, bool)>,
    ) {
        let source_params = self.index.definitions[source].type_params();
        let mut work = vec![(actual, false)];
        while let Some((current, wrapped)) = work.pop() {
            match current {
                IType::Named { path, args } => {
                    if args.is_empty()
                        && let Some(source_param) = source_params
                            .iter()
                            .position(|parameter| parameter.name == *path)
                    {
                        edges.push((
                            self.index.param_offsets[source] + source_param,
                            self.index.param_offsets[target] + target_param,
                            wrapped,
                        ));
                        continue;
                    }
                    work.extend(args.iter().map(|argument| (argument, true)));
                }
                IType::Tuple(elements) => {
                    work.extend(elements.iter().map(|element| (element, true)));
                }
                IType::Fn { params, ret, .. } => {
                    work.extend(params.iter().map(|parameter| (&parameter.ty, true)));
                    work.push((ret, true));
                }
            }
        }
    }

    fn collect_edges(
        &self,
        source: usize,
        root: &IType,
        edges: &mut Vec<(usize, usize, bool)>,
    ) {
        let mut work = vec![root];
        while let Some(current) = work.pop() {
            match current {
                IType::Tuple(elements) => work.extend(elements),
                IType::Fn { .. } => {}
                IType::Named { path, args } => {
                    if let Some((_, capability)) =
                        builtin_capability_after_local(&self.index, path)
                    {
                        if capability == BuiltinCapability::Transparent {
                            work.extend(args);
                        }
                        continue;
                    }
                    let Some(target) = self.index.local(path) else {
                        continue;
                    };
                    for (position, argument) in args.iter().enumerate() {
                        self.add_occurrence_edges(
                            source,
                            target,
                            position,
                            argument,
                            edges,
                        );
                        if self.growth[target][position] {
                            work.push(argument);
                        }
                    }
                }
            }
        }
    }

    fn reject_generative_cycles(&self) -> Result<(), ImportCompatibilityError> {
        let node_count = self.index.total_params();
        let mut edges = Vec::new();
        for source in 0..self.index.definitions.len() {
            for value in self.index.definitions[source].values() {
                self.collect_edges(source, value, &mut edges);
            }
        }
        let mut forward = vec![Vec::new(); node_count];
        let mut reverse = vec![Vec::new(); node_count];
        for &(from, to, _) in &edges {
            let Some(outgoing) = forward.get_mut(from) else {
                return Err(ImportCompatibilityError::ReturnSummaryGenerativeCapabilityGraph);
            };
            outgoing.push(to);
            let Some(incoming) = reverse.get_mut(to) else {
                return Err(ImportCompatibilityError::ReturnSummaryGenerativeCapabilityGraph);
            };
            incoming.push(from);
        }

        let mut seen = vec![false; node_count];
        let mut order = Vec::with_capacity(node_count);
        for start in 0..node_count {
            if seen[start] {
                continue;
            }
            seen[start] = true;
            let mut stack = vec![(start, 0usize)];
            while let Some((node, edge_index)) = stack.pop() {
                if let Some(&next) = forward[node].get(edge_index) {
                    stack.push((node, edge_index + 1));
                    if !seen[next] {
                        seen[next] = true;
                        stack.push((next, 0));
                    }
                } else {
                    order.push(node);
                }
            }
        }

        let mut component = vec![usize::MAX; node_count];
        let mut component_id = 0usize;
        while let Some(start) = order.pop() {
            if component[start] != usize::MAX {
                continue;
            }
            component[start] = component_id;
            let mut stack = vec![start];
            while let Some(node) = stack.pop() {
                for &next in &reverse[node] {
                    if component[next] == usize::MAX {
                        component[next] = component_id;
                        stack.push(next);
                    }
                }
            }
            component_id += 1;
        }
        if edges
            .iter()
            .any(|&(from, to, positive)| positive && component[from] == component[to])
        {
            return Err(ImportCompatibilityError::ReturnSummaryGenerativeCapabilityGraph);
        }
        Ok(())
    }

    fn may_borrow(&self, ty: &IType, type_params: &[ITypeParam]) -> bool {
        let facts = self.eval_borrow(ty, type_params, &self.borrow);
        facts.intrinsic || facts.params.into_iter().any(|dependent| dependent)
    }

    fn may_supply_return_provenance(&self, ty: &IType, type_params: &[ITypeParam]) -> bool {
        self.may_borrow(ty, type_params)
            || self.return_cleanup(ty, type_params)
                == Some(align_sema::hir::ReturnCleanupAbi::DynamicBit)
            || self.contains_noncleanup_move_builtin(ty)
    }

    fn contains_noncleanup_move_builtin(&self, ty: &IType) -> bool {
        let mut work = vec![ty];
        while let Some(current) = work.pop() {
            match current {
                IType::Tuple(elements) => work.extend(elements),
                IType::Fn { .. } => {}
                IType::Named { path, args } => {
                    if align_sema::builtin_spelling_is_move(path) == Some(true)
                        && align_sema::builtin_spelling_needs_return_cleanup(path) == Some(false)
                    {
                        return true;
                    }
                    if matches!(path.as_str(), "Option" | "Result") {
                        work.extend(args);
                    }
                }
            }
        }
        false
    }
}

/// Authenticate decoded parallel-transfer roots against the complete interface type graph.
/// Structural codec checks run while each function record is read; nominal borrow capability can
/// only be decided after the later struct/enum/resource definitions are available.
pub(crate) fn decoded_parallel_transfer_roots_are_borrow_capable(
    summary: &InterfaceSummary,
) -> bool {
    if summary.fns.iter().all(|function| function.parallel_transfer_params.is_empty()) {
        return true;
    }
    let Ok(index) = LocalDefinitionIndex::new(summary) else {
        return false;
    };
    let Ok(analysis) = CapabilityAnalysis::new(index) else {
        return false;
    };
    summary.fns.iter().all(|function| {
        function.parallel_transfer_params.iter().all(|&root| {
            function.params.get(root as usize).is_some_and(|parameter| {
                analysis.may_borrow(&parameter.ty, &function.type_params)
                    || matches!(parameter.mode, ParamMode::Borrow | ParamMode::BorrowMut)
            })
        })
    })
}

fn validate_type_params(
    type_params: &[ITypeParam],
    index: &LocalDefinitionIndex<'_>,
) -> Result<(), ImportCompatibilityError> {
    let mut seen = HashSet::new();
    for parameter in type_params {
        if !seen.insert(parameter.name.as_str()) {
            return Err(ImportCompatibilityError::DuplicateTypeParameter(
                parameter.name.clone(),
            ));
        }
    }
    for parameter in type_params {
        if index.local(&parameter.name).is_some() {
            return Err(ImportCompatibilityError::TypeParameterShadowsLocalType(
                parameter.name.clone(),
            ));
        }
    }
    Ok(())
}

fn validate_import_type_shape(
    ty: &IType,
    index: &LocalDefinitionIndex<'_>,
    type_params: &[ITypeParam],
) -> Result<(), ImportCompatibilityError> {
    let mut work = vec![ty];
    while let Some(current) = work.pop() {
        match current {
            IType::Tuple(elements) => work.extend(elements.iter().rev()),
            IType::Fn { params, ret, .. } => {
                work.push(ret);
                work.extend(params.iter().rev().map(|parameter| &parameter.ty));
            }
            IType::Named { path, args } => {
                let parameter = type_params
                    .iter()
                    .find(|parameter| parameter.name == *path);
                if args.is_empty() && parameter.is_some() {
                    continue;
                }
                let expected = if let Some((arity, _)) =
                    builtin_capability_after_local(index, path)
                {
                    Some(arity)
                } else {
                    index
                        .local(path)
                        .map(|definition| index.definitions[definition].type_params().len())
                };
                if let Some(expected) = expected {
                    if args.len() != expected {
                        return Err(ImportCompatibilityError::InvalidTypeArity {
                            name: path.clone(),
                            expected,
                            actual: args.len(),
                        });
                    }
                } else if let Some(parameter) = parameter {
                    return Err(ImportCompatibilityError::TypeParameterWithArguments(
                        parameter.name.clone(),
                    ));
                } else if !path.contains('.') || index.is_missing_qualified_local(path) {
                    return Err(ImportCompatibilityError::UnresolvedBareType(path.clone()));
                }
                work.extend(args.iter().rev());
            }
        }
    }
    Ok(())
}

fn validate_import_shapes(
    summary: &InterfaceSummary,
    index: &LocalDefinitionIndex<'_>,
) -> Result<(), ImportCompatibilityError> {
    for function in &summary.fns {
        validate_type_params(&function.type_params, index)?;
        for parameter in &function.params {
            validate_import_type_shape(&parameter.ty, index, &function.type_params)?;
        }
        validate_import_type_shape(&function.ret, index, &function.type_params)?;
    }
    for structure in &summary.structs {
        validate_type_params(&structure.type_params, index)?;
        for (_, ty) in &structure.fields {
            validate_import_type_shape(ty, index, &structure.type_params)?;
        }
    }
    for enumeration in &summary.enums {
        validate_type_params(&enumeration.type_params, index)?;
        for (_, payload) in &enumeration.variants {
            for ty in payload {
                validate_import_type_shape(ty, index, &enumeration.type_params)?;
            }
        }
    }
    for resource in &summary.resources {
        validate_type_params(&resource.type_params, index)?;
    }
    for constant in &summary.consts {
        if let Some(ty) = &constant.ty {
            validate_import_type_shape(ty, index, &[])?;
        }
    }
    Ok(())
}

fn validate_import_resources(
    summary: &InterfaceSummary,
) -> Result<(), ImportCompatibilityError> {
    for resource in &summary.resources {
        if resource.generic_arity as usize != resource.type_params.len() {
            return Err(ImportCompatibilityError::ResourceArityMismatch(
                resource.name.clone(),
            ));
        }
        if resource.representation_version != 1 {
            return Err(
                ImportCompatibilityError::ResourceRepresentationVersion {
                    name: resource.name.clone(),
                    version: resource.representation_version,
                },
            );
        }
        let mut qualified = format!("__align_resource_drop${}", summary.unit);
        qualified.push('$');
        qualified.push_str(&resource.name);
        let entry = format!("__align_resource_drop${}", resource.name);
        if resource.drop_thunk != qualified
            && !(summary.unit == "main" && resource.drop_thunk == entry)
        {
            return Err(ImportCompatibilityError::ResourceDropThunk(
                resource.name.clone(),
            ));
        }
        if resource.drop_abi_fingerprint != *b"align-res-drop-1" {
            return Err(ImportCompatibilityError::ResourceDropAbi(
                resource.name.clone(),
            ));
        }
    }
    Ok(())
}

fn validate_import_summary_header(
    borrow: &ReturnBorrowSummary,
    region: &ReturnRegionSummary,
    allow_return_roots: bool,
) -> Result<(), ImportCompatibilityError> {
    let summaries_agree = match (borrow, region) {
        (ReturnBorrowSummary::None, ReturnRegionSummary::None) => true,
        (
            ReturnBorrowSummary::Roots {
                params: borrow_params,
                captures: borrow_captures,
            },
            ReturnRegionSummary::Roots {
                params: region_params,
                captures: region_captures,
            },
        ) => borrow_params == region_params && borrow_captures == region_captures,
        _ => false,
    };
    if !summaries_agree {
        return Err(ImportCompatibilityError::ReturnSummaryDisagreement);
    }
    if !allow_return_roots
        && (!matches!(borrow, ReturnBorrowSummary::None)
            || !matches!(region, ReturnRegionSummary::None))
    {
        return Err(ImportCompatibilityError::ReturnSummaryOnUnsupportedSignature);
    }
    Ok(())
}

fn validate_import_type_headers(ty: &IType) -> Result<(), ImportCompatibilityError> {
    let mut work = vec![ty];
    while let Some(current) = work.pop() {
        match current {
            IType::Named { args, .. } => work.extend(args.iter().rev()),
            IType::Tuple(elements) => work.extend(elements.iter().rev()),
            IType::Fn {
                params,
                ret,
                return_borrow,
                return_region,
                return_cleanup: _,
            } => {
                validate_import_summary_header(return_borrow, return_region, false)?;
                work.push(ret);
                work.extend(params.iter().rev().map(|param| &param.ty));
            }
        }
    }
    Ok(())
}

fn validate_import_headers(
    summary: &InterfaceSummary,
) -> Result<(), ImportCompatibilityError> {
    for function in &summary.fns {
        for param in &function.params {
            validate_import_type_headers(&param.ty)?;
        }
        validate_import_type_headers(&function.ret)?;
        validate_import_summary_header(
            &function.return_borrow,
            &function.return_region,
            function.generic_body.is_none(),
        )?;
    }
    for structure in &summary.structs {
        for (_, ty) in &structure.fields {
            validate_import_type_headers(ty)?;
        }
    }
    for enumeration in &summary.enums {
        for (_, payload) in &enumeration.variants {
            for ty in payload {
                validate_import_type_headers(ty)?;
            }
        }
    }
    for constant in &summary.consts {
        if let Some(ty) = &constant.ty {
            validate_import_type_headers(ty)?;
        }
    }
    Ok(())
}

fn parse_generic_fragment(
    name: &str,
    body: &str,
    attributes: &str,
) -> Result<align_ast::Item, ImportCompatibilityError> {
    let source = format!("pub {attributes}{}", body.trim_start());
    let mut diags = align_diag::Diagnostics::new();
    let tokens = align_lexer::tokenize(0, &source, &mut diags);
    let mut file = align_parser::parse_file(tokens, &mut diags);
    if diags.has_errors()
        || file.module.is_some()
        || !file.imports.is_empty()
        || file.items.len() != 1
    {
        return Err(ImportCompatibilityError::GenericBodySyntax(
            name.to_string(),
        ));
    }
    Ok(file.items.remove(0))
}

fn validate_generic_function(function: &IFnSig) -> Result<(), ImportCompatibilityError> {
    let Some(body) = &function.generic_body else {
        return if function.type_params.is_empty() {
            Ok(())
        } else {
            Err(ImportCompatibilityError::GenericBodyMismatch(
                function.name.clone(),
            ))
        };
    };
    if function.type_params.is_empty() {
        return Err(ImportCompatibilityError::GenericBodyMismatch(
            function.name.clone(),
        ));
    }
    let align_ast::Item::Fn(parsed) =
        parse_generic_fragment(&function.name, body, "")?
    else {
        return Err(ImportCompatibilityError::GenericBodyMismatch(
            function.name.clone(),
        ));
    };
    let params = parsed
        .params
        .iter()
        .map(|param| IParam {
            mode: param.mode,
            ty: convert_type(&param.ty),
        })
        .collect::<Vec<_>>();
    if parsed.name.name != function.name
        || convert_type_params(&parsed.type_params) != function.type_params
        || params != function.params
        || convert_ret(&parsed.ret) != function.ret
    {
        return Err(ImportCompatibilityError::GenericBodyMismatch(
            function.name.clone(),
        ));
    }
    Ok(())
}

fn validate_generic_struct(structure: &IStructDef) -> Result<(), ImportCompatibilityError> {
    if !structure.type_params.is_empty() && structure.c_repr {
        return Err(ImportCompatibilityError::GenericCLayoutUnsupported(
            structure.name.clone(),
        ));
    }
    let Some(body) = &structure.generic_body else {
        return if structure.type_params.is_empty() {
            Ok(())
        } else {
            Err(ImportCompatibilityError::GenericBodyMismatch(
                structure.name.clone(),
            ))
        };
    };
    if structure.type_params.is_empty() {
        return Err(ImportCompatibilityError::GenericBodyMismatch(
            structure.name.clone(),
        ));
    }
    let mut attributes = String::new();
    if let Some(align) = structure.align {
        attributes.push_str(&format!("align({align}) "));
    }
    if structure.c_repr {
        attributes.push_str("layout(C) ");
    }
    let align_ast::Item::Struct(parsed) =
        parse_generic_fragment(&structure.name, body, &attributes)?
    else {
        return Err(ImportCompatibilityError::GenericBodyMismatch(
            structure.name.clone(),
        ));
    };
    let fields = parsed
        .fields
        .iter()
        .map(|field| (field.name.name.clone(), convert_type(&field.ty)))
        .collect::<Vec<_>>();
    if parsed.name.name != structure.name
        || convert_type_params(&parsed.type_params) != structure.type_params
        || fields != structure.fields
        || parsed.align != structure.align
        || parsed.c_repr != structure.c_repr
    {
        return Err(ImportCompatibilityError::GenericBodyMismatch(
            structure.name.clone(),
        ));
    }
    Ok(())
}

fn validate_generic_enum(enumeration: &IEnumDef) -> Result<(), ImportCompatibilityError> {
    let Some(body) = &enumeration.generic_body else {
        return if enumeration.type_params.is_empty() {
            Ok(())
        } else {
            Err(ImportCompatibilityError::GenericBodyMismatch(
                enumeration.name.clone(),
            ))
        };
    };
    if enumeration.type_params.is_empty() {
        return Err(ImportCompatibilityError::GenericBodyMismatch(
            enumeration.name.clone(),
        ));
    }
    let align_ast::Item::Enum(parsed) =
        parse_generic_fragment(&enumeration.name, body, "")?
    else {
        return Err(ImportCompatibilityError::GenericBodyMismatch(
            enumeration.name.clone(),
        ));
    };
    let variants = parsed
        .variants
        .iter()
        .map(|variant| {
            (
                variant.name.name.clone(),
                variant.payload.iter().map(convert_type).collect::<Vec<_>>(),
            )
        })
        .collect::<Vec<_>>();
    if parsed.name.name != enumeration.name
        || convert_type_params(&parsed.type_params) != enumeration.type_params
        || variants != enumeration.variants
    {
        return Err(ImportCompatibilityError::GenericBodyMismatch(
            enumeration.name.clone(),
        ));
    }
    Ok(())
}

fn validate_import_generic_bodies(
    summary: &InterfaceSummary,
) -> Result<(), ImportCompatibilityError> {
    for function in &summary.fns {
        validate_generic_function(function)?;
    }
    for structure in &summary.structs {
        validate_generic_struct(structure)?;
    }
    for enumeration in &summary.enums {
        validate_generic_enum(enumeration)?;
    }
    Ok(())
}

fn validate_import_summaries(
    params: &[IParam],
    ret: &IType,
    borrow: &ReturnBorrowSummary,
    region: &ReturnRegionSummary,
    analysis: &CapabilityAnalysis<'_>,
    type_params: &[ITypeParam],
) -> Result<(), ImportCompatibilityError> {
    let ret_may_supply_provenance = analysis.may_supply_return_provenance(ret, type_params);
    let param_may_supply_provenance = params
        .iter()
        .map(|param| analysis.may_supply_return_provenance(&param.ty, type_params))
        .collect::<Vec<_>>();
    for roots in [
        match borrow {
            ReturnBorrowSummary::None => None,
            ReturnBorrowSummary::Roots { params, captures } => {
                Some((params.as_slice(), captures.as_slice()))
            }
        },
        match region {
            ReturnRegionSummary::None => None,
            ReturnRegionSummary::Roots { params, captures } => {
                Some((params.as_slice(), captures.as_slice()))
            }
        },
    ]
    .into_iter()
    .flatten()
    {
        if !ret_may_supply_provenance {
            return Err(ImportCompatibilityError::ReturnSummaryOnNonBorrowingType);
        }
        if !roots.1.is_empty() {
            return Err(ImportCompatibilityError::ReturnSummaryCaptureRoot);
        }
        for &index in roots.0 {
            let Some((parameter, &may_supply_provenance)) = params
                .get(index as usize)
                .zip(param_may_supply_provenance.get(index as usize))
            else {
                return Err(ImportCompatibilityError::ReturnSummaryRootCannotBorrow(
                    index,
                ));
            };
            if !may_supply_provenance
                && !matches!(parameter.mode, ParamMode::Borrow | ParamMode::BorrowMut)
            {
                return Err(ImportCompatibilityError::ReturnSummaryRootCannotBorrow(
                    index,
                ));
            }
        }
    }
    Ok(())
}

fn validate_return_cleanup_metadata(
    ty: &IType,
    type_params: &[ITypeParam],
    analysis: &CapabilityAnalysis<'_>,
) -> Result<(), ImportCompatibilityError> {
    let mut work = vec![ty];
    while let Some(current) = work.pop() {
        match current {
            IType::Named { args, .. } => work.extend(args.iter().rev()),
            IType::Tuple(elements) => work.extend(elements.iter().rev()),
            IType::Fn {
                params,
                ret,
                return_cleanup,
                ..
            } => {
                if params.iter().any(|parameter| {
                    matches!(parameter.mode, ParamMode::Borrow | ParamMode::BorrowMut)
                        && matches!(&parameter.ty, IType::Named { path, args }
                            if path == "region" && args.is_empty())
                }) {
                    return Err(ImportCompatibilityError::BorrowParamRegion);
                }
                if let Some(expected) = analysis.return_cleanup(ret, type_params)
                    && *return_cleanup != expected
                {
                    return Err(ImportCompatibilityError::ReturnCleanupMismatch);
                }
                work.push(ret);
                work.extend(params.iter().rev().map(|parameter| &parameter.ty));
            }
        }
    }
    Ok(())
}

/// Validate that a decoded interface uses the enabled semantic subset. Codec validation has
/// already proved canonical return summaries; this gate proves ownership-dependent mode facts
/// before reconstructing imported source.
pub fn validate_for_import(
    summary: &InterfaceSummary,
) -> Result<(), ImportCompatibilityError> {
    let index = LocalDefinitionIndex::new(summary)?;
    validate_import_resources(summary)?;
    validate_import_shapes(summary, &index)?;
    validate_import_generic_bodies(summary)?;
    validate_import_headers(summary)?;
    let analysis = CapabilityAnalysis::new(index)?;

    for function in &summary.fns {
        if function.params.iter().any(|parameter| {
            matches!(parameter.mode, ParamMode::Borrow | ParamMode::BorrowMut)
                && matches!(&parameter.ty, IType::Named { path, args }
                    if path == "region" && args.is_empty())
        }) {
            return Err(ImportCompatibilityError::BorrowParamRegion);
        }
        if function.type_params.is_empty()
            && let Some(expected) = analysis.return_cleanup(&function.ret, &[])
            && function.return_cleanup != expected
        {
            return Err(ImportCompatibilityError::ReturnCleanupMismatch);
        }
        for parameter in &function.params {
            validate_return_cleanup_metadata(
                &parameter.ty,
                &function.type_params,
                &analysis,
            )?;
        }
        validate_return_cleanup_metadata(&function.ret, &function.type_params, &analysis)?;
        validate_import_summaries(
            &function.params,
            &function.ret,
            &function.return_borrow,
            &function.return_region,
            &analysis,
            &function.type_params,
        )?;
        if function
            .parallel_transfer_params
            .windows(2)
            .any(|pair| pair[0] >= pair[1])
        {
            return Err(ImportCompatibilityError::ParallelTransferRootsNonCanonical);
        }
        if function.generic_body.is_some() && !function.parallel_transfer_params.is_empty() {
            return Err(ImportCompatibilityError::ReturnSummaryOnUnsupportedSignature);
        }
        for &index in &function.parallel_transfer_params {
            let Some(parameter) = function.params.get(index as usize) else {
                return Err(ImportCompatibilityError::ReturnSummaryRootCannotBorrow(
                    index,
                ));
            };
            if !analysis.may_borrow(&parameter.ty, &function.type_params)
                && !matches!(parameter.mode, ParamMode::Borrow | ParamMode::BorrowMut)
            {
                return Err(ImportCompatibilityError::ReturnSummaryRootCannotBorrow(
                    index,
                ));
            }
        }
    }
    for structure in &summary.structs {
        if structure.type_params.is_empty() {
            for (_, field) in &structure.fields {
                validate_return_cleanup_metadata(field, &[], &analysis)?;
            }
        }
    }
    for enumeration in &summary.enums {
        if enumeration.type_params.is_empty() {
            for (_, payload) in &enumeration.variants {
                for ty in payload {
                    validate_return_cleanup_metadata(ty, &[], &analysis)?;
                }
            }
        }
    }
    Ok(())
}

fn render_fn(f: &IFnSig) -> String {
    if let Some(body) = &f.generic_body {
        // A generic `pub` template ships its full declaration (incl. body) as source — the consumer
        // monomorphizes it. `fd.span` starts at `fn`, so re-add the `pub` the slice omitted.
        return format!("pub {}\n", body.trim_start());
    }
    // A non-generic `pub` fn: signature only, with an empty body. The body is never type-checked (the
    // module is interface-only) and the function is never emitted into the consumer's program; the
    // signature exists only so the consumer resolves `dep.f(...)`. Parameter names are synthesized
    // (the summary is positional — names are not an interface property).
    let params = f
        .params
        .iter()
        .enumerate()
        .map(|(i, p)| {
            let mode = match p.mode {
                ParamMode::ByValue => "",
                ParamMode::Out => "out ",
                ParamMode::Borrow => "borrow ",
                ParamMode::BorrowMut => "borrow mut ",
            };
            format!("{mode}arg{i}: {}", render_itype(&p.ty))
        })
        .collect::<Vec<_>>()
        .join(", ");
    let ret = if is_unit_itype(&f.ret) { String::new() } else { format!(" -> {}", render_itype(&f.ret)) };
    format!("pub fn {}({params}){ret} {{}}\n", f.name)
}

fn render_resource(resource: &IResourceDef, unit: &str) -> String {
    // The internal hook path is not a public interface field. This declaration is parsed only in
    // an interface-only module; sema restores the producer-owned thunk metadata out of band and
    // deliberately never resolves this inert placeholder path.
    format!(
        "pub resource {}{} = {}.internal.resource.__align_interface_drop_{}\n",
        resource.name,
        render_type_params(&resource.type_params),
        unit,
        resource.name,
    )
}

/// Render an imported unit's `InterfaceSummary` back to Align source: exactly its public surface, as
/// the consumer's per-unit sema must re-parse it to resolve `dep.name`. `dep_units` are the names of
/// the other units in the transitive dependency set; each is `import`ed so any module reference in a
/// generic template body (which is opaque here) still resolves. Interface-only modules emit no
/// diagnostics in sema, so the (possibly unused) imports are silent.
pub fn summary_to_source(
    summary: &InterfaceSummary,
    dep_units: &[&str],
) -> Result<String, ImportCompatibilityError> {
    validate_for_import(summary)?;
    let mut out = String::new();
    let mut builtin_type_imports = std::collections::BTreeSet::<String>::new();
    let mut collect_type = |root: &IType| {
        let mut work = vec![root];
        while let Some(ty) = work.pop() {
            match ty {
                IType::Named { path, args } => {
                    match path.as_str() {
                        "crypto.argon2_params" => {
                            builtin_type_imports.insert("std.crypto".to_string());
                        }
                        "regex.regex_match" => {
                            builtin_type_imports.insert("std.regex".to_string());
                        }
                        "http_sse_event" | "http.http_sse_event" => {
                            builtin_type_imports.insert("std.http".to_string());
                        }
                        _ => {}
                    }
                    work.extend(args);
                }
                IType::Tuple(elements) => work.extend(elements),
                IType::Fn { params, ret, .. } => {
                    work.push(ret);
                    work.extend(params.iter().map(|parameter| &parameter.ty));
                }
            }
        }
    };
    for structure in &summary.structs {
        for (_, ty) in &structure.fields {
            collect_type(ty);
        }
    }
    for enumeration in &summary.enums {
        for (_, payload) in &enumeration.variants {
            for ty in payload {
                collect_type(ty);
            }
        }
    }
    for function in &summary.fns {
        for parameter in &function.params {
            collect_type(&parameter.ty);
        }
        collect_type(&function.ret);
    }
    for constant in &summary.consts {
        if let Some(ty) = &constant.ty {
            collect_type(ty);
        }
    }
    // Render the capability imports and transitive dependency imports from one canonical set. A
    // dependency can itself be the builtin module required by a public nominal type (for example,
    // `std.crypto` for `crypto.argon2_params`); emitting both sources separately would synthesize a
    // duplicate import that sema rejects even though the producer summary is valid.
    let mut imports = builtin_type_imports;
    for dep in dep_units {
        if *dep != summary.unit {
            imports.insert((*dep).to_string());
        }
    }
    for import in imports {
        out.push_str(&format!("import {import}\n"));
    }
    for c in &summary.consts {
        out.push_str("pub ");
        out.push_str(&c.name);
        if let Some(ty) = &c.ty {
            out.push_str(": ");
            out.push_str(&render_itype(ty));
        }
        out.push_str(" := ");
        out.push_str(&c.value_src);
        out.push('\n');
    }
    for s in &summary.structs {
        out.push_str(&render_struct(s));
    }
    for e in &summary.enums {
        out.push_str(&render_enum(e));
    }
    for resource in &summary.resources {
        out.push_str(&render_resource(resource, &summary.unit));
    }
    for f in &summary.fns {
        out.push_str(&render_fn(f));
    }
    Ok(out)
}

/// The cross-unit effect seeds an importer needs: the 3-valued effect bit of every **non-generic**
/// `pub` function in `summary`, keyed by its canonical (mangled) name. Generic functions are excluded
/// — their monomorphs are instantiated into the importer's own program and their effects recomputed
/// from the instantiated body. `is_entry` is the imported unit's entry flag (always `false` for a real
/// dependency; the parameter mirrors [`align_sema::Module::is_entry`]).
pub fn summary_effects(
    summary: &InterfaceSummary,
    is_entry: bool,
) -> HashMap<String, align_sema::FnEffect> {
    let mut m = HashMap::new();
    for f in &summary.fns {
        if f.generic_body.is_some() {
            continue;
        }
        let canonical = mangle(&summary.unit, is_entry, &f.name);
        let e = match f.effect {
            Effect::Pure => align_sema::FnEffect::Pure,
            Effect::Impure => align_sema::FnEffect::Impure,
            Effect::Unknown => align_sema::FnEffect::Unknown,
        };
        m.insert(canonical, e);
    }
    m
}

/// The cross-unit return-provenance seeds an importer needs, keyed by canonical (mangled) function
/// name. Generic functions are excluded because their bodies are instantiated in the consumer and
/// their summaries are inferred there.
pub fn summary_return_provenance(
    summary: &InterfaceSummary,
    is_entry: bool,
) -> align_sema::ExternalReturnProvenance {
    let mut facts = HashMap::new();
    for function in &summary.fns {
        if function.generic_body.is_some() {
            continue;
        }
        facts.insert(
            mangle(&summary.unit, is_entry, &function.name),
            (
                function.return_borrow.clone(),
                function.return_region.clone(),
                function.return_cleanup,
                function.parallel_transfer_params.clone(),
            ),
        );
    }
    facts
}


/// Producer-owned resource facts keyed by the canonical non-entry nominal identity used when a
/// dependency is reconstructed as an interface-only module.
pub fn summary_resource_facts(
    summary: &InterfaceSummary,
) -> align_sema::ExternalResourceFacts {
    summary
        .resources
        .iter()
        .map(|resource| {
            (
                mangle(&summary.unit, false, &resource.name),
                align_sema::ExternalResourceFact {
                    generic_arity: resource.generic_arity,
                    representation_version: resource.representation_version,
                    drop_thunk: resource.drop_thunk.clone(),
                    drop_abi_fingerprint: resource.drop_abi_fingerprint,
                },
            )
        })
        .collect()
}

/// Producer-checked raw-hook body facts for per-unit consumers. Generic functions are excluded:
/// resource hooks are required to be non-generic before this fact is consulted.
pub fn summary_resource_hook_facts(
    summary: &InterfaceSummary,
    is_entry: bool,
) -> align_sema::ExternalResourceHookFacts {
    summary
        .fns
        .iter()
        .filter(|function| function.generic_body.is_none())
        .map(|function| {
            (
                mangle(&summary.unit, is_entry, &function.name),
                function.resource_hook_body,
            )
        })
        .collect()
}

#[cfg(test)]
mod builtin_spelling_tests {
    use super::*;

    /// The interface layer knows builtin *spellings*; `align_sema` knows what each one owns. Since
    /// import validation compares a recorded cleanup bit against sema's answer for a spelling, the
    /// two sets must describe one language. Sweep both directions so a spelling only one side knows
    /// fails here instead of rejecting a valid interface as a return-cleanup mismatch.
    #[test]
    fn the_ownership_bridge_and_the_builtin_capability_set_cover_the_same_spellings() {
        // Heads whose ownership needs a nominal argument or a local definition, so the bridge
        // deliberately does not answer for them; the capability table and the definition index do.
        let nominal_or_local = [
            "soa",
            "json.scanner",
            "resource_ref",
            "json.kind",
            "Error",
            "core.Error",
            "argon2_params",
            "crypto.argon2_params",
            "regex_match",
            "regex.regex_match",
            "http_sse_event",
            "http.http_sse_event",
            // Walked into their arguments by the ownership analysis itself.
            "Option",
            "Result",
        ];
        for (spelling, _, _) in BUILTIN_CAPABILITIES {
            let bridged = align_sema::builtin_spelling_needs_return_cleanup(spelling);
            if nominal_or_local.contains(spelling) {
                assert_eq!(
                    bridged, None,
                    "`{spelling}` is resolved by the analysis, not the ownership bridge"
                );
            } else {
                assert!(
                    bridged.is_some(),
                    "`{spelling}` is an interface spelling with no ownership answer — a function \
                     returning it would be validated against a guess"
                );
            }
        }
        for (spelling, _) in align_sema::BUILTIN_SPELLING_TYS {
            assert!(
                builtin_capability(spelling).is_some(),
                "the ownership bridge answers for `{spelling}`, which this crate does not accept \
                 as a builtin spelling"
            );
        }
        // The prefix-parsed heads the table cannot list: both sides must still agree they are
        // builtins that own nothing.
        for spelling in ["vec4", "mask8"] {
            assert!(builtin_capability(spelling).is_some());
            assert_eq!(
                align_sema::builtin_spelling_needs_return_cleanup(spelling),
                None,
                "a `{spelling}` head carries its element in an argument; the analysis walks it"
            );
        }
        // An integer spelling is parsed, not listed, and must agree on both sides.
        for spelling in ["i8", "u64"] {
            assert!(builtin_capability(spelling).is_some());
            assert_eq!(
                align_sema::builtin_spelling_needs_return_cleanup(spelling),
                Some(false)
            );
        }
        // A non-canonical integer spelling is whatever the *resolver's* parser says it is (`i08`
        // resolves to `i8` there), so the bridge reuses that parser rather than a second one.
        assert_eq!(
            align_sema::builtin_spelling_needs_return_cleanup("i08"),
            align_sema::builtin_spelling_needs_return_cleanup("i8"),
            "the bridge must classify an integer spelling exactly as the type resolver does"
        );
    }
}
