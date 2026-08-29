//! M15 S3a: the incremental **codegen-stage** cache substrate (`docs/impl/10-cache-first-optimization.md`
//! §6). v1 caches ONE stage — per-unit object bytes — because the frontend walk (sema + lowering) is
//! cheap relative to LLVM optimize+emit and it *produces* the key inputs (`impl_hash`, interface
//! hashes). Sema always re-runs; only codegen is cached.
//!
//! ## Layout (under the resolved cache root)
//! ```text
//! cas/<hex[0..2]>/<hex>          immutable content-addressed object blobs (hex = 32-char Hash128)
//! actions/codegen/<full-digest> a manifest: the decomposed key components + the result blob digest
//! index/codegen/<slot-digest>   a pointer to the LATEST manifest published for a unit-slot
//! ```
//!
//! ## Two indexes, and why
//! The action manifest is addressed by the **full** codegen key digest, so an exact revert reproduces
//! the same digest and re-hits an old entry that was never overwritten (doc-10 §7 "source edit then
//! exact revert → old CAS artifact hit"). But a *first-differing-component* miss reason cannot come
//! from full-key addressing alone — a changed key lands at a different path, so there is nothing to
//! diff against. The `index/` slot pointer supplies that: it is addressed by only the stable-core key
//! components (cache-format version + compiler build id + unit path), so after a source/profile/flag
//! edit the prior manifest is still found and its decomposed components are diffed against the new key
//! to name the first difference (`FirstDiff`). The slot pointer affects observability only, never
//! correctness — a hit still requires the full-key action manifest + a digest-verified blob.
//!
//! ## Fail-closed
//! Every cache read is untrusted input. The manifest codec (below) is a hand-rolled versioned
//! length-prefixed decoder mirroring [`align_interface::codec`]: an unknown version, a truncated
//! buffer, a bad tag, bad UTF-8, or trailing bytes all return [`CacheDecodeError`], never a panic, and
//! length prefixes never pre-allocate from an untrusted count. Every CAS blob is digest-verified on
//! read; a mismatch unlinks the blob, prints an always-on corruption note, and rebuilds. Publication
//! is private staging + same-directory atomic rename, so a partial entry is never visible.

use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::OnceLock;

#[cfg(unix)]
use std::os::unix::fs::OpenOptionsExt;

use align_interface::{Hash128, Hash128Stream};
use align_mir::ProgramCall;

/// The cache **schema** version — the on-disk layout namespace. A bump changes the default-root
/// subdirectory (`.../alignc/<schema>/`), isolating an old tree wholesale. Independent of the KEY
/// format version below (which lives inside the key and invalidates individual entries).
pub const CACHE_SCHEMA_VERSION: u32 = 1;

/// The codegen **key-format** version — component #1 of the codegen key. A bump changes every full and
/// slot digest, so no entry written by an older layout can be reused. Distinct from the manifest wire
/// format version below.
///
/// **Bumped to 2 at ThinLTO S2**: the ThinLTO cache introduces two new cacheable phases
/// (`prelink-bitcode` + thin-backend object) that share this version component. The bump also drops
/// every S3a single-phase entry cleanly (they carried `cache_format_version == 1`), so no stale
/// object can be reused across the S2 layout change.
///
/// **Bumped to 3 at instrument-PGO S2**: [`CodegenKey`] gains the [`PgoKey`] `pgo_mode` component (the
/// settled `PgoMode { Off | Instrument | Use(Hash128) }` cache identity), so an instrumented / profile-
/// use object can never be served to an ordinary build. The bump also drops every pre-PGO codegen entry
/// cleanly (they carried `cache_format_version == 2`), so no PGO-blind object survives the layout change.
///
/// **Bumped to 4 at build-performance item 4**: every codegen-family key gains the nominal build id
/// of the dynamically loaded LLVM library immediately after its semantic version.
///
/// **Bumped to 5 at build-performance item 6**: ThinLTO prelink/backend keys identify the
/// destination partition, and backend import edges/digests identify the exact source partition.
pub const CACHE_KEY_FORMAT_VERSION: u32 = 5;

/// The manifest wire-format version. Bump on ANY change to the encoded byte layout; an old manifest
/// then fails closed on decode (treated as a miss, its bytes unreferenced). **Bumped to 2 at ThinLTO
/// S2**: the codegen-key layout lost its dead `cross_unit_opt_digest` field (ThinLTO composes via the
/// separate `prelink`/`thinbackend` phase keys instead), and the two ThinLTO manifests were added.
/// **Bumped to 3 at instrument-PGO S2**: the codegen-key manifest body gains the [`PgoKey`] `pgo_mode`
/// field (a tag byte + an optional `Hash128` profdata digest), so the wire layout changed.
const MANIFEST_FORMAT_VERSION: u32 = 5;

/// The stderr note emitted (always on, per doc-10 §6.4 fail-closed matrix) when a cache blob fails its
/// digest check and is discarded before a rebuild.
pub(crate) const CORRUPT_NOTE: &str = "alignc: cache entry corrupt; rebuilding";

/// A read cap for untrusted length-prefixed sequences: pre-allocate at most this many elements up
/// front (mirrors `align_interface::codec`'s `n.min(1024)` guard), so a garbage/huge length cannot
/// drive an allocation bomb — the real bytes still have to be present to grow past it.
const SEQ_PREALLOC_CAP: usize = 1024;

/// Cache manifests embed frontend summaries, so the bound is deliberately far above the release
/// corpus while still preventing an installer-owned exact path from driving an unbounded read.
pub(crate) const CACHE_MANIFEST_MAX_BYTES: u64 = 64 * 1024 * 1024;

/// Cache objects are per-unit native artifacts. Larger objects remain valid build outputs but are
/// deliberately not published/reused until this explicit resource contract is widened.
const CACHE_CAS_MAX_BYTES: u64 = 256 * 1024 * 1024;

const CACHE_COPY_BUFFER_BYTES: usize = 64 * 1024;

// ---- key ----------------------------------------------------------------------------------------

/// The instrument-PGO cache-key component (the settled `PgoMode { Off | Instrument | Use(Hash128) }`,
/// `docs/impl/07-roadmap.md` "Instrument-PGO design SETTLED" (g)). This is the KEY-side form of the
/// driver-facing `align_driver::PgoMode`: that one carries the profile's on-disk PATH (a CLI concern),
/// this one carries the content DIGEST of the profdata BYTES so the key is path-independent — the same
/// profile reached via a different path yields the same key (and hits), and editing the profile bytes
/// changes the key (and misses with [`FirstDiff::PgoProfile`]).
///
/// It is a KEY COMPONENT, not a separate CAS namespace — the `rt_lto`/`rt_lto_digest` precedent (same
/// artifact kind, same key shape), the ThinLTO-S2 lesson applied in reverse. `Off` / `Instrument` /
/// `Use(digest)` produce three structurally-disjoint full-key digests, so an instrumented object can
/// never be served to an ordinary (or a profile-use) build.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum PgoKey {
    /// No PGO — the byte-identical default (`--pgo-*` absent). The only variant an `emit-obj` /
    /// non-PGO per-unit build ever uses.
    Off,
    /// `--pgo-instrument`: a `-fprofile-generate`-equivalent object (counters + `__llvm_prf` metadata).
    Instrument,
    /// `--pgo-use <file.profdata>`: a `-fprofile-use` object. The `Hash128` is the content digest of the
    /// merged `.profdata` bytes, computed ONCE per invocation after the profile is validated. Those exact
    /// bytes are then snapshotted to a private staged copy that libLLVM reads (see
    /// `align_driver::StagedProfdata`): the digest here and the profile the object is optimized with come
    /// from the SAME bytes, so a `Use` HIT is provably valid for the digested profile even if the user
    /// rewrites the original file mid-build. Without the snapshot, a mid-build rewrite would publish
    /// differently-optimized objects under this key — cache poisoning.
    Use(Hash128),
}

/// The decomposed codegen action key (doc-10 §6.2). The FULL set is hashed into the action-manifest
/// path and stored verbatim in the manifest; a stable-core SUBSET is hashed into the slot-pointer path
/// (see [`CodegenKey::slot_digest`]). Comparing a decoded prior key against a fresh one yields the
/// [`FirstDiff`].
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct CodegenKey {
    /// #1 cache key-format version ([`CACHE_KEY_FORMAT_VERSION`]).
    pub cache_format_version: u32,
    /// #2 compiler build id — the hash of the running `alignc` binary bytes ([`compiler_build_id`]).
    pub compiler_build_id: Hash128,
    /// #3 frontend schema id (`align_interface::FORMAT_VERSION`).
    pub frontend_schema: u32,
    /// #3 (cont.) located vs normal MIR namespace — an `explain-opt`-shaped located entry can never be
    /// shared with a normal build. Always `false` on the cached build paths (located is uncached).
    pub located: bool,
    /// #4 the unit's `impl_hash` (complete structural MIR codegen-input fingerprint).
    pub impl_hash: Hash128,
    /// #4 (cont.) the unit's transitive dependency interface hashes, sorted by unit name. A private
    /// body edit in a dep leaves these byte-identical; a public-surface (or generic-body) change flips
    /// the dep's interface hash here, forcing this unit to miss.
    pub dep_interface_hashes: Vec<(String, Hash128)>,
    /// #5 the explicit export/root set, sorted + deduped (order-independent, it only toggles linkage).
    pub exports: Vec<String>,
    /// #6 target triple.
    pub target_triple: String,
    /// #6 (cont.) object format (`0` = ELF, `1` = Mach-O).
    pub object_format: u8,
    /// #7 resolved cpu (never the literal `"native"`).
    pub resolved_cpu: String,
    /// #7 (cont.) resolved feature set.
    pub resolved_features: String,
    /// #8 profile name.
    pub profile_name: String,
    /// #8 (cont.) middle-end pass pipeline string.
    pub pipeline: String,
    /// #8 (cont.) TargetMachine codegen opt level (`none`/`less`/`default`/`aggressive`).
    pub codegen_opt: String,
    /// #9 relocation model.
    pub reloc_model: String,
    /// #9 (cont.) code model.
    pub code_model: String,
    /// #10 exact LLVM version (`major.minor.patch`).
    pub llvm_version: String,
    /// #10 (cont.) nominal producer identity of the loaded LLVM library.
    pub llvm_build_id: Hash128,
    /// #11 rt-lto mode.
    pub rt_lto: bool,
    /// #11 (cont.) merged runtime-bitcode digest (present iff `rt_lto`).
    pub rt_lto_digest: Option<Hash128>,
    /// #12 instrument-PGO mode ([`PgoKey`]) — `Off`/`Instrument`/`Use(profdata-digest)`. Isolates an
    /// instrumented / profile-use object from an ordinary one; `Use` folds in the profile content digest
    /// so a bytes edit misses ([`FirstDiff::PgoProfile`]) and a revert re-hits.
    pub pgo_mode: PgoKey,
    /// The unit's module path — part of the slot identity (different units get different slots) and a
    /// component of the full key (harmless: distinct units already differ by `impl_hash`).
    pub unit: String,
}

impl CodegenKey {
    /// The full-key digest → the `actions/codegen/<digest>` path. Hashes every component.
    pub fn full_digest(&self) -> Hash128 {
        let mut w = Writer::new();
        write_full_key(&mut w, self);
        Hash128::of(&w.buf)
    }

    /// The slot digest → the `index/codegen/<digest>` pointer path. Hashes only the stable-core
    /// components (cache-format version + compiler build id + unit path). Excludes everything a normal
    /// in-place edit tweaks (impl_hash / dep hashes / exports / profile / rt-lto), so the prior
    /// manifest stays findable for the [`FirstDiff`] diff after such an edit.
    pub fn slot_digest(&self) -> Hash128 {
        let mut w = Writer::new();
        w.u32(self.cache_format_version);
        w.h128(self.compiler_build_id);
        w.str(&self.unit);
        Hash128::of(&w.buf)
    }

    /// Classify the first cache-key component that differs from `current` using the same ordered
    /// classifier as a cache lookup. Exposed for identity owners that compare keys produced by
    /// independent compiler worktrees; callers must not infer a cache hit from this result alone.
    pub fn first_diff(&self, current: &Self) -> FirstDiff {
        first_diff(self, current)
    }

    /// Digest every full-key input except the compiler build id. Identity owners use this to
    /// compare independently built keys without maintaining a second hand-written serializer.
    pub fn non_compiler_build_digest(&self) -> Hash128 {
        let mut key = self.clone();
        key.compiler_build_id = Hash128 { lo: 0, hi: 0 };
        key.full_digest()
    }
}

/// The first key component (in a fixed priority order) that differs between a decoded prior manifest
/// and the fresh key — the structured miss reason (doc-10 §6.5). `tests assert this enum, never
/// elapsed time. `NoPriorEntry` = no slot pointer existed to diff against; `CorruptEntry` = a stored
/// blob failed its digest check (a rebuild-triggering corruption, not a component diff).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
#[non_exhaustive]
pub enum FirstDiff {
    NoPriorEntry,
    CacheFormatVersion,
    CompilerBuildId,
    FrontendSchema,
    Target,
    Cpu,
    LlvmVersion,
    RelocCodeModel,
    /// The unit's own `impl_hash` (MIR fingerprint) changed — a private body edit.
    MirDigest,
    DepInterfaceHashes,
    /// (Unit-frontend stage) the unit's own source bytes changed.
    UnitSource,
    /// (Unit-frontend stage) a process-global `ALIGN_*` compiler toggle changed. Distinct from
    /// `Profile`: a toggle changes what the FRONTEND lowers, not how the backend optimizes.
    EnvToggle,
    Exports,
    Profile,
    RtLto,
    /// The instrument-PGO mode ([`PgoKey`]) changed — `Off`↔`Instrument`↔`Use`, or the `Use` profdata
    /// content digest differs (the profile bytes were edited/re-merged). A structural isolation boundary
    /// AND the profile-staleness invalidation, in one component.
    PgoProfile,
    /// (ThinLTO backend phase) the unit's OWN prelink bitcode content digest changed — the unit's own
    /// code changed, so its imported/optimized/emitted object must be rebuilt.
    PrelinkInput,
    /// (ThinLTO backend phase) a cross-unit input changed: an inbound import edge `(src, GUID, kind)`,
    /// this unit's outbound export/promotion set, or the prelink content digest of an import-source
    /// unit (a private-body edit in a unit this one imports from). The unit's own prelink may still hit.
    CrossUnitImports,
    CorruptEntry,
}

impl FirstDiff {
    /// A short human-readable miss reason for the `--cache-stats` surface.
    pub fn reason(self) -> &'static str {
        match self {
            FirstDiff::NoPriorEntry => "no prior entry",
            FirstDiff::CacheFormatVersion => "cache-format version",
            FirstDiff::CompilerBuildId => "compiler build id",
            FirstDiff::FrontendSchema => "frontend schema",
            FirstDiff::Target => "target",
            FirstDiff::Cpu => "cpu/features",
            FirstDiff::LlvmVersion => "llvm version/build",
            FirstDiff::RelocCodeModel => "reloc/code model",
            FirstDiff::MirDigest => "implementation changed",
            FirstDiff::DepInterfaceHashes => "dependency interface changed",
            FirstDiff::UnitSource => "unit source changed",
            FirstDiff::EnvToggle => "compiler toggle",
            FirstDiff::Exports => "export set",
            FirstDiff::Profile => "profile",
            FirstDiff::RtLto => "rt-lto mode",
            FirstDiff::PgoProfile => "pgo mode/profile",
            FirstDiff::PrelinkInput => "own code changed",
            FirstDiff::CrossUnitImports => "cross-unit imports changed",
            FirstDiff::CorruptEntry => "corrupt entry rebuilt",
        }
    }
}

