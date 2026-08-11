//! Process-in-memory memoization of repeated identical compilations
//! (`docs/impl/10-cache-first-optimization.md` §6.6).
//!
//! The on-disk codegen cache (`cache.rs`) makes a *second process* cheap. This module makes a
//! *second identical compilation inside one process* cheap. The two are complementary and share the
//! same soundness model: an artifact is reused only when a canonical encoding of every input that
//! can change it is byte-identical.
//!
//! Four stages are memoized, all keyed by content only:
//!
//! * **program** — the whole-program sema result ([`crate::check`]) for an error-free program;
//! * **unit** — one per-unit frontend result (`walk_per_unit`'s checked summary + lowered MIR) for a
//!   unit that checked without errors and owns NO static descriptors;
//! * **lowering** — the MIR one checked HIR program lowers to;
//! * **object** — the object bytes [`crate::emit_object_file`] produces for one MIR program.
//!
//! Every stage is a PURE optimization: a hit must be indistinguishable from a miss. Diagnostics are
//! therefore replayed rather than dropped (a clean unit routinely warns), and anything whose replay
//! would have to reproduce a side effect outside this module — a static-input publication lock, a
//! metadata file read, a `SourceMap`-dependent line table — is never retained at all. See the
//! closure matrix in `docs/impl/10-cache-first-optimization.md` §6.6.
//!
//! Lifetime is the process. Nothing is invalidated, because nothing that a key does not already
//! cover can change while the process runs: a source edit changes the hashed source text, and a
//! compiler/LLVM/target change cannot happen without a new process. All four maps are guarded by one
//! mutex and are safe to use from the parallel per-unit codegen workers.

use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, MutexGuard, OnceLock};

use align_interface::{Hash128, InterfaceSummary};

/// Key-encoding version. Bump when the key material below changes meaning, so a stale in-memory
/// encoding can never be confused with a new one within one process image.
const KEY_FORMAT_VERSION: u32 = 1;

/// Retained object bytes are capped so a pathological build cannot trade unbounded RSS for compile
/// time. Objects are small in practice (the whole eight-module `pkg.db` per-unit build is under one
/// megabyte), so this bound is never reached by a normal workload; it exists to make the worst case
/// stated rather than discovered. Insertion simply stops once the budget is spent — there is no
/// eviction, which keeps a hit deterministic for the process's whole life.
const OBJECT_BUDGET_BYTES: u64 = 256 * 1024 * 1024;

/// Retained frontend artifacts are capped by COUNT rather than bytes: an HIR or MIR program has no
/// cheap size, and the population is bounded by the distinct units and programs a process compiles
/// (tens, in the largest driver test binary). One budget per map.
const ENTRY_BUDGET: usize = 512;

/// Whether the memo is active. On by default: every in-process consumer that compiles the same
/// input twice benefits, and a consumer that never repeats pays only one structural hash per
/// object. [`set_enabled`] exists so a test can prove that the memo is unobservable by running the
/// same build with it off.
static ENABLED: AtomicBool = AtomicBool::new(true);

/// One memoized per-unit frontend result: everything `walk_per_unit` publishes for a clean,
/// descriptor-free unit. Such a unit's descriptor and artifact lists are empty by construction, so
/// only its summary, MIR, (descriptor-free but digest-bearing) static-input manifest, and its own
/// diagnostics have to be replayed.
pub(crate) struct CachedUnit {
    pub(crate) summary: InterfaceSummary,
    pub(crate) mir: align_mir::Program,
    pub(crate) static_inputs: crate::static_inputs::StaticInputManifest,
    /// The unit's diagnostics, in emission order, with the file component of each span REMOVED.
    ///
    /// A clean unit can still warn — `pkg.db` alone emits sixteen `lossy conversion` warnings — and
    /// dropping those on a replay would make the memo observable, so they are replayed rather than
    /// used to disqualify the unit. Only the byte offsets are stored: the `FileId` is an index into
    /// the walk's own `SourceMap` and is reattached from the unit's file in the replaying walk. A
    /// unit whose diagnostics do not all point into its OWN source file is not retained at all
    /// (`unit_diagnostics`), because no such reattachment would be correct.
    pub(crate) diagnostics: Vec<CachedDiagnostic>,
}

