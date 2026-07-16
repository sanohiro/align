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

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::OnceLock;

use align_interface::Hash128;

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
pub const CACHE_KEY_FORMAT_VERSION: u32 = 2;

/// The manifest wire-format version. Bump on ANY change to the encoded byte layout; an old manifest
/// then fails closed on decode (treated as a miss, its bytes unreferenced).
const MANIFEST_FORMAT_VERSION: u32 = 1;

/// The stderr note emitted (always on, per doc-10 §6.4 fail-closed matrix) when a cache blob fails its
/// digest check and is discarded before a rebuild.
const CORRUPT_NOTE: &str = "alignc: cache entry corrupt; rebuilding";

/// A read cap for untrusted length-prefixed sequences: pre-allocate at most this many elements up
/// front (mirrors `align_interface::codec`'s `n.min(1024)` guard), so a garbage/huge length cannot
/// drive an allocation bomb — the real bytes still have to be present to grow past it.
const SEQ_PREALLOC_CAP: usize = 1024;

// ---- key ----------------------------------------------------------------------------------------

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
    /// #4 the unit's `impl_hash` (stable, location-free MIR fingerprint).
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
    /// #11 rt-lto mode.
    pub rt_lto: bool,
    /// #11 (cont.) merged runtime-bitcode digest (present iff `rt_lto`).
    pub rt_lto_digest: Option<Hash128>,
    /// #12 the (empty-in-v1) cross-unit-opt digest.
    pub cross_unit_opt_digest: Vec<u8>,
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
}

