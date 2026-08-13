//! The persistent **per-unit frontend** cache (`docs/impl/10-cache-first-optimization.md` §6.7).
//!
//! [`crate::memo`] makes a second identical compilation cheap inside ONE process. [`crate::cache`]
//! makes a second *codegen* cheap across processes. Neither makes a second *frontend* cheap across
//! processes, and that is the last stage every new process recomputes: the codegen key's
//! `impl_hash` is produced BY the frontend, so the shipped object cache cannot be consulted until
//! sema and lowering have already run.
//!
//! This module stores, per unit, exactly what a dependent build needs in order not to check that
//! unit again:
//!
//! ```text
//! summary_bytes   align_interface::serialize(&InterfaceSummary) — carries interface_hash AND
//!                 impl_hash, so the codegen key can be built WITHOUT the MIR
//! diagnostics     the unit's own diagnostics, FileId removed, replayed on a hit
//! link_libs       the unit MIR's link_libs — NOT derivable from summary.capabilities, because a
//!                 user-declared `extern "C" link("m")` library is not a capability
//! ```
//!
//! The MIR is deliberately absent (persisting it would need a canonical codec for
//! `align_mir::Program`, and the object it produces is already content-addressed). A unit served
//! from here is `Reused`; if codegen later needs its MIR, the driver rehydrates that ONE unit and
//! verifies the result against this entry.
//!
//! ## Key
//!
//! Frontend-only, so one entry serves every profile, cpu, LLVM version, `--rt-lto` setting, and PGO
//! mode: `lower_to_mir_per_unit` reads none of them. Structural over the complete reachable
//! definition graph — the unit's exact source bytes plus, for every member of its transitive import
//! closure, the canonical digest of that member's whole summary and of its own closure names.
//!
//! **No `Debug` rendering appears in any key or any stored byte.** The in-process memo can key on a
//! total `Debug` because the rendering is a constant of the process image; on disk it is not a
//! stable format. Every component here is a scalar, a UTF-8 string, or a digest over an existing
//! versioned canonical codec.
//!
//! ## Fail-closed
//!
//! Every stored byte is untrusted input. The manifest decoder is the same hand-rolled versioned
//! length-prefixed discipline as [`crate::cache`]: an unknown version, truncation, a bad tag, bad
//! UTF-8, or trailing bytes all return [`crate::cache::CacheDecodeError`], never a panic. The key is
//! compared against the freshly computed one BEFORE any semantic check, and the value region
//! carries its own digest — the key covers none of the summary, diagnostics, or link libraries, so
//! without it a bit flip in `impl_hash` could address a genuine OLDER object for the same unit.

use std::path::{Path, PathBuf};

use align_interface::Hash128;

use crate::cache::{
    self, CacheDecodeError, CacheOutcome, CacheStage, FirstDiff, Reader, Writer,
};

/// The unit-frontend **key-format** version — component #1 of every unit key. A bump changes every
/// full and slot digest, so no entry written by an older key layout can be reused. Deliberately
/// independent of [`crate::cache::CACHE_KEY_FORMAT_VERSION`]: bumping one must not drop the other
/// stage's entries.
pub const UNIT_KEY_FORMAT_VERSION: u32 = 1;

/// The unit manifest wire-format version. Bump on ANY change to the encoded byte layout; an old
/// manifest then fails closed on decode (a clean miss, its bytes unreferenced).
const UNIT_MANIFEST_FORMAT_VERSION: u32 = 1;

/// The manifest phase discriminator, disjoint from `cache.rs`'s `PHASE_TAG_PRELINK` (1) and
/// `PHASE_TAG_BACKEND` (2), so a unit manifest can never be mis-decoded as a ThinLTO one even under
/// a digest collision across the (independent) action namespaces.
const UNIT_PHASE_TAG: u8 = 3;

/// The process-global compiler toggles folded into every unit key, in this exact order.
///
/// They change what the frontend lowers (`ALIGN_CONST_POOL` in sema; the other three in MIR
/// lowering) and the measurement suites flip them with `set_var` inside one process, so they are
/// genuine key material. [`crate::cache::CacheContext::from_env`] additionally forces the whole
/// cache off while any is set, which means two addressable entries can never actually disagree
/// here — they are keyed anyway so that relaxing that guard later cannot silently unkey them.
pub const KEYED_ENV_TOGGLES: [&str; 4] = [
    "ALIGN_CONST_POOL",
    "ALIGN_NEEDLE_HOIST",
    "ALIGN_BUFFER_DONATE",
    "ALIGN_SORT_ADAPTIVE",
];

// ---- key ------------------------------------------------------------------------------------

/// One keyed environment toggle's state. `Absent` and `Present("")` are distinct: an exported empty
/// value is not the same input as an unset variable.
#[derive(Clone, PartialEq, Eq, Debug)]
#[non_exhaustive]
pub enum EnvToggle {
    Absent,
    Present(String),
}

impl EnvToggle {
    fn read(name: &str) -> EnvToggle {
        match std::env::var(name) {
            Ok(value) => EnvToggle::Present(value),
            Err(_) => EnvToggle::Absent,
        }
    }
}