/// One replayable diagnostic: everything but the `FileId`, which the replaying walk supplies.
pub(crate) struct CachedDiagnostic {
    pub(crate) severity: align_diag::Severity,
    pub(crate) message: String,
    pub(crate) lo: u32,
    pub(crate) hi: u32,
}

/// The replayable form of `diags`, or `None` when this unit must not be retained.
///
/// Every diagnostic must carry a span in the unit's OWN file (`unit_file`). A span in another file
/// — a synthesized `<interface:…>` pseudo-file, or the `Span::new(0, 0, 0)` placeholder used by the
/// walk's internal-error paths — cannot be reattached correctly in another walk, where that id
/// denotes a different file, so the unit is left uncached instead.
pub(crate) fn unit_diagnostics(
    diags: &align_diag::Diagnostics,
    unit_file: align_span::FileId,
) -> Option<Vec<CachedDiagnostic>> {
    diags
        .iter()
        .map(|diagnostic| {
            let span = diagnostic.span.filter(|span| span.file == unit_file)?;
            Some(CachedDiagnostic {
                severity: diagnostic.severity,
                message: diagnostic.message.clone(),
                lo: span.lo,
                hi: span.hi,
            })
        })
        .collect()
}

/// Hit/miss and retention counters. Cumulative for the process (or since the last [`clear`]).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct MemoStats {
    /// Whole-program sema results served from memory.
    pub program_hits: u64,
    /// Whole-program sema results computed (whether or not they were then retained).
    pub program_misses: u64,
    /// Per-unit frontend results served from memory.
    pub unit_hits: u64,
    /// Per-unit frontend results computed (whether or not they were then retained).
    pub unit_misses: u64,
    /// HIR-to-MIR lowerings served from memory.
    pub lowering_hits: u64,
    /// HIR-to-MIR lowerings computed (whether or not they were then retained).
    pub lowering_misses: u64,
    /// Object files served from memory.
    pub object_hits: u64,
    /// Object files produced by codegen (whether or not they were then retained).
    pub object_misses: u64,
    /// Retained object bytes.
    pub object_bytes: u64,
    /// Artifacts not retained because a budget was already spent.
    pub refused: u64,
}

/// One memoized whole-program sema result: what [`crate::check`] publishes, plus the diagnostics the
/// sema step emitted.
///
/// Unlike a per-unit diagnostic, a whole-program one keeps its `FileId` verbatim: the caller only
/// uses this memo when the walk's file ids are the canonical `0..units` assignment, and the key pins
/// the ordered unit list, so id `i` denotes the same unit's source in every run that can hit.
pub(crate) struct CachedProgram {
    pub(crate) program: align_sema::hir::Program,
    pub(crate) static_descriptors: Vec<crate::StaticDescriptor>,
    pub(crate) diagnostics: Vec<align_diag::Diagnostic>,
}

#[derive(Default)]
struct Store {
    programs: HashMap<Hash128, CachedProgram>,
    units: HashMap<Hash128, CachedUnit>,
    lowerings: HashMap<Hash128, align_mir::Program>,
    /// Object bytes are the one shareable value (plain `Vec<u8>`), so they are handed out behind an
    /// `Arc`: `emit_object_file` runs on the parallel per-unit codegen workers, and copying a
    /// multi-megabyte object while holding the store mutex would serialize them.
    objects: HashMap<Hash128, Arc<Vec<u8>>>,
    stats: MemoStats,
}

fn store() -> MutexGuard<'static, Store> {
    static STORE: OnceLock<Mutex<Store>> = OnceLock::new();
    // A poisoned lock means a previous holder panicked while the maps were being mutated. The maps
    // are plain owned data with no cross-field invariant a panic can break (each insert is one
    // `HashMap::insert` plus a counter), so recovering the guard is sound and keeps one unrelated
    // panicking test from turning every later compilation in the process into a panic.
    STORE
        .get_or_init(|| Mutex::new(Store::default()))
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

/// Whether the memo is currently active.
pub fn enabled() -> bool {
    ENABLED.load(Ordering::Relaxed)
}

/// Turn the memo on or off. Disabling does not drop what is already retained (use [`clear`]);
/// it only stops further lookups and insertions.
pub fn set_enabled(on: bool) {
    ENABLED.store(on, Ordering::Relaxed);
}

/// The cumulative counters.
pub fn stats() -> MemoStats {
    store().stats
}

