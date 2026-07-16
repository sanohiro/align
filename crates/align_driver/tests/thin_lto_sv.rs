//! ThinLTO SV — the verification bundle that CLOSES the ThinLTO arc (S0–SV). It pins what S1
//! (`thin_lto.rs`) and S2 (`thin_lto_cache.rs`) did not. The DAG corpus / `Proj` guard / `ThinBuilt`
//! result live in `common` (shared with `thin_lto_cache.rs`).
//!
//!  1. **Build-twice determinism as a permanent gate** — independent COLD `--thin-lto` builds with
//!     SEPARATE cache roots produce byte-identical objects AND exe, at N=4 with real cross-unit
//!     imports (main → {b → c, d}); a single matrix gate pins same-input AND cross-`-j` determinism
//!     cold; a hot-serve gate pins that a cold publish at one `-j` is served byte-identically to hot
//!     builds at other `-j`; a real-driver subprocess gate pins the end-to-end exe (with explicit
//!     cold-independence via `--cache-stats`).
//!  2. **Summary-level stale-summary fail-closed mutation** — the S2 corruption gate replaces a cached
//!     prelink blob with NON-bitcode garbage; SV replaces it with a *structurally valid but different*
//!     prelink `.bc` (a sibling unit's bitcode in 2a, a different-body build of the same unit in 2b).
//!     Both still parse as bitcode, so only the CONTENT DIGEST can reject them → `CorruptEntry`
//!     eviction + deterministic rebuild + correct exe.
//!  3. **Explicit compile-time regression bound** — a COLD `--thin-lto` build's wall-time vs the
//!     flag-off cold build on the fixed 4-unit chain stays under a generous cap (interleaved A/B per
//!     round, best-of-N min; see `gate_sv3` for why this cannot flake).
//!  4. **Invalidation-matrix completion (unit level)** — the LLVM-version, compiler-build-id, and
//!     `--rt-lto` components of the prelink/backend keys each yield disjoint keys (never mix); plus an
//!     END-TO-END gate that `--rt-lto` on vs off changes a unit's own prelink `.bc` content.
//!
//! Hole finding (2a): the read path (`materialize_blob`) DOES digest-verify every CAS blob against the
//! manifest's stored `blob_digest` before use — shared by both ThinLTO phases via `try_hit_phase`. So
//! 2a found NO hole; the rejection is by digest, which SV proves holds for valid-but-different bitcode.

mod common;
use common::*;

use std::path::PathBuf;
use std::process::Command;
use std::time::Instant;

use align_driver::CacheLookup;

fn backend() -> bool {
    backend_available()
}

/// LLVM bitcode magic (`BC\xC0\xDE`). Used to self-document that a swapped-in blob still *parses* as
/// bitcode, so the rejection can only be the content-digest check (not a parse failure).
fn is_llvm_bitcode(bytes: &[u8]) -> bool {
    bytes.starts_with(&[0x42, 0x43, 0xC0, 0xDE])
}

// ================================================================================================
// Gate SV1: build-twice determinism (permanent gate)
// ================================================================================================

/// The determinism matrix: three independent COLD `--thin-lto` builds of the N=4 DAG (real cross-unit
/// imports), each with its OWN fresh cache root, at `-j1` / `-j2` / `-j4`. Every pair must produce
/// byte-identical objects AND a byte-identical exe. This single gate pins BOTH same-input determinism
/// (any two) AND cross-`-j` cold determinism with no cache reuse (S2 gate6 pins `-j1==-j4` only with
/// the cache DISABLED; here the cache is ENABLED-but-cold, so the parallel claim order can leak into
/// neither an object nor a cached artifact).
#[test]
fn gate_sv1_build_twice_determinism_matrix() {
    if !backend() {
        return;
    }
    let proj = Proj::dag("det-matrix", C_V1);
    let builds: Vec<ThinBuilt> = [(1usize, "r1"), (2, "r2"), (4, "r3")]
        .into_iter()
        .map(|(jobs, root)| {
            let b = thin_build(&proj, &CacheContext::at(proj.dir.join(root)), jobs);
            assert!(b.all_miss(), "each separate-root build is cold");
            b
        })
        .collect();

    let objs: Vec<Vec<Vec<u8>>> = builds.iter().map(|b| b.obj_bytes()).collect();
    assert_eq!(objs[0], objs[1], "cold -j1 and -j2 builds must emit byte-identical objects");
    assert_eq!(objs[0], objs[2], "cold -j1 and -j4 builds must emit byte-identical objects");
    if cc_available() {
        let exes: Vec<Vec<u8>> = builds.iter().map(|b| b.exe_bytes(&proj)).collect();
        assert_eq!(exes[0], exes[1], "cold -j1 and -j2 builds must link a byte-identical exe");
        assert_eq!(exes[0], exes[2], "cold -j1 and -j4 builds must link a byte-identical exe");
    }
}

