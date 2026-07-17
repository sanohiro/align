//! Instrument-PGO S2 cache-composition gates (`--pgo-instrument` / `--pgo-use` × the object cache).
//!
//! S2 deletes the S1 total cache bypass: a PGO build now flows through the NORMAL cached + parallel
//! per-unit path, composed via the [`PgoKey`] key component on [`CodegenKey`]. These gates assert the
//! composition is CORRECT — `CacheOutcome` / `FirstDiff` enums, digests, and blob identity, NEVER wall
//! time — with the load-bearing property being Gate 4: an instrumented (or profile-use) object must
//! NEVER be served to an ordinary build (the settled correctness bug class).
//!
//! Two layers:
//!   * KEY / CAS-composition gates (a synthetic producer, no LLVM) — always run. They exercise the real
//!     key/slot/manifest/CAS machinery over the `PgoKey` component: full-digest disjointness, the
//!     `FirstDiff::PgoProfile` diff, the profdata-digest revert row, path-independence, and the
//!     rt-lto × pgo composition. This is where the isolation invariant is proven exhaustively.
//!   * End-to-end gates (real `emit_object_pgo` / subprocess `alignc`) — backend / tool gated (skip when
//!     absent, matching the repo's external-tool convention). They prove the REAL PGO emit path caches
//!     and re-hits, and that a build is byte-identical cold vs hot at the executable level.

mod common;
use common::*;

use std::path::{Path, PathBuf};
use std::process::Command;

// `common::*` re-exports BuildTarget / CacheContext / FirstDiff / Hash128 / Profile / SourceMap /
// backend_available / build_per_unit / Proj / thin_nonce, plus the shared PGO helpers (hh / BRANCHY /
// profile_rt_available / make_profdata); import only the driver API it does not.
use align_driver::{codegen_units_parallel, CacheLookup, CodegenKey, PgoKey, PgoMode};

// =================================================================================================
// KEY / CAS-composition layer (synthetic producer — always run, no backend)
// =================================================================================================

/// A fully-specified [`CodegenKey`] with fixed values (no target resolution / no backend), so a test
/// can clone it and vary ONLY `pgo_mode` (or `rt_lto*`) to probe one component's effect on the digest.
/// `pgo_mode` defaults to `Off` — the ordinary-build key an instrumented object must never be served to.
fn base_key() -> CodegenKey {
    CodegenKey {
        cache_format_version: align_driver::CACHE_KEY_FORMAT_VERSION,
        compiler_build_id: hh(1),
        frontend_schema: 1,
        located: false,
        impl_hash: hh(2),
        dep_interface_hashes: vec![("dep".to_string(), hh(3))],
        exports: vec![],
        target_triple: "x86_64-unknown-linux-gnu".to_string(),
        object_format: 0,
        resolved_cpu: "x86-64".to_string(),
        resolved_features: String::new(),
        profile_name: "release".to_string(),
        pipeline: "default<O2>".to_string(),
        codegen_opt: "O2".to_string(),
        reloc_model: "pic".to_string(),
        code_model: "default".to_string(),
        llvm_version: "22.1.8".to_string(),
        rt_lto: false,
        rt_lto_digest: None,
        pgo_mode: PgoKey::Off,
        unit: "main".to_string(),
    }
}

