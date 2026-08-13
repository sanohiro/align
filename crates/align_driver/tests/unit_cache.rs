//! Owner tests for the persistent per-unit **frontend** cache
//! (`docs/impl/10-cache-first-optimization.md` §6.7, closure matrix P1–P34).
//!
//! The codec, key, and first-diff cells live beside the code in `src/unit_cache.rs`; the fid-space
//! and memo-complexity cells live in `src/lib.rs`. This file owns the behavioral cells: what a hit
//! serves, what a miss recomputes, what a tampered entry does, and what none of it changes.
//!
//! Every test builds into its OWN temp project directory and its OWN cache root, so nothing here
//! depends on test ordering, on the ambient `ALIGNC_CACHE`, or on the user's cache. Nothing mutates
//! the environment: a test that needs a toggle or a different `ALIGNC_CACHE` spawns `alignc`.

use align_driver::unit_cache::{self, DepState, EnvToggle, UnitEntry, UnitKey};
use align_driver::{
    build_package, build_per_unit_located, CacheContext, PackageBuild, UnitReuse,
};
use align_span::SourceMap;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

// ---- harness ---------------------------------------------------------------------------------
//
// Deliberately local rather than reaching into `tests/common/mod.rs`: this is a leaf owner test,
// and the shared harness is code-tier infrastructure that one file's needs must not reshape.

static NONCE: AtomicU64 = AtomicU64::new(0);

/// Serializes every test in this file.
///
/// The in-process memo is process-global and every test here builds, so two tests running
/// concurrently would interleave their `memo::stats()` contributions and make the promotion cell
/// (P2-1) unobservable. Cargo already runs each integration test target in its own process, so this
/// lock is confined to this file — and the suite is a few seconds either way.
fn serial() -> std::sync::MutexGuard<'static, ()> {
    static LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());
    LOCK.lock().unwrap_or_else(|poisoned| poisoned.into_inner())
}

fn unique(tag: &str) -> PathBuf {
    let n = NONCE.fetch_add(1, Ordering::Relaxed);
    std::env::temp_dir().join(format!("align-unitcache-{}-{}-{tag}", std::process::id(), n))
}

/// A temp project plus its own cache root, both removed on drop.
struct Proj {
    dir: PathBuf,
    cache: PathBuf,
}

impl Proj {
    fn new(tag: &str, files: &[(&str, &str)]) -> Proj {
        let root = unique(tag);
        let proj = Proj { dir: root.join("proj"), cache: root.join("cache") };
        std::fs::create_dir_all(&proj.dir).expect("create project dir");
        for (name, src) in files {
            proj.write(name, src);
        }
        proj
    }

    fn write(&self, name: &str, src: &str) {
        let path = self.dir.join(name);
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).expect("create module dir");
        }
        std::fs::write(path, src).expect("write module");
    }

    fn entry(&self) -> String {
        self.dir.join("main.align").display().to_string()
    }

    fn context(&self) -> CacheContext {
        CacheContext::at(self.cache.clone())
    }

    /// One package build against this project's own cache root.
    fn build(&self, reuse: UnitReuse) -> PackageBuild {
        let src = std::fs::read_to_string(self.dir.join("main.align")).expect("read entry");
        let mut sm = SourceMap::new();
        let build = build_package(&mut sm, &self.entry(), &src, &self.context(), reuse);
        assert!(
            !build.diags.has_errors(),
            "unexpected errors:\n{}",
            align_driver::format_diagnostics(&sm, &build.diags)
        );
        build
    }

    /// A build that is allowed to report errors.
    fn build_dirty(&self, reuse: UnitReuse) -> (PackageBuild, String) {
        let src = std::fs::read_to_string(self.dir.join("main.align")).expect("read entry");
        let mut sm = SourceMap::new();
        let build = build_package(&mut sm, &self.entry(), &src, &self.context(), reuse);
        let rendered = align_driver::format_diagnostics(&sm, &build.diags);
        (build, rendered)
    }

    fn unit_entries(&self) -> Vec<PathBuf> {
        list(&self.cache.join("actions").join("unit"))
    }

    /// Drop only the OBJECT stage, keeping every frontend entry: the "frontend hits, codegen
    /// misses" state a profile switch produces, and the only state that forces rehydration.
    fn clear_objects(&self) {
        let _ = std::fs::remove_dir_all(self.cache.join("actions").join("codegen"));
        let _ = std::fs::remove_dir_all(self.cache.join("cas"));
    }
}

impl Drop for Proj {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(self.dir.parent().unwrap_or(&self.dir));
    }
}

fn list(dir: &Path) -> Vec<PathBuf> {
    let Ok(entries) = std::fs::read_dir(dir) else { return Vec::new() };
    let mut out: Vec<PathBuf> = entries.filter_map(|e| e.ok().map(|e| e.path())).collect();
    out.sort();
    out
}

fn frontend(build: &PackageBuild, unit: &str) -> Option<align_driver::CacheOutcome> {
    build.units.iter().find(|u| u.unit == unit).and_then(|u| u.frontend.clone())
}

fn hit(build: &PackageBuild, unit: &str) -> bool {
    frontend(build, unit).is_some_and(|o| o.hit)
}

fn miss_reason(build: &PackageBuild, unit: &str) -> Option<align_driver::FirstDiff> {
    frontend(build, unit).and_then(|o| o.miss_reason)
}

/// A two-module project: `lib` exports a function `main` calls.
fn two_module(tag: &str) -> Proj {
    Proj::new(
        tag,
        &[
            ("lib.align", "module lib\n\npub fn twice(n: i64) -> i64 = n * 2\n\nfn helper() -> i64 = 1\n"),
            ("main.align", "import lib\n\nfn main() {\n    print(lib.twice(21))\n}\n"),
        ],
    )
}

// ---- P1 / P3: what an edit invalidates -------------------------------------------------------

#[test]
fn a_private_body_edit_misses_only_its_own_unit() {
    let _serial = serial();
    let proj = two_module("private-edit");
    let cold = proj.build(UnitReuse::Allowed);
    assert!(!hit(&cold, "lib") && !hit(&cold, "main"));
    assert_eq!(miss_reason(&cold, "lib"), Some(align_driver::FirstDiff::NoPriorEntry));

    // A private body edit: the public surface is byte-identical, so `lib`'s interface_hash and its
    // serialized summary are unchanged and `main` must still hit.
    proj.write(
        "lib.align",
        "module lib\n\npub fn twice(n: i64) -> i64 = n * 2\n\nfn helper() -> i64 = 2\n",
    );
    let warm = proj.build(UnitReuse::Allowed);
    assert!(!hit(&warm, "lib"), "the edited unit must miss");
    assert_eq!(miss_reason(&warm, "lib"), Some(align_driver::FirstDiff::UnitSource));
    assert!(hit(&warm, "main"), "a private body edit must not invalidate a consumer");
}