/// One transitive-closure member's contribution to a unit's key.
#[derive(Clone, PartialEq, Eq, Debug)]
#[non_exhaustive]
pub enum DepState {
    /// In the closure but WITHOUT a summary at this point in the walk — the unit was checked against
    /// an absent dependency, which is a different sema input from a present one. Recorded
    /// explicitly rather than omitted so the encoding stays injective.
    Missing,
    Present {
        /// The dependency's `interface_hash` — `Hash128` over its canonical interface SURFACE
        /// (`align_interface::encode_interface_surface`: signatures, type defs, exported resources,
        /// consts, effect bits, and generic template bodies).
        ///
        /// Deliberately NOT a digest of the whole serialized summary: that encoding also carries
        /// `impl_hash`, so keying on it would make every private body edit in a dependency
        /// invalidate all of its consumers — exactly the interface/implementation split M15 exists
        /// to provide. This is the same component the shipped `CodegenKey.dep_interface_hashes`
        /// uses, so both cache stages agree on what a consumer depends on.
        ///
        /// It covers everything the consumer actually reads: `summary_to_source` renders the
        /// surface, and the four external fact maps (effects, return/parallel provenance, resource
        /// facts, resource hooks) are all derived from it. Capabilities are excluded from the
        /// surface by design and never reach a consumer's check — they are its own MIR's concern.
        interface_hash: Hash128,
        /// A digest over the dependency's OWN transitive closure module names, in its own walk
        /// order. Together with `interface_hash` this covers the rendered interface source exactly:
        /// `align_interface::summary_to_source(summary, dep_units)` is a pure function of those two
        /// inputs, so keying them is equivalent to keying the rendering — without having to render
        /// anything on a hit.
        closure_digest: Hash128,
    },
}

/// One closure member as it appears in a key.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct UnitDep {
    pub module: String,
    pub state: DepState,
}

/// The decomposed per-unit frontend key. The FULL set is hashed into the action-manifest path and
/// stored verbatim in the manifest; a stable-core subset is hashed into the slot-pointer path.
///
/// Every backend knob is deliberately absent — profile, pipeline, codegen opt level, resolved
/// cpu/features, reloc/code model, LLVM version, `rt_lto`, PGO mode — because
/// `lower_to_mir_per_unit` consumes none of them. One entry therefore serves a dev build and a
/// release build alike. So are the cwd, the project root, every absolute path, file mtimes, and the
/// cache root itself.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct UnitKey {
    /// K1 key-format version ([`UNIT_KEY_FORMAT_VERSION`]).
    pub key_format_version: u32,
    /// K2 the compiler fingerprint. An enabled [`crate::cache::CacheContext`] guarantees this is a
    /// REAL identity (`cache::compiler_build_id_available`), never a guessed one.
    pub compiler_fingerprint: Hash128,
    /// K3 the interface-codec schema (`align_interface::FORMAT_VERSION`).
    pub frontend_schema: u32,
    /// K4 the four keyed toggles, in [`KEYED_ENV_TOGGLES`] order.
    pub env_toggles: Vec<EnvToggle>,
    /// K5 target triple. MIR lowering is target-independent today; this is kept for the same reason
    /// `PrelinkKey` keeps it — a future target-dependent layout decision must not be able to serve
    /// an x86-64 frontend result to an aarch64 build.
    pub target_triple: String,
    /// K5 (cont.) object format (`0` = ELF, `1` = Mach-O).
    pub object_format: u8,
    /// K6 the unit's module path and entry flag.
    pub unit: String,
    pub is_entry: bool,
    /// K7 a digest of the unit's exact source bytes.
    pub source_digest: Hash128,
    /// K8 the unit's TRANSITIVE import closure, in the walk's DFS order, every member present.
    pub deps: Vec<UnitDep>,
}

impl UnitKey {
    /// The full-key digest → the `actions/unit/<digest>` path.
    pub fn full_digest(&self) -> Hash128 {
        let mut w = Writer::new();
        write_key(&mut w, self);
        Hash128::of(&w.buf)
    }

    /// The slot digest → the `index/unit/<digest>` pointer path. Stable core only (phase tag +
    /// key-format version + compiler fingerprint + unit path), so the prior manifest stays findable
    /// after an ordinary edit and can name the first differing component. Observability only: a hit
    /// still requires the full-key manifest.
    pub fn slot_digest(&self) -> Hash128 {
        let mut w = Writer::new();
        w.u8(UNIT_PHASE_TAG);
        w.u32(self.key_format_version);
        w.h128(self.compiler_fingerprint);
        w.str(&self.unit);
        Hash128::of(&w.buf)
    }

    /// The keyed toggles as they stand right now.
    pub fn current_env_toggles() -> Vec<EnvToggle> {
        KEYED_ENV_TOGGLES.iter().map(|name| EnvToggle::read(name)).collect()
    }
}

/// The first key component that differs between a decoded prior manifest and the fresh key, in a
/// fixed priority order. The stable-core components are guaranteed equal when the slot pointer was
/// found by [`UnitKey::slot_digest`], but they are still checked as a defensive fallthrough.
fn first_diff(stored: &UnitKey, current: &UnitKey) -> FirstDiff {
    if stored.frontend_schema != current.frontend_schema {
        return FirstDiff::FrontendSchema;
    }
    if stored.target_triple != current.target_triple || stored.object_format != current.object_format {
        return FirstDiff::Target;
    }
    if stored.env_toggles != current.env_toggles {
        return FirstDiff::EnvToggle;
    }
    if stored.source_digest != current.source_digest || stored.is_entry != current.is_entry {
        return FirstDiff::UnitSource;
    }
    if stored.deps != current.deps {
        return FirstDiff::DepInterfaceHashes;
    }
    if stored.key_format_version != current.key_format_version {
        return FirstDiff::CacheFormatVersion;
    }
    if stored.compiler_fingerprint != current.compiler_fingerprint {
        return FirstDiff::CompilerBuildId;
    }
    // Unreachable on a genuine full-key miss (some component must differ); a defensive fallback.
    FirstDiff::NoPriorEntry
}

// ---- value ----------------------------------------------------------------------------------