/// A fresh temp cache dir + a distinct object output path, cleaned on drop.
struct CacheDir(PathBuf);
impl CacheDir {
    fn new(tag: &str) -> CacheDir {
        let d = std::env::temp_dir().join(format!("align_pgo_s2_{}_{}_{}", tag, std::process::id(), thin_nonce()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).expect("mkdir cache");
        CacheDir(d)
    }
    fn ctx(&self) -> CacheContext {
        CacheContext::at(self.0.join("cache"))
    }
    fn obj(&self, name: &str) -> PathBuf {
        self.0.join(name)
    }
}
impl Drop for CacheDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

/// Publish `key`'s object with a SYNTHETIC producer (the bytes identify the mode, so a wrong-mode serve
/// would be observable). Returns the `CacheOutcome` (always a miss on first publish).
fn publish_synthetic(cache: &CacheContext, key: &CodegenKey, obj: &Path, marker: &[u8]) {
    let out = cache
        .codegen(key, obj, |o| std::fs::write(o, marker).map_err(|e| e.to_string()))
        .expect("synthetic codegen");
    assert!(!out.hit, "first publish of a key is a miss");
}

// ---- Gate 1 + 4: isolation — off / instrument / use never share an object ----------------------

#[test]
fn key_pgo_modes_produce_three_disjoint_full_digests_same_slot() {
    let off = base_key();
    let mut instr = base_key();
    instr.pgo_mode = PgoKey::Instrument;
    let mut use_a = base_key();
    use_a.pgo_mode = PgoKey::Use(hh(100));

    // Three structurally-disjoint full-key digests → three distinct CAS blobs → never a shared object.
    let (do_, di, du) = (off.full_digest(), instr.full_digest(), use_a.full_digest());
    assert_ne!(do_, di, "off vs instrument must not share a full-key digest");
    assert_ne!(do_, du, "off vs use must not share a full-key digest");
    assert_ne!(di, du, "instrument vs use must not share a full-key digest");

    // Same stable-core slot (so a mode switch is diffable to `FirstDiff::PgoProfile`, not a fresh slot).
    assert_eq!(off.slot_digest(), instr.slot_digest());
    assert_eq!(off.slot_digest(), use_a.slot_digest());
}

#[test]
fn cache_never_serves_an_instrumented_object_to_an_ordinary_build() {
    // The load-bearing correctness gate. Publish an ORDINARY (Off) object, then look the cache up with
    // the INSTRUMENT key sharing the same unit/slot: it must MISS (not serve the ordinary blob), with a
    // `FirstDiff::PgoProfile` reason. Then the reverse. Then prove each mode re-hits its OWN blob.
    let cd = CacheDir::new("isolate");
    let cache = cd.ctx();

    let off = base_key();
    publish_synthetic(&cache, &off, &cd.obj("off.o"), b"ORDINARY-OBJECT");

    let mut instr = base_key();
    instr.pgo_mode = PgoKey::Instrument;
    match cache.lookup(&instr, &cd.obj("probe_instr.o")) {
        CacheLookup::Miss { reason } => assert_eq!(
            reason,
            Some(FirstDiff::PgoProfile),
            "an instrument build must MISS an ordinary object, and name the PGO component"
        ),
        CacheLookup::Hit(_) => panic!("BUG: instrument build served the ORDINARY object — the S2 isolation bug class"),
    }
    // Publish the instrument object; now an ordinary rebuild must still hit its OWN (ordinary) blob,
    // never the instrumented one.
    publish_synthetic(&cache, &instr, &cd.obj("instr.o"), b"INSTRUMENTED-OBJECT");
    match cache.lookup(&off, &cd.obj("probe_off.o")) {
        CacheLookup::Hit(_) => {
            let got = std::fs::read(cd.obj("probe_off.o")).expect("served object");
            assert_eq!(got, b"ORDINARY-OBJECT", "ordinary rebuild must serve the ORDINARY blob, not the instrumented one");
        }
        CacheLookup::Miss { reason } => panic!("ordinary rebuild unexpectedly missed its own blob: {reason:?}"),
    }
    // And the instrument object re-hits with its own bytes.
    match cache.lookup(&instr, &cd.obj("probe_instr2.o")) {
        CacheLookup::Hit(_) => {
            let got = std::fs::read(cd.obj("probe_instr2.o")).expect("served object");
            assert_eq!(got, b"INSTRUMENTED-OBJECT");
        }
        CacheLookup::Miss { reason } => panic!("instrument rebuild unexpectedly missed: {reason:?}"),
    }
}

// ---- Gate: profdata digest — edit → miss(PgoProfile); revert → old CAS blob HIT ----------------

#[test]
fn profdata_bytes_edit_misses_and_revert_rehits_the_old_blob() {
    let cd = CacheDir::new("digest");
    let cache = cd.ctx();

    // A profile-use build with digest d1.
    let mut use1 = base_key();
    use1.pgo_mode = PgoKey::Use(hh(0xD1));
    publish_synthetic(&cache, &use1, &cd.obj("use1.o"), b"USE-OBJECT-FOR-PROFILE-1");

    // Editing the profdata bytes changes the digest (d2) → the use key misses, naming the PGO component.
    let mut use2 = base_key();
    use2.pgo_mode = PgoKey::Use(hh(0xD2));
    assert_ne!(use1.full_digest(), use2.full_digest(), "distinct profdata digests → distinct keys");
    match cache.lookup(&use2, &cd.obj("probe_use2.o")) {
        CacheLookup::Miss { reason } => assert_eq!(reason, Some(FirstDiff::PgoProfile), "edited profile → PgoProfile miss"),
        CacheLookup::Hit(_) => panic!("edited profdata unexpectedly hit the old profile's object"),
    }
    // Publish the d2 object, then REVERT to d1: the original CAS blob (content-addressed by the d1 key)
    // is still present → HIT, serving the exact original bytes. (Content-addressing = a revert re-hits.)
    publish_synthetic(&cache, &use2, &cd.obj("use2.o"), b"USE-OBJECT-FOR-PROFILE-2");
    match cache.lookup(&use1, &cd.obj("probe_use1.o")) {
        CacheLookup::Hit(_) => {
            let got = std::fs::read(cd.obj("probe_use1.o")).expect("served object");
            assert_eq!(got, b"USE-OBJECT-FOR-PROFILE-1", "reverting the profdata re-hits the ORIGINAL profile's cached object");
        }
        CacheLookup::Miss { reason } => panic!("revert did not re-hit the original blob: {reason:?}"),
    }
}

// ---- Gate: path-independence — same profile bytes via a different path → HIT --------------------

#[test]
fn same_profile_bytes_via_different_path_hit() {
    // The `Use` digest is over the profdata BYTES, not its path. Two files with identical content at
    // different paths yield the same digest → the same key → a HIT. (This is what the driver's
    // `Hash128::of(&std::fs::read(path))` guarantees; here it is asserted at the digest that keys it.)
    let cd = CacheDir::new("pathindep");
    let cache = cd.ctx();

    let bytes = b"identical merged profdata content";
    let path_a = cd.obj("a/app.profdata");
    let path_b = cd.obj("b/renamed.profdata");
    std::fs::create_dir_all(path_a.parent().unwrap()).unwrap();
    std::fs::create_dir_all(path_b.parent().unwrap()).unwrap();
    std::fs::write(&path_a, bytes).unwrap();
    std::fs::write(&path_b, bytes).unwrap();
    let dig_a = Hash128::of(&std::fs::read(&path_a).unwrap());
    let dig_b = Hash128::of(&std::fs::read(&path_b).unwrap());
    assert_eq!(dig_a, dig_b, "identical bytes at different paths → identical digest (path-independent)");

    let mut key_a = base_key();
    key_a.pgo_mode = PgoKey::Use(dig_a);
    publish_synthetic(&cache, &key_a, &cd.obj("a.o"), b"USE-OBJECT");

    let mut key_b = base_key();
    key_b.pgo_mode = PgoKey::Use(dig_b);
    assert_eq!(key_a.full_digest(), key_b.full_digest(), "same bytes ⇒ same full-key digest ⇒ hit");
    match cache.lookup(&key_b, &cd.obj("probe_b.o")) {
        CacheLookup::Hit(_) => {}
        CacheLookup::Miss { reason } => panic!("the same profile via a different path should HIT: {reason:?}"),
    }
}

// ---- Gate: rt-lto × pgo-use — both components present, disjoint from either alone ---------------

#[test]
fn rt_lto_and_pgo_use_compose_into_a_distinct_key() {
    // `--rt-lto` × `--pgo-use` composes (settled): the key must carry BOTH the rt-lto digest AND the PGO
    // digest, and be disjoint from a key with only one of them (or neither).
    let plain = base_key();

    let mut rt_only = base_key();
    rt_only.rt_lto = true;
    rt_only.rt_lto_digest = Some(hh(0xF7));

    let mut pgo_only = base_key();
    pgo_only.pgo_mode = PgoKey::Use(hh(0xD1));

    let mut both = base_key();
    both.rt_lto = true;
    both.rt_lto_digest = Some(hh(0xF7));
    both.pgo_mode = PgoKey::Use(hh(0xD1));

    let d_plain = plain.full_digest();
    let d_rt = rt_only.full_digest();
    let d_pgo = pgo_only.full_digest();
    let d_both = both.full_digest();
    for (a, b, msg) in [
        (d_both, d_plain, "both vs neither"),
        (d_both, d_rt, "both vs rt-lto-only"),
        (d_both, d_pgo, "both vs pgo-only"),
        (d_rt, d_pgo, "rt-lto-only vs pgo-only"),
    ] {
        assert_ne!(a, b, "rt-lto × pgo composition must be a distinct key ({msg})");
    }
}

// =================================================================================================
// End-to-end layer (real emit / subprocess — backend / tool gated)
// =================================================================================================

/// Build `BRANCHY`'s single unit through `codegen_units_parallel` with `pgo` + `cache`, into a fresh
/// object path. Returns `(outcome.hit, object-bytes)`.
fn run_units(proj: &Proj, cache: &CacheContext, pgo: &PgoMode, tag: &str) -> (bool, Vec<u8>) {
    let entry = proj.dir.join(&proj.entry);
    let src = std::fs::read_to_string(&entry).expect("read entry");
    let mut sm = SourceMap::new();
    let walk = build_per_unit(&mut sm, &entry.display().to_string(), &src);
    assert!(!walk.diags.has_errors(), "unexpected build errors");
    let obj = proj.dir.join(format!("{tag}.o"));
    let build = codegen_units_parallel(
        &walk.units, std::slice::from_ref(&obj), cache, &BuildTarget::Baseline, Profile::Release, false, 1, pgo,
    )
    .expect("codegen_units_parallel");
    assert_eq!(build.outcomes.len(), 1, "single-unit program");
    (build.outcomes[0].hit, std::fs::read(&obj).expect("object bytes"))
}

// ---- Gate: the REAL instrument emit path caches and re-hits (cold miss → hot hit, byte-identical) --

#[test]
fn real_instrument_object_caches_and_rehits() {
    if !backend_available() {
        return;
    }
    let proj = Proj::new("real-instr", &[("prog.align", BRANCHY)], "prog.align");
    let cache = proj.cache();

    let (hit0, bytes0) = run_units(&proj, &cache, &PgoMode::Instrument, "cold");
    assert!(!hit0, "cold instrument build misses");
    let (hit1, bytes1) = run_units(&proj, &cache, &PgoMode::Instrument, "hot");
    assert!(hit1, "a second identical instrument build HITS the cache (S2: PGO is cached, not bypassed)");
    assert_eq!(bytes0, bytes1, "the CAS-served instrument object is byte-identical to the cold one");

    // And an ORDINARY build on the same cache root must MISS (never served the instrumented object).
    let (hit_off, _) = run_units(&proj, &cache, &PgoMode::Off, "ord");
    assert!(!hit_off, "an ordinary build must MISS on a cache holding only an instrumented object");
}

// ---- Gate: cross-cold determinism — two from-scratch instrument builds are byte-identical --------

#[test]
fn instrument_object_is_deterministic_across_fresh_caches() {
    if !backend_available() {
        return;
    }
    let p1 = Proj::new("det1", &[("prog.align", BRANCHY)], "prog.align");
    let p2 = Proj::new("det2", &[("prog.align", BRANCHY)], "prog.align");
    let (_, b1) = run_units(&p1, &p1.cache(), &PgoMode::Instrument, "a");
    let (_, b2) = run_units(&p2, &p2.cache(), &PgoMode::Instrument, "b");
    assert_eq!(b1, b2, "instrument codegen is deterministic (a cold build reproduces byte-for-byte)");
}

// =================================================================================================
// Subprocess layer — exe-level byte-identity + --cache-stats under PGO (tool gated)
// =================================================================================================

fn alignc_with_cache(cache_root: &Path) -> Command {
    let mut c = Command::new(env!("CARGO_BIN_EXE_alignc"));
    c.env("ALIGNC_CACHE", cache_root);
    c
}

/// Read the produced executable's bytes for a `build` at `dir/prog`.
fn exe_bytes(dir: &Path) -> Vec<u8> {
    std::fs::read(dir.join("prog")).expect("read produced exe")
}

// ---- Gate: exe-level cold-vs-hit byte-identity + --cache-stats reports (no bypass line) ----------

#[test]
fn instrument_exe_is_byte_identical_cold_vs_hit_and_cache_stats_report_normal() {
    if !backend_available() || !profile_rt_available() {
        return;
    }
    let d = std::env::temp_dir().join(format!("align_pgo_s2_exe_instr_{}_{}", std::process::id(), thin_nonce()));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).unwrap();
    std::fs::write(d.join("prog.align"), BRANCHY).unwrap();
    let cache_root = d.join("cache");

