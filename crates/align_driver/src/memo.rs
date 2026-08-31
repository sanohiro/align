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
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex, MutexGuard, OnceLock};

use align_interface::{Hash128, InterfaceSummary};

/// Key-encoding version. Bump when the key material below changes meaning, so a stale in-memory
/// encoding can never be confused with a new one within one process image.
const KEY_FORMAT_VERSION: u32 = 2;

/// Default retention budget, in ESTIMATED retained bytes across all four maps
/// ([`set_budget`] overrides it).
///
/// A one-shot `alignc` run exits long before any bound matters, but an embedder — align-llm today,
/// a language server later — keeps one process alive across thousands of compilations, so the bound
/// has to be denominated in memory rather than in entries. Insertion simply stops once the budget is
/// spent: there is no eviction, which keeps a hit deterministic for the process's whole life and
/// keeps a refusal from changing any output.
///
/// **How an entry is charged.** Objects are charged their exact byte length. The three frontend maps
/// are charged a *proxy*: the length of the canonical rendering the store site already holds.
///
/// ```text
/// program    the production HIR plus optional   the retained production artifact and private
///            test overlay `Debug` renderings    combined test view
/// lowering   the HIR rendering built for the    the MIR is derived from it and of the same order
///            key (free)
/// unit       the unit's key material (free)     contains the unit's full source and every
///                                               dependency interface it was checked against
/// object     exact byte length                  exact
/// ```
///
/// The proxy is an estimate, not a measurement — a rendering is wider than the structure it prints,
/// and a `Vec` reserves beyond its length — so the budget bounds retention within a constant factor,
/// not exactly. That is the point: retention must be bounded and must stop at the bound. Measured
/// scale for the eight-module `pkg.db` package: one whole-program HIR renders to 1.8 MB, one
/// per-unit key is a few hundred kilobytes, and one per-unit object set is under a megabyte, so the
/// default admits roughly two hundred distinct whole-program compilations before it binds — past the
/// largest owner suite (44 distinct programs) and far short of a long-lived embedder's lifetime.
const DEFAULT_BUDGET_BYTES: u64 = 768 * 1024 * 1024;

/// The retention budget in estimated bytes. Configuration, not content: [`clear`] does not reset it.
static BUDGET_BYTES: AtomicU64 = AtomicU64::new(DEFAULT_BUDGET_BYTES);

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
    /// (`crate::unit_cache::replayable_diagnostics`), because no such reattachment would be correct.
    pub(crate) diagnostics: Vec<CachedDiagnostic>,
    /// Always `true`: [`unit_store`]'s precondition is a descriptor-free unit. Carried as data so a
    /// consumer that must exclude descriptor-owning units (the persistent cache's publish path) can
    /// check the fact instead of trusting a comment about who called `unit_store`.
    pub(crate) static_descriptors_were_empty: bool,
}

/// One replayable diagnostic: everything but the `FileId`, which the replaying walk supplies.
///
/// Shared with the persistent cache rather than duplicated: the two stages store the same three
/// fields under the same own-file rule, and a second copy of that rule is a second place for it to
/// drift. See [`crate::unit_cache::replayable_diagnostics`], the single filter.
pub(crate) use crate::unit_cache::CachedDiagnostic;

/// Hit/miss and retention counters. Cumulative for the process (or since the last [`clear`]).
///
/// Every stage has a `hits` / `misses` pair, where a MISS is a LOOKUP that found nothing. A stage
/// the memo declines outright — the memo off, a located lowering, a non-canonical file-id
/// assignment — performs no lookup and is counted in neither.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct MemoStats {
    /// Whole-program sema results served from memory.
    pub program_hits: u64,
    /// Lookups of a whole-program sema result that found nothing.
    pub program_misses: u64,
    /// Per-unit frontend results served from memory.
    pub unit_hits: u64,
    /// Lookups of a per-unit frontend result that found nothing.
    pub unit_misses: u64,
    /// HIR-to-MIR lowerings served from memory.
    pub lowering_hits: u64,
    /// Lookups of a lowering that found nothing.
    pub lowering_misses: u64,
    /// Object files served from memory.
    pub object_hits: u64,
    /// Lookups of object bytes that found nothing.
    pub object_misses: u64,
    /// Estimated bytes currently retained across all four maps, charged as documented on
    /// [`DEFAULT_BUDGET_BYTES`].
    pub retained_bytes: u64,
    /// Artifacts not retained because the budget was already spent, or because a concurrent
    /// emission to the same object path made the read-back untrustworthy.
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
    pub(crate) test_overlay: Option<align_sema::TestOverlay>,
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
    /// Object paths with an emission in flight: `(concurrent emitters, ever contended)`.
    ///
    /// `emit_object_file` retains the bytes it reads BACK from the file codegen just wrote. If two
    /// threads emitted different programs to one path, that read could observe the other emission
    /// and retain foreign bytes under this key. No caller does that — `codegen_units_parallel` gives
    /// each unit its own path and the test harnesses embed the pid and a per-test name — but the
    /// consequence would be silent and wrong, so the invariant is enforced rather than assumed:
    /// while a path is contended, BOTH emissions skip retention. The build itself is untouched, so
    /// this can never turn a working caller into a failing one.
    emitting: HashMap<std::path::PathBuf, (u32, bool)>,
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

