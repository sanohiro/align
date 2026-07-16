//! ThinLTO S2 (`--thin-lto` cache composition + parallelism) gates. Each library-level gate asserts a
//! `CacheOutcome` (`stage` / `hit` / `miss_reason`) — never elapsed time — over the two cacheable
//! phases (`ThinLtoPrelink` + `ThinLtoBackend`); the serial thin-link between them always reruns.
//! The DAG corpus / `Proj` guard / `ThinBuilt` result live in `common` (shared with `thin_lto_sv.rs`).
//!
//! Gates:
//!  1. Headline incremental win — A→B→C(+D): edit C's PRIVATE body → C prelink misses; only units
//!     whose import list / import-source digests changed miss backend; a unit that imports nothing
//!     from C hits BOTH phases.
//!  2. pub-signature change → transitive dependents miss prelink (interface hash) AND backend.
//!  3. Import-sensitive precision — a private-body edit in an IMPORTED-FROM unit misses the importer's
//!     backend even though the importer's own prelink hits.
//!  4. Toggle isolation — `--thin-lto` on/off never mix objects (structurally disjoint key namespaces).
//!  5. Cold-vs-hit byte-identity through both phases — a second identical build is all-hit AND
//!     byte-identical (objects + exe).
//!  6. Parallel == `-j 1` byte-identity under `--thin-lto`.
//!  7. Cross-process second build all-hit (subprocess, `--cache-stats`).
//!  8. Corruption — a corrupted cached prelink blob is evicted + rebuilt (loud), exe still correct.

mod common;
use common::*;

use std::path::{Path, PathBuf};
use std::process::Command;

fn backend() -> bool {
    backend_available()
}

// ---- Gate 1: headline incremental win -----------------------------------------------------------

#[test]
fn gate1_private_body_edit_precise_backend_invalidation() {
    if !backend() {
        return;
    }
    let proj = Proj::dag("headline", C_V1);
    let cache = proj.cache();

    let cold = thin_build(&proj, &cache, 2);
    assert!(cold.all_miss(), "cold ThinLTO build is all-miss");
    if cc_available() {
        assert_eq!(cold.run(&proj), DAG_OUT_V1); // (10*2 + 100) + 5
    }

    // Edit ONLY c's private helper body: c's impl_hash + prelink `.bc` change; its interface does not.
    proj.write("c.align", C_BODY);
    let hot = thin_build(&proj, &cache, 2);

    // c: its own code changed → both phases miss.
    assert!(!hot.prelink("c").hit);
    assert_eq!(hot.prelink("c").miss_reason, Some(FirstDiff::MirDigest), "c prelink misses on its own MIR digest");
    assert!(!hot.backend("c").hit);
    assert_eq!(hot.backend("c").miss_reason, Some(FirstDiff::PrelinkInput), "c backend misses on its own prelink input");

    // b: imports c; its own source unchanged → prelink HIT, but backend misses (c's digest changed).
    assert!(hot.prelink("b").hit, "b's own prelink is unchanged by a c private-body edit");
    assert!(!hot.backend("b").hit);
    assert_eq!(hot.backend("b").miss_reason, Some(FirstDiff::CrossUnitImports), "b backend misses on the changed import source");

    // d: imports NOTHING from c → hits BOTH phases (the incremental-precision headline).
    assert!(hot.prelink("d").hit, "d prelink hits");
    assert!(hot.backend("d").hit, "d imports nothing from c → its backend hits");

    // main: reaches c transitively through b → backend misses (its import-source digest changed).
    assert!(hot.prelink("main").hit, "main's own prelink is unchanged");
    assert!(!hot.backend("main").hit, "main transitively imports from c → backend misses");

    if cc_available() {
        assert_eq!(hot.run(&proj), DAG_OUT_BODY, "the rebuild took effect: (10*3 + 100) + 5");
    }
}

// ---- Gate 2: pub-signature change → transitive dependents miss prelink AND backend --------------

#[test]
fn gate2_pub_signature_change_invalidates_dependents_both_phases() {
    if !backend() {
        return;
    }
    let proj = Proj::dag("pubsig", C_V1);
    let cache = proj.cache();
    let cold = thin_build(&proj, &cache, 2);
    assert!(cold.all_miss());

    // A PUBLIC-surface change to c (new pub fn) flips c's interface hash.
    proj.write("c.align", C_PUB);
    let hot = thin_build(&proj, &cache, 2);

    // c itself: prelink + backend miss (its own impl changed).
    assert!(!hot.prelink("c").hit && !hot.backend("c").hit);

    // b keys on c's interface hash → b prelink misses on the DEP interface, and its backend misses too.
    assert!(!hot.prelink("b").hit);
    assert_eq!(
        hot.prelink("b").miss_reason,
        Some(FirstDiff::DepInterfaceHashes),
        "a pub-signature change in c misses b's prelink on the dep interface hash"
    );
    assert!(!hot.backend("b").hit, "b's backend misses too (c's prelink digest changed)");

    // main transitively depends on c's interface → prelink + backend miss.
    assert!(!hot.prelink("main").hit, "main's transitive dep interface changed → prelink misses");
    assert!(!hot.backend("main").hit);

    // d is unaffected by c's public surface → both phases hit.
    assert!(hot.prelink("d").hit && hot.backend("d").hit, "d is independent of c → both phases hit");
}