/// One replayable diagnostic: everything but the `FileId`, which the replaying walk supplies from
/// its own `loaded[i].fid`.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct CachedDiagnostic {
    pub severity: align_diag::Severity,
    pub message: String,
    pub lo: u32,
    pub hi: u32,
}

/// One unit's persisted frontend result.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct UnitEntry {
    /// `align_interface::serialize(&summary)`. Opaque here; decoded with `deserialize`, which is
    /// itself version-checked and fail-closed.
    pub summary_bytes: Vec<u8>,
    /// The unit's own diagnostics, in emission order, with the file component of each span removed.
    /// A clean unit routinely warns — `pkg.db` emits sixteen `lossy conversion` warnings — so
    /// warnings are REPLAYED rather than used to disqualify the unit, which would exclude exactly
    /// the expensive modules.
    pub diagnostics: Vec<CachedDiagnostic>,
    /// The unit MIR's `link_libs`, in MIR order.
    ///
    /// Not derivable from `summary.capabilities`: `align_mir::lower_program` builds this as the
    /// user-declared `extern "C" link("name")` libraries (validated in sema) FOLLOWED BY the
    /// capability-derived ones. Only the second half corresponds to `capabilities`, so a unit that
    /// declares an FFI library and uses no gated builtin has a non-empty `link_libs` and an empty
    /// `capabilities`. Re-deriving would silently drop that unit's `-l` flag.
    pub link_libs: Vec<String>,
}