/// Override the retention budget, in estimated bytes (see [`DEFAULT_BUDGET_BYTES`] for how an entry
/// is charged). Lowering it below what is already retained refuses every FURTHER insertion; it never
/// drops or invalidates an entry, because a refusal must not change any output. Configuration, not
/// content — [`clear`] does not reset it.
pub fn set_budget(bytes: u64) {
    BUDGET_BYTES.store(bytes, Ordering::Relaxed);
}

/// The retention budget currently in force.
pub fn budget() -> u64 {
    BUDGET_BYTES.load(Ordering::Relaxed)
}

/// Reserve `charge` estimated bytes, or report that the budget is spent. Callers hold `guard`.
fn reserve(guard: &mut Store, charge: u64) -> bool {
    if guard.stats.retained_bytes.saturating_add(charge) > budget() {
        guard.stats.refused += 1;
        return false;
    }
    guard.stats.retained_bytes += charge;
    true
}

/// Drop every retained artifact and reset the counters.
pub fn clear() {
    let mut guard = store();
    guard.programs.clear();
    guard.units.clear();
    guard.lowerings.clear();
    guard.objects.clear();
    // `emitting` is in-flight state owned by live `EmitGuard`s, not retained content: clearing it
    // would leave a guard removing an entry it no longer owns.
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
        // Through `field`, like every other component: a bare `NAME=value` line is not injective
        // (a value containing a newline could impersonate the next toggle).
        match std::env::var(name) {
            Ok(value) => field(out, name, &value),
            Err(_) => field(out, name, "\u{1}unset"),
        }
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
/// `seeded` is the diagnostic sink sema is handed. It is key material, not just context: sema READS
/// the sink — `declaration_has_prior_error` suppresses static-descriptor discovery for a function the
/// loader already reported an error inside — so two runs whose prior diagnostics differ can produce
/// different descriptors from the same sources.
///
/// The caller must additionally have established the CANONICAL file-id assignment (unit `i` owns
/// `FileId` `i`) before using this key, because the retained HIR and diagnostics carry file ids
/// verbatim. Without that precondition the same source list could be checked under a different
/// id assignment, and a replayed span would name the wrong file.
pub(crate) fn program_key(
    units: &[(&str, bool, &str)],
    seeded: &align_diag::Diagnostics,
) -> Hash128 {
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
    field(&mut material, "seeded", &seeded.len().to_string());
    for diagnostic in seeded.iter() {
        field(&mut material, "sev", &format!("{:?}", diagnostic.severity));
        field(&mut material, "span", &format!("{:?}", diagnostic.span));
        field(&mut material, "msg", &diagnostic.message);
    }
    Hash128::of(material.as_bytes())
}

/// The canonical key for one HIR-to-MIR lowering.
///
/// `variant` distinguishes the whole-program and per-unit visibility models, which lower the same
/// HIR differently. The HIR identity is the versioned span-erased production projection. It
/// includes the ownership fact associated with each expression in stored traversal order and
/// rejects orphan span-keyed facts before lookup.
pub(crate) fn lowering_key(
    hir: &align_sema::hir::Program,
    variant: &str,
) -> Option<(Hash128, u64)> {
    let mut material = String::with_capacity(64);
    material.push_str("align-inproc-lowering-v");
    material.push_str(&KEY_FORMAT_VERSION.to_string());
    material.push('\n');
    env_toggles(&mut material);
    field(&mut material, "variant", variant);
    let projection = align_sema::production_codegen_projection(hir)?;
    let mut projection_hex = String::with_capacity(projection.len() * 2);
    for byte in projection {
        use std::fmt::Write as _;
        let _ = write!(projection_hex, "{byte:02x}");
    }
    field(&mut material, "hir", &projection_hex);
    Some((Hash128::of(material.as_bytes()), material.len() as u64))
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
) -> (Hash128, u64) {
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
    (Hash128::of(material.as_bytes()), material.len() as u64)
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
        test_overlay: hit.test_overlay.clone(),
        diagnostics: hit.diagnostics.clone(),
    };
    guard.stats.program_hits += 1;
    Some(cloned)
}