#[test]
fn a_public_surface_edit_misses_the_reverse_dependent() {
    let _serial = serial();
    let proj = two_module("surface-edit");
    proj.build(UnitReuse::Allowed);
    proj.write("lib.align", "module lib\n\npub fn twice(n: i64) -> i64 = n * 2\n\npub fn added() -> i64 = 3\n");
    let warm = proj.build(UnitReuse::Allowed);
    assert!(!hit(&warm, "lib"));
    assert!(!hit(&warm, "main"), "a changed public surface must invalidate the consumer");
    assert_eq!(
        miss_reason(&warm, "main"),
        Some(align_driver::FirstDiff::DepInterfaceHashes),
        "and it must be attributed to the dependency, not to main's own source"
    );
}

// ---- P2: the key is content-only -------------------------------------------------------------

#[test]
fn the_same_package_in_a_different_directory_hits() {
    let _serial = serial();
    let first = two_module("relocate-a");
    let cold = first.build(UnitReuse::Allowed);
    assert!(!hit(&cold, "lib"));

    // A different project directory, a different entry path, the SAME sources — and the same cache
    // root. No absolute path may enter the key, so every unit must hit.
    let second = Proj::new(
        "relocate-b",
        &[
            ("lib.align", "module lib\n\npub fn twice(n: i64) -> i64 = n * 2\n\nfn helper() -> i64 = 1\n"),
            ("main.align", "import lib\n\nfn main() {\n    print(lib.twice(21))\n}\n"),
        ],
    );
    std::fs::create_dir_all(&second.cache).ok();
    // Point the second project at the first project's cache root.
    let relocated = Proj { dir: second.dir.clone(), cache: first.cache.clone() };
    let warm = relocated.build(UnitReuse::Allowed);
    assert!(hit(&warm, "lib") && hit(&warm, "main"), "the key must be content-only");
}

// ---- P4: a dependency's own closure is key material ------------------------------------------

#[test]
fn a_dependency_closure_change_misses_its_dependents() {
    let _serial = serial();
    // `mid` re-exports nothing from `leaf`, so adding the import leaves `mid`'s summary shape the
    // same size — but `mid`'s rendered interface depends on its own closure, so `main` must miss.
    let proj = Proj::new(
        "closure-change",
        &[
            ("leaf.align", "module leaf\n\npub fn one() -> i64 = 1\n"),
            ("mid.align", "module mid\n\npub fn two() -> i64 = 2\n"),
            ("main.align", "import mid\n\nfn main() {\n    print(mid.two())\n}\n"),
        ],
    );
    proj.build(UnitReuse::Allowed);
    proj.write("mid.align", "module mid\n\nimport leaf\n\npub fn two() -> i64 = 2\n");
    let warm = proj.build(UnitReuse::Allowed);
    assert!(!hit(&warm, "mid"), "mid's own source changed");
    assert!(
        !hit(&warm, "main"),
        "mid's transitive closure grew, which changes the interface source main is checked against"
    );
}

// ---- P8 / P9: cross-process reuse is indistinguishable from a cold build ----------------------

#[test]
fn a_reused_build_equals_a_cold_build() {
    let _serial = serial();
    let proj = two_module("equivalence");

    // Cold, with reuse FORBIDDEN: the reference result, produced exactly as before this cache
    // existed.
    let reference = proj.build(UnitReuse::Forbidden);
    assert!(reference.units.iter().all(|u| u.frontend.is_none()), "a forbidden walk declines the stage");
    let ref_shape: Vec<(String, Vec<u8>, Vec<String>, bool)> = reference
        .units
        .iter()
        .map(|u| {
            (
                u.unit.clone(),
                align_interface::serialize(&u.summary),
                u.link_libs.clone(),
                u.is_reused(),
            )
        })
        .collect();

    // Populate, then serve.
    proj.build(UnitReuse::Allowed);
    let warm = proj.build(UnitReuse::Allowed);
    assert!(warm.units.iter().all(|u| u.is_reused()), "every unit should now come from disk");
    let warm_shape: Vec<(String, Vec<u8>, Vec<String>, bool)> = warm
        .units
        .iter()
        .map(|u| {
            (
                u.unit.clone(),
                align_interface::serialize(&u.summary),
                u.link_libs.clone(),
                false,
            )
        })
        .collect();
    assert_eq!(
        ref_shape, warm_shape,
        "unit order, canonical summary bytes (hence both hashes), and link libraries must match"
    );
}

#[test]
fn a_second_process_reuses_the_first_process_frontend() {
    let _serial = serial();
    let proj = two_module("cross-process");
    let exe = env!("CARGO_BIN_EXE_alignc");
    let run = |label: &str| -> String {
        let out = std::process::Command::new(exe)
            .args(["build", &proj.entry(), "--cache-stats"])
            .env("ALIGNC_CACHE", &proj.cache)
            .current_dir(&proj.dir)
            .output()
            .unwrap_or_else(|e| panic!("{label}: spawn alignc: {e}"));
        String::from_utf8_lossy(&out.stderr).into_owned()
    };
    let cold = run("cold");
    assert!(cold.contains("lib frontend miss"), "cold stderr:\n{cold}");
    let warm = run("warm");
    assert!(
        warm.contains("lib frontend hit") && warm.contains("main frontend hit"),
        "a second PROCESS must serve both units from disk; warm stderr:\n{warm}"
    );
    assert!(
        warm.contains("2 frontend: 2 hit, 0 miss"),
        "the frontend block must summarize; warm stderr:\n{warm}"
    );
    // The codegen block is unchanged and still printed after it.
    let frontend_at = warm.find("2 frontend:").expect("frontend summary");
    let codegen_at = warm.find("2 unit(s):").expect("codegen summary");
    assert!(frontend_at < codegen_at, "the frontend block precedes the unchanged codegen block");
}

// ---- P10 / P10b: diagnostics ------------------------------------------------------------------