/// The first differing component of `current` vs a decoded prior `stored` key, in a fixed priority
/// order. The stable-core components (cache-format version / compiler build id / unit) are guaranteed
/// equal when the slot pointer was found by [`CodegenKey::slot_digest`], but they are still checked
/// last as a defensive fallthrough.
fn first_diff(stored: &CodegenKey, current: &CodegenKey) -> FirstDiff {
    if stored.frontend_schema != current.frontend_schema || stored.located != current.located {
        return FirstDiff::FrontendSchema;
    }
    if stored.llvm_version != current.llvm_version || stored.llvm_build_id != current.llvm_build_id {
        return FirstDiff::LlvmVersion;
    }
    if stored.target_triple != current.target_triple || stored.object_format != current.object_format {
        return FirstDiff::Target;
    }
    if stored.resolved_cpu != current.resolved_cpu || stored.resolved_features != current.resolved_features {
        return FirstDiff::Cpu;
    }
    if stored.reloc_model != current.reloc_model || stored.code_model != current.code_model {
        return FirstDiff::RelocCodeModel;
    }
    if stored.impl_hash != current.impl_hash {
        return FirstDiff::MirDigest;
    }
    if stored.dep_interface_hashes != current.dep_interface_hashes {
        return FirstDiff::DepInterfaceHashes;
    }
    if stored.exports != current.exports {
        return FirstDiff::Exports;
    }
    if stored.profile_name != current.profile_name
        || stored.pipeline != current.pipeline
        || stored.codegen_opt != current.codegen_opt
    {
        return FirstDiff::Profile;
    }
    if stored.rt_lto != current.rt_lto || stored.rt_lto_digest != current.rt_lto_digest {
        return FirstDiff::RtLto;
    }
    if stored.pgo_mode != current.pgo_mode {
        return FirstDiff::PgoProfile;
    }
    if stored.cache_format_version != current.cache_format_version {
        return FirstDiff::CacheFormatVersion;
    }
    if stored.compiler_build_id != current.compiler_build_id {
        return FirstDiff::CompilerBuildId;
    }
    // Unreachable on a genuine full-key miss (some component must differ); a defensive fallback.
    FirstDiff::NoPriorEntry
}

// ---- outcome ------------------------------------------------------------------------------------

/// Which cache stage an outcome describes. The default per-unit path caches only `Codegen`; a
/// `--thin-lto` build caches two phases per unit — the summary-bearing `ThinLtoPrelink` bitcode and
/// the final `ThinLtoBackend` object (the serial thin-link between them is never cached).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
#[non_exhaustive]
pub enum CacheStage {
    /// The per-unit FRONTEND entry (`unit_cache`): the interface summary, replayable diagnostics,
    /// and link libraries a unit's sema + lowering produced. Cross-process reuse of this stage is
    /// what lets a build skip checking a unit entirely.
    UnitFrontend,
    Codegen,
    ThinLtoPrelink,
    ThinLtoBackend,
}

impl CacheStage {
    /// A short label for the `--cache-stats` surface. `Codegen` prints empty (the single-phase
    /// format `<unit> hit` is unchanged); the ThinLTO phases print their name.
    pub fn label(self) -> &'static str {
        match self {
            CacheStage::UnitFrontend => "frontend",
            CacheStage::Codegen => "",
            CacheStage::ThinLtoPrelink => "prelink",
            CacheStage::ThinLtoBackend => "backend",
        }
    }
}

/// The structured per-unit cache result (doc-10 §6.5). `hit == true` ⇒ the object came from the CAS;
/// `hit == false` with `Some(reason)` ⇒ an enabled-cache miss with its first-differing reason;
/// `hit == false` with `None` ⇒ the cache was disabled (not consulted).
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct CacheOutcome {
    pub stage: CacheStage,
    pub unit: String,
    pub hit: bool,
    pub miss_reason: Option<FirstDiff>,
}

// ---- context ------------------------------------------------------------------------------------

/// The cache root, or disabled. Resolved once from `ALIGNC_CACHE` ([`CacheContext::from_env`]).
pub enum CacheContext {
    /// Cache off — `codegen` runs the producer verbatim (today's byte-identical path, no lookup).
    Disabled,
    /// Cache on, rooted at this directory.
    ///
    /// `#[non_exhaustive]` so this variant can only be built inside the crate — every enabled cache
    /// therefore comes from [`CacheContext::from_env`] or [`CacheContext::at`], which are the two
    /// places that enforce the fail-closed compiler-identity rule. Without it, an external caller
    /// could write `CacheContext::Enabled { root }` and obtain a cache under an unidentifiable
    /// compiler, silently bypassing that rule. Nothing outside `cache.rs` constructs or matches this
    /// variant today, so the attribute costs nothing.
    #[non_exhaustive]
    Enabled {
        primary: PathBuf,
        packaged: Option<PathBuf>,
    },
}

impl CacheContext {
    /// Resolve the cache from `ALIGNC_CACHE` (doc-10 §6.1). **Default-ON (M15 S3b): unset ⇒ ENABLED**
    /// at `${XDG_CACHE_HOME:-~/.cache}/alignc/<schema>` (same as `on`). `off` (or an empty value) ⇒
    /// disabled — the operability hatch, not a compat shim. Any other value ⇒ that path used as the
    /// root verbatim (schema skew inside a shared root is handled by the fail-closed key/manifest
    /// versions). If the default root cannot be resolved (no `HOME`/`XDG_CACHE_HOME`), the on/unset
    /// case degrades to disabled rather than guessing a root.
    ///
    /// Fail-closed on an unidentifiable compiler: when the running executable cannot be hashed there
    /// is no id that distinguishes this compiler build from another, so the cache is off entirely
    /// ([`compiler_build_id_available`]). That check is **deferred to the moment an enabled cache is
    /// about to be built**, so a disabled build — `off`, a measurement toggle, or an unresolvable
    /// default root — never pays the executable read and its two hash passes.
    pub fn from_env() -> CacheContext {
        CacheContext::from_env_when(compiler_build_id_available)
    }

    /// [`CacheContext::from_env`] with the build-id availability supplied as a THUNK — the seam its
    /// owner tests drive (a real host always has a readable executable), and the mechanism that
    /// keeps the probe off the disabled paths: the thunk is called at most once, and only after a
    /// root has actually been resolved.
    fn from_env_when(build_id_available: impl FnOnce() -> bool) -> CacheContext {
        // Fail-closed measurement guard: the `ALIGN_SORT_ADAPTIVE` (doc-12 §4.1),
        // `ALIGN_NEEDLE_HOIST` (doc-13 §6.6), `ALIGN_BUFFER_DONATE` (doc-10 §8.1), and
        // `ALIGN_CONST_POOL` (doc-13 §8.4) toggles change emitted codegen for `.sort()`/
        // `.sort_by_key()`, `where(str.contains)`, donated materialization
        // (`make().map(f).to_array()`), and pooled constant-array bindings respectively. Each effect
        // already flows into the per-unit `impl_hash` (the pooling decision changes the lowered MIR —
        // a `StoreConstArray` in place of the element stores — so the MIR fingerprint captures it and
        // the object-cache key differs), but force the cache **off** whenever any is set so a
        // probe/baseline build can never read or publish a cross-toggle object into the shared cache
        // under any future refactor. Zero effect on normal builds (toggles unset).
        if std::env::var_os("ALIGN_SORT_ADAPTIVE").is_some()
            || std::env::var_os("ALIGN_NEEDLE_HOIST").is_some()
            || std::env::var_os("ALIGN_BUFFER_DONATE").is_some()
            || std::env::var_os("ALIGN_CONST_POOL").is_some()
        {
            return CacheContext::Disabled;
        }
        // Resolve the ROOT first and probe the compiler identity only if one exists: every arm that
        // yields no root returns here, before the thunk is ever called.
        let resolved = match std::env::var("ALIGNC_CACHE") {
            Err(_) => default_cache_root().map(|root| (root, true)), // unset ⇒ default-ON
            Ok(v) if v.is_empty() || v == "off" => None,             // explicit off
            Ok(v) if v == "on" => default_cache_root().map(|root| (root, true)),
            Ok(path) => Some((PathBuf::from(path), false)),
        };
        match resolved {
            Some((primary, include_packaged)) => enable_or_note(
                primary,
                include_packaged.then(packaged_cache_root).flatten(),
                build_id_available(),
            ),
            None => CacheContext::Disabled,
        }
    }

    /// Construct an enabled cache rooted at `root` (used by tests and the `on` path). Subject to the
    /// same fail-closed compiler-identity rule as [`CacheContext::from_env`]: an unidentifiable
    /// compiler yields [`CacheContext::Disabled`], never an enabled cache under a guessed id.
    pub fn at(root: PathBuf) -> CacheContext {
        CacheContext::at_when(root, compiler_build_id_available)
    }

    /// [`CacheContext::at`] with the build-id availability supplied as a thunk — the owner-test
    /// seam. `at` always intends an enabled cache, so the thunk is always called.
    fn at_when(root: PathBuf, build_id_available: impl FnOnce() -> bool) -> CacheContext {
        enable_or_note(root, None, build_id_available())
    }

    /// Whether the cache is on. The caller gates key construction on this so a disabled build (the
    /// default) never pays for the codegen-key inputs — notably the one-time `alignc`-binary hash in
    /// [`compiler_build_id`] and the target/LLVM identity resolution.
    pub fn is_enabled(&self) -> bool {
        matches!(self, CacheContext::Enabled { .. })
    }

    /// Whether codegen-family cache reuse is available. Frontend reuse needs only the compiler
    /// fingerprint, while object/prelink/backend reuse additionally requires the nominal identity
    /// of the dynamically loaded LLVM producer.
    pub fn codegen_is_enabled(&self) -> bool {
        if !self.is_enabled() {
            return false;
        }
        if align_codegen_llvm::loaded_llvm_build_id().is_some() {
            return true;
        }
        static NOTED: OnceLock<()> = OnceLock::new();
        NOTED.get_or_init(|| {
            eprintln!("alignc: codegen cache disabled (cannot identify loaded LLVM build)")
        });
        false
    }

    /// The root `alignc cache clear` operates on, honoring `ALIGNC_CACHE` path resolution even when the
    /// cache is currently disabled (`off` clears the DEFAULT root — the one a later `on` would use).
    /// An explicit path resolves to that path; anything else resolves to the default XDG root; `None`
    /// only when the default cannot be resolved (no `HOME`/`XDG_CACHE_HOME`).
    pub fn clear_root() -> Option<PathBuf> {
        match std::env::var("ALIGNC_CACHE") {
            Ok(v) if !v.is_empty() && v != "off" && v != "on" => Some(PathBuf::from(v)),
            _ => default_cache_root(),
        }
    }

    /// The resolved cache root, or `None` when the cache is off. The unit-frontend namespace
    /// (`unit_cache`) shares this one root and this one enable switch, so there is no second
    /// environment variable and no second identity gate to keep in step.
    pub(crate) fn root(&self) -> Option<&Path> {
        match self {
            CacheContext::Disabled => None,
            CacheContext::Enabled { primary, .. } => Some(primary),
        }
    }

    /// Adjacent immutable release cache, present only for the default/on environment modes.
    pub(crate) fn packaged_root(&self) -> Option<&Path> {
        match self {
            CacheContext::Disabled => None,
            CacheContext::Enabled { packaged, .. } => packaged.as_deref(),
        }
    }

    /// Unit-frontend lookup in primary-then-packaged order. Provenance is intentionally absent
    /// from the outcome: an exact hit has identical semantics, while publication always targets
    /// the primary root through the existing walk.
    pub(crate) fn lookup_unit(
        &self,
        key: &crate::unit_cache::UnitKey,
        source_len: usize,
    ) -> crate::unit_cache::UnitLookup {
        let Some(primary) = self.root() else {
            return crate::unit_cache::UnitLookup::Miss { reason: None };
        };
        if crate::unit_cache::is_rejected(primary, key) {
            return crate::unit_cache::UnitLookup::Miss {
                reason: Some(FirstDiff::CorruptEntry),
            };
        }
        match crate::unit_cache::lookup(primary, key, source_len) {
            hit @ crate::unit_cache::UnitLookup::Hit(_) => hit,
            crate::unit_cache::UnitLookup::Miss {
                reason: primary_reason,
            } => match self.packaged_root() {
                Some(packaged) => {
                    match crate::unit_cache::lookup_packaged(packaged, primary, key, source_len) {
                        hit @ crate::unit_cache::UnitLookup::Hit(_) => hit,
                        crate::unit_cache::UnitLookup::Miss {
                            reason: packaged_reason,
                        } => crate::unit_cache::UnitLookup::Miss {
                            reason: match primary_reason {
                                Some(FirstDiff::NoPriorEntry) | None => packaged_reason,
                                reason => reason,
                            },
                        },
                    }
                }
                None => crate::unit_cache::UnitLookup::Miss {
                    reason: primary_reason,
                },
            },
        }
    }

    /// The serial cache lookup for one unit — the first half of [`codegen`], exposed so the parallel
    /// build driver can do all lookups serially and then produce only the MISSES in parallel (the
    /// settled S3 design). On an enabled HIT the CAS blob is written verbatim to `obj_out` and
    /// [`CacheLookup::Hit`] carries the outcome. A [`CacheLookup::Miss`] carries the first-differing
    /// reason (its object is NOT produced — the caller must `produce` it then [`publish_after_miss`]).
    /// A disabled cache is [`CacheLookup::Miss`] with `None` reason (never consulted, no key work).
    pub fn lookup(&self, key: &CodegenKey, obj_out: &Path) -> CacheLookup {
        if !self.codegen_is_enabled() {
            return CacheLookup::Miss { reason: None };
        }
        let (primary, packaged) = match self {
            CacheContext::Disabled => return CacheLookup::Miss { reason: None },
            CacheContext::Enabled { primary, packaged } => (primary, packaged.as_deref()),
        };
        let primary_result = try_hit(
            primary,
            &action_manifest_path(primary, key.full_digest()),
            key,
            obj_out,
            ReadPolicy::Writable,
        );
        if matches!(primary_result, HitResult::Hit) {
            return CacheLookup::Hit(CacheOutcome {
                stage: CacheStage::Codegen,
                unit: key.unit.clone(),
                hit: true,
                miss_reason: None,
            });
        }
        let packaged_result = packaged.map_or(HitResult::Miss, |root| {
            try_hit(
                root,
                &action_manifest_path(root, key.full_digest()),
                key,
                obj_out,
                ReadPolicy::Packaged,
            )
        });
        match packaged_result {
            HitResult::Hit => CacheLookup::Hit(CacheOutcome {
                stage: CacheStage::Codegen,
                unit: key.unit.clone(),
                hit: true,
                miss_reason: None,
            }),
            HitResult::Corrupt | HitResult::Miss
                if matches!(primary_result, HitResult::Corrupt)
                    || matches!(packaged_result, HitResult::Corrupt) =>
            {
                CacheLookup::Miss {
                    reason: Some(FirstDiff::CorruptEntry),
                }
            }
            HitResult::Corrupt => CacheLookup::Miss {
                reason: Some(FirstDiff::CorruptEntry),
            },
            // Reason computed BEFORE any publish overwrites the slot pointer (the prior key is diffed).
            HitResult::Miss => CacheLookup::Miss {
                reason: Some(match diff_against_slot(primary, key) {
                    FirstDiff::NoPriorEntry => packaged
                        .map(|root| diff_against_slot(root, key))
                        .unwrap_or(FirstDiff::NoPriorEntry),
                    reason => reason,
                }),
            },
        }
    }

    /// Publish an already-produced object to the cache after a [`CacheLookup::Miss`] — best-effort (a
    /// cache WRITE failure never fails an otherwise-correct build; the object at `obj_out` is already
    /// valid and link reads it directly). A no-op when the cache is disabled. Safe to call from a
    /// worker thread (only writes into the content-addressed store + index).
    pub fn publish_after_miss(&self, key: &CodegenKey, obj_out: &Path) {
        if !self.codegen_is_enabled() {
            return;
        }
        if let CacheContext::Enabled { primary, .. } = self {
            publish(primary, key, obj_out);
        }
    }

    /// Run the codegen stage for one unit through the cache (the serial composition of [`lookup`] +
    /// `produce` + [`publish_after_miss`]). On an enabled hit, the CAS blob is written verbatim to
    /// `obj_out` and no producer runs. On a miss (or when disabled), `produce(obj_out)` runs today's
    /// codegen verbatim, then (when enabled) the object bytes are published. Returns the structured
    /// [`CacheOutcome`]; a producer error propagates as `Err`.
    pub fn codegen<F>(&self, key: &CodegenKey, obj_out: &Path, produce: F) -> Result<CacheOutcome, String>
    where
        F: FnOnce(&Path) -> Result<(), String>,
    {
        match self.lookup(key, obj_out) {
            CacheLookup::Hit(outcome) => Ok(outcome),
            CacheLookup::Miss { reason } => {
                produce(obj_out)?;
                self.publish_after_miss(key, obj_out);
                Ok(CacheOutcome {
                    stage: CacheStage::Codegen,
                    unit: key.unit.clone(),
                    hit: false,
                    miss_reason: reason,
                })
            }
        }
    }
}

/// The result of a serial [`CacheContext::lookup`]. A `Hit` has already written `obj_out`; a `Miss`
/// requires the caller to produce the object and then [`CacheContext::publish_after_miss`].
pub enum CacheLookup {
    Hit(CacheOutcome),
    Miss { reason: Option<FirstDiff> },
}