/// The first key component (in a fixed priority order) that differs between a decoded prior manifest
/// and the fresh key — the structured miss reason (doc-10 §6.5). `tests assert this enum, never
/// elapsed time. `NoPriorEntry` = no slot pointer existed to diff against; `CorruptEntry` = a stored
/// blob failed its digest check (a rebuild-triggering corruption, not a component diff).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
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
    Exports,
    Profile,
    RtLto,
    CrossUnitOpt,
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
            FirstDiff::LlvmVersion => "llvm version",
            FirstDiff::RelocCodeModel => "reloc/code model",
            FirstDiff::MirDigest => "implementation changed",
            FirstDiff::DepInterfaceHashes => "dependency interface changed",
            FirstDiff::Exports => "export set",
            FirstDiff::Profile => "profile",
            FirstDiff::RtLto => "rt-lto mode",
            FirstDiff::CrossUnitOpt => "cross-unit-opt",
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
    if stored.target_triple != current.target_triple || stored.object_format != current.object_format {
        return FirstDiff::Target;
    }
    if stored.resolved_cpu != current.resolved_cpu || stored.resolved_features != current.resolved_features {
        return FirstDiff::Cpu;
    }
    if stored.llvm_version != current.llvm_version {
        return FirstDiff::LlvmVersion;
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
    if stored.cross_unit_opt_digest != current.cross_unit_opt_digest {
        return FirstDiff::CrossUnitOpt;
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
pub enum CacheStage {
    Codegen,
    ThinLtoPrelink,
    ThinLtoBackend,
}

impl CacheStage {
    /// A short label for the `--cache-stats` surface. `Codegen` prints empty (the single-phase
    /// format `<unit> hit` is unchanged); the ThinLTO phases print their name.
    pub fn label(self) -> &'static str {
        match self {
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
    Enabled { root: PathBuf },
}

impl CacheContext {
    /// Resolve the cache from `ALIGNC_CACHE` (doc-10 §6.1). **Default-ON (M15 S3b): unset ⇒ ENABLED**
    /// at `${XDG_CACHE_HOME:-~/.cache}/alignc/<schema>` (same as `on`). `off` (or an empty value) ⇒
    /// disabled — the operability hatch, not a compat shim. Any other value ⇒ that path used as the
    /// root verbatim (schema skew inside a shared root is handled by the fail-closed key/manifest
    /// versions). If the default root cannot be resolved (no `HOME`/`XDG_CACHE_HOME`), the on/unset
    /// case degrades to disabled rather than guessing a root.
    pub fn from_env() -> CacheContext {
        // Fail-closed measurement guard: the `ALIGN_SORT_ADAPTIVE` toggle (doc-12 §4.1) changes
        // emitted codegen for `.sort()`/`.sort_by_key()` units. Its effect already flows into the
        // per-unit `impl_hash` (it is read only in MIR lowering, so the MIR fingerprint captures it
        // and the object-cache key differs — verified), but force the cache **off** whenever it is set
        // so a `bench/adaptive_sort` baseline build can never read or publish a cross-toggle object
        // into the shared cache under any future refactor. Zero effect on normal builds (toggle unset).
        if std::env::var_os("ALIGN_SORT_ADAPTIVE").is_some() {
            return CacheContext::Disabled;
        }
        let default_on = || match default_cache_root() {
            Some(root) => CacheContext::Enabled { root },
            None => CacheContext::Disabled,
        };
        match std::env::var("ALIGNC_CACHE") {
            Err(_) => default_on(),                                       // unset ⇒ default-ON
            Ok(v) if v.is_empty() || v == "off" => CacheContext::Disabled, // explicit off
            Ok(v) if v == "on" => default_on(),
            Ok(path) => CacheContext::Enabled { root: PathBuf::from(path) },
        }
    }

    /// Construct an enabled cache rooted at `root` (used by tests and the `on` path).
    pub fn at(root: PathBuf) -> CacheContext {
        CacheContext::Enabled { root }
    }

    /// Whether the cache is on. The caller gates key construction on this so a disabled build (the
    /// default) never pays for the codegen-key inputs — notably the one-time `alignc`-binary hash in
    /// [`compiler_build_id`] and the target/LLVM identity resolution.
    pub fn is_enabled(&self) -> bool {
        matches!(self, CacheContext::Enabled { .. })
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

    /// The serial cache lookup for one unit — the first half of [`codegen`], exposed so the parallel
    /// build driver can do all lookups serially and then produce only the MISSES in parallel (the
    /// settled S3 design). On an enabled HIT the CAS blob is written verbatim to `obj_out` and
    /// [`CacheLookup::Hit`] carries the outcome. A [`CacheLookup::Miss`] carries the first-differing
    /// reason (its object is NOT produced — the caller must `produce` it then [`publish_after_miss`]).
    /// A disabled cache is [`CacheLookup::Miss`] with `None` reason (never consulted, no key work).
    pub fn lookup(&self, key: &CodegenKey, obj_out: &Path) -> CacheLookup {
        let root = match self {
            CacheContext::Disabled => return CacheLookup::Miss { reason: None },
            CacheContext::Enabled { root } => root,
        };
        let action_path = action_manifest_path(root, key.full_digest());
        match try_hit(root, &action_path, key, obj_out) {
            HitResult::Hit => CacheLookup::Hit(CacheOutcome {
                stage: CacheStage::Codegen,
                unit: key.unit.clone(),
                hit: true,
                miss_reason: None,
            }),
            HitResult::Corrupt => CacheLookup::Miss { reason: Some(FirstDiff::CorruptEntry) },
            // Reason computed BEFORE any publish overwrites the slot pointer (the prior key is diffed).
            HitResult::Miss => CacheLookup::Miss { reason: Some(diff_against_slot(root, key)) },
        }
    }

    /// Publish an already-produced object to the cache after a [`CacheLookup::Miss`] — best-effort (a
    /// cache WRITE failure never fails an otherwise-correct build; the object at `obj_out` is already
    /// valid and link reads it directly). A no-op when the cache is disabled. Safe to call from a
    /// worker thread (only writes into the content-addressed store + index).
    pub fn publish_after_miss(&self, key: &CodegenKey, obj_out: &Path) {
        if let CacheContext::Enabled { root } = self {
            publish(root, key, obj_out);
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
    let bytes = match std::fs::read(obj_out) {
        Ok(b) => b,
        Err(e) => {
            eprintln!("alignc: cache not populated (cannot read produced object {}): {e}", obj_out.display());
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

/// The compiler build id: the hash of the running `alignc` binary bytes, memoized once per process.
/// Covers dev rebuilds where the crate version is unchanged — any codegen/lowering source change
/// rebuilds the binary and flips this. Falls back to a version-derived constant only if the executable
/// cannot be read (which never happens on the supported platforms); a fallback id lives in a disjoint
/// namespace, so it can never collide with a real-id entry.
pub fn compiler_build_id() -> Hash128 {
    static ID: OnceLock<Hash128> = OnceLock::new();
    *ID.get_or_init(|| {
        std::env::current_exe()
            .ok()
            .and_then(|p| std::fs::read(p).ok())
            .map(|b| Hash128::of(&b))
            .unwrap_or_else(|| {
                let fallback = format!("alignc-build-id-fallback-{}", env!("CARGO_PKG_VERSION"));
                Hash128::of(fallback.as_bytes())
            })
    })
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

fn action_manifest_path(root: &Path, full: Hash128) -> PathBuf {
    root.join("actions").join("codegen").join(full.to_hex())
}

fn slot_pointer_path(root: &Path, slot: Hash128) -> PathBuf {
    root.join("index").join("codegen").join(slot.to_hex())
}

/// `cas/<hex[0..2]>/<hex>` for a blob digest (hex is 32 chars, so the 2-char shard prefix is safe).
fn cas_blob_path(root: &Path, digest: Hash128) -> PathBuf {
    let hex = digest.to_hex();
    root.join("cas").join(&hex[..2]).join(&hex)
}

enum HitResult {
    Hit,
    /// No usable prior entry (absent / undecodable / foreign manifest): a clean miss.
    Miss,
    /// A prior entry existed but its blob failed the digest check — rebuild after unlinking + noting.
    Corrupt,
}

/// Attempt a hit at `action_path`. Fail-closed at every step: a missing/undecodable manifest is a
/// clean [`HitResult::Miss`]; a manifest whose stored key does not match `key` (a hash collision) is a
/// miss; a missing or digest-mismatched blob is [`HitResult::Corrupt`] (note + unlink + rebuild). On a
/// verified hit the blob is written to `obj_out`.
fn try_hit(root: &Path, action_path: &Path, key: &CodegenKey, obj_out: &Path) -> HitResult {
    let manifest_bytes = match std::fs::read(action_path) {
        Ok(b) => b,
        Err(_) => return HitResult::Miss,
    };
    let (stored_key, blob_digest) = match deserialize_manifest(&manifest_bytes) {
        Ok(v) => v,
        Err(_) => return HitResult::Miss, // version skew / garbage: unreferenced, rebuild fresh
    };
    // Defense in depth against a full-digest collision: the stored components must equal the key.
    if &stored_key != key {
        return HitResult::Miss;
    }
    materialize_blob(root, blob_digest, obj_out)
}

/// Read the CAS blob for `blob_digest`, verify its content digest, and write it to `out_path`. A
/// missing or digest-mismatched blob is [`HitResult::Corrupt`] (always-on note + unlink; doc-10 §6.4
/// fail-closed matrix); a verified blob that cannot be written back is a clean [`HitResult::Miss`]
/// (rebuild in place). Shared by the single-phase codegen cache and both ThinLTO phases.
fn materialize_blob(root: &Path, blob_digest: Hash128, out_path: &Path) -> HitResult {
    let blob_path = cas_blob_path(root, blob_digest);
    let blob = match std::fs::read(&blob_path) {
        Ok(b) => b,
        Err(_) => {
            // The action manifest references a blob that is gone: treat as corruption, rebuild.
            eprintln!("{CORRUPT_NOTE}");
            return HitResult::Corrupt;
        }
    };
    if Hash128::of(&blob) != blob_digest {
        // Corrupted blob bytes: unlink + always-on note + rebuild.
        let _ = std::fs::remove_file(&blob_path);
        eprintln!("{CORRUPT_NOTE}");
        return HitResult::Corrupt;
    }
    match std::fs::write(out_path, &blob) {
        Ok(()) => HitResult::Hit,
        // Cannot materialize the artifact from a verified blob: fall back to rebuilding it in place.
        Err(_) => HitResult::Miss,
    }
}

/// Compute the [`FirstDiff`] for a miss by reading the unit's slot pointer and diffing its decoded key
/// against `key`. No slot pointer (or an undecodable one) ⇒ [`FirstDiff::NoPriorEntry`].
fn diff_against_slot(root: &Path, key: &CodegenKey) -> FirstDiff {
    let path = slot_pointer_path(root, key.slot_digest());
    match std::fs::read(&path) {
        Ok(bytes) => match deserialize_manifest(&bytes) {
            Ok((stored_key, _)) => first_diff(&stored_key, key),
            Err(_) => FirstDiff::NoPriorEntry,
        },
        Err(_) => FirstDiff::NoPriorEntry,
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
fn publish_file(final_path: &Path, bytes: &[u8]) -> Result<(), String> {
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

struct Writer {
    buf: Vec<u8>,
}

impl Writer {
    fn new() -> Writer {
        Writer { buf: Vec::new() }
    }
    fn u8(&mut self, v: u8) {
        self.buf.push(v);
    }
    fn bool(&mut self, v: bool) {
        self.u8(v as u8);
    }
    fn u32(&mut self, v: u32) {
        self.buf.extend_from_slice(&v.to_le_bytes());
    }
    fn u64(&mut self, v: u64) {
        self.buf.extend_from_slice(&v.to_le_bytes());
    }
    fn h128(&mut self, h: Hash128) {
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
    fn bytes(&mut self, b: &[u8]) {
        self.u32(u32_len(b.len()));
        self.buf.extend_from_slice(b);
    }
    fn str(&mut self, s: &str) {
        self.bytes(s.as_bytes());
    }
}

/// Narrow a length to the `u32` prefix width, or panic loudly. Producer-side, compiler-internal data
/// (never untrusted input) — matching `align_interface::codec::u32_len`; the reader stays Err-based.
fn u32_len(n: usize) -> u32 {
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
    w.bool(k.rt_lto);
    w.opt_h128(k.rt_lto_digest);
    w.bytes(&k.cross_unit_opt_digest);
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
}

struct Reader<'a> {
    buf: &'a [u8],
    pos: usize,
}

impl<'a> Reader<'a> {
    fn new(buf: &'a [u8]) -> Reader<'a> {
        Reader { buf, pos: 0 }
    }
    fn take(&mut self, n: usize) -> Result<&'a [u8], CacheDecodeError> {
        let end = self.pos.checked_add(n).ok_or(CacheDecodeError::Truncated)?;
        let s = self.buf.get(self.pos..end).ok_or(CacheDecodeError::Truncated)?;
        self.pos = end;
        Ok(s)
    }
    fn u8(&mut self) -> Result<u8, CacheDecodeError> {
        Ok(self.take(1)?[0])
    }
    fn bool(&mut self) -> Result<bool, CacheDecodeError> {
        match self.u8()? {
            0 => Ok(false),
            1 => Ok(true),
            tag => Err(CacheDecodeError::BadTag { what: "bool", tag }),
        }
    }
    fn u32(&mut self) -> Result<u32, CacheDecodeError> {
        Ok(u32::from_le_bytes(self.take(4)?.try_into().unwrap()))
    }
    fn u64(&mut self) -> Result<u64, CacheDecodeError> {
        Ok(u64::from_le_bytes(self.take(8)?.try_into().unwrap()))
    }
    fn h128(&mut self) -> Result<Hash128, CacheDecodeError> {
        Ok(Hash128 { lo: self.u64()?, hi: self.u64()? })
    }
    fn opt_h128(&mut self) -> Result<Option<Hash128>, CacheDecodeError> {
        match self.u8()? {
            0 => Ok(None),
            1 => Ok(Some(self.h128()?)),
            tag => Err(CacheDecodeError::BadTag { what: "option", tag }),
        }
    }
    /// A length prefix, then that many bytes — bounds-checked (the `take` validates the count against
    /// the real buffer, so a huge length simply fails `Truncated`, never pre-allocates).
    fn bytes(&mut self) -> Result<Vec<u8>, CacheDecodeError> {
        let n = self.u32()? as usize;
        Ok(self.take(n)?.to_vec())
    }
    fn str(&mut self) -> Result<String, CacheDecodeError> {
        let bytes = self.bytes()?;
        String::from_utf8(bytes).map_err(|_| CacheDecodeError::BadUtf8)
    }
    /// A length prefix, then `f` that many times. Pre-allocates at most [`SEQ_PREALLOC_CAP`] to bound a
    /// garbage-length allocation; the real elements still have to be present to grow further.
    fn seq<T>(&mut self, mut f: impl FnMut(&mut Reader<'a>) -> Result<T, CacheDecodeError>) -> Result<Vec<T>, CacheDecodeError> {
        let n = self.u32()? as usize;
        let mut out = Vec::with_capacity(n.min(SEQ_PREALLOC_CAP));
        for _ in 0..n {
            out.push(f(self)?);
        }
        Ok(out)
    }
    fn finish(self) -> Result<(), CacheDecodeError> {
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
    let rt_lto = r.bool()?;
    let rt_lto_digest = r.opt_h128()?;
    let cross_unit_opt_digest = r.bytes()?;
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
            rt_lto,
            rt_lto_digest,
            cross_unit_opt_digest,
            unit,
        },
        blob_digest,
    ))
}

// ================================================================================================
// ThinLTO S2: the two cacheable phases (`docs/impl/07-roadmap.md` ThinLTO S2). A `--thin-lto` build
// caches per-unit PRELINK bitcode (phase 1, `prelink-bitcode` part-kind) and the per-unit BACKEND
// object (phase 3); the serial thin-link (phase 2) is never cached but always runs, so cross-unit
// import decisions are recomputed fresh every build. Both keys reuse the CAS + manifest discipline
// above (private staging + atomic rename, digest-verified reads, fail-closed decode).
// ================================================================================================

// ---- phase 1: prelink key -----------------------------------------------------------------------

/// The cache key for one unit's ThinLTO **prelink bitcode** (phase 1). It is today's codegen key
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
    pub rt_lto: bool,
    pub rt_lto_digest: Option<Hash128>,
    pub unit: String,
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
        Hash128::of(&w.buf)
    }
}

/// First differing component of a decoded prior prelink key vs the fresh one. No cpu/reloc components
/// (excluded from the prelink key); everything else mirrors [`first_diff`]'s priority.
fn prelink_first_diff(stored: &PrelinkKey, current: &PrelinkKey) -> FirstDiff {
    if stored.frontend_schema != current.frontend_schema || stored.located != current.located {
        return FirstDiff::FrontendSchema;
    }
    if stored.target_triple != current.target_triple || stored.object_format != current.object_format {
        return FirstDiff::Target;
    }
    if stored.llvm_version != current.llvm_version {
        return FirstDiff::LlvmVersion;
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

/// One inbound cross-module import edge into this unit, keyed by stable unit ids: `(src, GUID, kind)`
/// where `kind == true` is a definition import. Sorted before hashing so the key is order-independent.
pub type InboundImport = (String, u64, bool);

/// The cache key for one unit's ThinLTO **backend object** (phase 3) — the PRECISE cross-unit digest.
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
    /// Sorted, deduped `(src_unit, prelink_digest)` for every import-source unit.
    pub import_source_digests: Vec<(String, Hash128)>,
    /// The `--export` root set (entry unit only; sorted+deduped) — it widens the preserve set.
    pub exports: Vec<String>,
    pub unit: String,
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
        Hash128::of(&w.buf)
    }
}

/// First differing component of a decoded prior backend key vs the fresh one. The target/backend bits
/// come first (they subsume many entries at once), then the unit's own prelink content, then the
/// cross-unit inputs, then the export set.
fn backend_first_diff(stored: &BackendKey, current: &BackendKey) -> FirstDiff {
    if stored.llvm_version != current.llvm_version {
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
        let root = match self {
            CacheContext::Disabled => return CacheLookup::Miss { reason: None },
            CacheContext::Enabled { root } => root,
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
        if let CacheContext::Enabled { root } = self {
            publish_phase(
                root,
                &phase_action_path(root, "prelink", key.full_digest()),
                &phase_slot_path(root, "prelink", key.slot_digest()),
                bc_out,
                |digest| serialize_prelink_manifest(key, digest),
            );
        }
    }

    /// Phase-3 lookup: on an enabled hit, the object CAS blob is written verbatim to `obj_out` and
    /// [`CacheLookup::Hit`] carries the [`CacheStage::ThinLtoBackend`] outcome; a miss carries the
    /// first-differing reason.
    pub fn lookup_backend(&self, key: &BackendKey, obj_out: &Path) -> CacheLookup {
        let root = match self {
            CacheContext::Disabled => return CacheLookup::Miss { reason: None },
            CacheContext::Enabled { root } => root,
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
        if let CacheContext::Enabled { root } = self {
            publish_phase(
                root,
                &phase_action_path(root, "thinbackend", key.full_digest()),
                &phase_slot_path(root, "thinbackend", key.slot_digest()),
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
    let manifest_bytes = match std::fs::read(action_path) {
        Ok(b) => b,
        Err(_) => return HitResult::Miss,
    };
    match matched_blob(&manifest_bytes) {
        Some(blob_digest) => materialize_blob(root, blob_digest, out_path),
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
    match std::fs::read(&path) {
        Ok(bytes) => diff(&bytes).unwrap_or(FirstDiff::NoPriorEntry),
        Err(_) => FirstDiff::NoPriorEntry,
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
    let bytes = match std::fs::read(out_path) {
        Ok(b) => b,
        Err(e) => {
            eprintln!("alignc: cache not populated (cannot read produced artifact {}): {e}", out_path.display());
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
    /// A sorted-inbound-import sequence `(src, guid, kind)`.
    fn inbound_imports(&mut self, v: &[InboundImport]) {
        self.u32(u32_len(v.len()));
        for (src, guid, kind) in v {
            self.str(src);
            self.u64(*guid);
            self.bool(*kind);
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
    w.bool(k.rt_lto);
    w.opt_h128(k.rt_lto_digest);
    w.str(&k.unit);
}

fn write_backend_key(w: &mut Writer, k: &BackendKey) {
    w.u8(PHASE_TAG_BACKEND);
    w.u32(k.cache_format_version);
    w.h128(k.compiler_build_id);
    w.str(&k.llvm_version);
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
    w.digest_seq(&k.import_source_digests);
    w.str_seq(&k.exports);
    w.str(&k.unit);
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
    fn inbound_imports(&mut self) -> Result<Vec<InboundImport>, CacheDecodeError> {
        self.seq(|r| Ok((r.str()?, r.u64()?, r.bool()?)))
    }
    fn u64_seq(&mut self) -> Result<Vec<u64>, CacheDecodeError> {
        self.seq(|r| r.u64())
    }
    fn digest_seq(&mut self) -> Result<Vec<(String, Hash128)>, CacheDecodeError> {
        self.seq(|r| Ok((r.str()?, r.h128()?)))
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
        rt_lto: r.bool()?,
        rt_lto_digest: r.opt_h128()?,
        unit: r.str()?,
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
        import_source_digests: r.digest_seq()?,
        exports: r.str_seq()?,
        unit: r.str()?,
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
            rt_lto: false,
            rt_lto_digest: None,
            cross_unit_opt_digest: Vec::new(),
            unit: "main".to_string(),
        }
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
        let mut k = base.clone();
        k.cross_unit_opt_digest = vec![1];
        assert_eq!(first_diff(&base, &k), FirstDiff::CrossUnitOpt);
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
        assert_eq!(clear_cache(&root), Ok(true));
        assert!(!root.join("cas").exists() && !root.join("actions").exists() && !root.join("index").exists());
        assert!(root.join("keep").join("f").exists(), "clear must not touch anything but its own subtrees");
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

    // ---- ThinLTO S2 key codecs + first-diff -----------------------------------------------------

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
            rt_lto: false,
            rt_lto_digest: None,
            unit: "main".to_string(),
        }
    }

    fn sample_backend_key() -> BackendKey {
        BackendKey {
            cache_format_version: CACHE_KEY_FORMAT_VERSION,
            compiler_build_id: Hash128 { lo: 1, hi: 2 },
            llvm_version: "22.1.8".to_string(),
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
            inbound_imports: vec![("lib".to_string(), 42, true)],
            outbound_exports: vec![7, 11],
            import_source_digests: vec![("lib".to_string(), Hash128 { lo: 10, hi: 11 })],
            exports: Vec::new(),
            unit: "main".to_string(),
        }
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
        k.import_source_digests[0].1 = Hash128 { lo: 0, hi: 0 };
        assert_eq!(backend_first_diff(&base, &k), FirstDiff::CrossUnitImports);
        // Inbound import edge changed → CrossUnitImports.
        let mut k = base.clone();
        k.inbound_imports[0].2 = false;
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
    }
}