/// Cross-`-j` HOT-serve: a cold publish at `-j2` into a SHARED cache root must be served
/// byte-identically to hot all-hit builds at `-j4` and `-j1` (objects + exe). This closes the
/// different-`-j` hot-serve gap the settled SV list named alongside S2 gate5 (which pins cold-vs-hit
/// only at the SAME `-j`).
#[test]
fn gate_sv1_cross_jobs_hot_serve_byte_identical() {
    if !backend() {
        return;
    }
    let proj = Proj::dag("det-hotjobs", C_V1);
    let cache = proj.cache(); // ONE shared root
    let cold = thin_build(&proj, &cache, 2);
    assert!(cold.all_miss(), "the first build is cold");
    let cold_objs = cold.obj_bytes();
    let cold_exe = cc_available().then(|| cold.exe_bytes(&proj));

    for jobs in [4usize, 1] {
        let hot = thin_build(&proj, &cache, jobs);
        assert!(hot.all_hit(), "a repeat build at -j{jobs} hits every phase");
        assert_eq!(cold_objs, hot.obj_bytes(), "the -j2 cold objects are served byte-identically at -j{jobs}");
        if let Some(ref exe) = cold_exe {
            assert_eq!(*exe, hot.exe_bytes(&proj), "the -j2 cold exe is served byte-identically at -j{jobs}");
        }
    }
}

/// End-to-end (real driver, subprocess): two cold `alignc build --thin-lto` runs with two distinct
/// `ALIGNC_CACHE` roots produce a byte-identical executable. The exe is deleted before each build (and
/// re-existence asserted) so the comparison can never pass by reading a stale artifact, and each
/// build's coldness/independence is asserted via `--cache-stats` (`0 hit, 4 miss` in both phases).
#[test]
fn gate_sv1_subprocess_build_twice_byte_identical() {
    if !backend() || !cc_available() {
        return;
    }
    let proj = Proj::dag("det-subproc", C_V1);
    let alignc = env!("CARGO_BIN_EXE_alignc");
    let exe_path = proj.dir.join("main");
    let build = |root: &str| -> Vec<u8> {
        let _ = std::fs::remove_file(&exe_path); // never let a stale exe satisfy the comparison
        let out = Command::new(alignc)
            .args(["build", "main.align", "--thin-lto", "--cache-stats", "-p", "release"])
            .current_dir(&proj.dir)
            .env("ALIGNC_CACHE", proj.dir.join(root))
            .output()
            .expect("spawn alignc");
        assert!(out.status.success(), "--thin-lto build failed: {}", String::from_utf8_lossy(&out.stderr));
        let err = String::from_utf8_lossy(&out.stderr);
        assert!(err.contains("prelink: 0 hit, 4 miss"), "the build must be cold (all prelink miss):\n{err}");
        assert!(err.contains("backend: 0 hit, 4 miss"), "the build must be cold (all backend miss):\n{err}");
        assert!(exe_path.exists(), "the build produced the exe");
        std::fs::read(&exe_path).expect("read built exe")
    };
    let a = build("xr-a");
    let b = build("xr-b");
    assert_eq!(a, b, "two cold subprocess --thin-lto builds (distinct cache roots) must be byte-identical");
}

// ================================================================================================
// Gate SV2: summary-level stale-summary fail-closed mutation
// ================================================================================================