/// Drop every retained artifact and reset the counters.
pub fn clear() {
    let mut guard = store();
    guard.programs.clear();
    guard.units.clear();
    guard.lowerings.clear();
    guard.objects.clear();
    guard.stats = MemoStats::default();
}

/// The process-global compiler toggles that change lowering or checking, folded into every key.
///
/// `ALIGN_CONST_POOL` (sema), `ALIGN_NEEDLE_HOIST`, `ALIGN_BUFFER_DONATE`, and `ALIGN_SORT_ADAPTIVE`
/// (MIR lowering) are read at each lowering, not once per process, and the measurement tests flip
/// them with `set_var` *inside* one test binary. They are therefore genuine key material: without
/// them a toggled build could be served an artifact produced under the opposite setting. Codegen
/// itself reads no environment, but the object key carries the same fingerprint so that a future
/// codegen toggle cannot silently become unkeyed.
fn env_toggles(out: &mut String) {
    for name in [
        "ALIGN_CONST_POOL",
        "ALIGN_NEEDLE_HOIST",
        "ALIGN_BUFFER_DONATE",
        "ALIGN_SORT_ADAPTIVE",
    ] {
        out.push_str(name);
        out.push('=');
        match std::env::var(name) {
            Ok(value) => out.push_str(&value),
            Err(_) => out.push_str("\u{1}unset"),
        }
        out.push('\n');
    }
}

/// Append `field` = `value` with an explicit length so no two different field sequences can encode
/// to the same bytes.
fn field(out: &mut String, name: &str, value: &str) {
    out.push_str(name);
    out.push('=');
    out.push_str(&value.len().to_string());
    out.push(':');
    out.push_str(value);
    out.push('\n');
}

/// The canonical key for one whole-program sema result.
///
/// `align_sema::check_program_with_static_descriptors` reads exactly the module list: each unit's
/// module path, entry flag, and AST, and an AST is a pure function of its source text. `units` is
/// that list in the order it is passed to sema.
///
/// The caller must additionally have established the CANONICAL file-id assignment (unit `i` owns
/// `FileId` `i`) before using this key, because the retained HIR and diagnostics carry file ids
/// verbatim. Without that precondition the same source list could be checked under a different
/// id assignment, and a replayed span would name the wrong file.
pub(crate) fn program_key(units: &[(&str, bool, &str)]) -> Hash128 {
    let mut material = String::with_capacity(4096);
    material.push_str("align-inproc-program-v");
    material.push_str(&KEY_FORMAT_VERSION.to_string());
    material.push('\n');
    env_toggles(&mut material);
    field(&mut material, "units", &units.len().to_string());
    for (path, is_entry, src) in units {
        field(&mut material, "unit", path);
        field(&mut material, "entry", if *is_entry { "1" } else { "0" });
        field(&mut material, "src", src);
    }
    Hash128::of(material.as_bytes())
}

/// The canonical key for one HIR-to-MIR lowering.
///
/// `variant` distinguishes the whole-program and per-unit visibility models, which lower the same
/// HIR differently. The HIR itself is fingerprinted with its total derived `Debug`, the same
/// technique `align_interface::codegen_impl_hash` uses for MIR: a field added to any HIR node
/// appears in the rendering automatically, so a new field cannot silently escape the key.
pub(crate) fn lowering_key(hir: &align_sema::hir::Program, variant: &str) -> Hash128 {
    let mut material = String::with_capacity(64);
    material.push_str("align-inproc-lowering-v");
    material.push_str(&KEY_FORMAT_VERSION.to_string());
    material.push('\n');
    env_toggles(&mut material);
    field(&mut material, "variant", variant);
    field(&mut material, "hir", &format!("{hir:?}"));
    Hash128::of(material.as_bytes())
}

