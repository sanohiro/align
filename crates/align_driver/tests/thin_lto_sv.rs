//! ThinLTO SV — the verification bundle that CLOSES the ThinLTO arc (S0–SV). It pins what S1
//! (`thin_lto.rs`) and S2 (`thin_lto_cache.rs`) did not:
//!
//!  1. **Build-twice determinism as a permanent gate** — two independent COLD `--thin-lto` builds
//!     (separate cache roots, same inputs, same `-j`) produce byte-identical objects AND a
//!     byte-identical final exe, at N=4 with real cross-unit imports; plus cold determinism across
//!     DIFFERENT `-j` values with no cache reuse (separate cold roots, `-j1` vs `-j4`).
//!  2. **Summary-level stale-summary fail-closed mutation** — the S2 corruption gate replaces a
//!     cached prelink blob with NON-bitcode garbage; SV replaces it with a *structurally valid but
//!     different* prelink `.bc` (a sibling unit's bitcode in 2a, a different-body build of the same
//!     unit in 2b). The read path must reject on the CONTENT DIGEST (not on a bitcode-parse failure)
//!     → `CorruptEntry` eviction + deterministic rebuild + correct exe. This proves the digest-verify
//!     is what closes the hole, independent of whether the swapped bytes parse.
//!  3. **Explicit compile-time regression bound** — a COLD `--thin-lto` build's wall-time vs the
//!     flag-off cold build on the fixed 4-unit chain stays under a generous cap (best-of-N min over
//!     the real DEBUG-`alignc` subprocess; see `gate_sv3` for why this cannot flake).
//!  4. **Invalidation-matrix completion (unit level)** — the LLVM-version, compiler-build-id, and
//!     `--rt-lto` components of the prelink/backend keys each yield disjoint keys (never mix), a
//!     `--rt-lto`-on/off pair produces disjoint prelink keys, and two `--rt-lto` digests differ. Pure
//!     key/manifest tests (no real second LLVM, no backend), simulating each matrix row by mutating
//!     one key component.
//!
//! Hole finding (2a): the read path (`materialize_blob`) DOES digest-verify every CAS blob against the
//! manifest's stored `blob_digest` before use — shared by both ThinLTO phases via `try_hit_phase`. So
//! 2a found NO hole; the rejection is by digest, which SV proves holds for valid-but-different bitcode.

use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Instant;

use align_driver::{
    build_per_unit, build_thin_lto, link_objects, BackendKey, BuildTarget, CacheContext,
    CacheLookup, CacheOutcome, CacheStage, FirstDiff, Hash128, InboundImport, PrelinkKey, Profile,
};
use align_span::SourceMap;

static NONCE: AtomicU64 = AtomicU64::new(0);
fn nonce() -> u64 {
    NONCE.fetch_add(1, Ordering::Relaxed)
}

fn backend() -> bool {
    align_driver::backend_available()
}

fn cc_available() -> bool {
    Command::new("cc")
        .arg("--version")
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .map(|s| s.success())
        .unwrap_or(false)
}

/// The 4-unit DAG with real cross-unit imports: main → {b → c, d}. `c` has a private helper (an
/// editable body behind a stable interface). Duplicated from `thin_lto_cache.rs` — each integration
/// test file is its own crate.
const C_V1: &str = "module c\nfn helper(x: i64) -> i64 = x * 2\npub fn cval() -> i64 = helper(10)\n";
const C_BODY: &str = "module c\nfn helper(x: i64) -> i64 = x * 3\npub fn cval() -> i64 = helper(10)\n";
const B_SRC: &str = "module b\nimport c\npub fn bval() -> i64 = c.cval() + 100\n";
const D_SRC: &str = "module d\npub fn dval() -> i64 = 5\n";
const MAIN_SRC: &str = "import b\nimport d\nfn main() {\n  print(b.bval() + d.dval())\n}\n";

