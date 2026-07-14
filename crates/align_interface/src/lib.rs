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
//! * `impl_hash` — over the unit's own implementation. Changes on any body edit; consumers do not
//!   depend on it.
//!
//! ## Honest S1a compromises (recorded)
//!
//! * **`impl_hash` source.** MIR is whole-program today, not per-unit separable, so `impl_hash` is
//!   taken over the unit's **source bytes** (a change to any body changes the source → the hash;
//!   never under-invalidates — a comment/whitespace edit over-invalidates the unit's own object, but
//!   no consumer, which is sound). S2 replaces this with the unit's own canonical location-free MIR
//!   (`docs/impl/10` §6.2). Marked `TODO(m15-s2)` at the call site.
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
//! ## Known finding (out of S1a scope — do NOT fix here)
//!
//! Sema does **not** reject a `pub` fn whose signature references a NON-`pub` type (verified: a `pub
//! fn make() -> Secret` over a private `Secret` type-checks and its cross-module caller binds the
//! value). The M15 completeness argument assumes this is rejected. Until it is, such a summary names
//! the private type in the signature but does **not** carry its definition (the interface is not
//! self-contained in that case). Recorded, not worked around.

mod codec;
mod hash;

pub use codec::{deserialize, serialize, DecodeError, FORMAT_VERSION};
pub use hash::Hash128;

use std::collections::HashMap;

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
    Fn { params: Vec<IType>, ret: Box<IType> },
}

/// One parameter of a `pub` signature. **Names are intentionally excluded** (Align calls are
/// positional — renaming a parameter is not an interface change); only the `out` marker and the type
/// are ABI-relevant.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct IParam {
    /// The `out` marker (`fn f(out dst: slice<i64>, ...)`), the noalias-writeback ABI bit.
    pub is_out: bool,
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
    /// The 3-valued effect bit (part of the interface — flipping Pure→Impure is an interface change).
    pub effect: Effect,
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
    /// Exported sum types, sorted by name.
    pub enums: Vec<IEnumDef>,
    /// Exported consts, sorted by name.
    pub consts: Vec<IConst>,
    /// The unit's capability set (gated external libraries its code needs), sorted. Link-summary
    /// data; NOT folded into `interface_hash`.
    pub capabilities: Vec<String>,
    /// Hash of the canonical interface surface (signatures + type defs + consts + effect bits +
    /// generic template bodies). Consumers depend on this ONLY.
    pub interface_hash: Hash128,
    /// Hash of the unit's implementation (S1a: its source bytes; S2: its own MIR).
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
        align_ast::Type::Fn { params, ret, .. } => {
            IType::Fn { params: params.iter().map(convert_type).collect(), ret: Box::new(convert_type(ret)) }
        }
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
) -> Vec<InterfaceSummary> {
    let effects: HashMap<String, Effect> =
        align_sema::fn_effects(program).into_iter().map(|(k, v)| (k, v.into())).collect();
    let caps_by_unit = partition_capabilities(modules, mir);

    let mut summaries = Vec::with_capacity(modules.len());
    for m in modules {
        let empty = String::new();
        let src = sources.get(&m.path).unwrap_or(&empty);

        let mut fns: Vec<IFnSig> = Vec::new();
        let mut structs: Vec<IStructDef> = Vec::new();
        let mut enums: Vec<IEnumDef> = Vec::new();
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
                        fns.push(IFnSig {
                            name: fd.name.name.clone(),
                            type_params: convert_type_params(&fd.type_params),
                            params: fd
                                .params
                                .iter()
                                .map(|p| IParam { is_out: p.is_out, ty: convert_type(&p.ty) })
                                .collect(),
                            ret: convert_ret(&fd.ret),
                            effect,
                            generic_body: is_generic.then(|| safe_slice(src, fd.span)),
                        });
                    }
                    // Non-pub fns are module-private: not part of the exported interface surface.
                }
                align_ast::Item::Struct(sd) => {
                    if is_pub(sd.vis) {
                        let is_generic = !sd.type_params.is_empty();
                        structs.push(IStructDef {
                            name: sd.name.name.clone(),
                            type_params: convert_type_params(&sd.type_params),
                            fields: sd
                                .fields
                                .iter()
                                .map(|f| (f.name.name.clone(), convert_type(&f.ty)))
                                .collect(),
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
                        enums.push(IEnumDef {
                            name: ed.name.name.clone(),
                            type_params: convert_type_params(&ed.type_params),
                            variants: ed
                                .variants
                                .iter()
                                .map(|v| {
                                    (v.name.name.clone(), v.payload.iter().map(convert_type).collect())
                                })
                                .collect(),
                            generic_body: is_generic.then(|| safe_slice(src, ed.span)),
                        });
                    }
                    // Non-pub enums are module-private: not part of the exported interface surface.
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
        consts.sort_by(|a, b| a.name.cmp(&b.name));

        let mut capabilities = caps_by_unit.get(&m.path).cloned().unwrap_or_default();
        capabilities.sort();
        capabilities.dedup();

        // Assemble without hashes, compute them, then fill them in.
        let mut summary = InterfaceSummary {
            unit: m.path.clone(),
            fns,
            structs,
            enums,
            consts,
            capabilities,
            interface_hash: Hash128 { lo: 0, hi: 0 },
            impl_hash: Hash128 { lo: 0, hi: 0 },
        };
        summary.interface_hash = Hash128::of(&codec::encode_interface_surface(&summary));
        // TODO(m15-s2): replace source-byte impl identity with the unit's own canonical
        // location-free MIR (doc-10 §6.2), once MIR is per-unit separable.
        summary.impl_hash = Hash128::of(src.as_bytes());
        summaries.push(summary);
    }
    summaries
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
        let caps = align_mir::function_capabilities(f);
        if caps.is_empty() {
            continue;
        }
        let unit = owning_unit(&f.name).or(entry_unit.as_ref());
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