/// Every diagnostic in replayable form, or `None` when this unit must not be retained.
///
/// A span in another file — a synthesized `<interface:…>` pseudo-file, or the `Span::new(0, 0, 0)`
/// placeholder the walk's internal-error paths use — cannot be reattached correctly in another
/// walk, where that id denotes a different file. `unit_file` must always be the caller-space id
/// `load_units` assigned (`loaded[i].fid`), never an id derived locally.
pub fn replayable_diagnostics(
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

// ---- codec ----------------------------------------------------------------------------------

fn write_key(w: &mut Writer, k: &UnitKey) {
    w.u32(k.key_format_version);
    w.h128(k.compiler_fingerprint);
    w.u32(k.frontend_schema);
    w.u32(cache::u32_len(k.env_toggles.len()));
    for (name, toggle) in KEYED_ENV_TOGGLES.iter().zip(k.env_toggles.iter()) {
        w.str(name);
        match toggle {
            EnvToggle::Absent => w.u8(0),
            EnvToggle::Present(value) => {
                w.u8(1);
                w.str(value);
            }
        }
    }
    w.str(&k.target_triple);
    w.u8(k.object_format);
    w.str(&k.unit);
    w.bool(k.is_entry);
    w.h128(k.source_digest);
    w.u32(cache::u32_len(k.deps.len()));
    for dep in &k.deps {
        w.str(&dep.module);
        match &dep.state {
            DepState::Missing => w.u8(0),
            DepState::Present { interface_hash, closure_digest } => {
                w.u8(1);
                w.h128(*interface_hash);
                w.h128(*closure_digest);
            }
        }
    }
}

fn read_key(r: &mut Reader<'_>) -> Result<UnitKey, CacheDecodeError> {
    let key_format_version = r.u32()?;
    let compiler_fingerprint = r.h128()?;
    let frontend_schema = r.u32()?;
    let toggle_count = r.u32()? as usize;
    if toggle_count != KEYED_ENV_TOGGLES.len() {
        // A fixed-arity sequence: a different count is a foreign or damaged manifest, never a
        // shorter valid one.
        return Err(CacheDecodeError::BadTag { what: "toggle count", tag: 0 });
    }
    let mut env_toggles = Vec::with_capacity(KEYED_ENV_TOGGLES.len());
    for expected in KEYED_ENV_TOGGLES {
        let name = r.str()?;
        if name != expected {
            return Err(CacheDecodeError::BadTag { what: "toggle name", tag: 0 });
        }
        env_toggles.push(match r.u8()? {
            0 => EnvToggle::Absent,
            1 => EnvToggle::Present(r.str()?),
            tag => return Err(CacheDecodeError::BadTag { what: "toggle", tag }),
        });
    }
    let target_triple = r.str()?;
    let object_format = match r.u8()? {
        format @ (0 | 1) => format,
        tag => return Err(CacheDecodeError::BadTag { what: "object format", tag }),
    };
    let unit = r.str()?;
    let is_entry = r.bool()?;
    let source_digest = r.h128()?;
    let deps = r.seq(|r| {
        let module = r.str()?;
        let state = match r.u8()? {
            0 => DepState::Missing,
            1 => DepState::Present { interface_hash: r.h128()?, closure_digest: r.h128()? },
            tag => return Err(CacheDecodeError::BadTag { what: "dep state", tag }),
        };
        Ok(UnitDep { module, state })
    })?;
    Ok(UnitKey {
        key_format_version,
        compiler_fingerprint,
        frontend_schema,
        env_toggles,
        target_triple,
        object_format,
        unit,
        is_entry,
        source_digest,
        deps,
    })
}

fn severity_tag(severity: align_diag::Severity) -> u8 {
    match severity {
        align_diag::Severity::Error => 0,
        align_diag::Severity::Warning => 1,
    }
}

fn write_value(w: &mut Writer, entry: &UnitEntry) {
    w.bytes(&entry.summary_bytes);
    w.u32(cache::u32_len(entry.diagnostics.len()));
    for diagnostic in &entry.diagnostics {
        w.u8(severity_tag(diagnostic.severity));
        w.str(&diagnostic.message);
        w.u32(diagnostic.lo);
        w.u32(diagnostic.hi);
    }
    w.u32(cache::u32_len(entry.link_libs.len()));
    for lib in &entry.link_libs {
        w.str(lib);
    }
}

fn read_value(r: &mut Reader<'_>) -> Result<UnitEntry, CacheDecodeError> {
    let summary_bytes = r.bytes()?;
    let diagnostics = r.seq(|r| {
        let severity = match r.u8()? {
            0 => align_diag::Severity::Error,
            1 => align_diag::Severity::Warning,
            tag => return Err(CacheDecodeError::BadTag { what: "severity", tag }),
        };
        Ok(CachedDiagnostic { severity, message: r.str()?, lo: r.u32()?, hi: r.u32()? })
    })?;
    let link_libs = r.seq(|r| r.str())?;
    Ok(UnitEntry { summary_bytes, diagnostics, link_libs })
}

/// The complete manifest bytes: wire version, phase tag, key region, value region, and a digest over
/// the exact value-region bytes.
pub fn serialize_manifest(key: &UnitKey, entry: &UnitEntry) -> Vec<u8> {
    let mut w = Writer::new();
    w.u32(UNIT_MANIFEST_FORMAT_VERSION);
    w.u8(UNIT_PHASE_TAG);
    write_key(&mut w, key);
    let value_start = w.buf.len();
    write_value(&mut w, entry);
    let value_digest = Hash128::of(&w.buf[value_start..]);
    w.h128(value_digest);
    w.buf
}

/// Decode a manifest and check it against `expected`, in this exact order:
///
/// ```text
/// 1  wire version                       UnknownVersion
/// 2  phase tag                          BadTag
/// 3  key region, field by field         structural errors
/// 4  decoded key == expected            Ok(None) — a clean MISS, not damage
/// 5  value region, field by field       structural errors
/// 6  value_digest over [value_start..)  SemanticRange
/// 7  no trailing bytes                  TrailingBytes
/// ```
///
/// Step 4 precedes every semantic check for the same reason `cache::try_hit` compares the stored key
/// immediately: a manifest that does not belong to this key is not evidence of damage, and must not
/// unlink anything. Everything that fails AFTER step 4 is damage to an entry that provably is ours.
pub fn deserialize_manifest(
    bytes: &[u8],
    expected: &UnitKey,
) -> Result<Option<(UnitKey, UnitEntry)>, CacheDecodeError> {
    decode(bytes, Some(expected))
}

/// Decode a manifest WITHOUT a key to compare against — every other check still applies. For
/// inspection (an owner test, a future `alignc cache` subcommand); the build path always goes
/// through [`deserialize_manifest`], which additionally proves the entry is the one it asked for.
pub fn decode_manifest(bytes: &[u8]) -> Option<(UnitKey, UnitEntry)> {
    decode(bytes, None).ok().flatten()
}

fn decode(
    bytes: &[u8],
    expected: Option<&UnitKey>,
) -> Result<Option<(UnitKey, UnitEntry)>, CacheDecodeError> {
    let mut r = Reader::new(bytes);
    let version = r.u32()?;
    if version != UNIT_MANIFEST_FORMAT_VERSION {
        return Err(CacheDecodeError::UnknownVersion(version));
    }
    let tag = r.u8()?;
    if tag != UNIT_PHASE_TAG {
        return Err(CacheDecodeError::BadTag { what: "unit phase", tag });
    }
    let key = read_key(&mut r)?;
    if let Some(expected) = expected
        && &key != expected
    {
        return Ok(None);
    }
    let value_start = r.pos;
    let entry = read_value(&mut r)?;
    let value_end = r.pos;
    let stored_digest = r.h128()?;
    let computed = Hash128::of(bytes.get(value_start..value_end).ok_or(CacheDecodeError::Truncated)?);
    if computed != stored_digest {
        return Err(CacheDecodeError::SemanticRange);
    }
    r.finish()?;
    Ok(Some((key, entry)))
}

/// Decode only far enough to recover a prior key for the miss-reason diff. Used on the slot pointer,
/// where the stored key is by construction NOT equal to the current one.
fn deserialize_key_only(bytes: &[u8]) -> Result<UnitKey, CacheDecodeError> {
    let mut r = Reader::new(bytes);
    let version = r.u32()?;
    if version != UNIT_MANIFEST_FORMAT_VERSION {
        return Err(CacheDecodeError::UnknownVersion(version));
    }
    let tag = r.u8()?;
    if tag != UNIT_PHASE_TAG {
        return Err(CacheDecodeError::BadTag { what: "unit phase", tag });
    }
    read_key(&mut r)
}

// ---- store ----------------------------------------------------------------------------------

fn action_path(root: &Path, full: Hash128) -> PathBuf {
    root.join("actions").join("unit").join(full.to_hex())
}

fn slot_path(root: &Path, slot: Hash128) -> PathBuf {
    root.join("index").join("unit").join(slot.to_hex())
}

/// A verified hit: the stored entry plus its already-decoded summary. `lookup` has to decode the
/// summary anyway to prove the entry is intact, so it hands the value over rather than making the
/// caller decode the same bytes twice.
pub struct UnitHit {
    pub entry: UnitEntry,
    pub summary: align_interface::InterfaceSummary,
}

/// The result of one unit-frontend lookup.
#[non_exhaustive]
pub enum UnitLookup {
    /// Served from disk: replay this entry instead of checking the unit.
    Hit(Box<UnitHit>),
    /// Not served. `reason` is `None` only when the cache was not consulted at all.
    Miss { reason: Option<FirstDiff> },
}

/// Look one unit up. Fail-closed at every step; a damaged entry that provably belongs to this key is
/// unlinked and reported as [`FirstDiff::CorruptEntry`], while a foreign or version-skewed one is a
/// clean miss that leaves the bytes alone.
///
/// `source_len` bounds the replayed diagnostic spans: the key already pins the source by digest, so
/// an out-of-range offset here is damage, not skew (a corrupted manifest is not bound by the key).
/// Rejecting it is what keeps a later `format_diagnostics` from slicing past end-of-file.
pub fn lookup(root: &Path, key: &UnitKey, source_len: usize) -> UnitLookup {
    let path = action_path(root, key.full_digest());
    let bytes = match std::fs::read(&path) {
        Ok(bytes) => bytes,
        Err(_) => return UnitLookup::Miss { reason: Some(diff_against_slot(root, key)) },
    };
    match deserialize_manifest(&bytes, key) {
        // A version-skewed or structurally foreign manifest: unreferenced, rebuild fresh.
        Err(CacheDecodeError::UnknownVersion(_)) | Ok(None) => {
            UnitLookup::Miss { reason: Some(diff_against_slot(root, key)) }
        }
        Err(_) => corrupt(&path),
        Ok(Some((_, entry))) => {
            // Steps 8 then 9, in that order: decode the summary before range-checking the spans, so
            // a manifest that fails both reports the earlier cause. Both are past the key
            // comparison, so both mean damage to an entry that provably is ours.
            let Ok(summary) = align_interface::deserialize(&entry.summary_bytes) else {
                return corrupt(&path);
            };
            if entry
                .diagnostics
                .iter()
                .any(|d| d.lo > d.hi || d.hi as usize > source_len)
            {
                return corrupt(&path);
            }
            UnitLookup::Hit(Box::new(UnitHit { entry, summary }))
        }
    }
}

/// Unlink a damaged entry, note it, and report a rebuild.
///
/// Best-effort in both directions: an unlink failure only means the next build repeats the same
/// self-heal, and a concurrent producer that republishes between this `remove_file` and its own
/// rename simply wins — it writes a correct manifest, which is the outcome we wanted anyway. The
/// slot pointer is deliberately left alone: it is observability only (it names the first differing
/// component of a later miss) and removing it would cost that diagnosis with no safety gain, since
/// a hit always requires the full-key manifest this just removed.
fn corrupt(path: &Path) -> UnitLookup {
    let _ = std::fs::remove_file(path);
    eprintln!("{}", cache::CORRUPT_NOTE);
    UnitLookup::Miss { reason: Some(FirstDiff::CorruptEntry) }
}

/// Unlink the entry for `key` — the rehydration-verification failure path, where the entry decoded
/// and matched but recomputation disagreed with it.
pub fn invalidate(root: &Path, key: &UnitKey) {
    let _ = std::fs::remove_file(action_path(root, key.full_digest()));
    let _ = std::fs::remove_file(slot_path(root, key.slot_digest()));
}

fn diff_against_slot(root: &Path, key: &UnitKey) -> FirstDiff {
    match std::fs::read(slot_path(root, key.slot_digest())) {
        Ok(bytes) => match deserialize_key_only(&bytes) {
            Ok(stored) => first_diff(&stored, key),
            Err(_) => FirstDiff::NoPriorEntry,
        },
        Err(_) => FirstDiff::NoPriorEntry,
    }
}

/// Publish one unit's frontend result: the full-key action manifest plus the unit-slot pointer.
///
/// Best-effort, exactly like the codegen cache: populating the cache must never fail a build whose
/// frontend result is already correct in memory. Two processes producing the same key write
/// byte-identical manifests (the summary codec is canonical, diagnostics are in emission order,
/// link libraries are in MIR order, and the digest is a function of those bytes), so the
/// staged-write + same-directory rename's last-writer-wins is harmless.
pub fn publish(root: &Path, key: &UnitKey, entry: &UnitEntry) {
    let manifest = serialize_manifest(key, entry);
    let result = cache::publish_file(&action_path(root, key.full_digest()), &manifest)
        .and_then(|()| cache::publish_file(&slot_path(root, key.slot_digest()), &manifest));
    if let Err(error) = result {
        eprintln!("alignc: cache not populated: {error}");
    }
}

/// The structured outcome for one unit's frontend stage, for `--cache-stats` and the owner tests.
pub fn outcome(unit: &str, hit: bool, reason: Option<FirstDiff>) -> CacheOutcome {
    CacheOutcome {
        stage: CacheStage::UnitFrontend,
        unit: unit.to_string(),
        hit,
        miss_reason: reason,
    }
}

/// A digest over an ordered list of module names — the `closure_digest` of [`DepState::Present`].
/// Length-prefixed per element so no two different name lists can encode to the same bytes.
pub fn closure_digest(modules: &[String]) -> Hash128 {
    let mut w = Writer::new();
    w.u32(cache::u32_len(modules.len()));
    for module in modules {
        w.str(module);
    }
    Hash128::of(&w.buf)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn hh(n: u64) -> Hash128 {
        Hash128 { lo: n, hi: n + 1 }
    }

    fn sample_key() -> UnitKey {
        UnitKey {
            key_format_version: UNIT_KEY_FORMAT_VERSION,
            compiler_fingerprint: hh(1),
            frontend_schema: align_interface::FORMAT_VERSION,
            env_toggles: vec![
                EnvToggle::Absent,
                EnvToggle::Present("off".to_string()),
                EnvToggle::Absent,
                EnvToggle::Absent,
            ],
            target_triple: "x86_64-unknown-linux-gnu".to_string(),
            object_format: 0,
            unit: "pkg.db".to_string(),
            is_entry: false,
            source_digest: hh(10),
            deps: vec![
                UnitDep { module: "pkg.db.internal".to_string(), state: DepState::Missing },
                UnitDep {
                    module: "pkg.db.sqlite".to_string(),
                    state: DepState::Present { interface_hash: hh(20), closure_digest: hh(30) },
                },
            ],
        }
    }

    fn sample_entry() -> UnitEntry {
        UnitEntry {
            summary_bytes: vec![1, 2, 3, 4],
            diagnostics: vec![CachedDiagnostic {
                severity: align_diag::Severity::Warning,
                message: "lossy conversion".to_string(),
                lo: 7,
                hi: 11,
            }],
            link_libs: vec!["sqlite3".to_string()],
        }
    }


    /// P2-2, semantic -> byte: the exact bytes, transcribed BY HAND from the format table in
    /// `docs/impl/10-cache-first-optimization.md` §6.7, not captured from the encoder.
    ///
    /// Everything but the trailing digest is written out literally, so a changed width, tag value,
    /// field order, or length-prefix convention fails here even if encoder and decoder change
    /// together. The 16-byte trailer is by definition `Hash128` OF the preceding value region, so it
    /// is checked as that rule rather than as a magic constant.
    const GOLDEN_PREFIX_HEX: &str = concat!(
        "01000000",                         // manifest_format_version = 1 (u32 LE)
        "03",                               // phase_tag = 3
        // -- key region --
        "01000000",                         // key_format_version = 1
        "01000000000000000200000000000000", // compiler_fingerprint = Hash128 { lo: 1, hi: 2 }
        "06000000",                         // frontend_schema = align_interface::FORMAT_VERSION = 6
        "04000000",                         // env_toggles: exactly 4
        "10000000414c49474e5f434f4e53545f504f4f4c", "00",             // ALIGN_CONST_POOL   Absent
        "12000000414c49474e5f4e4545444c455f484f495354", "01", "030000006f6666", // ..NEEDLE_HOIST Present("off")
        "13000000414c49474e5f4255464645525f444f4e415445", "00",       // ALIGN_BUFFER_DONATE Absent
        "13000000414c49474e5f534f52545f414441505449564500",           // ALIGN_SORT_ADAPTIVE Absent
        "180000007838365f36342d756e6b6e6f776e2d6c696e75782d676e75",   // target_triple
        "00",                               // object_format = 0 (ELF)
        "06000000706b672e6462",             // unit = "pkg.db"
        "00",                               // is_entry = false
        "0a000000000000000b00000000000000", // source_digest = Hash128 { lo: 10, hi: 11 }
        "02000000",                         // deps: 2
        "0f000000706b672e64622e696e7465726e616c", "00",               // pkg.db.internal  Missing
        "0d000000706b672e64622e73716c697465", "01",                   // pkg.db.sqlite    Present
        "14000000000000001500000000000000",                           //   interface_hash 20/21
        "1e000000000000001f00000000000000",                           //   closure_digest 30/31
        // -- value region --
        "0400000001020304",                 // summary_bytes = [1,2,3,4]
        "01000000",                         // diagnostics: 1
        "01",                               // severity = 1 (Warning)
        "100000006c6f73737920636f6e76657273696f6e",                   // "lossy conversion"
        "07000000", "0b000000",             // lo = 7, hi = 11
        "01000000", "0700000073716c69746533",                         // link_libs = ["sqlite3"]
    );

    fn golden_key() -> UnitKey {
        UnitKey {
            key_format_version: 1,
            compiler_fingerprint: Hash128 { lo: 1, hi: 2 },
            frontend_schema: align_interface::FORMAT_VERSION,
            env_toggles: vec![
                EnvToggle::Absent,
                EnvToggle::Present("off".to_string()),
                EnvToggle::Absent,
                EnvToggle::Absent,
            ],
            target_triple: "x86_64-unknown-linux-gnu".to_string(),
            object_format: 0,
            unit: "pkg.db".to_string(),
            is_entry: false,
            source_digest: Hash128 { lo: 10, hi: 11 },
            deps: vec![
                UnitDep { module: "pkg.db.internal".to_string(), state: DepState::Missing },
                UnitDep {
                    module: "pkg.db.sqlite".to_string(),
                    state: DepState::Present {
                        interface_hash: Hash128 { lo: 20, hi: 21 },
                        closure_digest: Hash128 { lo: 30, hi: 31 },
                    },
                },
            ],
        }
    }

    fn golden_entry() -> UnitEntry {
        UnitEntry {
            summary_bytes: vec![1, 2, 3, 4],
            diagnostics: vec![CachedDiagnostic {
                severity: align_diag::Severity::Warning,
                message: "lossy conversion".to_string(),
                lo: 7,
                hi: 11,
            }],
            link_libs: vec!["sqlite3".to_string()],
        }
    }

    fn from_hex(hex: &str) -> Vec<u8> {
        assert!(hex.len() % 2 == 0);
        (0..hex.len() / 2)
            .map(|i| u8::from_str_radix(&hex[i * 2..i * 2 + 2], 16).expect("hex"))
            .collect()
    }

    /// The exact byte range the value digest covers, per the format table: everything after the key
    /// region and before the 16-byte trailer.
    fn golden_value_start() -> usize {
        let mut w = Writer::new();
        w.u32(UNIT_MANIFEST_FORMAT_VERSION);
        w.u8(UNIT_PHASE_TAG);
        write_key(&mut w, &golden_key());
        w.buf.len()
    }

    #[test]
    fn golden_vector_semantic_to_byte() {
        assert_eq!(
            align_interface::FORMAT_VERSION,
            6,
            "the golden vector transcribes frontend_schema = 6; re-transcribe it if the interface \
             codec version moves"
        );
        let prefix = from_hex(GOLDEN_PREFIX_HEX);
        let encoded = serialize_manifest(&golden_key(), &golden_entry());
        assert_eq!(
            encoded.len(),
            prefix.len() + 16,
            "the manifest is the transcribed prefix plus one 16-byte value digest"
        );
        assert_eq!(&encoded[..prefix.len()], &prefix[..], "encoded bytes must match the format table");
        // The trailer is the digest rule, checked independently of the encoder's own call.
        let value = &prefix[golden_value_start()..];
        let expected = Hash128::of(value);
        let mut trailer = Writer::new();
        trailer.h128(expected);
        assert_eq!(&encoded[prefix.len()..], &trailer.buf[..], "the trailer is Hash128 OF the value region");
    }

    #[test]
    fn golden_vector_byte_to_semantic() {
        let mut bytes = from_hex(GOLDEN_PREFIX_HEX);
        let value_digest = Hash128::of(&bytes[golden_value_start()..]);
        let mut trailer = Writer::new();
        trailer.h128(value_digest);
        bytes.extend_from_slice(&trailer.buf);

        let (key, entry) = decode_manifest(&bytes).expect("the transcribed bytes must decode");
        // Field by field, not one struct comparison: a wrong field that happens to round-trip
        // through the same struct would otherwise hide.
        assert_eq!(key.key_format_version, 1);
        assert_eq!(key.compiler_fingerprint, Hash128 { lo: 1, hi: 2 });
        assert_eq!(key.frontend_schema, align_interface::FORMAT_VERSION);
        assert_eq!(key.env_toggles.len(), 4);
        assert_eq!(key.env_toggles[0], EnvToggle::Absent);
        assert_eq!(key.env_toggles[1], EnvToggle::Present("off".to_string()));
        assert_eq!(key.env_toggles[2], EnvToggle::Absent);
        assert_eq!(key.env_toggles[3], EnvToggle::Absent);
        assert_eq!(key.target_triple, "x86_64-unknown-linux-gnu");
        assert_eq!(key.object_format, 0);
        assert_eq!(key.unit, "pkg.db");
        assert!(!key.is_entry);
        assert_eq!(key.source_digest, Hash128 { lo: 10, hi: 11 });
        assert_eq!(key.deps.len(), 2);
        assert_eq!(key.deps[0].module, "pkg.db.internal");
        assert_eq!(key.deps[0].state, DepState::Missing);
        assert_eq!(key.deps[1].module, "pkg.db.sqlite");
        assert_eq!(
            key.deps[1].state,
            DepState::Present {
                interface_hash: Hash128 { lo: 20, hi: 21 },
                closure_digest: Hash128 { lo: 30, hi: 31 },
            }
        );
        assert_eq!(entry.summary_bytes, vec![1, 2, 3, 4]);
        assert_eq!(entry.diagnostics.len(), 1);
        assert_eq!(entry.diagnostics[0].severity, align_diag::Severity::Warning);
        assert_eq!(entry.diagnostics[0].message, "lossy conversion");
        assert_eq!(entry.diagnostics[0].lo, 7);
        assert_eq!(entry.diagnostics[0].hi, 11);
        assert_eq!(entry.link_libs, vec!["sqlite3".to_string()]);
        // And it is the same value the round-trip test builds.
        assert_eq!((key, entry), (golden_key(), golden_entry()));
    }

    #[test]
    fn manifest_round_trips() {
        let key = sample_key();
        let entry = sample_entry();
        let bytes = serialize_manifest(&key, &entry);
        let (decoded_key, decoded_entry) =
            deserialize_manifest(&bytes, &key).expect("decode").expect("matches");
        assert_eq!(decoded_key, key);
        assert_eq!(decoded_entry, entry);
    }

    #[test]
    fn a_foreign_key_is_a_clean_miss_not_damage() {
        // Step 4: a manifest for another key decodes fine and reports "not mine" — it must NEVER be
        // classified as corruption, because that would unlink a healthy entry.
        let bytes = serialize_manifest(&sample_key(), &sample_entry());
        let mut other = sample_key();
        other.source_digest = hh(999);
        assert_eq!(deserialize_manifest(&bytes, &other), Ok(None));
    }

    #[test]
    fn a_value_region_flip_is_caught_by_the_value_digest() {
        // The key covers NONE of summary_bytes / diagnostics / link_libs. Without the value digest a
        // bit flip in the encoded `impl_hash` would address a genuine OLDER object for this unit.
        let key = sample_key();
        let bytes = serialize_manifest(&key, &sample_entry());
        // Locate the value region: everything between the key and the trailing 16-byte digest.
        let mut probe = Writer::new();
        probe.u32(UNIT_MANIFEST_FORMAT_VERSION);
        probe.u8(UNIT_PHASE_TAG);
        write_key(&mut probe, &key);
        let value_start = probe.buf.len();
        let value_end = bytes.len() - 16;
        assert!(value_start < value_end);
        let mut flipped_any = false;
        for i in value_start..value_end {
            let mut mutated = bytes.clone();
            mutated[i] ^= 0x01;
            match deserialize_manifest(&mutated, &key) {
                // Either the flip breaks the encoding structurally, or it survives decoding and the
                // value digest catches it. What must never happen is a silent `Ok(Some(..))`.
                Err(_) => flipped_any = true,
                Ok(None) => panic!("a value-region flip must not read as a foreign key"),
                Ok(Some(_)) => panic!("a value-region flip at byte {i} was not detected"),
            }
        }
        assert!(flipped_any);
    }

    #[test]
    fn decode_is_fail_closed() {
        let key = sample_key();
        let bytes = serialize_manifest(&key, &sample_entry());
        // Trailing byte.
        let mut trailing = bytes.clone();
        trailing.push(0xff);
        assert_eq!(deserialize_manifest(&trailing, &key), Err(CacheDecodeError::TrailingBytes));
        // Truncation at EVERY boundary — never a panic, never a value.
        for cut in 0..bytes.len() {
            assert!(deserialize_manifest(&bytes[..cut], &key).is_err(), "truncation at {cut}");
        }
        // Wrong wire version.
        let mut w = Writer::new();
        w.u32(UNIT_MANIFEST_FORMAT_VERSION + 1);
        assert_eq!(
            deserialize_manifest(&w.buf, &key),
            Err(CacheDecodeError::UnknownVersion(UNIT_MANIFEST_FORMAT_VERSION + 1))
        );
        // Wrong phase tag: a ThinLTO prelink manifest starts with tag 1, a backend one with 2.
        let mut w = Writer::new();
        w.u32(UNIT_MANIFEST_FORMAT_VERSION);
        w.u8(1);
        assert_eq!(
            deserialize_manifest(&w.buf, &key),
            Err(CacheDecodeError::BadTag { what: "unit phase", tag: 1 })
        );
        // Garbage never panics.
        for chunk in [&b""[..], &b"\x01"[..], &[0xde, 0xad, 0xbe, 0xef][..]] {
            let _ = deserialize_manifest(chunk, &key);
        }
    }

    #[test]
    fn the_toggle_sequence_is_fixed_arity_and_fixed_order() {
        let key = sample_key();
        let bytes = serialize_manifest(&key, &sample_entry());
        // A different count or a renamed toggle is a foreign/damaged manifest, never a shorter valid
        // one: the four toggles are a fixed-shape record, not an open list.
        let mut short = key.clone();
        short.env_toggles.pop();
        let mut w = Writer::new();
        w.u32(UNIT_MANIFEST_FORMAT_VERSION);
        w.u8(UNIT_PHASE_TAG);
        write_key(&mut w, &short);
        assert!(matches!(
            deserialize_manifest(&w.buf, &key),
            Err(CacheDecodeError::BadTag { what: "toggle count", .. })
        ));
        // Sanity: the unmutated manifest still matches.
        assert!(deserialize_manifest(&bytes, &key).expect("decode").is_some());
    }

    #[test]
    fn absent_and_empty_toggles_are_distinct_keys() {
        let mut absent = sample_key();
        absent.env_toggles[0] = EnvToggle::Absent;
        let mut empty = sample_key();
        empty.env_toggles[0] = EnvToggle::Present(String::new());
        assert_ne!(absent.full_digest(), empty.full_digest());
    }

    #[test]
    fn slot_digest_ignores_diffable_components_and_tracks_the_core() {
        let a = sample_key();
        let mut b = a.clone();
        b.source_digest = hh(777);
        b.deps.clear();
        b.target_triple = "aarch64-apple-darwin".to_string();
        assert_eq!(a.slot_digest(), b.slot_digest());
        assert_ne!(a.full_digest(), b.full_digest());
        let mut c = a.clone();
        c.unit = "other".to_string();
        assert_ne!(a.slot_digest(), c.slot_digest());
    }

    #[test]
    fn first_diff_priority() {
        let base = sample_key();
        let mut k = base.clone();
        k.frontend_schema += 1;
        assert_eq!(first_diff(&base, &k), FirstDiff::FrontendSchema);
        let mut k = base.clone();
        k.target_triple = "aarch64-apple-darwin".to_string();
        assert_eq!(first_diff(&base, &k), FirstDiff::Target);
        let mut k = base.clone();
        k.env_toggles[0] = EnvToggle::Present("on".to_string());
        assert_eq!(first_diff(&base, &k), FirstDiff::EnvToggle);
        let mut k = base.clone();
        k.source_digest = hh(42);
        assert_eq!(first_diff(&base, &k), FirstDiff::UnitSource);
        let mut k = base.clone();
        k.deps[1].state = DepState::Missing;
        assert_eq!(first_diff(&base, &k), FirstDiff::DepInterfaceHashes);
        let mut k = base.clone();
        k.key_format_version += 1;
        assert_eq!(first_diff(&base, &k), FirstDiff::CacheFormatVersion);
        let mut k = base.clone();
        k.compiler_fingerprint = hh(4242);
        assert_eq!(first_diff(&base, &k), FirstDiff::CompilerBuildId);
        // The unit's own source outranks a simultaneous dependency change.
        let mut k = base.clone();
        k.source_digest = hh(5);
        k.deps[1].state = DepState::Missing;
        assert_eq!(first_diff(&base, &k), FirstDiff::UnitSource);
    }

    #[test]
    fn a_missing_dependency_is_keyed_distinctly_from_a_present_one() {
        let mut present = sample_key();
        present.deps[0].state =
            DepState::Present { interface_hash: hh(20), closure_digest: hh(30) };
        assert_ne!(sample_key().full_digest(), present.full_digest());
    }

    #[test]
    fn closure_digest_is_order_and_boundary_sensitive() {
        let a = closure_digest(&["a".to_string(), "b".to_string()]);
        let b = closure_digest(&["b".to_string(), "a".to_string()]);
        let c = closure_digest(&["ab".to_string()]);
        assert_ne!(a, b, "closure ORDER is semantic (it is the module order sema sees)");
        assert_ne!(a, c, "length prefixes keep `a`,`b` from colliding with `ab`");
    }
}