/// The canonical key for one per-unit frontend result.
///
/// The complete input to `walk_per_unit`'s per-unit check is: the unit's own module path, entry
/// flag, and source text; the interface-only dependency modules it is checked against, as the exact
/// rendered source each was parsed from, in the order they are passed to sema; and the four
/// external fact maps seeded from those dependencies' summaries. Everything else the unit's result
/// depends on is either a compile-time constant of this process image or one of the environment
/// toggles above.
///
/// Dependencies are keyed by their rendered interface SOURCE, not by their `interface_hash`: the
/// source is what sema actually consumes, so it needs no assumption about which parts of a summary
/// the hash covers.
pub(crate) fn unit_key(
    unit: &str,
    is_entry: bool,
    src: &str,
    interfaces: &[(&str, &str)],
    external: ExternalFacts<'_>,
) -> Hash128 {
    let mut material = String::with_capacity(src.len() + 4096);
    material.push_str("align-inproc-unit-v");
    material.push_str(&KEY_FORMAT_VERSION.to_string());
    material.push('\n');
    env_toggles(&mut material);
    field(&mut material, "unit", unit);
    field(&mut material, "entry", if is_entry { "1" } else { "0" });
    field(&mut material, "src", src);
    field(&mut material, "interfaces", &interfaces.len().to_string());
    for (path, source) in interfaces {
        field(&mut material, "dep", path);
        field(&mut material, "iface", source);
    }
    facts(&mut material, "effects", external.effects);
    facts(&mut material, "provenance", external.return_provenance);
    facts(&mut material, "resources", external.resources);
    facts(&mut material, "hooks", external.resource_hooks);
    Hash128::of(material.as_bytes())
}

/// The four cross-unit fact maps the per-unit check is seeded with, borrowed together so the key
/// and the sema call cannot disagree about which set was used.
pub(crate) struct ExternalFacts<'a> {
    pub(crate) effects: &'a HashMap<String, align_sema::FnEffect>,
    pub(crate) return_provenance: &'a align_sema::ExternalReturnProvenance,
    pub(crate) resources: &'a align_sema::ExternalResourceFacts,
    pub(crate) resource_hooks: &'a align_sema::ExternalResourceHookFacts,
}

/// Append one external-fact map in name order. `HashMap` iteration order is not deterministic, so
/// the entries are sorted; the values are rendered with `Debug`, which is total over their fields by
/// derivation, so a new field cannot silently escape the key.
fn facts<V: std::fmt::Debug>(out: &mut String, name: &str, map: &HashMap<String, V>) {
    let mut entries: Vec<(&String, &V)> = map.iter().collect();
    entries.sort_by(|left, right| left.0.cmp(right.0));
    field(out, name, &entries.len().to_string());
    for (key, value) in entries {
        field(out, "k", key);
        field(out, "v", &format!("{value:?}"));
    }
}

/// The canonical key for one object emission.
///
/// `emit_object` builds its LLVM module from exactly (`mir`, `target`, `profile`, `exports`,
/// `rt_lto`) — the module is named by a constant, never by the output path, so two emissions that
/// agree on these produce byte-identical objects. The MIR is fingerprinted with the same complete
/// structural rendering the on-disk codegen cache uses for its `impl_hash`
/// (`align_interface::codegen_impl_hash`, a total `Debug` of `align_mir::Program`). The remaining
/// components of the on-disk key — compiler build id, LLVM version, resolved target identity,
/// object format — are constants of the running process image and cannot differ between two calls
/// in it, except through `target`, which is keyed directly.
pub(crate) fn object_key(
    mir: &align_mir::Program,
    target: &align_codegen_llvm::BuildTarget,
    profile: align_codegen_llvm::Profile,
    exports: &[String],
    rt_lto: bool,
) -> Hash128 {
    let mut material = String::with_capacity(256);
    material.push_str("align-inproc-object-v");
    material.push_str(&KEY_FORMAT_VERSION.to_string());
    material.push('\n');
    env_toggles(&mut material);
    field(&mut material, "target", &format!("{target:?}"));
    field(&mut material, "profile", profile.name());
    field(&mut material, "rt_lto", if rt_lto { "1" } else { "0" });
    field(&mut material, "exports", &exports.len().to_string());
    for export in exports {
        field(&mut material, "export", export);
    }
    let impl_hash = align_interface::codegen_impl_hash(mir);
    field(&mut material, "mir", &format!("{impl_hash:?}"));
    Hash128::of(material.as_bytes())
}

/// The memoized whole-program sema result for `key`, if the memo is on and holds one.
///
/// Returned BY VALUE, not behind an `Arc`: `hir::FnTy` carries a `Cell<FnEffect>`, so a checked HIR
/// is neither `Sync` nor safely shareable. A deep clone is also what makes a hit indistinguishable
/// from a miss — the caller gets a program it solely owns, exactly as `align_sema` would have handed
/// it, and the retained snapshot cannot be reached by anything downstream.
pub(crate) fn program_lookup(key: Hash128) -> Option<CachedProgram> {
    if !enabled() {
        return None;
    }
    let mut guard = store();
    let Some(hit) = guard.programs.get(&key) else {
        guard.stats.program_misses += 1;
        return None;
    };
    let cloned = CachedProgram {
        program: hit.program.clone(),
        static_descriptors: hit.static_descriptors.clone(),
        diagnostics: hit.diagnostics.clone(),
    };
    guard.stats.program_hits += 1;
    Some(cloned)
}