/// A warning-bearing unit. A clean unit that still warns is the common case — `pkg.db` alone emits
/// sixteen — so warnings must be REPLAYED, not used to disqualify the unit.
fn warning_project(tag: &str) -> Proj {
    Proj::new(
        tag,
        &[
            (
                "lib.align",
                "module lib\n\npub fn narrow(n: i64) -> i32 = n as i32\n",
            ),
            ("main.align", "import lib\n\nfn main() {\n    print(lib.narrow(7))\n}\n"),
        ],
    )
}

#[test]
fn warnings_replay_verbatim_and_exactly_once() {
    let _serial = serial();
    let proj = warning_project("warn-replay");
    let src = std::fs::read_to_string(proj.dir.join("main.align")).expect("read entry");

    let mut cold_map = SourceMap::new();
    let cold = build_package(&mut cold_map, &proj.entry(), &src, &proj.context(), UnitReuse::Allowed);
    let cold_text = align_driver::format_diagnostics(&cold_map, &cold.diags);
    assert!(
        cold_text.contains("lossy conversion"),
        "the fixture must actually warn, else this test proves nothing:\n{cold_text}"
    );

    let mut warm_map = SourceMap::new();
    let warm = build_package(&mut warm_map, &proj.entry(), &src, &proj.context(), UnitReuse::Allowed);
    let warm_text = align_driver::format_diagnostics(&warm_map, &warm.diags);
    assert!(hit(&warm, "lib"), "a warning must not disqualify the unit from being cached");
    assert_eq!(
        cold_text, warm_text,
        "severity, message, offsets, order, AND the rendered file/line must be identical"
    );

    // P10b: force a rehydration on top of the hit. The unit's diagnostics were already replayed, so
    // recomputing must VERIFY them, never append them a second time.
    proj.clear_objects();
    let mut rehydrated_map = SourceMap::new();
    let mut rehydrated =
        build_package(&mut rehydrated_map, &proj.entry(), &src, &proj.context(), UnitReuse::Allowed);
    let index = rehydrated.units.iter().position(|u| u.unit == "lib").expect("lib unit");
    assert!(rehydrated.units[index].is_reused());
    rehydrated.materialize(index).expect("rehydration must verify");
    let rehydrated_text = align_driver::format_diagnostics(&rehydrated_map, &rehydrated.diags);
    assert_eq!(
        cold_text, rehydrated_text,
        "a hit followed by a rehydration must emit the unit's warnings once, not twice"
    );
}

// ---- P12: an error is never published ---------------------------------------------------------

#[test]
fn a_unit_that_fails_to_check_is_never_published() {
    let _serial = serial();
    let proj = Proj::new(
        "failing-unit",
        &[
            ("lib.align", "module lib\n\npub fn broken() -> i64 = \"not an integer\"\n"),
            ("main.align", "import lib\n\nfn main() {\n    print(lib.broken())\n}\n"),
        ],
    );
    let (build, rendered) = proj.build_dirty(UnitReuse::Allowed);
    assert!(build.diags.has_errors(), "the fixture must fail to check:\n{rendered}");
    assert!(
        proj.unit_entries().is_empty(),
        "no unit may be published from a build with errors — a dependent would then be served an \
         interface for a program that does not compile"
    );
}

// ---- P14 / P26: the stage is declined outright -------------------------------------------------

#[test]
fn a_located_walk_never_serves_or_publishes() {
    let _serial = serial();
    let proj = two_module("located");
    let src = std::fs::read_to_string(proj.dir.join("main.align")).expect("read entry");
    let mut sm = SourceMap::new();
    let walk = build_per_unit_located(&mut sm, &proj.entry(), &src);
    assert!(!walk.diags.has_errors());
    assert!(
        proj.unit_entries().is_empty(),
        "located MIR depends on the walk's SourceMap, which the key does not cover"
    );
}

#[test]
fn a_disabled_cache_is_a_complete_bypass() {
    let _serial = serial();
    let proj = two_module("disabled");
    let src = std::fs::read_to_string(proj.dir.join("main.align")).expect("read entry");
    let mut sm = SourceMap::new();
    let build = build_package(&mut sm, &proj.entry(), &src, &CacheContext::Disabled, UnitReuse::Allowed);
    assert!(!build.diags.has_errors());
    assert!(build.units.iter().all(|u| u.frontend.is_none()), "no lookup, so neither hit nor miss");
    assert!(!proj.cache.exists(), "a disabled cache must not create its root");
}

#[test]
fn reuse_forbidden_neither_serves_nor_publishes() {
    let _serial = serial();
    let proj = two_module("forbidden");
    proj.build(UnitReuse::Forbidden);
    assert!(proj.unit_entries().is_empty(), "a forbidden walk must not populate the cache");
    // And it still cannot be SERVED even once entries exist.
    proj.build(UnitReuse::Allowed);
    assert!(!proj.unit_entries().is_empty());
    let forbidden = proj.build(UnitReuse::Forbidden);
    assert!(forbidden.units.iter().all(|u| !u.is_reused() && u.frontend.is_none()));
}

// ---- P15 / P15b: rehydration and its verification ----------------------------------------------

#[test]
fn a_rehydrated_unit_matches_the_cold_lowering() {
    let _serial = serial();
    let proj = two_module("rehydrate");
    let cold = proj.build(UnitReuse::Forbidden);
    let cold_mir: Vec<(String, align_interface::Hash128)> = cold
        .units
        .iter()
        .map(|u| (u.unit.clone(), align_interface::codegen_impl_hash(u.mir().expect("lowered"))))
        .collect();

    proj.build(UnitReuse::Allowed); // populate
    proj.clear_objects(); // frontend hits, every object misses
    let mut warm = proj.build(UnitReuse::Allowed);
    assert!(warm.units.iter().all(|u| u.is_reused()));
    let mut warm_mir = Vec::new();
    for index in 0..warm.units.len() {
        let unit = warm.units[index].unit.clone();
        let mir = warm.materialize(index).expect("rehydration must verify");
        warm_mir.push((unit, align_interface::codegen_impl_hash(mir)));
    }
    assert_eq!(cold_mir, warm_mir, "rehydrated MIR must be identical to the cold lowering");
    assert!(warm.units.iter().all(|u| !u.is_reused()), "a materialized unit is no longer reused");
}