/// 2a: replace c's cached PRELINK blob with a STRUCTURALLY-VALID but different `.bc` (unit d's own
/// prelink bitcode). It still parses as bitcode, so only the content-digest check can reject it. The
/// read path must evict + rebuild (`CorruptEntry`), and the deterministic rebuild re-hits the backend
/// object → the exe stays correct.
#[test]
fn gate_sv2a_valid_but_different_prelink_blob_rejected_by_digest() {
    if !backend() {
        return;
    }
    let proj = Proj::dag("stale-valid", C_V1);
    let cache = proj.cache();
    let cold = thin_build(&proj, &cache, 1);
    assert!(cold.all_miss(), "cold build all-miss");

    let root = proj.cache_root();
    let c_bc = std::fs::read(&cold.bc_paths[cold.unit_index("c")]).expect("read c prelink bc");
    let d_bc = std::fs::read(&cold.bc_paths[cold.unit_index("d")]).expect("read d prelink bc");
    assert!(is_llvm_bitcode(&c_bc) && is_llvm_bitcode(&d_bc), "both units' prelink outputs are real bitcode");
    assert_ne!(c_bc, d_bc, "c and d prelink bitcode differ in content");

    // Overwrite c's CAS blob (addressed by c's digest) with d's valid-but-different bitcode.
    let c_blob = cas_blob_path(&root, Hash128::of(&c_bc));
    assert!(c_blob.exists(), "the cold build published c's prelink CAS blob");
    std::fs::write(&c_blob, &d_bc).expect("swap in a valid-but-different .bc");

    let hot = thin_build(&proj, &cache, 1);
    assert_eq!(
        hot.prelink("c").miss_reason,
        Some(FirstDiff::CorruptEntry),
        "a valid-but-different prelink blob is rejected by the CONTENT DIGEST, not a parse failure"
    );
    assert!(hot.backend("c").hit, "the deterministic prelink rebuild re-hits c's (uncorrupted) backend object");
    if cc_available() {
        assert_eq!(hot.run(&proj), DAG_OUT_V1, "the exe is correct after the digest-rejection rebuild");
    }
}

/// 2b: the stale-manifest race shape — publish a valid manifest+blob, then replace the blob with a
/// DIFFERENT valid prelink `.bc` (a different-BODY build of the SAME unit `c`, digest mismatch). Same
/// digest-verify rejection path; proves the mutation is caught even when the replacement is a genuine
/// alternative prelink of the very unit being served. The alternate bitcode is harvested from a small
/// 2-unit variant project (no need for a full 4-unit build).
#[test]
fn gate_sv2b_stale_manifest_different_body_blob_rejected() {
    if !backend() {
        return;
    }
    let proj = Proj::dag("stale-body", C_V1);
    let cache = proj.cache();
    let cold = thin_build(&proj, &cache, 1);
    assert!(cold.all_miss());
    let c_bc_v1 = std::fs::read(&cold.bc_paths[cold.unit_index("c")]).expect("read c v1 prelink bc");

    // Harvest a genuinely different (but valid) prelink .bc for `c` from a minimal 2-unit variant whose
    // c body is edited (C_BODY).
    let alt_main = "import c\nfn main() {\n  print(c.cval())\n}\n";
    let alt = Proj::new("stale-body-alt", &[("c.align", C_BODY), ("main.align", alt_main)], "main.align");
    let alt_built = thin_build(&alt, &CacheContext::Disabled, 1);
    let c_bc_v2 = std::fs::read(&alt_built.bc_paths[alt_built.unit_index("c")]).expect("read c v2 prelink bc");
    assert!(is_llvm_bitcode(&c_bc_v2), "the variant's c prelink is real bitcode");
    assert_ne!(c_bc_v1, c_bc_v2, "the different body yields a different prelink .bc");

    // Publish-then-replace: overwrite the ORIGINAL c CAS blob with the different-body valid bitcode.
    let c_blob = cas_blob_path(&proj.cache_root(), Hash128::of(&c_bc_v1));
    assert!(c_blob.exists());
    std::fs::write(&c_blob, &c_bc_v2).expect("replace with a different valid prelink .bc");

    let hot = thin_build(&proj, &cache, 1);
    assert_eq!(
        hot.prelink("c").miss_reason,
        Some(FirstDiff::CorruptEntry),
        "a stale (different-body but valid) prelink blob is rejected on the content digest"
    );
    assert!(hot.backend("c").hit, "the rebuilt prelink is deterministic → c's backend object still hits");
    if cc_available() {
        assert_eq!(hot.run(&proj), DAG_OUT_V1, "the ORIGINAL c body is rebuilt + served (not the stale swapped body)");
    }
}

// ================================================================================================
// Gate SV3: explicit compile-time regression bound
// ================================================================================================