// ---- Gate 3: import-sensitive precision (2-unit) -----------------------------------------------

#[test]
fn gate3_imported_from_private_edit_misses_only_importer_backend() {
    if !backend() {
        return;
    }
    let lib_v1 = "module lib\nfn helper(x: i64) -> i64 = x + 1\npub fn foo(x: i64) -> i64 = helper(x) * 2\n";
    let lib_v2 = "module lib\nfn helper(x: i64) -> i64 = x + 9\npub fn foo(x: i64) -> i64 = helper(x) * 2\n";
    let main = "import lib\nfn main() {\n  print(lib.foo(3))\n}\n";
    let proj = Proj::new("import-precise", &[("lib.align", lib_v1), ("main.align", main)], "main.align");
    let cache = proj.cache();
    let cold = thin_build(&proj, &cache, 1);
    assert!(cold.all_miss());
    if cc_available() {
        assert_eq!(cold.run(&proj), "8\n"); // (3+1)*2
    }

    proj.write("lib.align", lib_v2);
    let hot = thin_build(&proj, &cache, 1);
    // The importer's OWN prelink hits; only its backend misses (on the changed import source).
    assert!(hot.prelink("main").hit, "the importer's own prelink hits (its source is unchanged)");
    assert!(!hot.backend("main").hit, "the importer's backend misses (imported body changed)");
    assert_eq!(hot.backend("main").miss_reason, Some(FirstDiff::CrossUnitImports));
    // lib itself misses both.
    assert!(!hot.prelink("lib").hit && !hot.backend("lib").hit);
    if cc_available() {
        assert_eq!(hot.run(&proj), "24\n", "the rebuild took effect: (3+9)*2");
    }
}

// ---- Gate 4: toggle isolation (--thin-lto on/off never mix objects) -----------------------------

#[test]
fn gate4_thin_and_nonthin_object_namespaces_are_disjoint() {
    if !backend() {
        return;
    }
    let proj = Proj::dag("toggle", C_V1);
    let cache = proj.cache();

    // A non-ThinLTO build (via the ordinary object cache) populates actions/codegen.
    let entry = proj.dir.join("main.align");
    let entry_src = std::fs::read_to_string(&entry).unwrap();
    let mut sm = SourceMap::new();
    let walk = build_per_unit(&mut sm, &entry.display().to_string(), &entry_src);
    let nonthin_objs: Vec<PathBuf> = (0..walk.units.len()).map(|i| proj.dir.join(format!("nt{i}.o"))).collect();
    align_driver::codegen_units_parallel(&walk.units, &nonthin_objs, &cache, &BuildTarget::Baseline, Profile::Release, false, 2, &align_driver::PgoMode::Off)
        .expect("non-thin build");

    // A ThinLTO build on the SAME cache root populates actions/prelink + actions/thinbackend.
    let _thin = thin_build(&proj, &cache, 2);

    let root = proj.cache_root();
    let codegen = root.join("actions").join("codegen");
    let prelink = root.join("actions").join("prelink");
    let thinbackend = root.join("actions").join("thinbackend");
    let count = |p: &Path| std::fs::read_dir(p).map(|d| d.filter_map(Result::ok).filter(|e| e.path().is_file()).count()).unwrap_or(0);
    assert!(count(&codegen) > 0, "the non-thin build populated actions/codegen");
    assert!(count(&prelink) > 0, "the thin build populated actions/prelink");
    assert!(count(&thinbackend) > 0, "the thin build populated actions/thinbackend");

    // Each toggle re-hits ONLY its own namespace: a repeat thin build is all-hit; a repeat non-thin
    // build is all-hit. Neither served the other's objects (distinct key structs + subdirs).
    let thin2 = thin_build(&proj, &cache, 2);
    assert!(thin2.all_hit(), "a repeat --thin-lto build hits its own (thinbackend/prelink) namespace");
    let nonthin2 = align_driver::codegen_units_parallel(&walk.units, &nonthin_objs, &cache, &BuildTarget::Baseline, Profile::Release, false, 2, &align_driver::PgoMode::Off)
        .expect("non-thin rebuild");
    assert!(nonthin2.outcomes.iter().all(|o| o.hit), "a repeat non-thin build hits its own (codegen) namespace");
}

// ---- Gate 5: cold-vs-hit byte-identity through both phases --------------------------------------