/// Retain one whole-program sema result. The caller must have established the canonical file-id
/// assignment and that the program checked without errors.
pub(crate) fn program_store(key: Hash128, program: CachedProgram) {
    if !enabled() {
        return;
    }
    let mut guard = store();
    if guard.programs.contains_key(&key) {
        return;
    }
    if guard.programs.len() >= ENTRY_BUDGET {
        guard.stats.refused += 1;
        return;
    }
    guard.programs.insert(key, program);
}

/// The memoized MIR for `key`, if the memo is on and holds it.
pub(crate) fn lowering_lookup(key: Hash128) -> Option<align_mir::Program> {
    if !enabled() {
        return None;
    }
    let mut guard = store();
    let Some(hit) = guard.lowerings.get(&key) else {
        guard.stats.lowering_misses += 1;
        return None;
    };
    let cloned = hit.clone();
    guard.stats.lowering_hits += 1;
    Some(cloned)
}

/// Retain one lowering result.
pub(crate) fn lowering_store(key: Hash128, mir: &align_mir::Program) {
    if !enabled() {
        return;
    }
    let mut guard = store();
    if guard.lowerings.contains_key(&key) {
        return;
    }
    if guard.lowerings.len() >= ENTRY_BUDGET {
        guard.stats.refused += 1;
        return;
    }
    guard.lowerings.insert(key, mir.clone());
}

/// The memoized per-unit result for `key`, if the memo is on and holds one.
pub(crate) fn unit_lookup(key: Hash128) -> Option<CachedUnit> {
    if !enabled() {
        return None;
    }
    let mut guard = store();
    let Some(hit) = guard.units.get(&key) else {
        guard.stats.unit_misses += 1;
        return None;
    };
    let cloned = CachedUnit {
        summary: hit.summary.clone(),
        mir: hit.mir.clone(),
        static_inputs: hit.static_inputs.clone(),
        diagnostics: hit
            .diagnostics
            .iter()
            .map(|diagnostic| CachedDiagnostic {
                severity: diagnostic.severity,
                message: diagnostic.message.clone(),
                lo: diagnostic.lo,
                hi: diagnostic.hi,
            })
            .collect(),
    };
    guard.stats.unit_hits += 1;
    Some(cloned)
}

/// Retain one per-unit result. The caller must have established that the unit checked without
/// errors, owns no static descriptors, and has replayable diagnostics ([`unit_diagnostics`]).
pub(crate) fn unit_store(key: Hash128, unit: CachedUnit) {
    if !enabled() {
        return;
    }
    let mut guard = store();
    if guard.units.contains_key(&key) {
        return;
    }
    if guard.units.len() >= ENTRY_BUDGET {
        guard.stats.refused += 1;
        return;
    }
    guard.units.insert(key, unit);
}

/// The memoized object bytes for `key`, if the memo is on and holds them.
pub(crate) fn object_lookup(key: Hash128) -> Option<Arc<Vec<u8>>> {
    if !enabled() {
        return None;
    }
    let mut guard = store();
    let Some(hit) = guard.objects.get(&key) else {
        guard.stats.object_misses += 1;
        return None;
    };
    let hit = Arc::clone(hit);
    guard.stats.object_hits += 1;
    Some(hit)
}

/// Retain one object's bytes.
pub(crate) fn object_store(key: Hash128, bytes: Vec<u8>) {
    if !enabled() {
        return;
    }
    let mut guard = store();
    if guard.objects.contains_key(&key) {
        return;
    }
    // Widening only: `usize` is at most 64 bits on every supported host, and the running total is
    // compared in `u64` so a 32-bit host cannot wrap the budget check.
    let len = bytes.len() as u64;
    if guard.stats.object_bytes.saturating_add(len) > OBJECT_BUDGET_BYTES {
        guard.stats.refused += 1;
        return;
    }
    guard.stats.object_bytes += len;
    guard.objects.insert(key, Arc::new(bytes));
}