/// A generous, non-flaky compile-time bound: a COLD `--thin-lto` build's wall-time over the fixed
/// 4-unit chain must stay under `CAP`× the flag-off cold build.
///
/// Why this cannot flake:
///   * Both builds are the SAME real DEBUG-`alignc` subprocess over the SAME sources at the SAME
///     `-p release` profile — the only difference is the `--thin-lto` flag. The large fixed cost
///     (process spawn + a debug-Rust frontend + the `cc` link) is IDENTICAL in both and appears in
///     BOTH numerator and denominator, pulling the ratio toward 1. ThinLTO's own delta (a second opt
///     pipeline pass + a ~0.1 ms thin-link) is a small fraction of that fixed cost.
///   * off/thin are INTERLEAVED within each round (A then B, same round) so a load spike that starts
///     mid-test hits both sides symmetrically — never the block-sequential bias where all `off` runs
///     precede all `thin` runs (`bench/README.md`).
///   * We take the BEST-OF-N min wall-time for each side (the least-scheduler-contended run) and keep
///     `min` (never a mean) — the min discards one-time page-in, so no separate warm-up is needed.
///   * `ALIGNC_CACHE=off` forces every run cold (no reuse skew).
///   * `CAP = 3.0` is ~2× the headroom over the observed ratio (~1.1–1.5× in practice), so ordinary
///     CI scheduler noise cannot cross it.
#[test]
fn gate_sv3_compile_time_regression_bound() {
    if !backend() || !cc_available() {
        return;
    }
    const CAP: f64 = 3.0;
    const ROUNDS: usize = 3;
    let proj = Proj::dag("ct-bound", C_V1);
    let alignc = env!("CARGO_BIN_EXE_alignc");

    let one_build = |thin: bool| -> f64 {
        let mut cmd = Command::new(alignc);
        cmd.args(["build", "main.align", "-p", "release"])
            .current_dir(&proj.dir)
            .env("ALIGNC_CACHE", "off");
        if thin {
            cmd.arg("--thin-lto");
        }
        let t0 = Instant::now();
        let out = cmd.output().expect("spawn alignc");
        let dt = t0.elapsed().as_secs_f64();
        assert!(out.status.success(), "build failed (thin={thin}): {}", String::from_utf8_lossy(&out.stderr));
        dt
    };

    // Interleave off/thin per round; keep the per-side min.
    let mut off = f64::INFINITY;
    let mut thin = f64::INFINITY;
    for _ in 0..ROUNDS {
        off = off.min(one_build(false));
        thin = thin.min(one_build(true));
    }
    let ratio = thin / off;
    assert!(
        ratio < CAP,
        "--thin-lto cold build wall-time regression {ratio:.2}x exceeds the {CAP:.1}x cap (off={off:.3}s, thin={thin:.3}s)"
    );
}

// ================================================================================================
// Gate SV4: invalidation-matrix completion
// ================================================================================================

// `hh` (a deterministic pseudo-`Hash128` from a seed) is hoisted into `common` and shared via the
// `use common::*` glob above.

fn base_prelink_key() -> PrelinkKey {
    PrelinkKey {
        cache_format_version: CACHE_KEY_FORMAT_VERSION,
        compiler_build_id: hh(1),
        frontend_schema: 1,
        located: false,
        impl_hash: hh(2),
        dep_interface_hashes: vec![("c".to_string(), hh(3))],
        exports: vec![],
        target_triple: "x86_64-unknown-linux-gnu".to_string(),
        object_format: 0,
        profile_name: "release".to_string(),
        pipeline: "default<O2>".to_string(),
        llvm_version: "22.1.8".to_string(),
        rt_lto: false,
        rt_lto_digest: None,
        unit: "b".to_string(),
    }
}

fn base_backend_key() -> BackendKey {
    BackendKey {
        cache_format_version: CACHE_KEY_FORMAT_VERSION,
        compiler_build_id: hh(1),
        llvm_version: "22.1.8".to_string(),
        target_triple: "x86_64-unknown-linux-gnu".to_string(),
        object_format: 0,
        resolved_cpu: "x86-64".to_string(),
        resolved_features: String::new(),
        reloc_model: "pic".to_string(),
        code_model: "default".to_string(),
        profile_name: "release".to_string(),
        pipeline: "default<O2>".to_string(),
        codegen_opt: "O2".to_string(),
        own_prelink_digest: hh(10),
        inbound_imports: vec![("c".to_string(), 42u64, true)],
        outbound_exports: vec![7u64],
        import_source_digests: vec![("c".to_string(), hh(11))],
        exports: vec![],
        unit: "b".to_string(),
    }
}