/// Republish a unit's entry with `mutate` applied, under its ORIGINAL key, then force a
/// rehydration and expect it to be rejected and the entry unlinked.
fn tamper_and_expect_rejection(tag: &str, mutate: impl FnOnce(&mut UnitEntry)) {
    let mut mutate = Some(mutate);
    let proj = two_module(tag);
    proj.build(UnitReuse::Allowed);
    // Recover the exact key the walk used by rebuilding and reading it back out of the store: the
    // manifest at `actions/unit/<full>` is the only place it is written, so re-deriving it here
    // would just re-implement the producer. Instead, tamper in place: read the manifest, decode it
    // with the key it stores, mutate the value, and re-serialize under that same key.
    let entries = proj.unit_entries();
    assert!(!entries.is_empty(), "the fixture must have published something");
    let mut tampered_any = false;
    for path in &entries {
        let bytes = std::fs::read(path).expect("read manifest");
        // Decode against every candidate key by asking the store: the manifest carries its own key,
        // so a self-check decode recovers it.
        let Some((key, mut entry)) = unit_cache::decode_manifest(&bytes) else { continue };
        if key.unit != "lib" {
            continue;
        }
        let Some(apply) = mutate.take() else { break };
        apply(&mut entry);
        std::fs::write(path, unit_cache::serialize_manifest(&key, &entry)).expect("rewrite");
        tampered_any = true;
    }
    assert!(tampered_any, "the `lib` entry must exist to tamper with");

    proj.clear_objects();
    let mut warm = proj.build(UnitReuse::Allowed);
    let index = warm.units.iter().position(|u| u.unit == "lib").expect("lib unit");
    assert!(warm.units[index].is_reused(), "the tampered entry still decodes, so it is served");
    let failure = warm.materialize(index).expect_err("verification must reject the tampered entry");
    eprintln!("{tag}: rejected with {failure:?}");
    // Self-healing: the entry is gone, so the next build recomputes and republishes.
    let after = proj.build(UnitReuse::Allowed);
    assert!(!hit(&after, "lib"), "the rejected entry must have been unlinked");
}

#[test]
fn rehydration_rejects_a_tampered_summary() {
    let _serial = serial();
    tamper_and_expect_rejection("tamper-summary", |entry| {
        // Flip a byte inside the canonical summary. The value digest is recomputed on rewrite, so
        // this is not a corruption test — it is the "the entry disagrees with recomputation" test.
        let last = entry.summary_bytes.len() - 1;
        entry.summary_bytes[last] ^= 0x01;
    });
}

#[test]
fn rehydration_rejects_tampered_link_libs() {
    let _serial = serial();
    tamper_and_expect_rejection("tamper-links", |entry| {
        entry.link_libs.push("nonexistent".to_string());
    });
}

#[test]
fn rehydration_rejects_tampered_diagnostics() {
    let _serial = serial();
    tamper_and_expect_rejection("tamper-diags", |entry| {
        entry.diagnostics.push(unit_cache::CachedDiagnostic {
            severity: align_diag::Severity::Warning,
            message: "fabricated".to_string(),
            lo: 0,
            hi: 1,
        });
    });
}

// ---- P11 / P23: damage is detected and self-heals ----------------------------------------------

#[test]
fn a_corrupted_entry_self_heals() {
    let _serial = serial();
    let proj = two_module("corrupt");
    proj.build(UnitReuse::Allowed);
    let entries = proj.unit_entries();
    assert_eq!(entries.len(), 2);
    // Damage the VALUE region without touching the key: structurally the manifest may still parse,
    // and the value digest is what has to catch it.
    let path = &entries[0];
    let mut bytes = std::fs::read(path).expect("read");
    let last = bytes.len() - 20; // inside the value region, before the 16-byte digest
    bytes[last] ^= 0xff;
    std::fs::write(path, &bytes).expect("write");

    let warm = proj.build(UnitReuse::Allowed);
    let corrupted = warm
        .units
        .iter()
        .find(|u| u.frontend.as_ref().is_some_and(|o| o.miss_reason == Some(align_driver::FirstDiff::CorruptEntry)));
    assert!(corrupted.is_some(), "a damaged entry must report CorruptEntry, not a silent hit");
    // Rebuilt and republished, so the next build hits again.
    let healed = proj.build(UnitReuse::Allowed);
    assert!(healed.units.iter().all(|u| u.is_reused()), "the cache must have healed itself");
}

#[test]
fn an_out_of_range_diagnostic_span_is_rejected() {
    let _serial = serial();
    let proj = warning_project("span-range");
    proj.build(UnitReuse::Allowed);
    for path in proj.unit_entries() {
        let bytes = std::fs::read(&path).expect("read");
        let Some((key, mut entry)) = unit_cache::decode_manifest(&bytes) else { continue };
        if entry.diagnostics.is_empty() {
            continue;
        }
        // A span past end-of-source would slice out of bounds when rendered.
        entry.diagnostics[0].hi = u32::MAX;
        std::fs::write(&path, unit_cache::serialize_manifest(&key, &entry)).expect("rewrite");
    }
    let warm = proj.build(UnitReuse::Allowed);
    assert!(
        warm.units.iter().any(|u| u
            .frontend
            .as_ref()
            .is_some_and(|o| o.miss_reason == Some(align_driver::FirstDiff::CorruptEntry))),
        "an out-of-range span must be rejected, not rendered"
    );
}

// ---- P6: a foreign compiler never hits ---------------------------------------------------------

#[test]
fn a_foreign_compiler_fingerprint_never_hits() {
    let _serial = serial();
    let proj = two_module("foreign-compiler");
    proj.build(UnitReuse::Allowed);
    let entries = proj.unit_entries();
    let bytes = std::fs::read(&entries[0]).expect("read");
    let (key, entry) = unit_cache::decode_manifest(&bytes).expect("decode");

    // Republish the identical VALUE under a key that differs only in the compiler fingerprint: it
    // must be unaddressable from this compiler, so the real entry is untouched and the build still
    // hits its own.
    let mut foreign = key.clone();
    foreign.compiler_fingerprint = align_interface::Hash128 { lo: 0xdead, hi: 0xbeef };
    unit_cache::publish(&proj.cache, &foreign, &entry);
    let warm = proj.build(UnitReuse::Allowed);
    assert!(warm.units.iter().all(|u| u.is_reused()), "our own entries still serve");
    // And the foreign entry is inert: looking it up under OUR key set never returns it.
    assert_ne!(foreign.full_digest(), key.full_digest());
}