/// Clear the cache under `root` by removing only the cache-owned entries (`cas`, `actions`, `index`)
/// — never the root itself, so an explicit `ALIGNC_CACHE=<shared dir>` is not nuked wholesale. Safe on
/// an absent root/entry (each missing one is skipped). Returns whether anything was removed.
///
/// A cache-owned entry that is a **symlink** is never followed: the link itself is unlinked, never its
/// target — so `clear` can never recurse out of the resolved root even if an entry was replaced by a
/// symlink. (The cache is a local, non-adversarial store, but this keeps the "delete only inside the
/// resolved root" guarantee unconditional.)
pub fn clear_cache(root: &Path) -> Result<bool, String> {
    let mut removed = false;
    for sub in ["cas", "actions", "index"] {
        let path = root.join(sub);
        // `symlink_metadata` does NOT follow a top-level symlink (unlike `metadata`), so we classify
        // the entry itself before deciding how to remove it.
        let meta = match std::fs::symlink_metadata(&path) {
            Ok(m) => m,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => continue,
            Err(e) => return Err(format!("cannot stat {}: {e}", path.display())),
        };
        let result = if meta.file_type().is_dir() {
            std::fs::remove_dir_all(&path) // a real dir: recurse (std's impl does not follow inner symlinks)
        } else {
            std::fs::remove_file(&path) // a symlink or a stray file: unlink the entry only, never a target
        };
        match result {
            Ok(()) => removed = true,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
            Err(e) => return Err(format!("cannot remove {}: {e}", path.display())),
        }
    }
    Ok(removed)
}

/// Publish a produced object to the cache, best-effort: the CAS blob + the full-key action manifest +
/// the unit-slot pointer. Any I/O failure is logged and swallowed — populating the cache is never
/// allowed to fail a build whose object was produced correctly.
fn publish(root: &Path, key: &CodegenKey, obj_out: &Path) {
    let bytes = match read_regular_bounded_file(obj_out, CACHE_CAS_MAX_BYTES) {
        Some(bytes) => bytes,
        None => {
            eprintln!(
                "alignc: cache not populated (produced object is unavailable, non-regular, or exceeds 256 MiB): {}",
                obj_out.display()
            );
            return;
        }
    };
    let blob_digest = Hash128::of(&bytes);
    let manifest = serialize_manifest(key, blob_digest);
    let result = publish_blob(root, blob_digest, &bytes)
        .and_then(|()| publish_file(&action_manifest_path(root, key.full_digest()), &manifest))
        .and_then(|()| publish_file(&slot_pointer_path(root, key.slot_digest()), &manifest));
    if let Err(e) = result {
        eprintln!("alignc: cache not populated: {e}");
    }
}

/// The always-on note text for a cache disabled because the compiler cannot be identified. A `const`
/// so the decision that produces it is assertable without capturing stderr.
const UNIDENTIFIABLE_COMPILER_NOTE: &str = "alignc: cache disabled (cannot read the running \
     executable, so this compiler build cannot be identified)";

/// The enable decision AS A VALUE: `Ok` the enabled cache, `Err` the note the caller must print.
///
/// Split from the printing on purpose. The rule under test is "an unidentifiable compiler yields no
/// enabled cache", and a test for it must not depend on stderr capture or on which earlier test
/// happened to consume the print-once latch.
fn decide_enabled(
    primary: PathBuf,
    packaged: Option<PathBuf>,
    build_id_available: bool,
) -> Result<CacheContext, &'static str> {
    if build_id_available {
        Ok(CacheContext::Enabled { primary, packaged })
    } else {
        Err(UNIDENTIFIABLE_COMPILER_NOTE)
    }
}

/// [`decide_enabled`], printing the note once per process on the disabled outcome. Once, because it
/// is a persistent-state decision the user should see, but repeating it per unit would bury the
/// build's real output.
fn enable_or_note(
    primary: PathBuf,
    packaged: Option<PathBuf>,
    build_id_available: bool,
) -> CacheContext {
    decide_enabled(primary, packaged, build_id_available).unwrap_or_else(|note| {
        static NOTED: OnceLock<()> = OnceLock::new();
        NOTED.get_or_init(|| eprintln!("{note}"));
        CacheContext::Disabled
    })
}

/// The running executable's bytes, or `None` when they cannot be read.
///
/// On Linux this reads `/proc/self/exe` **directly** instead of resolving it to a path first. The
/// kernel keeps the running image reachable through that link even after the file is unlinked or
/// replaced, so the bytes are provably this process's own image: both the resolve-then-read TOCTOU
/// window and the "the binary was deleted mid-build" failure condition disappear together.
///
/// macOS (the only other supported target) has no equivalent, so it resolves `current_exe()` and
/// reads that path. **Recorded residual risk:** between the resolve and the open, the path can be
/// replaced — an atomic `rename` over it by a concurrent `cargo build` is the realistic case — and
/// this process then fingerprints the INCOMING binary while running the old one. A later run of
/// that incoming binary computes the same id and could be served objects the old compiler produced.
/// That is a real, if narrow, soundness window, not merely a lost hit: it cannot be closed from
/// user space without a `/proc/self/exe` equivalent. A build-time-baked compiler fingerprint (the
/// recorded follow-up) closes it by not depending on the executable at all.
fn exe_bytes() -> Option<Vec<u8>> {
    #[cfg(target_os = "linux")]
    {
        std::fs::read("/proc/self/exe").ok()
    }
    #[cfg(not(target_os = "linux"))]
    {
        std::env::current_exe().ok().and_then(|p| std::fs::read(p).ok())
    }
}

/// The compiler build id and whether it is a REAL identity, decided from the executable's bytes.
///
/// A cache entry is only as sound as this component: it is the sole thing separating one compiler
/// build's artifacts from another's. The real id is the hash of the running executable's bytes,
/// which covers dev rebuilds at an unchanged crate version — any codegen/lowering source change
/// rebuilds the binary and flips it.
///
/// **There is deliberately no stable fallback.** A version-derived constant is shared by *every*
/// build of that version, so two different compilers that both failed to read their executable would
/// address the same entries and one could be served the other's object — a miscompile, not a missed
/// optimization. When the bytes are absent the id is therefore unique to this process (it can
/// collide with nothing) and the availability flag is `false`, which turns the cache off at every
/// construction site.
///
/// Split from the I/O in [`build_id`] so both arms are exercised by the owner tests through the
/// production code itself rather than a re-implementation of it.
fn build_id_from(exe_bytes: Option<Vec<u8>>) -> (Hash128, bool) {
    match exe_bytes {
        Some(bytes) => (Hash128::of(&bytes), true),
        None => {
            let stamp = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_nanos();
            let nonce = STAGE_NONCE.fetch_add(1, Ordering::Relaxed);
            let unique = format!(
                "alignc-build-id-unidentified-{}-{stamp}-{nonce}",
                std::process::id()
            );
            (Hash128::of(unique.as_bytes()), false)
        }
    }
}

/// `build_id_from(exe_bytes())`, memoized once per process — the I/O half, kept thin so the decision
/// half stays directly testable.
fn build_id() -> (Hash128, bool) {
    static ID: OnceLock<(Hash128, bool)> = OnceLock::new();
    *ID.get_or_init(|| build_id_from(exe_bytes()))
}

/// The compiler build id: the hash of the running executable's bytes. Only meaningful when
/// [`compiler_build_id_available`] is `true`; every enabled [`CacheContext`] guarantees that.
pub fn compiler_build_id() -> Hash128 {
    build_id().0
}

/// Whether the compiler build id is a real identity derived from the running executable's bytes.
/// `false` means the compiler cannot be distinguished from another build, so the cache must stay
/// off — enforced by [`CacheContext::from_env`] and [`CacheContext::at`].
pub fn compiler_build_id_available() -> bool {
    build_id().1
}

/// `${XDG_CACHE_HOME:-~/.cache}/alignc/<schema>`, or `None` if neither `XDG_CACHE_HOME` nor `HOME` is
/// set (then `ALIGNC_CACHE=on` degrades to disabled rather than guessing a root).
///
/// Platform story: the supported targets are Linux and macOS, and both use the XDG `~/.cache`
/// convention here deliberately (a settled S3 choice — one root layout, not macOS's
/// `~/Library/Caches`). There is intentionally **no** Windows `%LOCALAPPDATA%` branch: Windows is a
/// fail-closed unsupported target (`align_codegen_llvm::target_object_format` errors on it and linking
/// is unsupported), so a Windows build never reaches a successful link — a cache-root branch for it
/// would be dead code. If Windows ever becomes a real target, add the `%LOCALAPPDATA%` fallback here
/// together with the linker support, not before.
fn default_cache_root() -> Option<PathBuf> {
    let base = std::env::var_os("XDG_CACHE_HOME")
        .map(PathBuf::from)
        .filter(|p| !p.as_os_str().is_empty())
        .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".cache")))?;
    Some(base.join("alignc").join(CACHE_SCHEMA_VERSION.to_string()))
}

/// `<real-alignc-dir>/share/align/cache/<schema>`, without canonicalizing or scanning the tree.
/// Absence is ordinary (copying only the executable remains supported).
fn packaged_cache_root() -> Option<PathBuf> {
    let executable = std::env::current_exe().ok()?;
    let directory = executable.parent()?;
    let root = directory
        .join("share")
        .join("align")
        .join("cache")
        .join(CACHE_SCHEMA_VERSION.to_string());
    root.is_dir().then_some(root)
}

fn action_manifest_path(root: &Path, full: Hash128) -> PathBuf {
    root.join("actions").join("codegen").join(full.to_hex())
}

fn slot_pointer_path(root: &Path, slot: Hash128) -> PathBuf {
    root.join("index").join("codegen").join(slot.to_hex())
}

/// `cas/<hex[0..2]>/<hex>` for a blob digest (hex is 32 chars, so the 2-char shard prefix is safe).
/// `pub` so tests (and any external tooling) locate a CAS blob by the same rule the cache uses,
/// instead of re-deriving the sharding convention.
pub fn cas_blob_path(root: &Path, digest: Hash128) -> PathBuf {
    let hex = digest.to_hex();
    root.join("cas").join(&hex[..2]).join(&hex)
}

#[derive(Clone, Copy)]
enum HitResult {
    Hit,
    /// No usable prior entry (absent / undecodable / foreign manifest): a clean miss.
    Miss,
    /// A prior entry existed but its blob failed the digest check — rebuild after unlinking + noting.
    Corrupt,
}

#[derive(Clone, Copy)]
enum ReadPolicy {
    Writable,
    Packaged,
}

/// Open an exact cache path without letting a followed FIFO block before it can be classified.
/// The metadata belongs to the opened handle, so a concurrent path replacement cannot substitute a
/// different target after validation.
fn open_regular_bounded(path: &Path, max_bytes: Option<u64>) -> Option<(std::fs::File, usize)> {
    let mut options = std::fs::OpenOptions::new();
    options.read(true);
    #[cfg(unix)]
    options.custom_flags(libc::O_NONBLOCK);
    let file = options.open(path).ok()?;
    let metadata = file.metadata().ok()?;
    if !metadata.is_file() || max_bytes.is_some_and(|limit| metadata.len() > limit) {
        return None;
    }
    let length = usize::try_from(metadata.len()).ok()?;
    Some((file, length))
}

fn read_regular_bounded_file(path: &Path, max_bytes: u64) -> Option<Vec<u8>> {
    let (file, expected) = open_regular_bounded(path, Some(max_bytes))?;
    read_regular_bounded_from(file, expected, max_bytes)
}

fn read_regular_bounded_from(
    mut reader: impl Read,
    expected: usize,
    max_bytes: u64,
) -> Option<Vec<u8>> {
    let expected_u64 = u64::try_from(expected).ok()?;
    if expected_u64 > max_bytes {
        return None;
    }
    let mut bytes = Vec::with_capacity(expected);
    reader
        .by_ref()
        .take(expected_u64 + 1)
        .read_to_end(&mut bytes)
        .ok()?;
    (bytes.len() == expected).then_some(bytes)
}

/// Read one action/index manifest through the fixed cache-format resource bound. Exact regular-file
/// symlinks remain supported. Length disagreement catches shrink/growth after the handle metadata
/// snapshot, and the declared-length-plus-one reader caps both allocation and read work.
pub(crate) fn read_cache_manifest(path: &Path) -> Option<Vec<u8>> {
    read_regular_bounded_file(path, CACHE_MANIFEST_MAX_BYTES)
}

#[cfg(test)]
fn read_cache_manifest_from(reader: impl Read, expected: usize) -> Option<Vec<u8>> {
    read_regular_bounded_from(reader, expected, CACHE_MANIFEST_MAX_BYTES)
}

struct MaterializedStage {
    path: PathBuf,
    committed: bool,
}

impl Drop for MaterializedStage {
    fn drop(&mut self) {
        if !self.committed {
            let _ = std::fs::remove_file(&self.path);
        }
    }
}

/// Attempt a hit at `action_path`. Fail-closed at every step: a missing/undecodable manifest is a
/// clean [`HitResult::Miss`]; a manifest whose stored key does not match `key` (a hash collision) is a
/// miss; a missing or digest-mismatched blob is [`HitResult::Corrupt`] (note + unlink + rebuild). On a
/// verified hit the blob is written to `obj_out`.
fn try_hit(
    root: &Path,
    action_path: &Path,
    key: &CodegenKey,
    obj_out: &Path,
    policy: ReadPolicy,
) -> HitResult {
    let manifest_bytes = match read_cache_manifest(action_path) {
        Some(bytes) => bytes,
        None => return HitResult::Miss,
    };
    let (stored_key, blob_digest) = match deserialize_manifest(&manifest_bytes) {
        Ok(v) => v,
        Err(_) => return HitResult::Miss, // version skew / garbage: unreferenced, rebuild fresh
    };
    // Defense in depth against a full-digest collision: the stored components must equal the key.
    if &stored_key != key {
        return HitResult::Miss;
    }
    materialize_blob(root, blob_digest, obj_out, policy)
}

/// Read the CAS blob for `blob_digest`, verify its content digest, and write it to `out_path`. A
/// missing or digest-mismatched blob is [`HitResult::Corrupt`] (always-on note + unlink; doc-10 §6.4
/// fail-closed matrix); a verified blob that cannot be written back is a clean [`HitResult::Miss`]
/// (rebuild in place). Shared by the single-phase codegen cache and both ThinLTO phases.
fn materialize_blob(
    root: &Path,
    blob_digest: Hash128,
    out_path: &Path,
    policy: ReadPolicy,
) -> HitResult {
    let blob_path = cas_blob_path(root, blob_digest);
    let (mut blob, expected) = match open_regular_bounded(&blob_path, Some(CACHE_CAS_MAX_BYTES)) {
        Some(opened) => opened,
        None => {
            // A writable action pointing at an unavailable blob retains the existing self-heal
            // diagnosis. An immutable packaged blob may simply be absent or unreadable after an
            // installer/permission change; the public contract makes every such I/O failure a
            // clean miss and reserves the packaged-corruption note for bytes we actually read and
            // prove digest-bad.
            return match policy {
                ReadPolicy::Writable => {
                    remove_writable_cache_entry(&blob_path);
                    note_corrupt(policy);
                    HitResult::Corrupt
                }
                ReadPolicy::Packaged => HitResult::Miss,
            };
        }
    };
    let stage_path = staging_sibling(out_path);
    let mut stage = MaterializedStage {
        path: stage_path.clone(),
        committed: false,
    };
    let mut staged_file = match std::fs::File::create(&stage_path) {
        Ok(file) => file,
        Err(_) => return HitResult::Miss,
    };
    let mut hasher = Hash128Stream::for_len(expected);
    let mut copied = 0usize;
    let mut buffer = [0u8; CACHE_COPY_BUFFER_BYTES];
    loop {
        let read = match blob.read(&mut buffer) {
            Ok(0) => break,
            Ok(read) => read,
            Err(_) => return HitResult::Miss,
        };
        let Some(next) = copied.checked_add(read) else {
            return HitResult::Miss;
        };
        if next > expected || !hasher.update(&buffer[..read]) {
            return HitResult::Miss;
        }
        if staged_file.write_all(&buffer[..read]).is_err() {
            return HitResult::Miss;
        }
        copied = next;
    }
    if copied != expected {
        return HitResult::Miss;
    }
    let Some(actual_digest) = hasher.finish() else {
        return HitResult::Miss;
    };
    if actual_digest != blob_digest {
        // Corrupted blob bytes: unlink + always-on note + rebuild.
        if matches!(policy, ReadPolicy::Writable) {
            remove_writable_cache_entry(&blob_path);
        }
        note_corrupt(policy);
        return HitResult::Corrupt;
    }
    drop(staged_file);
    if std::fs::rename(&stage_path, out_path).is_err() {
        return HitResult::Miss;
    }
    stage.committed = true;
    HitResult::Hit
}