fn dag_files(cver: &str) -> [(&'static str, String); 4] {
    [
        ("c.align", cver.to_string()),
        ("b.align", B_SRC.to_string()),
        ("d.align", D_SRC.to_string()),
        ("main.align", MAIN_SRC.to_string()),
    ]
}

/// A throwaway multi-file project (removed on drop).
struct Proj {
    dir: PathBuf,
}

impl Proj {
    fn dag(tag: &str, cver: &str) -> Proj {
        let dir = std::env::temp_dir().join(format!("align-thin-sv-{}-{tag}-{}", std::process::id(), nonce()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("mkdir project");
        let proj = Proj { dir };
        for (name, src) in dag_files(cver) {
            proj.write(name, &src);
        }
        proj
    }
    fn write(&self, name: &str, src: &str) {
        std::fs::write(self.dir.join(name), src).expect("write source");
    }
}

impl Drop for Proj {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.dir);
    }
}

/// One library-level `--thin-lto` build over `proj`'s DAG. Fresh per-unit walk, fresh staging dir, so
/// a hit copies the CAS blob into a clean file. Returns the two-phase outcomes + artifact paths.
struct ThinBuilt {
    outcomes: Vec<CacheOutcome>,
    objs: Vec<PathBuf>,
    bc_paths: Vec<PathBuf>,
    link_libs: Vec<String>,
    units: Vec<String>,
}

fn thin_build(proj: &Proj, cache: &CacheContext, jobs: usize) -> ThinBuilt {
    let entry = proj.dir.join("main.align");
    let entry_src = std::fs::read_to_string(&entry).expect("read entry");
    let mut sm = SourceMap::new();
    let walk = build_per_unit(&mut sm, &entry.display().to_string(), &entry_src);
    assert!(!walk.diags.has_errors(), "unexpected build errors");
    let n = walk.units.len();
    let staging = proj.dir.join(format!("stg-{}", nonce()));
    std::fs::create_dir_all(&staging).expect("mkdir staging");
    let objs: Vec<PathBuf> = (0..n).map(|i| staging.join(format!("o{i}.o"))).collect();
    let bc_paths: Vec<PathBuf> = (0..n).map(|i| staging.join(format!("unit{i}.prelink.bc"))).collect();
    let outcomes = build_thin_lto(
        &walk.units, &objs, cache, &BuildTarget::Baseline, Profile::Release, &[], false, &staging, jobs,
    )
    .expect("thin-lto build");
    let mut link_libs: Vec<String> = Vec::new();
    for u in &walk.units {
        for l in &u.mir.link_libs {
            if !link_libs.contains(l) {
                link_libs.push(l.clone());
            }
        }
    }
    ThinBuilt {
        outcomes,
        objs,
        bc_paths,
        link_libs,
        units: walk.units.iter().map(|u| u.unit.clone()).collect(),
    }
}

impl ThinBuilt {
    fn phase(&self, unit: &str, stage: CacheStage) -> &CacheOutcome {
        self.outcomes
            .iter()
            .find(|o| o.unit == unit && o.stage == stage)
            .unwrap_or_else(|| panic!("no {stage:?} outcome for `{unit}`"))
    }
    fn prelink(&self, unit: &str) -> &CacheOutcome {
        self.phase(unit, CacheStage::ThinLtoPrelink)
    }
    fn backend(&self, unit: &str) -> &CacheOutcome {
        self.phase(unit, CacheStage::ThinLtoBackend)
    }
    fn unit_index(&self, unit: &str) -> usize {
        self.units.iter().position(|u| u == unit).expect("unit present")
    }
    fn obj_bytes(&self) -> Vec<Vec<u8>> {
        self.objs.iter().map(|p| std::fs::read(p).expect("read obj")).collect()
    }
    fn exe_bytes(&self, proj: &Proj) -> Vec<u8> {
        let obj_refs: Vec<&Path> = self.objs.iter().map(|p| p.as_path()).collect();
        let exe = proj.dir.join(format!("exe-{}", nonce()));
        link_objects(&obj_refs, &exe, &self.link_libs, Profile::Release).expect("link");
        std::fs::read(&exe).expect("read exe")
    }
    fn run(&self, proj: &Proj) -> String {
        let obj_refs: Vec<&Path> = self.objs.iter().map(|p| p.as_path()).collect();
        let exe = proj.dir.join(format!("exe-{}", nonce()));
        link_objects(&obj_refs, &exe, &self.link_libs, Profile::Release).expect("link");
        String::from_utf8_lossy(&Command::new(&exe).output().expect("run").stdout).into_owned()
    }
}

/// The `cas/<hex[..2]>/<hex>` blob path for a content digest under `root`.
fn cas_blob_path(root: &Path, digest: Hash128) -> PathBuf {
    let hex = digest.to_hex();
    root.join("cas").join(&hex[..2]).join(&hex)
}

/// LLVM bitcode magic (`BC\xC0\xDE`). Used to self-document that a swapped-in blob still *parses* as
/// bitcode, so the rejection can only be the content-digest check (not a parse failure).
fn is_llvm_bitcode(bytes: &[u8]) -> bool {
    bytes.starts_with(&[0x42, 0x43, 0xC0, 0xDE])
}

// ================================================================================================
// Gate SV1: build-twice determinism (permanent gate)
// ================================================================================================

/// Two independent COLD `--thin-lto` builds — separate cache roots, same inputs, same `-j` — produce
/// byte-identical objects AND a byte-identical final exe. N=4 with real cross-unit imports (main →
/// {b → c, d}), so promotion (`.llvm.<hash>` names) + import/export decisions must be stable.
#[test]
fn gate_sv1_build_twice_determinism_separate_roots() {
    if !backend() {
        return;
    }
    let proj = Proj::dag("det-roots", C_V1);
    let root_a = CacheContext::at(proj.dir.join("cache-a"));
    let root_b = CacheContext::at(proj.dir.join("cache-b"));
    // Both builds are COLD (each root is fresh) and use the SAME -j.
    let a = thin_build(&proj, &root_a, 2);
    let b = thin_build(&proj, &root_b, 2);
    assert!(a.outcomes.iter().all(|o| !o.hit), "build A is cold (fresh root)");
    assert!(b.outcomes.iter().all(|o| !o.hit), "build B is cold (fresh root)");
    assert_eq!(a.obj_bytes(), b.obj_bytes(), "two cold --thin-lto builds must emit byte-identical objects");
    if cc_available() {
        assert_eq!(a.exe_bytes(&proj), b.exe_bytes(&proj), "two cold --thin-lto builds must link a byte-identical exe");
    }
}

/// Cold determinism across DIFFERENT `-j` with NO cache reuse: two separate cold roots, one at `-j1`
/// and one at `-j4`, must still emit byte-identical objects + exe. (S2 gate6 pins `-j1 == -j4` with
/// cache DISABLED; this pins it with the cache ENABLED-but-cold, so the parallel claim order can't
/// leak into a cached artifact either.)
#[test]
fn gate_sv1_cold_determinism_across_jobs() {
    if !backend() {
        return;
    }
    let proj = Proj::dag("det-jobs", C_V1);
    let serial = thin_build(&proj, &CacheContext::at(proj.dir.join("cache-j1")), 1);
    let parallel = thin_build(&proj, &CacheContext::at(proj.dir.join("cache-j4")), 4);
    assert!(serial.outcomes.iter().all(|o| !o.hit) && parallel.outcomes.iter().all(|o| !o.hit), "both cold");
    assert_eq!(serial.obj_bytes(), parallel.obj_bytes(), "-j1 and -j4 cold builds must emit byte-identical objects");
    if cc_available() {
        assert_eq!(serial.exe_bytes(&proj), parallel.exe_bytes(&proj), "-j1 and -j4 cold builds must link a byte-identical exe");
    }
}

/// End-to-end (real driver, subprocess): two cold `alignc build --thin-lto` runs with two distinct
/// `ALIGNC_CACHE` roots produce a byte-identical executable.
#[test]
fn gate_sv1_subprocess_build_twice_byte_identical() {
    if !backend() || !cc_available() {
        return;
    }
    let proj = Proj::dag("det-subproc", C_V1);
    let alignc = env!("CARGO_BIN_EXE_alignc");
    let build = |root: &str| -> Vec<u8> {
        let out = Command::new(alignc)
            .args(["build", "main.align", "--thin-lto", "-p", "release"])
            .current_dir(&proj.dir)
            .env("ALIGNC_CACHE", proj.dir.join(root))
            .output()
            .expect("spawn alignc");
        assert!(out.status.success(), "--thin-lto build failed: {}", String::from_utf8_lossy(&out.stderr));
        std::fs::read(proj.dir.join("main")).expect("read built exe")
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
    let cache = CacheContext::at(proj.dir.join("cache"));
    let cold = thin_build(&proj, &cache, 1);
    assert!(cold.outcomes.iter().all(|o| !o.hit), "cold build all-miss");

    let root = proj.dir.join("cache");
    let c_bc = std::fs::read(&cold.bc_paths[cold.unit_index("c")]).expect("read c prelink bc");
    let d_bc = std::fs::read(&cold.bc_paths[cold.unit_index("d")]).expect("read d prelink bc");
    assert!(is_llvm_bitcode(&c_bc) && is_llvm_bitcode(&d_bc), "both units' prelink outputs are real bitcode");
    assert_ne!(c_bc, d_bc, "c and d prelink bitcode differ in content");

    // Overwrite c's CAS blob (addressed by c's digest) with d's valid-but-different bitcode.
    let c_blob = cas_blob_path(&root, Hash128::of(&c_bc));
    assert!(c_blob.exists(), "the cold build published c's prelink CAS blob");
    std::fs::write(&c_blob, &d_bc).expect("swap in a valid-but-different .bc");

    let hot = thin_build(&proj, &cache, 1);
    assert!(!hot.prelink("c").hit, "a content-mismatched (but valid) prelink blob is not served");
    assert_eq!(
        hot.prelink("c").miss_reason,
        Some(FirstDiff::CorruptEntry),
        "a valid-but-different prelink blob is rejected by the CONTENT DIGEST, not a parse failure"
    );
    assert!(hot.backend("c").hit, "the deterministic prelink rebuild re-hits c's (uncorrupted) backend object");
    if cc_available() {
        assert_eq!(hot.run(&proj), "125\n", "the exe is correct after the digest-rejection rebuild");
    }
}

/// 2b: the stale-manifest race shape — publish a valid manifest+blob, then replace the blob with a
/// DIFFERENT valid prelink `.bc` (a different-BODY build of the SAME unit `c`, digest mismatch). Same
/// digest-verify rejection path; proves the mutation is caught even when the replacement is a genuine
/// alternative prelink of the very unit being served.
#[test]
fn gate_sv2b_stale_manifest_different_body_blob_rejected() {
    if !backend() {
        return;
    }
    let proj = Proj::dag("stale-body", C_V1);
    let cache = CacheContext::at(proj.dir.join("cache"));
    let cold = thin_build(&proj, &cache, 1);
    assert!(cold.outcomes.iter().all(|o| !o.hit));
    let c_bc_v1 = std::fs::read(&cold.bc_paths[cold.unit_index("c")]).expect("read c v1 prelink bc");

    // Produce a genuinely different (but valid) prelink .bc for `c` by building a variant project whose
    // c body is edited. `alt.bc_paths` are in the variant's own staging.
    let alt_proj = Proj::dag("stale-body-alt", C_BODY);
    let alt = thin_build(&alt_proj, &CacheContext::Disabled, 1);
    let c_bc_v2 = std::fs::read(&alt.bc_paths[alt.unit_index("c")]).expect("read c v2 prelink bc");
    assert!(is_llvm_bitcode(&c_bc_v2), "the variant's c prelink is real bitcode");
    assert_ne!(c_bc_v1, c_bc_v2, "the different body yields a different prelink .bc");

    // Publish-then-replace: overwrite the ORIGINAL c CAS blob with the different-body valid bitcode.
    let root = proj.dir.join("cache");
    let c_blob = cas_blob_path(&root, Hash128::of(&c_bc_v1));
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
        assert_eq!(hot.run(&proj), "125\n", "the ORIGINAL c body is rebuilt + served (not the stale swapped body)");
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
///   * We take the BEST-OF-N min wall-time for each side (the least-scheduler-contended run), so the
///     ratio compares two reproducible best cases rather than two noisy samples.
///   * `ALIGNC_CACHE=off` forces every run cold (no reuse skew).
///   * `CAP = 3.0` is ~2× the headroom over the observed ratio (~1.1–1.5× in practice), so ordinary
///     CI scheduler noise cannot cross it.
#[test]
fn gate_sv3_compile_time_regression_bound() {
    if !backend() || !cc_available() {
        return;
    }
    const CAP: f64 = 3.0;
    const ROUNDS: usize = 5;
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
    let best = |thin: bool| (0..ROUNDS).map(|_| one_build(thin)).fold(f64::INFINITY, f64::min);

    // Warm up (page-in the alignc binary + link toolchain) so neither side pays a one-time cost.
    let _ = one_build(false);
    let off = best(false);
    let thin = best(true);
    let ratio = thin / off;
    assert!(
        ratio < CAP,
        "--thin-lto cold build wall-time regression {ratio:.2}x exceeds the {CAP:.1}x cap (off={off:.3}s, thin={thin:.3}s)"
    );
}

// ================================================================================================
// Gate SV4: invalidation-matrix completion (unit level — key/manifest, no backend)
// ================================================================================================

fn h(seed: u64) -> Hash128 {
    Hash128 { lo: seed, hi: seed.wrapping_mul(0x9E37_79B9_7F4A_7C15) }
}

fn base_prelink_key() -> PrelinkKey {
    PrelinkKey {
        cache_format_version: 2,
        compiler_build_id: h(1),
        frontend_schema: 1,
        located: false,
        impl_hash: h(2),
        dep_interface_hashes: vec![("c".to_string(), h(3))],
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
        cache_format_version: 2,
        compiler_build_id: h(1),
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
        own_prelink_digest: h(10),
        inbound_imports: vec![("c".to_string(), 42u64, true) as InboundImport],
        outbound_exports: vec![7u64],
        import_source_digests: vec![("c".to_string(), h(11))],
        exports: vec![],
        unit: "b".to_string(),
    }
}

/// Publish `bytes` as unit `key`'s prelink artifact, then look it up with `probe` (a possibly-mutated
/// key). Returns the lookup so a test can assert Hit/Miss + the first-diff reason — the real
/// key/slot/manifest machinery, no backend.
fn publish_then_lookup(bytes: &[u8], key: &PrelinkKey, probe: &PrelinkKey) -> CacheLookup {
    let dir = std::env::temp_dir().join(format!("align-sv4-{}-{}", std::process::id(), nonce()));
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
    let l = publish_then_lookup(b"BC\xc0\xde-prelink-a", &a, &b);
    assert!(!is_hit(&l), "a different LLVM version must not hit");
    assert_eq!(miss_reason(&l), Some(FirstDiff::LlvmVersion), "the miss reason names the LLVM version");
    // The same key re-hits its own artifact (baseline: publish and lookup are consistent).
    assert!(is_hit(&publish_then_lookup(b"BC\xc0\xde-prelink-a", &a, &a)), "the identical key re-hits");
}

/// Compiler-build-id is a prelink key component: a build-id change gives a disjoint full key (and a
/// disjoint slot), so two compiler builds never share a prelink artifact.
#[test]
fn gate_sv4_prelink_compiler_build_id_component() {
    let a = base_prelink_key();
    let mut b = a.clone();
    b.compiler_build_id = h(999);
    assert_ne!(a.full_digest(), b.full_digest(), "different compiler build ids must not share a prelink key");
    // compiler_build_id is part of the slot core, so a changed build id targets a fresh slot → a clean
    // miss (no prior entry to diff), never a stale hit.
    let l = publish_then_lookup(b"BC\xc0\xde-prelink-b", &a, &b);
    assert!(!is_hit(&l), "a different compiler build id must not hit");
    assert_eq!(miss_reason(&l), Some(FirstDiff::NoPriorEntry), "a build-id change targets a disjoint slot → no prior entry");
}

/// `--rt-lto` on/off produces disjoint prelink keys (never mix); the on-side further keys on the merge
/// digest, so two `--rt-lto` runs with different runtime bitcode also differ.
#[test]
fn gate_sv4_prelink_rt_lto_toggle_disjoint() {
    let off = base_prelink_key();
    let mut on1 = off.clone();
    on1.rt_lto = true;
    on1.rt_lto_digest = Some(h(500));
    let mut on2 = on1.clone();
    on2.rt_lto_digest = Some(h(501));

    assert_ne!(off.full_digest(), on1.full_digest(), "--rt-lto on/off must be disjoint prelink keys");
    assert_ne!(on1.full_digest(), on2.full_digest(), "two --rt-lto merge digests must be disjoint prelink keys");

    // End-to-end: publishing the flag-off artifact must NOT be served to an --rt-lto probe (rt_lto is
    // not in the slot core → the slot is found → the diff reports the rt-lto mode).
    let l = publish_then_lookup(b"BC\xc0\xde-rtlto-off", &off, &on1);
    assert!(!is_hit(&l), "an --rt-lto build must not be served the flag-off prelink");
    assert_eq!(miss_reason(&l), Some(FirstDiff::RtLto), "the miss reason names the rt-lto mode");

    // And the two never mix: publish BOTH to one root, each re-hits its OWN distinct artifact.
    let dir = std::env::temp_dir().join(format!("align-sv4-rt-{}-{}", std::process::id(), nonce()));
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
    bid.compiler_build_id = h(999);
    assert_ne!(base.full_digest(), bid.full_digest(), "compiler build id must split the backend key");

    // --rt-lto changes the unit's prelink bitcode (merged runtime bodies), which is pinned in the
    // backend key as `own_prelink_digest` (and every import-source's digest). A changed prelink digest
    // therefore splits the backend key — this is the rt-lto interaction at the backend phase.
    let mut prelink = base.clone();
    prelink.own_prelink_digest = h(777);
    assert_ne!(base.full_digest(), prelink.full_digest(), "a changed own prelink digest must split the backend key");

    // Cross-unit inputs (import edges / import-source digests / outbound exports) also split it.
    let mut xunit = base.clone();
    xunit.import_source_digests = vec![("c".to_string(), h(888))];
    assert_ne!(base.full_digest(), xunit.full_digest(), "a changed import-source digest must split the backend key");
}