// ---- P17 / P34: link libraries ----------------------------------------------------------------

#[test]
fn ffi_declared_link_libraries_survive_reuse() {
    let _serial = serial();
    // The load-bearing case for persisting `link_libs` at all: a `link("m")` library is NOT a
    // capability, so it does not appear in `summary.capabilities` and could not be re-derived.
    let proj = Proj::new(
        "ffi-link",
        &[
            (
                "num.align",
                "module num\n\nextern \"C\" link(\"m\") {\n  fn sqrt(x: f64) -> f64\n}\n\npub fn root(x: f64) -> f64 {\n  unsafe {\n    return sqrt(x)\n  }\n}\n",
            ),
            ("main.align", "import num\n\nfn main() {\n    print(num.root(81.0) as i64)\n}\n"),
        ],
    );
    let cold = proj.build(UnitReuse::Forbidden);
    let cold_libs: Vec<(String, Vec<String>)> =
        cold.units.iter().map(|u| (u.unit.clone(), u.link_libs.clone())).collect();
    assert!(
        cold_libs.iter().any(|(_, libs)| libs.iter().any(|l| l == "m")),
        "the fixture must actually declare an FFI library: {cold_libs:?}"
    );
    let num = cold.units.iter().find(|u| u.unit == "num").expect("num unit");
    assert!(
        num.summary.capabilities.is_empty(),
        "and it must NOT be a capability, else the test does not discriminate"
    );

    proj.build(UnitReuse::Allowed);
    let warm = proj.build(UnitReuse::Allowed);
    assert!(warm.units.iter().all(|u| u.is_reused()));
    let warm_libs: Vec<(String, Vec<String>)> =
        warm.units.iter().map(|u| (u.unit.clone(), u.link_libs.clone())).collect();
    assert_eq!(cold_libs, warm_libs, "a reused unit must carry the same link line");
}

// ---- P18: the static-input manifest is reconstructed, not stored -------------------------------

#[test]
fn the_reconstructed_static_input_manifest_matches_the_cold_one() {
    let _serial = serial();
    let proj = two_module("static-inputs");
    let cold = proj.build(UnitReuse::Forbidden);
    let cold_inputs: Vec<(String, Vec<u8>)> = cold
        .units
        .iter()
        .map(|u| (u.unit.clone(), u.static_inputs.canonical_bytes().expect("canonical manifest")))
        .collect();
    proj.build(UnitReuse::Allowed);
    let warm = proj.build(UnitReuse::Allowed);
    let warm_inputs: Vec<(String, Vec<u8>)> = warm
        .units
        .iter()
        .map(|u| (u.unit.clone(), u.static_inputs.canonical_bytes().expect("canonical manifest")))
        .collect();
    assert_eq!(
        cold_inputs, warm_inputs,
        "the manifest is a re-encoding of the summary for a descriptor-free unit, so reconstructing \
         it must be byte-identical to computing it"
    );
    // And the identity it feeds — the codegen key — is unchanged.
    for (cold_unit, warm_unit) in cold.units.iter().zip(warm.units.iter()) {
        assert_eq!(cold_unit.summary.impl_hash, warm_unit.summary.impl_hash);
    }
}

// ---- P21: concurrent producers agree ------------------------------------------------------------

#[test]
fn concurrent_producers_publish_identical_manifests() {
    let _serial = serial();
    let proj = two_module("concurrent");
    // Four processes racing to populate one cold cache root. Every publish is a staged write plus a
    // same-directory rename, and every producer of one key writes byte-identical bytes, so
    // last-writer-wins is harmless.
    let exe = env!("CARGO_BIN_EXE_alignc");
    let mut children = Vec::new();
    for _ in 0..4 {
        children.push(
            std::process::Command::new(exe)
                .args(["build", &proj.entry()])
                .env("ALIGNC_CACHE", &proj.cache)
            .current_dir(&proj.dir)
                .stdout(std::process::Stdio::null())
                .stderr(std::process::Stdio::null())
                .spawn()
                .expect("spawn"),
        );
    }
    for mut child in children {
        child.wait().expect("wait");
    }
    let entries = proj.unit_entries();
    assert_eq!(entries.len(), 2, "one entry per unit, however many producers raced");
    // Every surviving manifest decodes, and no two producers wrote different bytes for one key:
    // that is what makes last-writer-wins harmless.
    for path in &entries {
        let bytes = std::fs::read(path).expect("read manifest");
        let (key, entry) = unit_cache::decode_manifest(&bytes).expect("a raced manifest must decode");
        assert_eq!(
            bytes,
            unit_cache::serialize_manifest(&key, &entry),
            "a raced manifest must be exactly what re-encoding its own contents produces"
        );
    }
    // A fifth process serves from the raced cache. It has to be another `alignc`, not this test
    // binary: the compiler fingerprint is the hash of the RUNNING executable, so the test binary
    // deliberately addresses a different namespace than the CLI it spawns.
    let warm = std::process::Command::new(exe)
        .args(["build", &proj.entry(), "--cache-stats"])
        .env("ALIGNC_CACHE", &proj.cache)
            .current_dir(&proj.dir)
        .output()
        .expect("spawn");
    let stderr = String::from_utf8_lossy(&warm.stderr);
    assert!(
        stderr.contains("2 frontend: 2 hit, 0 miss"),
        "a raced cache is still a usable cache; stderr:\n{stderr}"
    );
}

// ---- P5: a measurement toggle turns the whole cache off -----------------------------------------

#[test]
fn a_measurement_toggle_disables_the_cache() {
    let _serial = serial();
    let proj = two_module("toggle");
    let exe = env!("CARGO_BIN_EXE_alignc");
    let status = std::process::Command::new(exe)
        .args(["build", &proj.entry()])
        .env("ALIGNC_CACHE", &proj.cache)
            .current_dir(&proj.dir)
        .env("ALIGN_CONST_POOL", "off")
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .expect("spawn");
    assert!(status.success());
    assert!(
        proj.unit_entries().is_empty(),
        "a probe/baseline build must never read or publish into the shared cache"
    );
}

// ---- P28 / P28b: composition with the in-process memo -------------------------------------------