/// Remove one exact writable cache entry without following it when it is a symlink. A malformed
/// real directory at a blob path is cache-owned too and must be removed recursively; otherwise it
/// would make every later atomic publication fail permanently.
fn remove_writable_cache_entry(path: &Path) {
    let Ok(metadata) = std::fs::symlink_metadata(path) else { return };
    if metadata.file_type().is_dir() {
        let _ = std::fs::remove_dir_all(path);
    } else {
        let _ = std::fs::remove_file(path);
    }
}

fn note_corrupt(policy: ReadPolicy) {
    match policy {
        ReadPolicy::Writable => eprintln!("{CORRUPT_NOTE}"),
        ReadPolicy::Packaged => note_packaged_corrupt(),
    }
}

pub(crate) fn note_packaged_corrupt() {
    static NOTED: OnceLock<()> = OnceLock::new();
    NOTED.get_or_init(|| eprintln!("alignc: packaged cache entry corrupt; rebuilding"));
}

/// Compute the [`FirstDiff`] for a miss by reading the unit's slot pointer and diffing its decoded key
/// against `key`. No slot pointer (or an undecodable one) ⇒ [`FirstDiff::NoPriorEntry`].
fn diff_against_slot(root: &Path, key: &CodegenKey) -> FirstDiff {
    let path = slot_pointer_path(root, key.slot_digest());
    match read_cache_manifest(&path) {
        Some(bytes) => match deserialize_manifest(&bytes) {
            Ok((stored_key, _)) => first_diff(&stored_key, key),
            Err(_) => FirstDiff::NoPriorEntry,
        },
        None => FirstDiff::NoPriorEntry,
    }
}

// ---- publication (private staging + atomic rename) ----------------------------------------------

static STAGE_NONCE: AtomicU64 = AtomicU64::new(0);

/// A unique sibling temp path in `final_path`'s parent, so the publish rename is same-directory (hence
/// atomic on POSIX, never cross-filesystem). Mirrors the `ArtifactStage` naming (pid + time + nonce).
fn staging_sibling(final_path: &Path) -> PathBuf {
    let parent = final_path.parent().unwrap_or_else(|| Path::new("."));
    let nonce = STAGE_NONCE.fetch_add(1, Ordering::Relaxed);
    let stamp = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos();
    parent.join(format!(".cache-stage-{}-{stamp}-{nonce}", std::process::id()))
}

/// Publish `bytes` at `final_path` by staged write + same-directory atomic rename. A concurrent
/// producer of the same key writes byte-identical content; last-writer-wins is harmless. Creating the
/// parent directories is idempotent.
pub(crate) fn publish_file(final_path: &Path, bytes: &[u8]) -> Result<(), String> {
    if let Some(parent) = final_path.parent() {
        std::fs::create_dir_all(parent).map_err(|e| format!("cannot create cache dir {}: {e}", parent.display()))?;
    }
    let tmp = staging_sibling(final_path);
    // On ANY error after the staging file is created — a failed (possibly partial) write or a failed
    // rename — remove `tmp` before returning, so an ordinary error never orphans a staging file in the
    // cache root. (doc-10 tolerates staging orphaned by a KILLED process; an error return must not.)
    if let Err(e) = std::fs::write(&tmp, bytes) {
        let _ = std::fs::remove_file(&tmp);
        return Err(format!("cannot stage cache file {}: {e}", tmp.display()));
    }
    if let Err(e) = std::fs::rename(&tmp, final_path) {
        let _ = std::fs::remove_file(&tmp);
        // A racing producer may already have published identical content; accept that, else fail.
        if !final_path.exists() {
            return Err(format!("cannot publish cache file {}: {e}", final_path.display()));
        }
    }
    Ok(())
}

/// Publish a CAS blob (immutable, content-addressed). If the blob already exists it is left untouched
/// (content-addressed ⇒ identical bytes), avoiding a redundant large-object rewrite.
fn publish_blob(root: &Path, digest: Hash128, bytes: &[u8]) -> Result<(), String> {
    let path = cas_blob_path(root, digest);
    if path.exists() {
        return Ok(());
    }
    publish_file(&path, bytes)
}

// ---- manifest codec (hand-rolled, versioned, length-prefixed, fail-closed) ----------------------

pub(crate) struct Writer {
    pub(crate) buf: Vec<u8>,
}

impl Writer {
    pub(crate) fn new() -> Writer {
        Writer { buf: Vec::new() }
    }
    pub(crate) fn u8(&mut self, v: u8) {
        self.buf.push(v);
    }
    pub(crate) fn bool(&mut self, v: bool) {
        self.u8(v as u8);
    }
    pub(crate) fn u32(&mut self, v: u32) {
        self.buf.extend_from_slice(&v.to_le_bytes());
    }
    pub(crate) fn u64(&mut self, v: u64) {
        self.buf.extend_from_slice(&v.to_le_bytes());
    }
    pub(crate) fn h128(&mut self, h: Hash128) {
        self.u64(h.lo);
        self.u64(h.hi);
    }
    fn opt_h128(&mut self, h: Option<Hash128>) {
        match h {
            Some(x) => {
                self.u8(1);
                self.h128(x);
            }
            None => self.u8(0),
        }
    }
    /// The [`PgoKey`] component: a tag byte (`0` Off, `1` Instrument, `2` Use) plus, for `Use`, the
    /// profdata content digest. Distinct tags make `Off`/`Instrument`/`Use` digests structurally
    /// disjoint; the `Use` digest folds the profile bytes into the key.
    fn pgo(&mut self, p: PgoKey) {
        match p {
            PgoKey::Off => self.u8(0),
            PgoKey::Instrument => self.u8(1),
            PgoKey::Use(d) => {
                self.u8(2);
                self.h128(d);
            }
        }
    }
    pub(crate) fn bytes(&mut self, b: &[u8]) {
        self.u32(u32_len(b.len()));
        self.buf.extend_from_slice(b);
    }
    pub(crate) fn str(&mut self, s: &str) {
        self.bytes(s.as_bytes());
    }
}

/// Narrow a length to the `u32` prefix width, or panic loudly. Producer-side, compiler-internal data
/// (never untrusted input) — matching `align_interface::codec::u32_len`; the reader stays Err-based.
pub(crate) fn u32_len(n: usize) -> u32 {
    u32::try_from(n).unwrap_or_else(|_| panic!("cache manifest field exceeds u32::MAX bytes — the format uses u32 length prefixes"))
}

/// Write every key component (the full digest input and the manifest body).
fn write_full_key(w: &mut Writer, k: &CodegenKey) {
    w.u32(k.cache_format_version);
    w.h128(k.compiler_build_id);
    w.u32(k.frontend_schema);
    w.bool(k.located);
    w.h128(k.impl_hash);
    w.u32(u32_len(k.dep_interface_hashes.len()));
    for (name, h) in &k.dep_interface_hashes {
        w.str(name);
        w.h128(*h);
    }
    w.u32(u32_len(k.exports.len()));
    for e in &k.exports {
        w.str(e);
    }
    w.str(&k.target_triple);
    w.u8(k.object_format);
    w.str(&k.resolved_cpu);
    w.str(&k.resolved_features);
    w.str(&k.profile_name);
    w.str(&k.pipeline);
    w.str(&k.codegen_opt);
    w.str(&k.reloc_model);
    w.str(&k.code_model);
    w.str(&k.llvm_version);
    w.h128(k.llvm_build_id);
    w.bool(k.rt_lto);
    w.opt_h128(k.rt_lto_digest);
    w.pgo(k.pgo_mode);
    w.str(&k.unit);
}

/// The complete manifest bytes: wire version + full key + result blob digest.
fn serialize_manifest(key: &CodegenKey, blob_digest: Hash128) -> Vec<u8> {
    let mut w = Writer::new();
    w.u32(MANIFEST_FORMAT_VERSION);
    write_full_key(&mut w, key);
    w.h128(blob_digest);
    w.buf
}

/// A fail-closed manifest decode failure — every variant is a hard rejection, never a partial value.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum CacheDecodeError {
    UnknownVersion(u32),
    Truncated,
    BadTag { what: &'static str, tag: u8 },
    BadUtf8,
    TrailingBytes,
    /// The bytes decoded structurally but a decoded value is out of range for the artifact it
    /// describes — a diagnostic span past end-of-source, or a value-region digest mismatch. Past
    /// the key comparison this means damage, not version skew, so the entry is unlinked.
    SemanticRange,
}

pub(crate) struct Reader<'a> {
    buf: &'a [u8],
    pub(crate) pos: usize,
}

impl<'a> Reader<'a> {
    pub(crate) fn new(buf: &'a [u8]) -> Reader<'a> {
        Reader { buf, pos: 0 }
    }
    fn take(&mut self, n: usize) -> Result<&'a [u8], CacheDecodeError> {
        let end = self.pos.checked_add(n).ok_or(CacheDecodeError::Truncated)?;
        let s = self.buf.get(self.pos..end).ok_or(CacheDecodeError::Truncated)?;
        self.pos = end;
        Ok(s)
    }
    pub(crate) fn u8(&mut self) -> Result<u8, CacheDecodeError> {
        Ok(self.take(1)?[0])
    }
    pub(crate) fn bool(&mut self) -> Result<bool, CacheDecodeError> {
        match self.u8()? {
            0 => Ok(false),
            1 => Ok(true),
            tag => Err(CacheDecodeError::BadTag { what: "bool", tag }),
        }
    }
    pub(crate) fn u32(&mut self) -> Result<u32, CacheDecodeError> {
        Ok(u32::from_le_bytes(self.take(4)?.try_into().unwrap()))
    }
    pub(crate) fn u64(&mut self) -> Result<u64, CacheDecodeError> {
        Ok(u64::from_le_bytes(self.take(8)?.try_into().unwrap()))
    }
    pub(crate) fn h128(&mut self) -> Result<Hash128, CacheDecodeError> {
        Ok(Hash128 { lo: self.u64()?, hi: self.u64()? })
    }
    fn opt_h128(&mut self) -> Result<Option<Hash128>, CacheDecodeError> {
        match self.u8()? {
            0 => Ok(None),
            1 => Ok(Some(self.h128()?)),
            tag => Err(CacheDecodeError::BadTag { what: "option", tag }),
        }
    }
    /// The [`PgoKey`] component (mirror of [`Writer::pgo`]). Fail-closed on an unknown tag.
    fn pgo(&mut self) -> Result<PgoKey, CacheDecodeError> {
        match self.u8()? {
            0 => Ok(PgoKey::Off),
            1 => Ok(PgoKey::Instrument),
            2 => Ok(PgoKey::Use(self.h128()?)),
            tag => Err(CacheDecodeError::BadTag { what: "pgo", tag }),
        }
    }
    /// A length prefix, then that many bytes — bounds-checked (the `take` validates the count against
    /// the real buffer, so a huge length simply fails `Truncated`, never pre-allocates).
    pub(crate) fn bytes(&mut self) -> Result<Vec<u8>, CacheDecodeError> {
        let n = self.u32()? as usize;
        Ok(self.take(n)?.to_vec())
    }
    pub(crate) fn str(&mut self) -> Result<String, CacheDecodeError> {
        let bytes = self.bytes()?;
        String::from_utf8(bytes).map_err(|_| CacheDecodeError::BadUtf8)
    }
    /// A length prefix, then `f` that many times. Pre-allocates at most [`SEQ_PREALLOC_CAP`] to bound a
    /// garbage-length allocation; the real elements still have to be present to grow further.
    pub(crate) fn seq<T>(&mut self, mut f: impl FnMut(&mut Reader<'a>) -> Result<T, CacheDecodeError>) -> Result<Vec<T>, CacheDecodeError> {
        let n = self.u32()? as usize;
        let mut out = Vec::with_capacity(n.min(SEQ_PREALLOC_CAP));
        for _ in 0..n {
            out.push(f(self)?);
        }
        Ok(out)
    }
    pub(crate) fn finish(self) -> Result<(), CacheDecodeError> {
        if self.pos == self.buf.len() {
            Ok(())
        } else {
            Err(CacheDecodeError::TrailingBytes)
        }
    }
}

/// Decode a manifest into `(key, blob_digest)`. Fail-closed: unknown wire version, truncation, a bad
/// tag, invalid UTF-8, or trailing bytes all return [`CacheDecodeError`], never a panic.
fn deserialize_manifest(bytes: &[u8]) -> Result<(CodegenKey, Hash128), CacheDecodeError> {
    let mut r = Reader::new(bytes);
    let version = r.u32()?;
    if version != MANIFEST_FORMAT_VERSION {
        return Err(CacheDecodeError::UnknownVersion(version));
    }
    let cache_format_version = r.u32()?;
    let compiler_build_id = r.h128()?;
    let frontend_schema = r.u32()?;
    let located = r.bool()?;
    let impl_hash = r.h128()?;
    let dep_interface_hashes = r.seq(|r| Ok((r.str()?, r.h128()?)))?;
    let exports = r.seq(|r| r.str())?;
    let target_triple = r.str()?;
    let object_format = r.u8()?;
    let resolved_cpu = r.str()?;
    let resolved_features = r.str()?;
    let profile_name = r.str()?;
    let pipeline = r.str()?;
    let codegen_opt = r.str()?;
    let reloc_model = r.str()?;
    let code_model = r.str()?;
    let llvm_version = r.str()?;
    let llvm_build_id = r.h128()?;
    let rt_lto = r.bool()?;
    let rt_lto_digest = r.opt_h128()?;
    let pgo_mode = r.pgo()?;
    let unit = r.str()?;
    let blob_digest = r.h128()?;
    r.finish()?;
    Ok((
        CodegenKey {
            cache_format_version,
            compiler_build_id,
            frontend_schema,
            located,
            impl_hash,
            dep_interface_hashes,
            exports,
            target_triple,
            object_format,
            resolved_cpu,
            resolved_features,
            profile_name,
            pipeline,
            codegen_opt,
            reloc_model,
            code_model,
            llvm_version,
            llvm_build_id,
            rt_lto,
            rt_lto_digest,
            pgo_mode,
            unit,
        },
        blob_digest,
    ))
}

// ================================================================================================
// ThinLTO S2: the two cacheable phases (`docs/impl/07-roadmap.md` ThinLTO S2). A `--thin-lto` build
// caches partition PRELINK bitcode (phase 1, `prelink-bitcode` part-kind) and partition BACKEND
// objects (phase 3); `WholeUnit` retains the original per-unit identity. The serial thin-link
// (phase 2) is never cached but always runs, so cross-partition import decisions are recomputed
// fresh every build. Both keys reuse the CAS + manifest discipline above (private staging + atomic
// rename, digest-verified reads, fail-closed decode).
// ================================================================================================

// ---- phase 1: prelink key -----------------------------------------------------------------------

/// Nominal identity of one ThinLTO cache partition. Function identity remains the validated logical
/// MIR name; the unit lives in [`ThinPartitionSource`] so equal consumer monomorphs in different
/// units remain distinct without changing MIR semantics.
#[derive(Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub enum PartitionKey {
    WholeUnit,
    Support,
    Function(ProgramCall),
}

#[derive(Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct ThinPartitionSource {
    pub unit: String,
    pub partition: PartitionKey,
}

#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub struct InboundImport {
    pub source: ThinPartitionSource,
    pub guid: u64,
    pub is_definition: bool,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ImportSourceDigest {
    pub source: ThinPartitionSource,
    pub prelink_digest: Hash128,
}

/// The cache key for one source partition's ThinLTO **prelink bitcode** (phase 1). It is today's codegen key
/// MINUS the pure backend/target codegen knobs (cpu / features / reloc / code model / machine
/// opt-level) — those cannot change the summary-bearing prelink bitcode bytes (the module's
/// datalayout is triple-derived, kept here; the cpu string only steers backend codegen, re-derived in
/// phase 3). Everything that CAN change the prelink `.bc` is present: the unit's own MIR fingerprint,
/// its transitive dep interface hashes, the IR pipeline/opt-level (via profile), the exact LLVM
/// version, the `--rt-lto` merge digest, and the compiler build id.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct PrelinkKey {
    pub cache_format_version: u32,
    pub compiler_build_id: Hash128,
    pub frontend_schema: u32,
    pub located: bool,
    pub impl_hash: Hash128,
    pub dep_interface_hashes: Vec<(String, Hash128)>,
    pub exports: Vec<String>,
    /// Kept (soundness): the triple fixes the module datalayout embedded in the bitcode, so an
    /// x86-64 prelink `.bc` must never be shared with an aarch64 build under the same other inputs.
    pub target_triple: String,
    pub object_format: u8,
    pub profile_name: String,
    pub pipeline: String,
    pub llvm_version: String,
    pub llvm_build_id: Hash128,
    pub rt_lto: bool,
    pub rt_lto_digest: Option<Hash128>,
    pub unit: String,
    pub partition: PartitionKey,
}