    // Cold build: --cache-stats must report a normal miss (NOT the S1 "bypassed under --pgo-*" line).
    let cold = alignc_with_cache(&cache_root)
        .current_dir(&d)
        .args(["--pgo-instrument", "--cache-stats", "--profile", "release", "build", "prog.align"])
        .output()
        .expect("cold build");
    assert_eq!(cold.status.code(), Some(0), "cold instrument build failed:\n{}", String::from_utf8_lossy(&cold.stderr));
    let cold_err = String::from_utf8_lossy(&cold.stderr);
    assert!(!cold_err.contains("bypassed"), "S2 must NOT print the S1 cache-bypass line:\n{cold_err}");
    let cold_out = String::from_utf8_lossy(&cold.stdout);
    let cold_stats = format!("{cold_out}{cold_err}");
    assert!(cold_stats.contains("miss"), "a cold PGO build must report a cache MISS via --cache-stats:\n{cold_stats}");
    let cold_exe = exe_bytes(&d);

    // Hot build (same cache): all-hit, and the produced exe is byte-identical to the cold one.
    let hot = alignc_with_cache(&cache_root)
        .current_dir(&d)
        .args(["--pgo-instrument", "--cache-stats", "--profile", "release", "build", "prog.align"])
        .output()
        .expect("hot build");
    assert_eq!(hot.status.code(), Some(0));
    let hot_stats = format!("{}{}", String::from_utf8_lossy(&hot.stdout), String::from_utf8_lossy(&hot.stderr));
    assert!(hot_stats.contains("hit"), "a second identical PGO build must report a cache HIT:\n{hot_stats}");
    assert_eq!(cold_exe, exe_bytes(&d), "the instrument executable is byte-identical cold vs hot");