#[test]
fn the_disk_cache_and_the_memo_compose() {
    let _serial = serial();
    let proj = two_module("memo-compose");
    // The memo is process-global and other tests share it, so this asserts only what is local: the
    // disk lookup runs on the DIGEST path, before anything renders, so it is consulted on every
    // fresh walk regardless of what the memo holds.
    let cold = proj.build(UnitReuse::Allowed);
    assert!(cold.units.iter().all(|u| !u.is_reused()));
    let warm = proj.build(UnitReuse::Allowed);
    assert!(
        warm.units.iter().all(|u| u.is_reused()),
        "the disk stage must be consulted even when the memo already holds this exact unit"
    );

    // P28b: a verified rehydration promotes into the memo, so materializing the same unit twice
    // across two walks does not rehydrate twice. Observable as: the second walk's materialize
    // succeeds and yields the identical MIR.
    proj.clear_objects();
    let mut first = proj.build(UnitReuse::Allowed);
    let index = first.units.iter().position(|u| u.unit == "lib").expect("lib");
    let first_hash = align_interface::codegen_impl_hash(first.materialize(index).expect("verify"));
    let mut second = proj.build(UnitReuse::Allowed);
    let index = second.units.iter().position(|u| u.unit == "lib").expect("lib");
    let second_hash = align_interface::codegen_impl_hash(second.materialize(index).expect("verify"));
    assert_eq!(first_hash, second_hash);
}

// ---- P25: the recorded outcome ------------------------------------------------------------------

#[test]
fn the_frontend_outcome_is_recorded_per_unit() {
    let _serial = serial();
    let proj = two_module("outcome");
    let cold = proj.build(UnitReuse::Allowed);
    for unit in &cold.units {
        let outcome = unit.frontend.as_ref().expect("an allowed walk records every unit");
        assert_eq!(outcome.stage, align_driver::CacheStage::UnitFrontend);
        assert_eq!(outcome.unit, unit.unit);
        assert!(!outcome.hit);
        assert_eq!(outcome.miss_reason, Some(align_driver::FirstDiff::NoPriorEntry));
    }
    let warm = proj.build(UnitReuse::Allowed);
    for unit in &warm.units {
        let outcome = unit.frontend.as_ref().expect("recorded");
        assert!(outcome.hit && outcome.miss_reason.is_none());
    }
}

// ---- P29: the render equivalence the key depends on ---------------------------------------------

#[test]
fn summary_to_source_depends_only_on_the_summary_and_its_closure() {
    let _serial = serial();
    // The key records `(summary_digest, closure_digest)` INSTEAD of the rendered interface source,
    // which is only sound because rendering is a pure function of exactly those two inputs. Two
    // projects in different directories with identical sources must render byte-identically.
    let first = two_module("render-pure-a");
    let second = two_module("render-pure-b");
    let render = |proj: &Proj| -> String {
        let build = proj.build(UnitReuse::Forbidden);
        let lib = build.units.iter().find(|u| u.unit == "lib").expect("lib");
        align_interface::summary_to_source(&lib.summary, &[]).expect("render")
    };
    assert_eq!(render(&first), render(&second));
}

// ---- key-shape guards used by the tests above ---------------------------------------------------

#[test]
fn a_synthetic_key_round_trips_through_the_store() {
    let _serial = serial();
    // Guards the tamper helpers: publishing a hand-built key/entry pair and reading it back must
    // recover exactly what was written, or every tamper test above would be vacuous.
    let proj = two_module("synthetic");
    let key = UnitKey {
        key_format_version: unit_cache::UNIT_KEY_FORMAT_VERSION,
        compiler_fingerprint: align_interface::Hash128 { lo: 1, hi: 2 },
        frontend_schema: align_interface::FORMAT_VERSION,
        env_toggles: vec![EnvToggle::Absent; 4],
        target_triple: "x86_64-unknown-linux-gnu".to_string(),
        object_format: 0,
        unit: "synthetic".to_string(),
        is_entry: false,
        source_digest: align_interface::Hash128 { lo: 3, hi: 4 },
        deps: vec![unit_cache::UnitDep {
            module: "dep".to_string(),
            state: DepState::Missing,
        }],
    };
    let entry = UnitEntry {
        summary_bytes: vec![9, 9, 9],
        diagnostics: Vec::new(),
        link_libs: vec!["z".to_string()],
    };
    std::fs::create_dir_all(&proj.cache).expect("create cache root");
    unit_cache::publish(&proj.cache, &key, &entry);
    let path = proj.cache.join("actions").join("unit").join(key.full_digest().to_hex());
    let bytes = std::fs::read(&path).expect("read back");
    let (decoded_key, decoded_entry) = unit_cache::decode_manifest(&bytes).expect("decode");
    assert_eq!(decoded_key, key);
    assert_eq!(decoded_entry, entry);
}

// ---- P13: the static-descriptor exclusion (the db-session firewall) ----------------------------

/// The shipped `pkg.db` modules, read from the repository at RUN time.
///
/// Read-only, and by path rather than `include_str!`, so this test never becomes a compile-time
/// dependency of `apps/db`. It needs a REAL descriptor because a static descriptor can only be
/// declared through a compiler-known constructor (`pkg.db.query`), which cannot be synthesized.
fn pkg_db_modules() -> Vec<(String, String)> {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("..").join("..").join("apps").join("db");
    [
        "db.align",
        "db/sqlite.align",
        "db/postgres.align",
        "db/internal.align",
        "db/internal/postgres_status.align",
        "db/internal/resource.align",
        "db/internal/descriptor.align",
        "db/internal/sqlite.align",
        "db/internal/postgres.align",
    ]
    .iter()
    .map(|rel| {
        let path = root.join("pkg").join(rel);
        let src = std::fs::read_to_string(&path)
            .unwrap_or_else(|e| panic!("read {}: {e}", path.display()));
        (format!("pkg/{rel}"), src)
    })
    .collect()
}