impl PrelinkKey {
    pub fn full_digest(&self) -> Hash128 {
        let mut w = Writer::new();
        write_prelink_key(&mut w, self);
        Hash128::of(&w.buf)
    }
    /// Slot pointer for the [`FirstDiff`] diff after an in-place edit — stable core only (phase tag +
    /// cache-format version + compiler build id + unit).
    pub fn slot_digest(&self) -> Hash128 {
        let mut w = Writer::new();
        w.u8(PHASE_TAG_PRELINK);
        w.u32(self.cache_format_version);
        w.h128(self.compiler_build_id);
        w.str(&self.unit);
        w.partition_key(&self.partition);
        Hash128::of(&w.buf)
    }
}

/// First differing component of a decoded prior prelink key vs the fresh one. No cpu/reloc components
/// (excluded from the prelink key); everything else mirrors [`first_diff`]'s priority.
fn prelink_first_diff(stored: &PrelinkKey, current: &PrelinkKey) -> FirstDiff {
    if stored.frontend_schema != current.frontend_schema || stored.located != current.located {
        return FirstDiff::FrontendSchema;
    }
    if stored.llvm_version != current.llvm_version || stored.llvm_build_id != current.llvm_build_id {
        return FirstDiff::LlvmVersion;
    }
    if stored.target_triple != current.target_triple || stored.object_format != current.object_format {
        return FirstDiff::Target;
    }
    if stored.impl_hash != current.impl_hash {
        return FirstDiff::MirDigest;
    }
    if stored.dep_interface_hashes != current.dep_interface_hashes {
        return FirstDiff::DepInterfaceHashes;
    }
    if stored.exports != current.exports {
        return FirstDiff::Exports;
    }
    if stored.profile_name != current.profile_name || stored.pipeline != current.pipeline {
        return FirstDiff::Profile;
    }
    if stored.rt_lto != current.rt_lto || stored.rt_lto_digest != current.rt_lto_digest {
        return FirstDiff::RtLto;
    }
    if stored.cache_format_version != current.cache_format_version {
        return FirstDiff::CacheFormatVersion;
    }
    if stored.compiler_build_id != current.compiler_build_id {
        return FirstDiff::CompilerBuildId;
    }
    FirstDiff::NoPriorEntry
}

// ---- phase 3: backend key -----------------------------------------------------------------------

/// The cache key for one source partition's ThinLTO **backend object** (phase 3) — the PRECISE
/// cross-partition digest.
/// A backend hit must be provably valid for the exact inputs the shim's entry-3 consumes:
///   * `own_prelink_digest` — this unit's prelink `.bc` content (its own code + local promotions);
///   * `inbound_imports` — the edges `(src, GUID, kind)` this unit imports (what gets pulled in);
///   * `outbound_exports` — the GUIDs of THIS unit's values that are referenced cross-module, which
///     drive `renameModuleForThinLTO`'s promotion of the unit's own locals (a leaf that is imported
///     FROM still rewrites its object). Derived from the thin-link export set restricted to this unit;
///   * `import_source_digests` — the prelink `.bc` content digest of every unit this one imports from,
///     so a private-body edit in an import source (which changes the inlined body / promoted symbol
///     names) invalidates the importer's object;
///   * the backend/target bits (cpu / features / reloc / code model / machine opt-level / profile).
///
/// Redundant defensive components (`cache_format_version`, `compiler_build_id`, `llvm_version`,
/// triple / object format) are also present; they are transitively captured by `own_prelink_digest`
/// but pinned explicitly so a backend hit is self-evidently target-consistent.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct BackendKey {
    pub cache_format_version: u32,
    pub compiler_build_id: Hash128,
    pub llvm_version: String,
    pub llvm_build_id: Hash128,
    pub target_triple: String,
    pub object_format: u8,
    pub resolved_cpu: String,
    pub resolved_features: String,
    pub reloc_model: String,
    pub code_model: String,
    pub profile_name: String,
    pub pipeline: String,
    pub codegen_opt: String,
    pub own_prelink_digest: Hash128,
    /// Sorted, deduped inbound import edges.
    pub inbound_imports: Vec<InboundImport>,
    /// Sorted, deduped GUIDs this unit exports cross-module (its promotion set).
    pub outbound_exports: Vec<u64>,
    /// Sorted, deduped source partition/digest records for every imported partition.
    pub import_source_digests: Vec<ImportSourceDigest>,
    /// The `--export` root set (entry unit only; sorted+deduped) — it widens the preserve set.
    pub exports: Vec<String>,
    pub unit: String,
    pub partition: PartitionKey,
}

impl BackendKey {
    pub fn full_digest(&self) -> Hash128 {
        let mut w = Writer::new();
        write_backend_key(&mut w, self);
        Hash128::of(&w.buf)
    }
    pub fn slot_digest(&self) -> Hash128 {
        let mut w = Writer::new();
        w.u8(PHASE_TAG_BACKEND);
        w.u32(self.cache_format_version);
        w.h128(self.compiler_build_id);
        w.str(&self.unit);
        w.partition_key(&self.partition);
        Hash128::of(&w.buf)
    }
}

/// First differing component of a decoded prior backend key vs the fresh one. The target/backend bits
/// come first (they subsume many entries at once), then the unit's own prelink content, then the
/// cross-unit inputs, then the export set.
fn backend_first_diff(stored: &BackendKey, current: &BackendKey) -> FirstDiff {
    if stored.llvm_version != current.llvm_version || stored.llvm_build_id != current.llvm_build_id
    {
        return FirstDiff::LlvmVersion;
    }
    if stored.target_triple != current.target_triple || stored.object_format != current.object_format {
        return FirstDiff::Target;
    }
    if stored.resolved_cpu != current.resolved_cpu || stored.resolved_features != current.resolved_features {
        return FirstDiff::Cpu;
    }
    if stored.reloc_model != current.reloc_model || stored.code_model != current.code_model {
        return FirstDiff::RelocCodeModel;
    }
    if stored.profile_name != current.profile_name
        || stored.pipeline != current.pipeline
        || stored.codegen_opt != current.codegen_opt
    {
        return FirstDiff::Profile;
    }
    if stored.own_prelink_digest != current.own_prelink_digest {
        return FirstDiff::PrelinkInput;
    }
    if stored.inbound_imports != current.inbound_imports
        || stored.import_source_digests != current.import_source_digests
        || stored.outbound_exports != current.outbound_exports
    {
        return FirstDiff::CrossUnitImports;
    }
    if stored.exports != current.exports {
        return FirstDiff::Exports;
    }
    if stored.cache_format_version != current.cache_format_version {
        return FirstDiff::CacheFormatVersion;
    }
    if stored.compiler_build_id != current.compiler_build_id {
        return FirstDiff::CompilerBuildId;
    }
    FirstDiff::NoPriorEntry
}

// ---- ThinLTO phase lookup / publish -------------------------------------------------------------

/// The action-manifest phase discriminator (a leading manifest byte + slot-digest byte), so a prelink
/// manifest can never be mis-decoded as a backend manifest even under a hash collision across the two
/// (independent) action namespaces.
const PHASE_TAG_PRELINK: u8 = 1;
const PHASE_TAG_BACKEND: u8 = 2;

impl CacheContext {
    /// Phase-1 lookup: on an enabled hit, the prelink `.bc` CAS blob is written verbatim to `bc_out`
    /// and [`CacheLookup::Hit`] carries the [`CacheStage::ThinLtoPrelink`] outcome; a miss carries the
    /// first-differing reason (the caller then produces the bitcode and calls [`publish_prelink`]).
    pub fn lookup_prelink(&self, key: &PrelinkKey, bc_out: &Path) -> CacheLookup {
        if !self.codegen_is_enabled() {
            return CacheLookup::Miss { reason: None };
        }
        let root = match self {
            CacheContext::Disabled => return CacheLookup::Miss { reason: None },
            CacheContext::Enabled { primary, .. } => primary,
        };
        let action_path = phase_action_path(root, "prelink", key.full_digest());
        let hit = try_hit_phase(root, &action_path, bc_out, |bytes| {
            deserialize_prelink_manifest(bytes).ok().filter(|(k, _)| k == key).map(|(_, d)| d)
        });
        match hit {
            HitResult::Hit => CacheLookup::Hit(CacheOutcome {
                stage: CacheStage::ThinLtoPrelink,
                unit: key.unit.clone(),
                hit: true,
                miss_reason: None,
            }),
            HitResult::Corrupt => CacheLookup::Miss { reason: Some(FirstDiff::CorruptEntry) },
            HitResult::Miss => CacheLookup::Miss {
                reason: Some(diff_phase_slot(root, "prelink", key.slot_digest(), |bytes| {
                    deserialize_prelink_manifest(bytes).ok().map(|(stored, _)| prelink_first_diff(&stored, key))
                })),
            },
        }
    }

    /// Publish an already-produced prelink `.bc` (best-effort; a cache-write failure never fails an
    /// otherwise-correct build). No-op when disabled. Safe from a worker thread.
    pub fn publish_prelink(&self, key: &PrelinkKey, bc_out: &Path) {
        if !self.codegen_is_enabled() {
            return;
        }
        if let CacheContext::Enabled { primary, .. } = self {
            publish_phase(
                primary,
                &phase_action_path(primary, "prelink", key.full_digest()),
                &phase_slot_path(primary, "prelink", key.slot_digest()),
                bc_out,
                |digest| serialize_prelink_manifest(key, digest),
            );
        }
    }

    /// Phase-3 lookup: on an enabled hit, the object CAS blob is written verbatim to `obj_out` and
    /// [`CacheLookup::Hit`] carries the [`CacheStage::ThinLtoBackend`] outcome; a miss carries the
    /// first-differing reason.
    pub fn lookup_backend(&self, key: &BackendKey, obj_out: &Path) -> CacheLookup {
        if !self.codegen_is_enabled() {
            return CacheLookup::Miss { reason: None };
        }
        let root = match self {
            CacheContext::Disabled => return CacheLookup::Miss { reason: None },
            CacheContext::Enabled { primary, .. } => primary,
        };
        let action_path = phase_action_path(root, "thinbackend", key.full_digest());
        let hit = try_hit_phase(root, &action_path, obj_out, |bytes| {
            deserialize_backend_manifest(bytes).ok().filter(|(k, _)| k == key).map(|(_, d)| d)
        });
        match hit {
            HitResult::Hit => CacheLookup::Hit(CacheOutcome {
                stage: CacheStage::ThinLtoBackend,
                unit: key.unit.clone(),
                hit: true,
                miss_reason: None,
            }),
            HitResult::Corrupt => CacheLookup::Miss { reason: Some(FirstDiff::CorruptEntry) },
            HitResult::Miss => CacheLookup::Miss {
                reason: Some(diff_phase_slot(root, "thinbackend", key.slot_digest(), |bytes| {
                    deserialize_backend_manifest(bytes).ok().map(|(stored, _)| backend_first_diff(&stored, key))
                })),
            },
        }
    }

    /// Publish an already-produced backend object (best-effort). No-op when disabled.
    pub fn publish_backend(&self, key: &BackendKey, obj_out: &Path) {
        if !self.codegen_is_enabled() {
            return;
        }
        if let CacheContext::Enabled { primary, .. } = self {
            publish_phase(
                primary,
                &phase_action_path(primary, "thinbackend", key.full_digest()),
                &phase_slot_path(primary, "thinbackend", key.slot_digest()),
                obj_out,
                |digest| serialize_backend_manifest(key, digest),
            );
        }
    }
}

fn phase_action_path(root: &Path, kind: &str, full: Hash128) -> PathBuf {
    root.join("actions").join(kind).join(full.to_hex())
}

fn phase_slot_path(root: &Path, kind: &str, slot: Hash128) -> PathBuf {
    root.join("index").join(kind).join(slot.to_hex())
}

/// A generic phase hit attempt. `matched_blob` decodes the manifest and returns the blob digest iff
/// the manifest decodes AND its stored key equals the current key; `None` ⇒ a clean miss.
fn try_hit_phase(
    root: &Path,
    action_path: &Path,
    out_path: &Path,
    matched_blob: impl FnOnce(&[u8]) -> Option<Hash128>,
) -> HitResult {
    let manifest_bytes = match read_cache_manifest(action_path) {
        Some(bytes) => bytes,
        None => return HitResult::Miss,
    };
    match matched_blob(&manifest_bytes) {
        Some(blob_digest) => materialize_blob(root, blob_digest, out_path, ReadPolicy::Writable),
        None => HitResult::Miss,
    }
}

/// A generic phase miss reason: read the slot pointer, decode it, and diff against the current key.
fn diff_phase_slot(
    root: &Path,
    kind: &str,
    slot_digest: Hash128,
    diff: impl FnOnce(&[u8]) -> Option<FirstDiff>,
) -> FirstDiff {
    let path = phase_slot_path(root, kind, slot_digest);
    match read_cache_manifest(&path) {
        Some(bytes) => diff(&bytes).unwrap_or(FirstDiff::NoPriorEntry),
        None => FirstDiff::NoPriorEntry,
    }
}

/// Publish a produced phase artifact: CAS blob + full-key action manifest + unit-slot pointer, all
/// best-effort (a populate failure never fails a build whose artifact is already correct on disk).
fn publish_phase(
    root: &Path,
    action_path: &Path,
    slot_path: &Path,
    out_path: &Path,
    make_manifest: impl Fn(Hash128) -> Vec<u8>,
) {
    let bytes = match read_regular_bounded_file(out_path, CACHE_CAS_MAX_BYTES) {
        Some(bytes) => bytes,
        None => {
            eprintln!(
                "alignc: cache not populated (produced artifact is unavailable, non-regular, or exceeds 256 MiB): {}",
                out_path.display()
            );
            return;
        }
    };
    let blob_digest = Hash128::of(&bytes);
    let manifest = make_manifest(blob_digest);
    let result = publish_blob(root, blob_digest, &bytes)
        .and_then(|()| publish_file(action_path, &manifest))
        .and_then(|()| publish_file(slot_path, &manifest));
    if let Err(e) = result {
        eprintln!("alignc: cache not populated: {e}");
    }
}

// ---- ThinLTO manifest codecs (fail-closed, versioned, length-prefixed) --------------------------

impl Writer {
    fn partition_key(&mut self, key: &PartitionKey) {
        match key {
            PartitionKey::WholeUnit => self.u8(0),
            PartitionKey::Support => self.u8(1),
            PartitionKey::Function(function) => {
                self.u8(2);
                self.str(function.as_str());
            }
        }
    }
    fn partition_source(&mut self, source: &ThinPartitionSource) {
        self.str(&source.unit);
        self.partition_key(&source.partition);
    }
    /// A sorted inbound-import sequence with exact source partition identity.
    fn inbound_imports(&mut self, v: &[InboundImport]) {
        self.u32(u32_len(v.len()));
        for import in v {
            self.partition_source(&import.source);
            self.u64(import.guid);
            self.bool(import.is_definition);
        }
    }
    fn u64_seq(&mut self, v: &[u64]) {
        self.u32(u32_len(v.len()));
        for x in v {
            self.u64(*x);
        }
    }
    fn digest_seq(&mut self, v: &[(String, Hash128)]) {
        self.u32(u32_len(v.len()));
        for (name, h) in v {
            self.str(name);
            self.h128(*h);
        }
    }
    fn import_source_digests(&mut self, v: &[ImportSourceDigest]) {
        self.u32(u32_len(v.len()));
        for digest in v {
            self.partition_source(&digest.source);
            self.h128(digest.prelink_digest);
        }
    }
    fn str_seq(&mut self, v: &[String]) {
        self.u32(u32_len(v.len()));
        for s in v {
            self.str(s);
        }
    }
}

fn write_prelink_key(w: &mut Writer, k: &PrelinkKey) {
    w.u8(PHASE_TAG_PRELINK);
    w.u32(k.cache_format_version);
    w.h128(k.compiler_build_id);
    w.u32(k.frontend_schema);
    w.bool(k.located);
    w.h128(k.impl_hash);
    w.digest_seq(&k.dep_interface_hashes);
    w.str_seq(&k.exports);
    w.str(&k.target_triple);
    w.u8(k.object_format);
    w.str(&k.profile_name);
    w.str(&k.pipeline);
    w.str(&k.llvm_version);
    w.h128(k.llvm_build_id);
    w.bool(k.rt_lto);
    w.opt_h128(k.rt_lto_digest);
    w.str(&k.unit);
    w.partition_key(&k.partition);
}