    let _ = std::fs::remove_dir_all(&d);
}

// ---- Gate: use-mode exe-level cold-vs-hit byte-identity (full round trip, tool gated) -----------

#[test]
fn use_exe_is_byte_identical_cold_vs_hit() {
    let d = std::env::temp_dir().join(format!("align_pgo_s2_exe_use_{}_{}", std::process::id(), thin_nonce()));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).unwrap();
    let Some(profdata) = make_profdata(&d, BRANCHY) else {
        let _ = std::fs::remove_dir_all(&d);
        return;
    };
    let cache_root = d.join("cache");
    let profarg = profdata.to_str().unwrap().to_string();

    let cold = alignc_with_cache(&cache_root)
        .current_dir(&d)
        .args(["--pgo-use", &profarg, "--profile", "release", "build", "prog.align"])
        .output()
        .expect("cold use build");
    assert_eq!(cold.status.code(), Some(0), "cold use build failed:\n{}", String::from_utf8_lossy(&cold.stderr));
    let cold_exe = exe_bytes(&d);

    let hot = alignc_with_cache(&cache_root)
        .current_dir(&d)
        .args(["--pgo-use", &profarg, "--cache-stats", "--profile", "release", "build", "prog.align"])
        .output()
        .expect("hot use build");
    assert_eq!(hot.status.code(), Some(0));
    let hot_stats = format!("{}{}", String::from_utf8_lossy(&hot.stdout), String::from_utf8_lossy(&hot.stderr));
    assert!(hot_stats.contains("hit"), "a second identical --pgo-use build must HIT:\n{hot_stats}");
    assert_eq!(cold_exe, exe_bytes(&d), "the profile-use executable is byte-identical cold vs hot");

    let _ = std::fs::remove_dir_all(&d);
}