/// Retain one whole-program sema result. The caller must have established the canonical file-id
/// assignment and that the program checked without errors.
///
/// The retention charge is the production HIR's own `Debug` length plus the private test overlay's
/// rendering when present. Rendering the production HIR costs about 4 ms for the eight-module
/// `pkg.db` program — 0.3% of the ~1.5 s sema step this insertion follows — and unlike the source
/// length (which the retained HIR exceeds sevenfold) it tracks the artifacts actually held. The
/// overlay owns a second combined program, descriptors, and catalog, so omitting it would let test
/// compilations exceed the configured retention budget without accounting.
pub(crate) fn program_store(key: Hash128, program: CachedProgram) {
    if !enabled() {
        return;
    }
    let charge = u64::try_from(format!("{:?}", program.program).len())
        .unwrap_or(u64::MAX)
        .saturating_add(program.test_overlay.as_ref().map_or(0, |overlay| {
            u64::try_from(format!("{overlay:?}").len()).unwrap_or(u64::MAX)
        }));
    let mut guard = store();
    if guard.programs.contains_key(&key) || !reserve(&mut guard, charge) {
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

/// Retain one lowering result. `hir_render_len` is the length of the HIR rendering
/// [`lowering_key`] already built, which the derived MIR is of the same order as.
pub(crate) fn lowering_store(key: Hash128, mir: &align_mir::Program, hir_render_len: u64) {
    if !enabled() {
        return;
    }
    let mut guard = store();
    if guard.lowerings.contains_key(&key) || !reserve(&mut guard, hir_render_len) {
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
        diagnostics: hit.diagnostics.clone(),
        static_descriptors_were_empty: hit.static_descriptors_were_empty,
    };
    guard.stats.unit_hits += 1;
    Some(cloned)
}

/// Retain one per-unit result. The caller must have established that the unit checked without
/// errors, owns no static descriptors, and has replayable diagnostics
/// ([`crate::unit_cache::replayable_diagnostics`]).
/// `key_material_len` is the length of the canonical key material [`unit_key`] already built: it
/// contains the unit's full source and every dependency interface it was checked against, which is
/// what the retained summary and MIR are derived from.
pub(crate) fn unit_store(key: Hash128, unit: CachedUnit, key_material_len: u64) {
    if !enabled() {
        return;
    }
    let mut guard = store();
    if guard.units.contains_key(&key) || !reserve(&mut guard, key_material_len) {
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

/// Retain one object's bytes, charged exactly.
pub(crate) fn object_store(key: Hash128, bytes: Vec<u8>) {
    if !enabled() {
        return;
    }
    // Widening only: `usize` is at most 64 bits on every supported host, and the running total is
    // compared in `u64` so a 32-bit host cannot wrap the budget check.
    let len = bytes.len() as u64;
    let mut guard = store();
    if guard.objects.contains_key(&key) || !reserve(&mut guard, len) {
        return;
    }
    guard.objects.insert(key, Arc::new(bytes));
}

/// Claim `path` for the duration of one object emission, so [`EmitGuard::exclusive`] can tell
/// whether the bytes read back from it are certainly this emission's own.
pub(crate) fn begin_emit(path: &std::path::Path) -> EmitGuard {
    let mut guard = store();
    let entry = guard.emitting.entry(path.to_path_buf()).or_insert((0, false));
    entry.0 += 1;
    if entry.0 > 1 {
        entry.1 = true;
    }
    EmitGuard {
        path: path.to_path_buf(),
    }
}

/// Releases one in-flight object emission. See `Store::emitting`.
pub(crate) struct EmitGuard {
    path: std::path::PathBuf,
}

impl EmitGuard {
    /// Whether this path had exactly one emitter for the whole emission. `false` means another
    /// thread was writing the same file, so the read-back may not be this emission's bytes and
    /// nothing may be retained — by either emitter, since both observe the contended flag.
    pub(crate) fn exclusive(&self) -> bool {
        let guard = store();
        guard
            .emitting
            .get(&self.path)
            .is_none_or(|(_, contended)| !*contended)
    }
}

impl Drop for EmitGuard {
    fn drop(&mut self) {
        let mut guard = store();
        let Some(entry) = guard.emitting.get_mut(&self.path) else {
            return;
        };
        entry.0 = entry.0.saturating_sub(1);
        if entry.0 == 0 {
            guard.emitting.remove(&self.path);
        }
    }
}