fn write_backend_key(w: &mut Writer, k: &BackendKey) {
    w.u8(PHASE_TAG_BACKEND);
    w.u32(k.cache_format_version);
    w.h128(k.compiler_build_id);
    w.str(&k.llvm_version);
    w.h128(k.llvm_build_id);
    w.str(&k.target_triple);
    w.u8(k.object_format);
    w.str(&k.resolved_cpu);
    w.str(&k.resolved_features);
    w.str(&k.reloc_model);
    w.str(&k.code_model);
    w.str(&k.profile_name);
    w.str(&k.pipeline);
    w.str(&k.codegen_opt);
    w.h128(k.own_prelink_digest);
    w.inbound_imports(&k.inbound_imports);
    w.u64_seq(&k.outbound_exports);
    w.import_source_digests(&k.import_source_digests);
    w.str_seq(&k.exports);
    w.str(&k.unit);
    w.partition_key(&k.partition);
}

fn serialize_prelink_manifest(key: &PrelinkKey, blob_digest: Hash128) -> Vec<u8> {
    let mut w = Writer::new();
    w.u32(MANIFEST_FORMAT_VERSION);
    write_prelink_key(&mut w, key);
    w.h128(blob_digest);
    w.buf
}

fn serialize_backend_manifest(key: &BackendKey, blob_digest: Hash128) -> Vec<u8> {
    let mut w = Writer::new();
    w.u32(MANIFEST_FORMAT_VERSION);
    write_backend_key(&mut w, key);
    w.h128(blob_digest);
    w.buf
}

impl<'a> Reader<'a> {
    fn phase_tag(&mut self, expect: u8) -> Result<(), CacheDecodeError> {
        let tag = self.u8()?;
        if tag == expect {
            Ok(())
        } else {
            Err(CacheDecodeError::BadTag { what: "phase", tag })
        }
    }
    fn partition_key(&mut self) -> Result<PartitionKey, CacheDecodeError> {
        match self.u8()? {
            0 => Ok(PartitionKey::WholeUnit),
            1 => Ok(PartitionKey::Support),
            2 => ProgramCall::try_from_logical(&self.str()?)
                .map(PartitionKey::Function)
                .map_err(|_| CacheDecodeError::SemanticRange),
            tag => Err(CacheDecodeError::BadTag {
                what: "partition",
                tag,
            }),
        }
    }
    fn partition_source(&mut self) -> Result<ThinPartitionSource, CacheDecodeError> {
        Ok(ThinPartitionSource {
            unit: self.str()?,
            partition: self.partition_key()?,
        })
    }
    fn inbound_imports(&mut self) -> Result<Vec<InboundImport>, CacheDecodeError> {
        self.seq(|r| {
            Ok(InboundImport {
                source: r.partition_source()?,
                guid: r.u64()?,
                is_definition: r.bool()?,
            })
        })
    }
    fn u64_seq(&mut self) -> Result<Vec<u64>, CacheDecodeError> {
        self.seq(|r| r.u64())
    }
    fn digest_seq(&mut self) -> Result<Vec<(String, Hash128)>, CacheDecodeError> {
        self.seq(|r| Ok((r.str()?, r.h128()?)))
    }
    fn import_source_digests(&mut self) -> Result<Vec<ImportSourceDigest>, CacheDecodeError> {
        self.seq(|r| {
            Ok(ImportSourceDigest {
                source: r.partition_source()?,
                prelink_digest: r.h128()?,
            })
        })
    }
    fn str_seq(&mut self) -> Result<Vec<String>, CacheDecodeError> {
        self.seq(|r| r.str())
    }
}

fn deserialize_prelink_manifest(bytes: &[u8]) -> Result<(PrelinkKey, Hash128), CacheDecodeError> {
    let mut r = Reader::new(bytes);
    let version = r.u32()?;
    if version != MANIFEST_FORMAT_VERSION {
        return Err(CacheDecodeError::UnknownVersion(version));
    }
    r.phase_tag(PHASE_TAG_PRELINK)?;
    let key = PrelinkKey {
        cache_format_version: r.u32()?,
        compiler_build_id: r.h128()?,
        frontend_schema: r.u32()?,
        located: r.bool()?,
        impl_hash: r.h128()?,
        dep_interface_hashes: r.digest_seq()?,
        exports: r.str_seq()?,
        target_triple: r.str()?,
        object_format: r.u8()?,
        profile_name: r.str()?,
        pipeline: r.str()?,
        llvm_version: r.str()?,
        llvm_build_id: r.h128()?,
        rt_lto: r.bool()?,
        rt_lto_digest: r.opt_h128()?,
        unit: r.str()?,
        partition: r.partition_key()?,
    };
    let blob_digest = r.h128()?;
    r.finish()?;
    Ok((key, blob_digest))
}