/// Publish `bytes` as unit `key`'s prelink artifact, then look it up with `probe` (a possibly-mutated
/// key). Returns the lookup so a test can assert Hit/Miss + the first-diff reason — the real
/// key/slot/manifest machinery, no backend.
fn publish_then_lookup(bytes: &[u8], key: &PrelinkKey, probe: &PrelinkKey) -> CacheLookup {
    let dir = std::env::temp_dir().join(format!("align-sv4-{}-{}", std::process::id(), thin_nonce()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let cache = CacheContext::at(dir.join("cache"));
    let bc = dir.join("in.bc");
    std::fs::write(&bc, bytes).unwrap();
    cache.publish_prelink(key, &bc);
    let out = dir.join("out.bc");
    let lookup = cache.lookup_prelink(probe, &out);
    let _ = std::fs::remove_dir_all(&dir);
    lookup
}

fn miss_reason(l: &CacheLookup) -> Option<FirstDiff> {
    match l {
        CacheLookup::Hit(_) => None,
        CacheLookup::Miss { reason } => *reason,
    }
}
fn is_hit(l: &CacheLookup) -> bool {
    matches!(l, CacheLookup::Hit(_))
}

/// LLVM-version is a prelink key component: a version change gives a disjoint full key (never mix) and,
/// since it is NOT in the slot core, an end-to-end lookup reports `LlvmVersion`. (Simulates the
/// "second LLVM" matrix row with a single key-component mutation — no real second toolchain.)
#[test]
fn gate_sv4_prelink_llvm_version_component() {
    let a = base_prelink_key();
    let mut b = a.clone();
    b.llvm_version = "23.0.0".to_string();
    assert_ne!(a.full_digest(), b.full_digest(), "different LLVM versions must not share a prelink key");
    assert_eq!(
        miss_reason(&publish_then_lookup(b"BC\xc0\xde-prelink-a", &a, &b)),
        Some(FirstDiff::LlvmVersion),
        "a different LLVM version misses, and the reason names the LLVM version"
    );
    // The same key re-hits its own artifact (baseline: publish and lookup are consistent).
    assert!(is_hit(&publish_then_lookup(b"BC\xc0\xde-prelink-a", &a, &a)), "the identical key re-hits");
}

/// Compiler-build-id is a prelink key component: a build-id change gives a disjoint full key (and a
/// disjoint slot), so two compiler builds never share a prelink artifact.
#[test]
fn gate_sv4_prelink_compiler_build_id_component() {
    let a = base_prelink_key();
    let mut b = a.clone();
    b.compiler_build_id = hh(999);
    assert_ne!(a.full_digest(), b.full_digest(), "different compiler build ids must not share a prelink key");
    // compiler_build_id is part of the slot core, so a changed build id targets a fresh slot → a clean
    // miss (no prior entry to diff), never a stale hit.
    assert_eq!(
        miss_reason(&publish_then_lookup(b"BC\xc0\xde-prelink-b", &a, &b)),
        Some(FirstDiff::NoPriorEntry),
        "a build-id change targets a disjoint slot → no prior entry (never a stale hit)"
    );
}

/// `--rt-lto` on/off produces disjoint prelink keys (never mix); the on-side further keys on the merge
/// digest, so two `--rt-lto` runs with different runtime bitcode also differ.
#[test]
fn gate_sv4_prelink_rt_lto_toggle_disjoint() {
    let off = base_prelink_key();
    let mut on1 = off.clone();
    on1.rt_lto = true;
    on1.rt_lto_digest = Some(hh(500));
    let mut on2 = on1.clone();
    on2.rt_lto_digest = Some(hh(501));

    assert_ne!(off.full_digest(), on1.full_digest(), "--rt-lto on/off must be disjoint prelink keys");
    assert_ne!(on1.full_digest(), on2.full_digest(), "two --rt-lto merge digests must be disjoint prelink keys");

    // End-to-end: the flag-off artifact must NOT be served to an --rt-lto probe (rt_lto is not in the
    // slot core → the slot is found → the diff reports the rt-lto mode).
    assert_eq!(
        miss_reason(&publish_then_lookup(b"BC\xc0\xde-rtlto-off", &off, &on1)),
        Some(FirstDiff::RtLto),
        "an --rt-lto build is not served the flag-off prelink; the reason names the rt-lto mode"
    );

    // And the two never mix: publish BOTH to one root, each re-hits its OWN distinct artifact.
    let dir = std::env::temp_dir().join(format!("align-sv4-rt-{}-{}", std::process::id(), thin_nonce()));
    std::fs::create_dir_all(&dir).unwrap();
    let cache = CacheContext::at(dir.join("cache"));
    let f_off = dir.join("off.bc");
    let f_on = dir.join("on.bc");
    std::fs::write(&f_off, b"OFF-artifact-bytes").unwrap();
    std::fs::write(&f_on, b"ON-artifact-bytes").unwrap();
    cache.publish_prelink(&off, &f_off);
    cache.publish_prelink(&on1, &f_on);
    let out_off = dir.join("r_off.bc");
    let out_on = dir.join("r_on.bc");
    assert!(is_hit(&cache.lookup_prelink(&off, &out_off)) && is_hit(&cache.lookup_prelink(&on1, &out_on)), "both hit their own");
    assert_eq!(std::fs::read(&out_off).unwrap(), b"OFF-artifact-bytes", "the flag-off lookup returns the flag-off artifact");
    assert_eq!(std::fs::read(&out_on).unwrap(), b"ON-artifact-bytes", "the --rt-lto lookup returns the --rt-lto artifact");
    let _ = std::fs::remove_dir_all(&dir);
}

/// The same three components pin the BACKEND key: LLVM version, compiler build id, and (transitively,
/// via `own_prelink_digest`) the `--rt-lto` mode each give a disjoint backend key — a backend object is
/// never shared across them.
#[test]
fn gate_sv4_backend_key_components_disjoint() {
    let base = base_backend_key();

    let mut llvm = base.clone();
    llvm.llvm_version = "23.0.0".to_string();
    assert_ne!(base.full_digest(), llvm.full_digest(), "LLVM version must split the backend key");

    let mut bid = base.clone();
    bid.compiler_build_id = hh(999);
    assert_ne!(base.full_digest(), bid.full_digest(), "compiler build id must split the backend key");

    // --rt-lto changes the unit's prelink bitcode (merged runtime bodies), which is pinned in the
    // backend key as `own_prelink_digest` (and every import-source's digest). A changed prelink digest
    // therefore splits the backend key — this is the rt-lto interaction at the backend phase.
    let mut prelink = base.clone();
    prelink.own_prelink_digest = hh(777);
    assert_ne!(base.full_digest(), prelink.full_digest(), "a changed own prelink digest must split the backend key");

    // Cross-unit inputs (import edges / import-source digests / outbound exports) also split it.
    let mut xunit = base.clone();
    xunit.import_source_digests = vec![("c".to_string(), hh(888))];
    assert_ne!(base.full_digest(), xunit.full_digest(), "a changed import-source digest must split the backend key");
}

/// END-TO-END grounding for the "rt-lto captured transitively through `own_prelink_digest`" claim:
/// building the SAME unit (one that references a runtime primitive, `str ==`) with `--rt-lto` ON vs
/// OFF must yield DIFFERENT prelink `.bc` content — because the runtime bodies are merged BEFORE the
/// prelink `.bc` is hashed (the `build_thin_lto` structural invariant). If they were equal, the
/// backend key's rt-lto sensitivity would rest on the prelink KEY's explicit `rt_lto` bit alone; this
/// proves the bitcode content itself carries the difference.
#[test]
fn gate_sv4_rt_lto_changes_prelink_bitcode_end_to_end() {
    if !backend() {
        return;
    }
    // A 2-unit program whose `lib` references the runtime string-equality primitive.
    let lib = "module lib\npub fn eq(x: str) -> bool = x == \"hello\"\n";
    let main = "import lib\nfn main() {\n  print(lib.eq(\"hello\"))\n}\n";
    let prelink_digest = |rt_lto: bool, tag: &str| -> Hash128 {
        let proj = Proj::new(tag, &[("lib.align", lib), ("main.align", main)], "main.align");
        let entry = proj.dir.join("main.align");
        let entry_src = std::fs::read_to_string(&entry).unwrap();
        let mut sm = SourceMap::new();
        let walk = build_per_unit(&mut sm, &entry.display().to_string(), &entry_src);
        let lib_i = walk.units.iter().position(|u| u.unit == "lib").expect("lib unit");
        let staging = proj.dir.join("stg");
        std::fs::create_dir_all(&staging).unwrap();
        let objs: Vec<PathBuf> = (0..walk.units.len()).map(|i| staging.join(format!("o{i}.o"))).collect();
        let build = build_thin_lto(
            &walk.units, &objs, &CacheContext::Disabled, &BuildTarget::Baseline, Profile::Release, &[], rt_lto, &staging, 1,
        )
        .expect("thin-lto build");
        Hash128::of(&std::fs::read(&build.prelink_bc[lib_i]).expect("read lib prelink bc"))
    };
    let off = prelink_digest(false, "rt-off");
    let on = prelink_digest(true, "rt-on");
    assert_ne!(off, on, "--rt-lto on vs off must change lib's own prelink .bc content (runtime bodies merged pre-prelink)");
}