#[test]
fn a_descriptor_owning_unit_is_never_cached() {
    let _serial = serial();
    let modules = pkg_db_modules();
    let mut files: Vec<(&str, &str)> =
        modules.iter().map(|(name, src)| (name.as_str(), src.as_str())).collect();
    // One descriptor-owning unit: a `pkg.db.query` constructor as the whole body of a named
    // zero-argument function is exactly the L5c static-descriptor form.
    let app = "module app.report\n\nimport pkg.db\n\npub Params { lo: i64 }\n\npub Row { id: i64 }\n\npub fn query() -> pkg.db.query<Params, Row> = pkg.db.query(\n  \"SELECT id FROM t WHERE id >= :lo\",\n  [],\n)\n";
    let main = "import pkg.db\nimport app.report\n\nfn main() {\n    print(1)\n}\n";
    files.push(("app/report.align", app));
    files.push(("main.align", main));
    let proj = Proj::new("descriptor-exclusion", &files);

    let (build, rendered) = proj.build_dirty(UnitReuse::Allowed);
    assert!(!build.diags.has_errors(), "the descriptor fixture must compile:\n{rendered}");
    let descriptor_owner = build
        .units
        .iter()
        .find(|u| u.unit == "app.report")
        .expect("the descriptor-owning unit must be built");
    assert!(!descriptor_owner.is_reused());

    // Which units did we actually publish?
    let published: std::collections::BTreeSet<String> = proj
        .unit_entries()
        .iter()
        .filter_map(|path| std::fs::read(path).ok())
        .filter_map(|bytes| unit_cache::decode_manifest(&bytes).map(|(key, _)| key.unit))
        .collect();
    assert!(
        published.contains("pkg.db.internal"),
        "the descriptor-FREE package units must publish, else this test proves nothing: {published:?}"
    );
    assert!(
        !published.contains("app.report"),
        "a unit owning static descriptors must NEVER be published: its result depends on files \
         under the project root and on the metadata publication lock, neither of which a replay \
         re-establishes. Published: {published:?}"
    );

    // And it is never SERVED either: a second build recomputes it while the package units hit.
    let warm = proj.build(UnitReuse::Allowed);
    assert!(!hit(&warm, "app.report"), "a descriptor-owning unit must never be served from disk");
    assert!(
        hit(&warm, "pkg.db.internal"),
        "while its descriptor-free dependencies still are — the exclusion is per unit, not per build"
    );
}

// ---- P3-5: closure_digest is load-bearing --------------------------------------------------

#[test]
fn a_closure_member_moving_within_the_loaded_set_misses_its_dependents() {
    let _serial = serial();
    // The discriminating shape for `closure_digest`. `main` imports `m1` and `m2`; `m1` imports
    // `leaf`, so all four modules are loaded either way and `main`'s dependency LIST is byte-equal
    // before and after. What changes is that `m2` gains an import of `leaf` without touching its own
    // public surface — so `m2`'s interface_hash is unchanged, its position in `main`'s DFS order is
    // unchanged, and only the rendering of `m2` (which carries its import lines) differs. Without
    // `closure_digest` in the key, `main` would be served an entry checked against a DIFFERENT
    // interface source.
    let proj = Proj::new(
        "closure-member-moves",
        &[
            ("leaf.align", "module leaf\n\npub fn one() -> i64 = 1\n"),
            ("m1.align", "module m1\n\nimport leaf\n\npub fn a() -> i64 = leaf.one()\n"),
            ("m2.align", "module m2\n\npub fn b() -> i64 = 2\n"),
            ("main.align", "import m1\nimport m2\n\nfn main() {\n    print(m1.a() + m2.b())\n}\n"),
        ],
    );
    let cold = proj.build(UnitReuse::Allowed);
    let dep_list = |build: &PackageBuild| -> Vec<(String, Vec<String>)> {
        build
            .units
            .iter()
            .map(|u| (u.unit.clone(), u.dep_interface_hashes.iter().map(|(m, _)| m.clone()).collect()))
            .collect()
    };
    let main_deps = |build: &PackageBuild| dep_list(build).into_iter().find(|(u, _)| u == "main");
    let cold_deps = main_deps(&cold);
    let cold_m2 = cold.units.iter().find(|u| u.unit == "m2").expect("m2").summary.interface_hash;

    // `m2` gains an import; its public surface does not move.
    proj.write("m2.align", "module m2\n\nimport leaf\n\npub fn b() -> i64 = 2\n");
    let warm = proj.build(UnitReuse::Allowed);
    let warm_m2 = warm.units.iter().find(|u| u.unit == "m2").expect("m2").summary.interface_hash;

    assert_eq!(cold_m2, warm_m2, "the fixture must leave m2's interface hash UNCHANGED");
    assert_eq!(
        cold_deps,
        main_deps(&warm),
        "and MAIN's dependency list unchanged — the loaded set never moved, so the only thing that \
         can discriminate main's key is m2's closure_digest"
    );
    assert!(!hit(&warm, "m2"), "m2's own source changed");
    assert!(
        !hit(&warm, "main"),
        "main is checked against m2's RENDERED interface, which now carries a different import set"
    );
    assert_eq!(miss_reason(&warm, "main"), Some(align_driver::FirstDiff::DepInterfaceHashes));
}

// ---- P2-1: a verified rehydration promotes into the memo -------------------------------------

#[test]
fn a_verified_rehydration_is_promoted_into_the_memo() {
    let _serial = serial();
    let proj = two_module("memo-promotion");
    proj.build(UnitReuse::Allowed); // populate disk (and, incidentally, the memo)

    // Start from "disk has it, the memo does not" — the state a fresh process is always in, and the
    // one the promotion exists for. `memo::clear` is safe here because this file is serialized.
    align_driver::memo::clear();
    proj.clear_objects();

    let before = align_driver::memo::stats();
    let mut first = proj.build(UnitReuse::Allowed);
    let index = first.units.iter().position(|u| u.unit == "lib").expect("lib");
    assert!(first.units[index].is_reused(), "the unit must come from disk, not the memo");
    first.materialize(index).expect("rehydration must verify");
    let after_first = align_driver::memo::stats();
    assert_eq!(
        after_first.unit_misses - before.unit_misses,
        1,
        "the first rehydration looks the memo up and finds nothing"
    );
    assert_eq!(after_first.unit_hits, before.unit_hits, "and certainly does not hit");

    // Second rehydration of the SAME unit, in the same process: the promotion must make it a memo
    // hit instead of a second full re-check.
    let mut second = proj.build(UnitReuse::Allowed);
    let index = second.units.iter().position(|u| u.unit == "lib").expect("lib");
    assert!(second.units[index].is_reused());
    second.materialize(index).expect("rehydration must verify");
    let after_second = align_driver::memo::stats();
    assert_eq!(
        after_second.unit_hits - after_first.unit_hits,
        1,
        "a verified rehydration must be promoted into the memo, so the next one is served from it"
    );
    assert_eq!(
        after_second.unit_misses, after_first.unit_misses,
        "and the second rehydration must not look anything else up"
    );
}