#[test]
fn gate5_cold_and_hot_are_byte_identical() {
    if !backend() {
        return;
    }
    let proj = Proj::dag("coldhit", C_V1);
    let cache = proj.cache();
    let cold = thin_build(&proj, &cache, 2);
    assert!(cold.all_miss());
    let cold_objs = cold.obj_bytes();

    let hot = thin_build(&proj, &cache, 2);
    assert!(hot.all_hit(), "a second identical --thin-lto build hits every phase of every unit");
    assert_eq!(cold_objs, hot.obj_bytes(), "hot object bytes equal cold object bytes (both phases)");

    if cc_available() {
        assert_eq!(cold.exe_bytes(&proj), hot.exe_bytes(&proj), "the cold and fully-hit exe are byte-identical");
    }
}

// ---- Gate 6: parallel == -j1 byte-identity ------------------------------------------------------

#[test]
fn gate6_parallel_equals_serial_byte_identity() {
    if !backend() {
        return;
    }
    // Cache OFF → every unit's prelink + backend is produced; the linked exe must not depend on -j.
    let p1 = Proj::dag("serial", C_V1);
    let serial = thin_build(&p1, &CacheContext::Disabled, 1);
    let p4 = Proj::dag("parallel", C_V1);
    let parallel = thin_build(&p4, &CacheContext::Disabled, 4);
    assert_eq!(serial.obj_bytes(), parallel.obj_bytes(), "-j 4 ThinLTO objects must be byte-identical to -j 1");
    if cc_available() {
        assert_eq!(serial.exe_bytes(&p1), parallel.exe_bytes(&p4), "-j 4 exe byte-identical to -j 1");
    }
}

// ---- Gate 7: cross-process second build all-hit -------------------------------------------------

#[test]
fn gate7_cross_process_second_build_all_hit() {
    if !backend() || !cc_available() {
        return;
    }
    let proj = Proj::dag("xproc", C_V1);
    let shared = proj.dir.join("xcache");
    let alignc = env!("CARGO_BIN_EXE_alignc");
    let run = || {
        Command::new(alignc)
            .args(["build", "main.align", "--thin-lto", "--cache-stats", "-p", "release"])
            .current_dir(&proj.dir)
            .env("ALIGNC_CACHE", &shared)
            .output()
            .expect("spawn alignc")
    };
    let cold = run();
    assert!(cold.status.success(), "cold --thin-lto build failed: {}", String::from_utf8_lossy(&cold.stderr));
    assert!(String::from_utf8_lossy(&cold.stderr).contains("miss"), "cold build reports misses");
    let exe1 = std::fs::read(proj.dir.join("main")).expect("read exe 1");

    let hot = run();
    assert!(hot.status.success(), "hot --thin-lto build failed: {}", String::from_utf8_lossy(&hot.stderr));
    let hot_err = String::from_utf8_lossy(&hot.stderr);
    assert!(hot_err.contains("prelink: 4 hit, 0 miss"), "second build: all prelink hit:\n{hot_err}");
    assert!(hot_err.contains("backend: 4 hit, 0 miss"), "second build: all backend hit:\n{hot_err}");
    let exe2 = std::fs::read(proj.dir.join("main")).expect("read exe 2");
    assert_eq!(exe1, exe2, "the cross-process all-hit --thin-lto exe is byte-identical");
    assert_eq!(Command::new(proj.dir.join("main")).output().unwrap().stdout, DAG_OUT_V1.as_bytes());
}

// ---- Gate 8: corrupted cached prelink blob → evicted + rebuilt + correct exe --------------------

#[test]
fn gate8_corrupted_prelink_blob_rebuilds() {
    if !backend() {
        return;
    }
    let proj = Proj::dag("corrupt", C_V1);
    let cache = proj.cache();
    let cold = thin_build(&proj, &cache, 1);
    assert!(cold.all_miss());

    // Corrupt the CAS blob backing c's cached PRELINK bitcode (its content-addressed digest). The CAS
    // path is derived by the SAME `cas_blob_path` rule the cache uses (not re-hardcoded here).
    let c_idx = cold.unit_index("c");
    let bc_bytes = std::fs::read(&cold.bc_paths[c_idx]).expect("read c prelink bc");
    let blob = cas_blob_path(&proj.cache_root(), Hash128::of(&bc_bytes));
    assert!(blob.exists(), "the cold build published c's prelink CAS blob");
    std::fs::write(&blob, b"not valid bitcode").expect("corrupt the blob");

    let hot = thin_build(&proj, &cache, 1);
    // c's prelink is evicted + rebuilt (a corruption event, not a clean miss).
    assert!(!hot.prelink("c").hit, "a corrupted prelink blob forces a rebuild");
    assert_eq!(
        hot.prelink("c").miss_reason,
        Some(FirstDiff::CorruptEntry),
        "a digest-verify failure on the prelink blob is a corruption event"
    );
    // The rebuilt `.bc` is deterministic → same digest → c's backend still hits its (uncorrupted) object.
    assert!(hot.backend("c").hit, "the deterministically-rebuilt prelink digest re-hits c's backend object");
    if cc_available() {
        assert_eq!(hot.run(&proj), DAG_OUT_V1, "the exe is correct after the corruption rebuild");
    }
}