fn deserialize_backend_manifest(bytes: &[u8]) -> Result<(BackendKey, Hash128), CacheDecodeError> {
    let mut r = Reader::new(bytes);
    let version = r.u32()?;
    if version != MANIFEST_FORMAT_VERSION {
        return Err(CacheDecodeError::UnknownVersion(version));
    }
    r.phase_tag(PHASE_TAG_BACKEND)?;
    let key = BackendKey {
        cache_format_version: r.u32()?,
        compiler_build_id: r.h128()?,
        llvm_version: r.str()?,
        llvm_build_id: r.h128()?,
        target_triple: r.str()?,
        object_format: r.u8()?,
        resolved_cpu: r.str()?,
        resolved_features: r.str()?,
        reloc_model: r.str()?,
        code_model: r.str()?,
        profile_name: r.str()?,
        pipeline: r.str()?,
        codegen_opt: r.str()?,
        own_prelink_digest: r.h128()?,
        inbound_imports: r.inbound_imports()?,
        outbound_exports: r.u64_seq()?,
        import_source_digests: r.import_source_digests()?,
        exports: r.str_seq()?,
        unit: r.str()?,
        partition: r.partition_key()?,
    };
    let blob_digest = r.h128()?;
    r.finish()?;
    Ok((key, blob_digest))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_key() -> CodegenKey {
        CodegenKey {
            cache_format_version: CACHE_KEY_FORMAT_VERSION,
            compiler_build_id: Hash128 { lo: 1, hi: 2 },
            frontend_schema: 1,
            located: false,
            impl_hash: Hash128 { lo: 3, hi: 4 },
            dep_interface_hashes: vec![("dep".to_string(), Hash128 { lo: 5, hi: 6 })],
            exports: vec!["a".to_string(), "b".to_string()],
            target_triple: "x86_64-unknown-linux-gnu".to_string(),
            object_format: 0,
            resolved_cpu: "x86-64-v2".to_string(),
            resolved_features: String::new(),
            profile_name: "release".to_string(),
            pipeline: "default<O2>".to_string(),
            codegen_opt: "default".to_string(),
            reloc_model: "PIC".to_string(),
            code_model: "Default".to_string(),
            llvm_version: "22.1.8".to_string(),
            llvm_build_id: Hash128 { lo: 7, hi: 8 },
            rt_lto: false,
            rt_lto_digest: None,
            pgo_mode: PgoKey::Off,
            unit: "main".to_string(),
        }
    }

    fn golden_bytes(hex: &str) -> Vec<u8> {
        let compact: String = hex.chars().filter(|c| !c.is_ascii_whitespace()).collect();
        assert_eq!(compact.len() % 2, 0);
        (0..compact.len() / 2)
            .map(|index| {
                u8::from_str_radix(&compact[index * 2..index * 2 + 2], 16).expect("golden hex")
            })
            .collect()
    }

    const CODEGEN_V5_GOLDEN: &str = concat!(
        "05000000",                         // manifest v5
        "05000000",                         // key v5
        "01000000000000000200000000000000", // compiler build
        "01000000",
        "00",                               // frontend schema, located
        "03000000000000000400000000000000", // impl
        "01000000",
        "03000000646570",
        "05000000000000000600000000000000", // deps
        "02000000",
        "0100000061",
        "0100000062", // exports
        "180000007838365f36342d756e6b6e6f776e2d6c696e75782d676e75",
        "00", // target
        "090000007838362d36342d7632",
        "00000000", // cpu, features
        "0700000072656c65617365",
        "0b00000064656661756c743c4f323e", // profile, pipeline
        "0700000064656661756c74",
        "03000000504943",
        "0700000044656661756c74", // backend
        "0600000032322e312e38",
        "07000000000000000800000000000000", // LLVM version/build
        "00",
        "00",
        "00",                               // rt-lto, optional digest, PGO
        "040000006d61696e",                 // unit
        "09000000000000000a00000000000000", // blob
    );

    #[test]
    fn codegen_v5_manifest_golden_is_bidirectional() {
        let expected = golden_bytes(CODEGEN_V5_GOLDEN);
        let key = sample_key();
        let blob = Hash128 { lo: 9, hi: 10 };
        assert_eq!(serialize_manifest(&key, blob), expected);
        assert_eq!(deserialize_manifest(&expected), Ok((key, blob)));
    }

    #[test]
    fn manifest_roundtrips() {
        let key = sample_key();
        let blob = Hash128 { lo: 9, hi: 10 };
        let bytes = serialize_manifest(&key, blob);
        let (dk, db) = deserialize_manifest(&bytes).expect("decode");
        assert_eq!(dk, key);
        assert_eq!(db, blob);
    }

    #[test]
    fn decode_is_fail_closed() {
        // Truncated.
        assert!(deserialize_manifest(&[0, 1, 2]).is_err());
        // Wrong wire version.
        let mut w = Writer::new();
        w.u32(MANIFEST_FORMAT_VERSION + 1);
        assert_eq!(deserialize_manifest(&w.buf), Err(CacheDecodeError::UnknownVersion(MANIFEST_FORMAT_VERSION + 1)));
        // Trailing bytes.
        let key = sample_key();
        let mut bytes = serialize_manifest(&key, Hash128 { lo: 0, hi: 0 });
        bytes.push(0xff);
        assert_eq!(deserialize_manifest(&bytes), Err(CacheDecodeError::TrailingBytes));
        // Garbage never panics.
        for chunk in [&b""[..], &b"\x01"[..], &[0xde, 0xad, 0xbe, 0xef][..]] {
            let _ = deserialize_manifest(chunk);
        }
    }

    #[test]
    fn slot_digest_ignores_diffable_components() {
        let a = sample_key();
        let mut b = a.clone();
        b.impl_hash = Hash128 { lo: 99, hi: 99 };
        b.profile_name = "dev".to_string();
        b.exports.clear();
        b.rt_lto = true;
        // Same slot (stable core unchanged), different full digest.
        assert_eq!(a.slot_digest(), b.slot_digest());
        assert_ne!(a.full_digest(), b.full_digest());
    }

    #[test]
    fn slot_digest_changes_on_stable_core() {
        let a = sample_key();
        let mut b = a.clone();
        b.unit = "other".to_string();
        assert_ne!(a.slot_digest(), b.slot_digest());
    }

    #[test]
    fn first_diff_priority() {
        let base = sample_key();
        // Every namespace-bearing component reports its own stable reason.
        let mut k = base.clone();
        k.frontend_schema += 1;
        assert_eq!(first_diff(&base, &k), FirstDiff::FrontendSchema);
        let mut k = base.clone();
        k.target_triple = "aarch64-unknown-linux-gnu".to_string();
        assert_eq!(first_diff(&base, &k), FirstDiff::Target);
        let mut k = base.clone();
        k.resolved_cpu = "native-cpu".to_string();
        assert_eq!(first_diff(&base, &k), FirstDiff::Cpu);
        let mut k = base.clone();
        k.llvm_version = "23.0.0".to_string();
        assert_eq!(first_diff(&base, &k), FirstDiff::LlvmVersion);
        let mut k = base.clone();
        k.llvm_build_id = Hash128 { lo: 99, hi: 100 };
        assert_eq!(first_diff(&base, &k), FirstDiff::LlvmVersion);
        k.target_triple = "aarch64-unknown-linux-gnu".to_string();
        k.resolved_cpu = "other-cpu".to_string();
        assert_eq!(
            first_diff(&base, &k),
            FirstDiff::LlvmVersion,
            "LLVM identity precedes simultaneous target/cpu differences"
        );
        let mut k = base.clone();
        k.reloc_model = "Static".to_string();
        assert_eq!(first_diff(&base, &k), FirstDiff::RelocCodeModel);
        // Only impl_hash differs → MirDigest.
        let mut k = base.clone();
        k.impl_hash = Hash128 { lo: 42, hi: 42 };
        assert_eq!(first_diff(&base, &k), FirstDiff::MirDigest);
        let mut k = base.clone();
        k.dep_interface_hashes[0].1 = Hash128 { lo: 55, hi: 66 };
        assert_eq!(first_diff(&base, &k), FirstDiff::DepInterfaceHashes);
        // Only profile differs → Profile.
        let mut k = base.clone();
        k.profile_name = "dev".to_string();
        k.pipeline = "default<O0>".to_string();
        k.codegen_opt = "none".to_string();
        assert_eq!(first_diff(&base, &k), FirstDiff::Profile);
        // Only exports differ → Exports.
        let mut k = base.clone();
        k.exports = vec!["z".to_string()];
        assert_eq!(first_diff(&base, &k), FirstDiff::Exports);
        // Only rt-lto differs → RtLto.
        let mut k = base.clone();
        k.rt_lto = true;
        k.rt_lto_digest = Some(Hash128 { lo: 7, hi: 7 });
        assert_eq!(first_diff(&base, &k), FirstDiff::RtLto);
        // Only the PGO mode differs → PgoProfile (each of the three transitions).
        let mut k = base.clone();
        k.pgo_mode = PgoKey::Instrument;
        assert_eq!(first_diff(&base, &k), FirstDiff::PgoProfile);
        let mut k = base.clone();
        k.pgo_mode = PgoKey::Use(Hash128 { lo: 100, hi: 200 });
        assert_eq!(first_diff(&base, &k), FirstDiff::PgoProfile);
        // A profdata bytes edit (same Use variant, different digest) → PgoProfile.
        let mut stored = base.clone();
        stored.pgo_mode = PgoKey::Use(Hash128 { lo: 1, hi: 1 });
        let mut k = base.clone();
        k.pgo_mode = PgoKey::Use(Hash128 { lo: 2, hi: 2 });
        assert_eq!(first_diff(&stored, &k), FirstDiff::PgoProfile);
        let mut k = base.clone();
        k.cache_format_version += 1;
        assert_eq!(first_diff(&base, &k), FirstDiff::CacheFormatVersion);
        let mut k = base.clone();
        k.compiler_build_id = Hash128 { lo: 77, hi: 88 };
        assert_eq!(first_diff(&base, &k), FirstDiff::CompilerBuildId);
        // impl_hash takes priority over a simultaneous exports change.
        let mut k = base.clone();
        k.impl_hash = Hash128 { lo: 1, hi: 1 };
        k.exports = vec!["z".to_string()];
        assert_eq!(first_diff(&base, &k), FirstDiff::MirDigest);
    }

    /// The `Some` arm: real bytes are a real identity, and different bytes are different identities.
    /// Exercised through the production decision function, not a re-implementation of it.
    #[test]
    fn readable_executable_bytes_are_a_real_identity() {
        let (id, available) = build_id_from(Some(b"pretend-compiler-bytes".to_vec()));
        assert!(available, "readable bytes must report a REAL identity");
        assert_eq!(id, Hash128::of(b"pretend-compiler-bytes"), "the id must be the bytes' hash");
        // The whole point of the component: two different compiler builds must not share an id.
        let (other, _) = build_id_from(Some(b"a different compiler build".to_vec()));
        assert_ne!(id, other);
    }

    /// The `None` arm: no shared constant, and the id is unique per computation, so two compilers
    /// that both fail to read their executable can never address one namespace. This is the defect
    /// being fixed — the removed fallback was `Hash128::of("alignc-build-id-fallback-<version>")`,
    /// identical in every build of one crate version.
    #[test]
    fn an_unreadable_executable_yields_a_unique_unavailable_id() {
        let (first, first_available) = build_id_from(None);
        let (second, second_available) = build_id_from(None);
        assert!(!first_available && !second_available, "absent bytes are NOT a real identity");
        assert_ne!(first, second, "the unavailable id must be unique, never a shared constant");
        let banned = Hash128::of(
            format!("alignc-build-id-fallback-{}", env!("CARGO_PKG_VERSION")).as_bytes(),
        );
        assert_ne!(first, banned);
        assert_ne!(second, banned);
    }

    #[test]
    fn an_unidentifiable_compiler_disables_the_cache() {
        let root = std::env::temp_dir().join("align-buildid-fail-closed");
        // The decision is a value, so the note is asserted without stderr capture and without
        // depending on which earlier test consumed the print-once latch.
        assert_eq!(
            decide_enabled(root.clone(), None, false).err(),
            Some(UNIDENTIFIABLE_COMPILER_NOTE),
            "an unidentifiable compiler must yield the note, never an enabled cache"
        );
        assert!(decide_enabled(root.clone(), None, true).is_ok());
        // Both construction seams are wired to that one decision.
        assert!(matches!(
            CacheContext::at_when(root.clone(), || false),
            CacheContext::Disabled
        ));
        assert!(matches!(
            CacheContext::at_when(root, || true),
            CacheContext::Enabled { .. }
        ));
        // And it outranks every `ALIGNC_CACHE` value.
        assert!(matches!(
            CacheContext::from_env_when(|| false),
            CacheContext::Disabled
        ));
    }

    /// The executable read + two hash passes must not be paid by a build that ends up disabled —
    /// the cost contract stated on `is_enabled` and at `emit_object_cached`'s disabled fast path.
    /// Asserted as an invariant over whatever the ambient environment is, so no test has to mutate
    /// `ALIGNC_CACHE` (which would race every other test in this binary).
    #[test]
    fn the_executable_is_probed_only_when_an_enabled_cache_is_produced() {
        let probes = std::cell::Cell::new(0u32);
        let context = CacheContext::from_env_when(|| {
            probes.set(probes.get() + 1);
            true
        });
        match context {
            CacheContext::Enabled { .. } => {
                assert_eq!(probes.get(), 1, "an enabled cache probes the executable exactly once")
            }
            CacheContext::Disabled => {
                assert_eq!(probes.get(), 0, "a disabled cache must not read or hash the executable")
            }
        }
    }

    #[test]
    fn clear_cache_removes_owned_subtrees_and_is_absent_safe() {
        let root = std::env::temp_dir().join(format!("align-clear-{}-{:p}", std::process::id(), &0u8 as *const _));
        let _ = std::fs::remove_dir_all(&root);
        // Absent root: safe, nothing removed.
        assert_eq!(clear_cache(&root), Ok(false));
        // Populate the three owned subtrees + an UNRELATED sibling that must survive.
        for sub in ["cas", "actions", "index"] {
            std::fs::create_dir_all(root.join(sub).join("x")).unwrap();
        }
        std::fs::create_dir_all(root.join("keep")).unwrap();
        std::fs::write(root.join("keep").join("f"), b"x").unwrap();
        std::fs::create_dir_all(root.join("rejected").join("unit")).unwrap();
        std::fs::write(root.join("rejected").join("unit").join("marker"), b"").unwrap();
        assert_eq!(clear_cache(&root), Ok(true));
        assert!(!root.join("cas").exists() && !root.join("actions").exists() && !root.join("index").exists());
        assert!(root.join("keep").join("f").exists(), "clear must not touch anything but its own subtrees");
        assert!(
            root.join("rejected").join("unit").join("marker").exists(),
            "an optimization-state clear cannot reauthorize a rejected key"
        );
        std::fs::remove_dir_all(&root).ok();
    }

    #[cfg(unix)]
    #[test]
    fn clear_cache_never_follows_a_symlinked_subtree() {
        use std::os::unix::fs::symlink;
        let base = std::env::temp_dir().join(format!("align-clearsym-{}-{:p}", std::process::id(), &0u8 as *const _));
        let root = base.join("root");
        let outside = base.join("outside");
        let _ = std::fs::remove_dir_all(&base);
        std::fs::create_dir_all(&root).unwrap();
        std::fs::create_dir_all(&outside).unwrap();
        let victim = outside.join("important");
        std::fs::write(&victim, b"do not delete").unwrap();
        // Replace the `cas` entry with a symlink pointing OUT of the cache root.
        symlink(&outside, root.join("cas")).unwrap();
        assert_eq!(clear_cache(&root), Ok(true));
        // The symlink is gone, but its target's contents are untouched (never followed out of root).
        assert!(!root.join("cas").exists(), "the symlink entry itself is removed");
        assert!(victim.exists(), "clear must NEVER delete through a symlink out of the resolved root");
        std::fs::remove_dir_all(&base).ok();
    }

    fn fallback_context(tag: &str) -> (std::path::PathBuf, std::path::PathBuf, CacheContext) {
        let base = std::env::temp_dir().join(format!(
            "align-fallback-{tag}-{}-{}",
            std::process::id(),
            STAGE_NONCE.fetch_add(1, Ordering::Relaxed)
        ));
        let primary = base.join("primary");
        let packaged = base.join("packaged");
        let context = decide_enabled(primary.clone(), Some(packaged.clone()), true).unwrap();
        (primary, packaged, context)
    }

    #[test]
    fn cache_file_bounds_accept_the_exact_limit_and_reject_the_next_byte() {
        let root = std::env::temp_dir().join(format!(
            "align-cache-bounds-{}-{}",
            std::process::id(),
            STAGE_NONCE.fetch_add(1, Ordering::Relaxed)
        ));
        std::fs::create_dir_all(&root).unwrap();
        let path = root.join("sparse");
        let file = std::fs::File::create(&path).unwrap();

        for limit in [CACHE_MANIFEST_MAX_BYTES, CACHE_CAS_MAX_BYTES] {
            file.set_len(limit).unwrap();
            let (_, observed) = open_regular_bounded(&path, Some(limit)).expect("exact limit");
            assert_eq!(u64::try_from(observed).unwrap(), limit);
            file.set_len(limit + 1).unwrap();
            assert!(open_regular_bounded(&path, Some(limit)).is_none(), "limit={limit}");
        }
        std::fs::remove_dir_all(root).ok();
    }

    #[test]
    fn cache_manifest_read_requires_metadata_and_stream_lengths_to_agree() {
        assert_eq!(
            read_cache_manifest_from(std::io::Cursor::new(b"abc"), 3),
            Some(b"abc".to_vec())
        );
        assert!(read_cache_manifest_from(std::io::Cursor::new(b"abc"), 4).is_none());
        assert!(read_cache_manifest_from(std::io::Cursor::new(b"abc"), 2).is_none());
    }

    #[test]
    fn private_materialization_is_removed_on_rename_failure_and_unwind() {
        let root = std::env::temp_dir().join(format!(
            "align-cache-stage-{}-{}",
            std::process::id(),
            STAGE_NONCE.fetch_add(1, Ordering::Relaxed)
        ));
        std::fs::create_dir_all(&root).unwrap();

        let bytes = b"valid-object";
        let digest = Hash128::of(bytes);
        let blob = cas_blob_path(&root, digest);
        std::fs::create_dir_all(blob.parent().unwrap()).unwrap();
        std::fs::write(&blob, bytes).unwrap();
        let output = root.join("output-directory");
        std::fs::create_dir(&output).unwrap();
        assert!(matches!(
            materialize_blob(&root, digest, &output, ReadPolicy::Packaged),
            HitResult::Miss
        ));
        assert!(output.is_dir());
        assert!(std::fs::read_dir(&root).unwrap().all(|entry| !entry
            .unwrap()
            .file_name()
            .to_string_lossy()
            .starts_with(".cache-stage-")));

        let unwind_path = root.join("unwind-stage");
        let result = std::panic::catch_unwind(|| {
            std::fs::write(&unwind_path, b"partial").unwrap();
            let _stage = MaterializedStage { path: unwind_path.clone(), committed: false };
            panic!("test unwind");
        });
        assert!(result.is_err());
        assert!(!unwind_path.exists());
        std::fs::remove_dir_all(root).ok();
    }

    #[cfg(unix)]
    #[test]
    fn cache_manifest_open_rejects_fifo_and_device_targets_without_reading() {
        let root = std::env::temp_dir().join(format!(
            "align-cache-special-{}-{}",
            std::process::id(),
            STAGE_NONCE.fetch_add(1, Ordering::Relaxed)
        ));
        std::fs::create_dir_all(&root).unwrap();
        let fifo = root.join("fifo");
        assert!(std::process::Command::new("mkfifo").arg(&fifo).status().unwrap().success());
        assert!(read_cache_manifest(&fifo).is_none());
        assert!(read_cache_manifest(Path::new("/dev/zero")).is_none());
        std::fs::remove_dir_all(root).ok();
    }

    #[test]
    fn codegen_fallback_hits_without_promotion_or_production() {
        let (primary, packaged, cache) = fallback_context("hit");
        let key = sample_key();
        let source = packaged.join("produced.o");
        std::fs::create_dir_all(&packaged).unwrap();
        std::fs::write(&source, b"packaged-object").unwrap();
        publish(&packaged, &key, &source);

        let output = packaged.join("materialized.o");
        let produced = std::cell::Cell::new(0usize);
        let outcome = cache
            .codegen(&key, &output, |_| {
                produced.set(produced.get() + 1);
                Ok(())
            })
            .unwrap();
        assert!(outcome.hit);
        assert_eq!(produced.get(), 0);
        assert_eq!(std::fs::read(&output).unwrap(), b"packaged-object");
        assert!(!primary.exists(), "a packaged hit is never promoted");
        std::fs::remove_dir_all(primary.parent().unwrap()).ok();
    }

    #[test]
    fn writable_hit_wins_and_packaged_survives_primary_corruption() {
        let (primary, packaged, cache) = fallback_context("precedence");
        let key = sample_key();
        std::fs::create_dir_all(&primary).unwrap();
        std::fs::create_dir_all(&packaged).unwrap();
        let primary_source = primary.join("primary.o");
        let packaged_source = packaged.join("packaged.o");
        std::fs::write(&primary_source, b"primary-object").unwrap();
        std::fs::write(&packaged_source, b"packaged-object").unwrap();
        publish(&primary, &key, &primary_source);
        publish(&packaged, &key, &packaged_source);

        let output = primary.join("out.o");
        assert!(matches!(cache.lookup(&key, &output), CacheLookup::Hit(_)));
        assert_eq!(std::fs::read(&output).unwrap(), b"primary-object");

        let primary_blob = cas_blob_path(&primary, Hash128::of(b"primary-object"));
        std::fs::write(&primary_blob, b"damaged").unwrap();
        assert!(matches!(cache.lookup(&key, &output), CacheLookup::Hit(_)));
        assert_eq!(std::fs::read(&output).unwrap(), b"packaged-object");
        assert!(
            !primary_blob.exists(),
            "writable corruption self-heals by unlinking"
        );
        assert_eq!(std::fs::read(&packaged_source).unwrap(), b"packaged-object");
        std::fs::remove_dir_all(primary.parent().unwrap()).ok();
    }

    #[test]
    fn packaged_corruption_is_immutable_and_a_miss_publishes_only_primary() {
        let (primary, packaged, cache) = fallback_context("corrupt");
        let key = sample_key();
        std::fs::create_dir_all(&packaged).unwrap();
        let source = packaged.join("source.o");
        std::fs::write(&source, b"packaged-object").unwrap();
        publish(&packaged, &key, &source);
        let blob = cas_blob_path(&packaged, Hash128::of(b"packaged-object"));
        std::fs::write(&blob, b"damaged-but-immutable").unwrap();
        let before = std::fs::read(&blob).unwrap();

        let output = primary.join("out.o");
        std::fs::create_dir_all(&primary).unwrap();
        let outcome = cache
            .codegen(&key, &output, |path| {
                std::fs::write(path, b"rebuilt-object").map_err(|error| error.to_string())
            })
            .unwrap();
        assert!(!outcome.hit);
        assert_eq!(outcome.miss_reason, Some(FirstDiff::CorruptEntry));
        assert_eq!(std::fs::read(&blob).unwrap(), before);
        assert!(action_manifest_path(&primary, key.full_digest()).is_file());
        assert_eq!(std::fs::read(&output).unwrap(), b"rebuilt-object");
        assert!(
            std::fs::read_dir(&primary)
                .unwrap()
                .all(|entry| !entry.unwrap().file_name().to_string_lossy().starts_with(".cache-stage-")),
            "a rejected streamed object must leave no private materialization"
        );
        std::fs::remove_dir_all(primary.parent().unwrap()).ok();
    }

    #[test]
    fn oversized_packaged_object_is_a_clean_miss_without_materialization() {
        let (primary, packaged, cache) = fallback_context("oversized-object");
        let key = sample_key();
        std::fs::create_dir_all(&packaged).unwrap();
        let source = packaged.join("source.o");
        std::fs::write(&source, b"packaged-object").unwrap();
        publish(&packaged, &key, &source);
        let blob = cas_blob_path(&packaged, Hash128::of(b"packaged-object"));
        std::fs::OpenOptions::new()
            .write(true)
            .open(&blob)
            .unwrap()
            .set_len(CACHE_CAS_MAX_BYTES + 1)
            .unwrap();

        std::fs::create_dir_all(&primary).unwrap();
        let output = primary.join("out.o");
        assert!(matches!(
            cache.lookup(&key, &output),
            CacheLookup::Miss {
                reason: Some(FirstDiff::NoPriorEntry)
            }
        ));
        assert!(!output.exists());
        assert_eq!(std::fs::metadata(&blob).unwrap().len(), CACHE_CAS_MAX_BYTES + 1);
        assert!(
            std::fs::read_dir(&primary)
                .unwrap()
                .all(|entry| !entry.unwrap().file_name().to_string_lossy().starts_with(".cache-stage-"))
        );
        std::fs::remove_dir_all(primary.parent().unwrap()).ok();
    }

    #[test]
    fn oversized_produced_object_is_not_published() {
        let (primary, _, _) = fallback_context("oversized-publication");
        let key = sample_key();
        std::fs::create_dir_all(&primary).unwrap();
        let source = primary.join("oversized.o");
        std::fs::File::create(&source)
            .unwrap()
            .set_len(CACHE_CAS_MAX_BYTES + 1)
            .unwrap();
        publish(&primary, &key, &source);
        assert!(!action_manifest_path(&primary, key.full_digest()).exists());
        assert!(!primary.join("cas").exists());
        assert_eq!(std::fs::metadata(source).unwrap().len(), CACHE_CAS_MAX_BYTES + 1);
        std::fs::remove_dir_all(primary.parent().unwrap()).ok();
    }

    #[test]
    fn oversized_writable_blob_is_removed_republished_and_hits_next_time() {
        let (primary, _, cache) = fallback_context("oversized-writable");
        let key = sample_key();
        std::fs::create_dir_all(&primary).unwrap();
        let source = primary.join("source.o");
        std::fs::write(&source, b"stable-object").unwrap();
        publish(&primary, &key, &source);
        let blob = cas_blob_path(&primary, Hash128::of(b"stable-object"));
        std::fs::OpenOptions::new()
            .write(true)
            .open(&blob)
            .unwrap()
            .set_len(CACHE_CAS_MAX_BYTES + 1)
            .unwrap();

        let output = primary.join("out.o");
        let outcome = cache
            .codegen(&key, &output, |path| {
                std::fs::write(path, b"stable-object").map_err(|error| error.to_string())
            })
            .unwrap();
        assert!(!outcome.hit);
        assert_eq!(outcome.miss_reason, Some(FirstDiff::CorruptEntry));
        assert_eq!(std::fs::read(&blob).unwrap(), b"stable-object");

        std::fs::remove_file(&output).unwrap();
        assert!(matches!(cache.lookup(&key, &output), CacheLookup::Hit(_)));
        assert_eq!(std::fs::read(output).unwrap(), b"stable-object");
        std::fs::remove_dir_all(primary.parent().unwrap()).ok();
    }

    #[test]
    fn unavailable_packaged_blob_is_a_clean_miss_and_remains_immutable() {
        let (primary, packaged, cache) = fallback_context("missing-blob");
        let key = sample_key();
        std::fs::create_dir_all(&packaged).unwrap();
        let source = packaged.join("source.o");
        std::fs::write(&source, b"packaged-object").unwrap();
        publish(&packaged, &key, &source);
        let action = action_manifest_path(&packaged, key.full_digest());
        let action_before = std::fs::read(&action).unwrap();
        let blob = cas_blob_path(&packaged, Hash128::of(b"packaged-object"));
        std::fs::remove_file(&blob).unwrap();

        std::fs::create_dir_all(&primary).unwrap();
        let output = primary.join("out.o");
        let outcome = cache
            .codegen(&key, &output, |path| {
                std::fs::write(path, b"rebuilt-object").map_err(|error| error.to_string())
            })
            .unwrap();
        assert!(!outcome.hit);
        assert_eq!(outcome.miss_reason, Some(FirstDiff::NoPriorEntry));
        assert_eq!(std::fs::read(&action).unwrap(), action_before);
        assert!(!blob.exists(), "a missing packaged blob stays missing");
        assert!(action_manifest_path(&primary, key.full_digest()).is_file());
        std::fs::remove_dir_all(primary.parent().unwrap()).ok();
    }

    #[test]
    fn primary_and_packaged_corruption_unlinks_only_the_writable_blob() {
        let (primary, packaged, cache) = fallback_context("double-corrupt");
        let key = sample_key();
        std::fs::create_dir_all(&primary).unwrap();
        std::fs::create_dir_all(&packaged).unwrap();
        let primary_source = primary.join("primary.o");
        let packaged_source = packaged.join("packaged.o");
        std::fs::write(&primary_source, b"primary-object").unwrap();
        std::fs::write(&packaged_source, b"packaged-object").unwrap();
        publish(&primary, &key, &primary_source);
        publish(&packaged, &key, &packaged_source);

        let primary_blob = cas_blob_path(&primary, Hash128::of(b"primary-object"));
        let packaged_blob = cas_blob_path(&packaged, Hash128::of(b"packaged-object"));
        std::fs::write(&primary_blob, b"damaged-primary").unwrap();
        std::fs::write(&packaged_blob, b"damaged-packaged").unwrap();
        let packaged_before = std::fs::read(&packaged_blob).unwrap();
        let output = primary.join("out.o");

        assert!(matches!(
            cache.lookup(&key, &output),
            CacheLookup::Miss {
                reason: Some(FirstDiff::CorruptEntry)
            }
        ));
        assert!(!primary_blob.exists(), "writable corruption is unlinked");
        assert_eq!(
            std::fs::read(&packaged_blob).unwrap(),
            packaged_before,
            "packaged corruption remains immutable"
        );
        std::fs::remove_dir_all(primary.parent().unwrap()).ok();
    }

    #[cfg(unix)]
    #[test]
    fn packaged_lookup_accepts_installer_symlinks_at_exact_manifest_and_blob_paths() {
        use std::os::unix::fs::symlink;

        let (primary, packaged, cache) = fallback_context("exact-symlinks");
        let source_root = primary.parent().unwrap().join("installer-owned");
        let key = sample_key();
        std::fs::create_dir_all(&source_root).unwrap();
        let source = source_root.join("source.o");
        std::fs::write(&source, b"linked-packaged-object").unwrap();
        publish(&source_root, &key, &source);

        let source_action = action_manifest_path(&source_root, key.full_digest());
        let packaged_action = action_manifest_path(&packaged, key.full_digest());
        std::fs::create_dir_all(packaged_action.parent().unwrap()).unwrap();
        symlink(&source_action, &packaged_action).unwrap();
        let digest = Hash128::of(b"linked-packaged-object");
        let source_blob = cas_blob_path(&source_root, digest);
        let packaged_blob = cas_blob_path(&packaged, digest);
        std::fs::create_dir_all(packaged_blob.parent().unwrap()).unwrap();
        symlink(&source_blob, &packaged_blob).unwrap();

        std::fs::create_dir_all(&primary).unwrap();
        let output = primary.join("out.o");
        assert!(matches!(cache.lookup(&key, &output), CacheLookup::Hit(_)));
        assert_eq!(std::fs::read(&output).unwrap(), b"linked-packaged-object");
        assert!(packaged_action.is_symlink());
        assert!(packaged_blob.is_symlink());
        std::fs::remove_dir_all(primary.parent().unwrap()).ok();
    }

    #[test]
    fn miss_diagnostics_prefer_writable_slot_then_packaged_slot() {
        let (primary, packaged, cache) = fallback_context("diff");
        let current = sample_key();
        let mut primary_prior = current.clone();
        primary_prior.impl_hash = Hash128 { lo: 90, hi: 91 };
        let mut packaged_prior = current.clone();
        packaged_prior.target_triple = "aarch64-unknown-linux-gnu".to_string();
        for (root, key) in [(&primary, &primary_prior), (&packaged, &packaged_prior)] {
            std::fs::create_dir_all(root).unwrap();
            let source = root.join("source.o");
            std::fs::write(&source, b"prior").unwrap();
            publish(root, key, &source);
        }
        let output = primary.join("out.o");
        assert!(matches!(
            cache.lookup(&current, &output),
            CacheLookup::Miss {
                reason: Some(FirstDiff::MirDigest)
            }
        ));
        std::fs::remove_dir_all(primary.parent().unwrap()).ok();
    }

    // ---- ThinLTO S2 key codecs + first-diff -----------------------------------------------------

    fn program_call(name: &str) -> ProgramCall {
        ProgramCall::try_from_logical(name).expect("valid test function identity")
    }

    fn sample_prelink_key() -> PrelinkKey {
        PrelinkKey {
            cache_format_version: CACHE_KEY_FORMAT_VERSION,
            compiler_build_id: Hash128 { lo: 1, hi: 2 },
            frontend_schema: 3,
            located: false,
            impl_hash: Hash128 { lo: 4, hi: 5 },
            dep_interface_hashes: vec![("dep".to_string(), Hash128 { lo: 6, hi: 7 })],
            exports: vec!["a".to_string()],
            target_triple: "x86_64-unknown-linux-gnu".to_string(),
            object_format: 0,
            profile_name: "release".to_string(),
            pipeline: "default<O2>".to_string(),
            llvm_version: "22.1.8".to_string(),
            llvm_build_id: Hash128 { lo: 12, hi: 13 },
            rt_lto: false,
            rt_lto_digest: None,
            unit: "main".to_string(),
            partition: PartitionKey::Function(program_call("f")),
        }
    }

    fn sample_backend_key() -> BackendKey {
        BackendKey {
            cache_format_version: CACHE_KEY_FORMAT_VERSION,
            compiler_build_id: Hash128 { lo: 1, hi: 2 },
            llvm_version: "22.1.8".to_string(),
            llvm_build_id: Hash128 { lo: 12, hi: 13 },
            target_triple: "x86_64-unknown-linux-gnu".to_string(),
            object_format: 0,
            resolved_cpu: "x86-64-v2".to_string(),
            resolved_features: String::new(),
            reloc_model: "PIC".to_string(),
            code_model: "Default".to_string(),
            profile_name: "release".to_string(),
            pipeline: "default<O2>".to_string(),
            codegen_opt: "default".to_string(),
            own_prelink_digest: Hash128 { lo: 8, hi: 9 },
            inbound_imports: vec![InboundImport {
                source: ThinPartitionSource {
                    unit: "lib".to_string(),
                    partition: PartitionKey::Function(program_call("callee")),
                },
                guid: 42,
                is_definition: true,
            }],
            outbound_exports: vec![7, 11],
            import_source_digests: vec![ImportSourceDigest {
                source: ThinPartitionSource {
                    unit: "lib".to_string(),
                    partition: PartitionKey::Function(program_call("callee")),
                },
                prelink_digest: Hash128 { lo: 10, hi: 11 },
            }],
            exports: Vec::new(),
            unit: "main".to_string(),
            partition: PartitionKey::Function(program_call("caller")),
        }
    }

    const PRELINK_V5_GOLDEN: &str = concat!(
        "05000000",
        "01",
        "05000000", // manifest, phase, key
        "01000000000000000200000000000000",
        "03000000",
        "00",                               // compiler, frontend, located
        "04000000000000000500000000000000", // impl
        "01000000",
        "03000000646570",
        "06000000000000000700000000000000", // deps
        "01000000",
        "0100000061", // exports
        "180000007838365f36342d756e6b6e6f776e2d6c696e75782d676e75",
        "00", // target
        "0700000072656c65617365",
        "0b00000064656661756c743c4f323e", // profile, pipeline
        "0600000032322e312e38",
        "0c000000000000000d00000000000000", // LLVM
        "00",
        "00",
        "040000006d61696e",                 // rt-lto, optional, unit
        "02",
        "0100000066",                       // function partition `f`
        "63000000000000006400000000000000", // blob
    );

    const BACKEND_V5_GOLDEN: &str = concat!(
        "05000000",
        "02",
        "05000000",                         // manifest, phase, key
        "01000000000000000200000000000000", // compiler
        "0600000032322e312e38",
        "0c000000000000000d00000000000000", // LLVM
        "180000007838365f36342d756e6b6e6f776e2d6c696e75782d676e75",
        "00", // target
        "090000007838362d36342d7632",
        "00000000", // cpu, features
        "03000000504943",
        "0700000044656661756c74", // reloc, code model
        "0700000072656c65617365",
        "0b00000064656661756c743c4f323e",   // profile, pipeline
        "0700000064656661756c74",           // codegen opt
        "08000000000000000900000000000000", // own prelink
        "01000000",
        "030000006c6962",
        "02",
        "0600000063616c6c6565",
        "2a00000000000000",
        "01", // inbound source partition, guid, definition
        "02000000",
        "0700000000000000",
        "0b00000000000000", // outbound
        "01000000",
        "030000006c6962",
        "02",
        "0600000063616c6c6565",
        "0a000000000000000b00000000000000", // imports
        "00000000",
        "040000006d61696e",                 // exports, unit
        "02",
        "0600000063616c6c6572",             // function partition `caller`
        "05000000000000000600000000000000", // blob
    );

    #[test]
    fn thin_codegen_v5_manifest_goldens_are_bidirectional() {
        let prelink = golden_bytes(PRELINK_V5_GOLDEN);
        let prelink_key = sample_prelink_key();
        let prelink_blob = Hash128 { lo: 99, hi: 100 };
        assert_eq!(
            serialize_prelink_manifest(&prelink_key, prelink_blob),
            prelink
        );
        assert_eq!(
            deserialize_prelink_manifest(&prelink),
            Ok((prelink_key, prelink_blob))
        );

        let backend = golden_bytes(BACKEND_V5_GOLDEN);
        let backend_key = sample_backend_key();
        let backend_blob = Hash128 { lo: 5, hi: 6 };
        assert_eq!(
            serialize_backend_manifest(&backend_key, backend_blob),
            backend
        );
        assert_eq!(
            deserialize_backend_manifest(&backend),
            Ok((backend_key, backend_blob))
        );
    }

    #[test]
    fn prelink_manifest_roundtrips_and_is_fail_closed() {
        let key = sample_prelink_key();
        let blob = Hash128 { lo: 99, hi: 100 };
        let bytes = serialize_prelink_manifest(&key, blob);
        let (dk, db) = deserialize_prelink_manifest(&bytes).expect("decode");
        assert_eq!(dk, key);
        assert_eq!(db, blob);
        // Trailing bytes, truncation, wrong version, and a backend manifest all fail closed.
        let mut trailing = bytes.clone();
        trailing.push(0xff);
        assert_eq!(deserialize_prelink_manifest(&trailing), Err(CacheDecodeError::TrailingBytes));
        assert!(deserialize_prelink_manifest(&bytes[..bytes.len() - 1]).is_err());
        // A backend manifest must NOT decode as a prelink manifest (phase tag guard).
        let backend_bytes = serialize_backend_manifest(&sample_backend_key(), blob);
        assert!(deserialize_prelink_manifest(&backend_bytes).is_err());
        for chunk in [&b""[..], &b"\x01"[..], &[0xde, 0xad][..]] {
            let _ = deserialize_prelink_manifest(chunk);
        }
    }

    #[test]
    fn backend_manifest_roundtrips_and_is_fail_closed() {
        let key = sample_backend_key();
        let blob = Hash128 { lo: 5, hi: 6 };
        let bytes = serialize_backend_manifest(&key, blob);
        let (dk, db) = deserialize_backend_manifest(&bytes).expect("decode");
        assert_eq!(dk, key);
        assert_eq!(db, blob);
        let mut trailing = bytes.clone();
        trailing.push(0);
        assert_eq!(deserialize_backend_manifest(&trailing), Err(CacheDecodeError::TrailingBytes));
        // A prelink manifest must NOT decode as a backend manifest.
        let prelink_bytes = serialize_prelink_manifest(&sample_prelink_key(), blob);
        assert!(deserialize_backend_manifest(&prelink_bytes).is_err());
    }

    #[test]
    fn prelink_and_backend_slots_never_collide() {
        // Same unit + core, different phase tag → distinct slot digests (the two namespaces stay
        // separate even in a shared index directory).
        let p = sample_prelink_key();
        let b = sample_backend_key();
        assert_ne!(p.slot_digest(), b.slot_digest());
        // Diffable components do not move the slot; the unit does.
        let mut p2 = p.clone();
        p2.impl_hash = Hash128 { lo: 0, hi: 0 };
        p2.profile_name = "fast".to_string();
        assert_eq!(p.slot_digest(), p2.slot_digest());
        assert_ne!(p.full_digest(), p2.full_digest());
        let mut p3 = p.clone();
        p3.unit = "other".to_string();
        assert_ne!(p.slot_digest(), p3.slot_digest());
    }

    #[test]
    fn prelink_first_diff_priority() {
        let base = sample_prelink_key();
        let mut k = base.clone();
        k.impl_hash = Hash128 { lo: 77, hi: 77 };
        assert_eq!(prelink_first_diff(&base, &k), FirstDiff::MirDigest);
        let mut k = base.clone();
        k.dep_interface_hashes[0].1 = Hash128 { lo: 1, hi: 1 };
        assert_eq!(prelink_first_diff(&base, &k), FirstDiff::DepInterfaceHashes);
        let mut k = base.clone();
        k.profile_name = "fast".to_string();
        k.pipeline = "default<O3>".to_string();
        assert_eq!(prelink_first_diff(&base, &k), FirstDiff::Profile);
        let mut k = base.clone();
        k.target_triple = "aarch64-unknown-linux-gnu".to_string();
        assert_eq!(prelink_first_diff(&base, &k), FirstDiff::Target);
        k.llvm_build_id = Hash128 { lo: 99, hi: 100 };
        assert_eq!(
            prelink_first_diff(&base, &k),
            FirstDiff::LlvmVersion,
            "LLVM identity precedes a simultaneous target difference"
        );
        // impl_hash wins over a simultaneous dep change.
        let mut k = base.clone();
        k.impl_hash = Hash128 { lo: 2, hi: 2 };
        k.dep_interface_hashes[0].1 = Hash128 { lo: 3, hi: 3 };
        assert_eq!(prelink_first_diff(&base, &k), FirstDiff::MirDigest);
    }

    #[test]
    fn backend_first_diff_priority() {
        let base = sample_backend_key();
        // Own prelink digest changed (own code) beats cross-unit.
        let mut k = base.clone();
        k.own_prelink_digest = Hash128 { lo: 0, hi: 0 };
        assert_eq!(backend_first_diff(&base, &k), FirstDiff::PrelinkInput);
        // Import-source digest changed (a dep private edit) with own prelink unchanged → CrossUnitImports.
        let mut k = base.clone();
        k.import_source_digests[0].prelink_digest = Hash128 { lo: 0, hi: 0 };
        assert_eq!(backend_first_diff(&base, &k), FirstDiff::CrossUnitImports);
        // Inbound import edge changed → CrossUnitImports.
        let mut k = base.clone();
        k.inbound_imports[0].is_definition = false;
        assert_eq!(backend_first_diff(&base, &k), FirstDiff::CrossUnitImports);
        // Outbound export set changed → CrossUnitImports.
        let mut k = base.clone();
        k.outbound_exports = vec![7];
        assert_eq!(backend_first_diff(&base, &k), FirstDiff::CrossUnitImports);
        // Pure backend bits.
        let mut k = base.clone();
        k.resolved_cpu = "native-cpu".to_string();
        assert_eq!(backend_first_diff(&base, &k), FirstDiff::Cpu);
        let mut k = base.clone();
        k.codegen_opt = "aggressive".to_string();
        assert_eq!(backend_first_diff(&base, &k), FirstDiff::Profile);
        // A backend-bit diff outranks a simultaneous cross-unit diff.
        let mut k = base.clone();
        k.resolved_cpu = "z".to_string();
        k.own_prelink_digest = Hash128 { lo: 0, hi: 0 };
        assert_eq!(backend_first_diff(&base, &k), FirstDiff::Cpu);
        k.llvm_build_id = Hash128 { lo: 99, hi: 100 };
        assert_eq!(
            backend_first_diff(&base, &k),
            FirstDiff::LlvmVersion,
            "LLVM identity precedes simultaneous backend/source differences"
        );
    }
}