// ---- P2-3: the bounded retry, end to end -------------------------------------------------------

#[test]
fn a_stale_entry_makes_the_cli_retry_once_and_succeed() {
    let _serial = serial();
    let proj = two_module("cli-retry");
    let exe = env!("CARGO_BIN_EXE_alignc");
    let build = |args: &[&str]| -> std::process::Output {
        let mut command = std::process::Command::new(exe);
        command
            .arg("build")
            .arg(proj.entry())
            .args(args)
            .env("ALIGNC_CACHE", &proj.cache)
            .current_dir(&proj.dir);
        command.output().expect("spawn alignc")
    };
    let cold = build(&[]);
    assert!(cold.status.success(), "cold: {}", String::from_utf8_lossy(&cold.stderr));

    // Make one entry disagree with what recomputing it produces, keeping the manifest well-formed
    // (its value digest is recomputed on rewrite) — this is the "the key was incomplete" path, not
    // the corruption path.
    let mut tampered = false;
    for path in proj.unit_entries() {
        let bytes = std::fs::read(&path).expect("read");
        let Some((key, mut entry)) = unit_cache::decode_manifest(&bytes) else { continue };
        if key.unit != "lib" {
            continue;
        }
        entry.link_libs.push("definitely-not-a-real-library".to_string());
        std::fs::write(&path, unit_cache::serialize_manifest(&key, &entry)).expect("rewrite");
        tampered = true;
    }
    assert!(tampered, "the `lib` entry must exist to tamper with");
    proj.clear_objects(); // force the rehydration that discovers the disagreement

    let retried = build(&["--cache-stats"]);
    let stderr = String::from_utf8_lossy(&retried.stderr).into_owned();
    assert!(retried.status.success(), "the retry must produce a working build:\n{stderr}");
    assert_eq!(
        stderr.matches("rebuilding this package without cache reuse").count(),
        1,
        "exactly one retry, announced once:\n{stderr}"
    );
    assert!(
        stderr.contains("cached unit `lib`: cached unit disagreed with recomputation (link libraries)"),
        "the note must name the unit and the disagreeing component:\n{stderr}"
    );
    // Exactly one report of each stats block: the failed attempt renders nothing.
    assert_eq!(stderr.matches("unit(s): ").count(), 1, "one codegen summary:\n{stderr}");
    assert_eq!(
        stderr.matches("frontend: ").count(),
        0,
        "the retry forbids reuse, so it consults no frontend entry and prints no frontend block:\n{stderr}"
    );
    // Healing takes one more ordinary build, and that is the correct shape: the retry FORBIDS
    // reuse, and a forbidden walk neither serves nor publishes, so the rejected entry is simply
    // absent afterwards. The next ordinary build recomputes and republishes it...
    let republish = build(&["--cache-stats"]);
    let republish_err = String::from_utf8_lossy(&republish.stderr).into_owned();
    assert!(republish.status.success());
    assert!(
        republish_err.contains("lib frontend miss (no prior entry)"),
        "the rejected entry must be gone, not silently reused:\n{republish_err}"
    );
    // ...and the one after it hits again.
    let healed = build(&["--cache-stats"]);
    let healed_err = String::from_utf8_lossy(&healed.stderr);
    assert!(healed.status.success());
    assert!(
        healed_err.contains("2 frontend: 2 hit, 0 miss"),
        "the cache must be healthy again two builds later:\n{healed_err}"
    );
}

// ---- P2-4: artifact-level equivalence ------------------------------------------------------------

#[test]
fn a_cold_build_and_an_all_hit_build_produce_the_same_executable() {
    let _serial = serial();
    let exe = env!("CARGO_BIN_EXE_alignc");
    let sources: &[(&str, &str)] = &[
        ("lib.align", "module lib\n\npub fn twice(n: i64) -> i64 = n * 2\n\nfn helper() -> i64 = 1\n"),
        ("main.align", "import lib\n\nfn main() {\n    print(lib.twice(21))\n}\n"),
    ];
    // Identical sources in two directories, sharing one cache root: the first build is cold, the
    // second is an all-hit served entirely from that root. The entry BASENAME is the same in both,
    // which matters on Mach-O — the linker embeds the output file's name in the ad-hoc code
    // signature, so two links differing only in destination NAME differ in bytes no matter what the
    // cache did (doc-10 §7 scopes the byte-identity row for exactly this reason).
    let cold = Proj::new("artifact-cold", sources);
    let second = Proj::new("artifact-warm", sources);
    // Borrow the second project's directory but the FIRST project's cache root. `second` stays alive
    // for the whole test; dropping it early would remove the directory out from under the build.
    let warm = Proj { dir: second.dir.clone(), cache: cold.cache.clone() };
    let run_build = |proj: &Proj| -> std::process::Output {
        std::process::Command::new(exe)
            .args(["build", &proj.entry(), "--cache-stats"])
            .env("ALIGNC_CACHE", &proj.cache)
            .current_dir(&proj.dir)
            .output()
            .expect("spawn alignc")
    };
    let cold_out = run_build(&cold);
    assert!(cold_out.status.success(), "{}", String::from_utf8_lossy(&cold_out.stderr));
    let warm_out = run_build(&warm);
    let warm_err = String::from_utf8_lossy(&warm_out.stderr);
    assert!(warm_out.status.success(), "{warm_err}");
    assert!(
        warm_err.contains("2 frontend: 2 hit, 0 miss") && warm_err.contains("2 unit(s): 2 hit, 0 miss"),
        "the second build must be an ALL-hit, else this compares nothing:\n{warm_err}"
    );

    let cold_exe = cold.dir.join("main");
    let warm_exe = warm.dir.join("main");
    let cold_bytes = std::fs::read(&cold_exe).expect("cold executable");
    let warm_bytes = std::fs::read(&warm_exe).expect("warm executable");
    assert_eq!(
        cold_bytes, warm_bytes,
        "an all-hit build must link a byte-identical executable to the cold build it reused"
    );
    // Belt and braces: the artifacts also behave identically.
    let cold_run = std::process::Command::new(&cold_exe).output().expect("run cold");
    let warm_run = std::process::Command::new(&warm_exe).output().expect("run warm");
    assert_eq!(cold_run.stdout, warm_run.stdout);
    assert_eq!(cold_run.status.code(), warm_run.status.code());
    assert_eq!(String::from_utf8_lossy(&cold_run.stdout).trim(), "42");
    drop(second);
}
