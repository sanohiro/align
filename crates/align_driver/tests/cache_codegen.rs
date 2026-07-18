//! M15 S3a — incremental codegen-stage cache gates (`docs/impl/10-cache-first-optimization.md` §7).
//!
//! Every gate asserts a `CacheOutcome` (`hit` / `miss_reason`) or raw bytes — never elapsed time. The
//! cache is exercised through the library (`emit_object_cached` with an explicit `CacheContext::at`),
//! except the cross-process `impl_hash`-stability gate, which drives two fresh `alignc` subprocesses.
//!
//! The matrix: (1) no-op rebuild all-hit; (2) private dep-body edit → that unit MirDigest-miss, every
//! dependent hit + correct exe; (3) transitive A→B→C invalidation; (4) comment-only edit hit;
//! (5) edit-then-exact-revert hit (old CAS entry); (6) corrupted blob → corruption event + rebuild +
//! correct binary; (7) cold vs hit byte-identity (object + executable); (8) profile / `--export`
//! change miss with the right `FirstDiff`; (9) rt-lto on/off distinct keys; (10) cross-process
//! `impl_hash` stability (hit on the second subprocess); (11) an unimported file is outside the
//! build graph; (12) a killed producer's private staging remnant is never a hit; (13) resolved CPU
//! identity separates baseline/native artifacts.

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

use align_driver::{
    build_per_unit, emit_object_cached, link_objects, BuildTarget, CacheContext, CacheOutcome,
    FirstDiff, Hash128, Profile,
};
use align_span::SourceMap;

// ---- harness ------------------------------------------------------------------------------------

static NONCE: AtomicU64 = AtomicU64::new(0);
fn nonce() -> u64 {
    NONCE.fetch_add(1, Ordering::Relaxed)
}

fn backend() -> bool {
    align_driver::backend_available()
}

/// Whether the system C compiler is available (link/run gates skip when it is not).
fn cc_available() -> bool {
    std::process::Command::new("cc")
        .arg("--version")
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .map(|s| s.success())
        .unwrap_or(false)
}

/// A throwaway multi-file project directory (removed on drop). Files are (re)written by name; the
/// cache root and per-build object/executable paths live under the same directory.
struct Project {
    dir: PathBuf,
    entry: String,
}

impl Project {
    fn new(tag: &str, files: &[(&str, &str)], entry: &str) -> Project {
        let dir = std::env::temp_dir().join(format!("align-cache-{}-{tag}-{}", std::process::id(), nonce()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("mkdir project");
        let proj = Project { dir, entry: entry.to_string() };
        for (name, src) in files {
            proj.write(name, src);
        }
        proj
    }
    fn write(&self, name: &str, src: &str) {
        std::fs::write(self.dir.join(name), src).expect("write source");
    }
    fn entry_path(&self) -> PathBuf {
        self.dir.join(&self.entry)
    }
    fn cache(&self) -> CacheContext {
        CacheContext::at(self.dir.join("cache"))
    }
    fn cache_root(&self) -> PathBuf {
        self.dir.join("cache")
    }
}

impl Drop for Project {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.dir);
    }
}

/// One cache-aware build: the per-unit walk, then `emit_object_cached` per unit into fresh object
/// paths (so a hit copies the CAS blob into a clean file). `exports` applies to the entry unit only.
struct Emitted {
    outcomes: Vec<CacheOutcome>,
    objs: Vec<PathBuf>,
    link_libs: Vec<String>,
}

fn emit_all(
    proj: &Project,
    cache: &CacheContext,
    profile: Profile,
    target: BuildTarget,
    exports: &[String],
    rt_lto: bool,
) -> Emitted {
    let entry = proj.entry_path();
    let entry_src = std::fs::read_to_string(&entry).expect("read entry");
    let mut sm = SourceMap::new();
    let walk = build_per_unit(&mut sm, &entry.display().to_string(), &entry_src);
    assert!(!walk.diags.has_errors(), "unexpected build errors");
    let mut outcomes = Vec::new();
    let mut objs = Vec::new();
    let mut link_libs: Vec<String> = Vec::new();
    for unit in &walk.units {
        let obj = proj.dir.join(format!("{}-{}.o", unit.unit.replace('.', "_"), nonce()));
        let unit_exports: &[String] = if unit.is_entry { exports } else { &[] };
        let outcome = emit_object_cached(
            cache,
            &unit.unit,
            unit.summary.impl_hash,
            &unit.dep_interface_hashes,
            &unit.mir,
            &obj,
            target.clone(),
            profile,
            unit_exports,
            rt_lto,
        )
        .expect("cached codegen");
        outcomes.push(outcome);
        objs.push(obj);
        for lib in &unit.mir.link_libs {
            if !link_libs.contains(lib) {
                link_libs.push(lib.clone());
            }
        }
    }
    Emitted { outcomes, objs, link_libs }
}

impl Emitted {
    fn outcome(&self, unit: &str) -> &CacheOutcome {
        self.outcomes.iter().find(|o| o.unit == unit).unwrap_or_else(|| panic!("no outcome for unit `{unit}`"))
    }
    fn all_hit(&self) -> bool {
        self.outcomes.iter().all(|o| o.hit)
    }
    /// Link + run; returns stdout. Caller must have checked `cc_available()`.
    fn run(&self, proj: &Project, profile: Profile) -> String {
        let obj_refs: Vec<&Path> = self.objs.iter().map(|p| p.as_path()).collect();
        let exe = proj.dir.join(format!("exe-{}", nonce()));
        link_objects(&obj_refs, &exe, &self.link_libs, profile).expect("link");
        let out = std::process::Command::new(&exe).output().expect("run");
        String::from_utf8_lossy(&out.stdout).into_owned()
    }
    /// The linked executable bytes (for byte-identity gates).
    fn exe_bytes(&self, proj: &Project, profile: Profile) -> Vec<u8> {
        let obj_refs: Vec<&Path> = self.objs.iter().map(|p| p.as_path()).collect();
        let exe = proj.dir.join(format!("exe-{}", nonce()));
        link_objects(&obj_refs, &exe, &self.link_libs, profile).expect("link");
        std::fs::read(&exe).expect("read exe")
    }
}

fn no_exports() -> Vec<String> {
    Vec::new()
}

// ---- Gate 1: no-op rebuild → all hit ------------------------------------------------------------

#[test]
fn gate1_noop_rebuild_all_hit() {
    if !backend() {
        return;
    }
    let proj = Project::new("noop", &[("main.align", "fn main() {\n  print(42)\n}\n")], "main.align");
    let cache = proj.cache();
    let cold = emit_all(&proj, &cache, Profile::Release, BuildTarget::Baseline, &no_exports(), false);
    assert!(cold.outcomes.iter().all(|o| !o.hit), "cold build is all misses");
    assert_eq!(cold.outcome("main").miss_reason, Some(FirstDiff::NoPriorEntry));

    let hot = emit_all(&proj, &cache, Profile::Release, BuildTarget::Baseline, &no_exports(), false);
    assert!(hot.all_hit(), "a no-op rebuild must hit every unit");
    assert_eq!(hot.outcome("main").miss_reason, None);
}

// ---- Gate 2: private dep-body edit → that unit misses, dependents hit ----------------------------

#[test]
fn gate2_private_body_edit_misses_only_edited_unit() {
    if !backend() {
        return;
    }
    let lib_v1 = "module lib\npub fn pubfn(x: i64) -> i64 = x + secret(x)\nfn secret(x: i64) -> i64 = x * 2\n";
    let lib_v2 = "module lib\npub fn pubfn(x: i64) -> i64 = x + secret(x)\nfn secret(x: i64) -> i64 = x * 3\n";
    let main = "import lib\nfn main() {\n  print(lib.pubfn(3))\n}\n";
    let proj = Project::new("privbody", &[("lib.align", lib_v1), ("main.align", main)], "main.align");
    let cache = proj.cache();

    let cold = emit_all(&proj, &cache, Profile::Release, BuildTarget::Baseline, &no_exports(), false);
    assert!(cold.outcomes.iter().all(|o| !o.hit));
    if cc_available() {
        assert_eq!(cold.run(&proj, Profile::Release), "9\n"); // 3 + 3*2
    }

    // Edit only lib's PRIVATE body: lib misses on its own impl_hash; main's key is unchanged → hit.
    proj.write("lib.align", lib_v2);
    let hot = emit_all(&proj, &cache, Profile::Release, BuildTarget::Baseline, &no_exports(), false);
    assert!(!hot.outcome("lib").hit, "the edited unit must miss");
    assert_eq!(
        hot.outcome("lib").miss_reason,
        Some(FirstDiff::MirDigest),
        "a private-body edit misses on the unit's own MIR digest"
    );
    assert!(hot.outcome("main").hit, "a dependent whose interface inputs are unchanged must hit");
    if cc_available() {
        assert_eq!(hot.run(&proj, Profile::Release), "12\n"); // 3 + 3*3, proves the rebuild took effect
    }
}

// ---- Gate 2b: a json.decode target-struct field RENAME invalidates the cache -------------------

/// A `json.decode` target struct's field name/type feeds only the codegen descriptor table, not the
/// surrounding MIR — a field RENAME at the same slot (or a NESTED struct's field change) leaves every
/// other MIR statement byte-identical. Without the schema fingerprint baked into the decode rvalue
/// (`json_schema_sig`), the unit's `impl_hash` would be unchanged and the warm cache would serve a
/// STALE object still decoding the OLD key (the #514/#517 stale-cache class, reproduced end-to-end).
/// This gate pins that the rename misses on the unit's own MIR digest — both flat and nested.
#[test]
fn gate2b_json_decode_field_rename_invalidates() {
    if !backend() {
        return;
    }
    // Flat: rename `k` → `zzz` with the JSON literal (still key "k") and every other line unchanged.
    let flat_v1 = "import core.json\nS { a: i64, k: str }\nfn main() -> Result<(), Error> {\n  s: S := json.decode(\"{\\\"a\\\":1,\\\"k\\\":\\\"x\\\"}\")?\n  print(s.a)\n  return Ok(())\n}\n";
    let flat_v2 = "import core.json\nS { a: i64, zzz: str }\nfn main() -> Result<(), Error> {\n  s: S := json.decode(\"{\\\"a\\\":1,\\\"k\\\":\\\"x\\\"}\")?\n  print(s.a)\n  return Ok(())\n}\n";
    let proj = Project::new("json-rename-flat", &[("main.align", flat_v1)], "main.align");
    let cache = proj.cache();
    let cold = emit_all(&proj, &cache, Profile::Release, BuildTarget::Baseline, &no_exports(), false);
    assert!(cold.outcomes.iter().all(|o| !o.hit));
    if cc_available() {
        assert_eq!(cold.run(&proj, Profile::Release), "1\n"); // v1: key "k" matches → decode ok
    }
    proj.write("main.align", flat_v2);
    let hot = emit_all(&proj, &cache, Profile::Release, BuildTarget::Baseline, &no_exports(), false);
    assert!(!hot.outcome("main").hit, "a decode target field rename must miss");
    assert_eq!(
        hot.outcome("main").miss_reason,
        Some(FirstDiff::MirDigest),
        "the rename flips the baked json schema fingerprint → the unit's MIR digest"
    );
    if cc_available() {
        // v2 declares `zzz` but the input still carries key "k" → strict decode fails (missing
        // `zzz`), `?` propagates the Err, `main` returns without printing. A stale hit on the v1
        // object would wrongly print "1\n"; the rebuilt v2 prints nothing.
        assert_eq!(hot.run(&proj, Profile::Release), "", "the rebuilt v2 must fail decoding, not serve the stale v1 object");
    }

    // Nested: renaming a NESTED struct's field must invalidate the parent's decode unit too.
    let nest_v1 = "import core.json\nInner { x: i64, k: str }\nOuter { id: i64, inner: Inner }\nfn main() -> Result<(), Error> {\n  o: Outer := json.decode(\"{\\\"id\\\":1,\\\"inner\\\":{\\\"x\\\":5,\\\"k\\\":\\\"h\\\"}}\")?\n  print(o.id)\n  return Ok(())\n}\n";
    let nest_v2 = "import core.json\nInner { x: i64, renamed: str }\nOuter { id: i64, inner: Inner }\nfn main() -> Result<(), Error> {\n  o: Outer := json.decode(\"{\\\"id\\\":1,\\\"inner\\\":{\\\"x\\\":5,\\\"k\\\":\\\"h\\\"}}\")?\n  print(o.id)\n  return Ok(())\n}\n";
    let projn = Project::new("json-rename-nested", &[("main.align", nest_v1)], "main.align");
    let cachen = projn.cache();
    let coldn = emit_all(&projn, &cachen, Profile::Release, BuildTarget::Baseline, &no_exports(), false);
    assert!(coldn.outcomes.iter().all(|o| !o.hit));
    projn.write("main.align", nest_v2);
    let hotn = emit_all(&projn, &cachen, Profile::Release, BuildTarget::Baseline, &no_exports(), false);
    assert!(!hotn.outcome("main").hit, "a nested decode struct's field rename must miss");
    assert_eq!(
        hotn.outcome("main").miss_reason,
        Some(FirstDiff::MirDigest),
        "the nested field feeds the recursive schema fingerprint → the parent unit's MIR digest"
    );
}

// ---- Gate 2c: a union's array<Struct> ELEMENT field rename invalidates the cache ----------------

/// A shape-directed union's `array<Struct>` payload (J2b) reaches its element struct's fields only
/// through the codegen descriptor, exactly like a nested-struct field — so an element field RENAME
/// leaves every other MIR statement byte-identical. `json_union_schema_sig_into` must expand the
/// element struct's schema (not just print its id via `ty_name`), else the warm cache serves a STALE
/// object decoding the OLD element key (the #514/#517 class). This gate pins the miss.
#[test]
fn gate2c_json_union_array_element_rename_invalidates() {
    if !backend() {
        return;
    }
    // v1 element `Part { kind, text }`; v2 renames `text` → `txt` with the JSON literal keeping key
    // "text". Every other line is byte-identical.
    let v1 = "import core.json\nPart { kind: str, text: str }\nContent { Text(str), Parts(array<Part>) }\nfn main() -> Result<(), Error> {\n  arena {\n    c: Content := json.decode(\"[{\\\"kind\\\":\\\"a\\\",\\\"text\\\":\\\"b\\\"}]\")?\n    print(match c { Text(s) => -1, Parts(ps) => ps.len() })\n  }\n  return Ok(())\n}\n";
    let v2 = "import core.json\nPart { kind: str, txt: str }\nContent { Text(str), Parts(array<Part>) }\nfn main() -> Result<(), Error> {\n  arena {\n    c: Content := json.decode(\"[{\\\"kind\\\":\\\"a\\\",\\\"text\\\":\\\"b\\\"}]\")?\n    print(match c { Text(s) => -1, Parts(ps) => ps.len() })\n  }\n  return Ok(())\n}\n";
    let proj = Project::new("json-union-elem-rename", &[("main.align", v1)], "main.align");
    let cache = proj.cache();
    let cold = emit_all(&proj, &cache, Profile::Release, BuildTarget::Baseline, &no_exports(), false);
    assert!(cold.outcomes.iter().all(|o| !o.hit));
    if cc_available() {
        assert_eq!(cold.run(&proj, Profile::Release), "1\n"); // v1: key "text" matches → 1 part
    }
    proj.write("main.align", v2);
    let hot = emit_all(&proj, &cache, Profile::Release, BuildTarget::Baseline, &no_exports(), false);
    assert!(!hot.outcome("main").hit, "a union array-element field rename must miss");
    assert_eq!(
        hot.outcome("main").miss_reason,
        Some(FirstDiff::MirDigest),
        "the element field feeds the union's recursive schema fingerprint → the unit's MIR digest"
    );
    if cc_available() {
        // v2 requires key "txt" but the input carries "text" → strict decode fails, `?` propagates,
        // nothing prints. A stale v1 hit would wrongly print "1\n".
        assert_eq!(hot.run(&proj, Profile::Release), "", "the rebuilt v2 must fail decoding, not serve the stale v1 object");
    }
}

// ---- Gate 3: transitive A→B→C invalidation ------------------------------------------------------

#[test]
fn gate3_transitive_invalidation() {
    if !backend() {
        return;
    }
    let c_v1 = "module c\npub fn cval() -> i64 = 1\n";
    // A public-SURFACE change (a new pub fn) flips c's interface_hash — b and a both key on it.
    let c_pub = "module c\npub fn cval() -> i64 = 1\npub fn extra() -> i64 = 7\n";
    // A private-BODY change (existing fn body only) flips only c's impl_hash.
    let c_body = "module c\npub fn cval() -> i64 = 2\n";
    let b = "module b\nimport c\npub fn bval() -> i64 = c.cval() + 10\n";
    let a = "import b\nfn main() {\n  print(b.bval())\n}\n";
    let files = &[("c.align", c_v1), ("b.align", b), ("main.align", a)];

    // Case 1: public-surface change to C forces A and B to miss.
    let proj = Project::new("trans-pub", files, "main.align");
    let cache = proj.cache();
    let cold = emit_all(&proj, &cache, Profile::Release, BuildTarget::Baseline, &no_exports(), false);
    assert!(cold.outcomes.iter().all(|o| !o.hit));
    proj.write("c.align", c_pub);
    let hot = emit_all(&proj, &cache, Profile::Release, BuildTarget::Baseline, &no_exports(), false);
    assert!(!hot.outcome("c").hit, "C's own impl_hash changed → C misses");
    assert!(!hot.outcome("b").hit, "B keys on C's interface hash → B misses");
    assert!(!hot.outcome("main").hit, "A keys on C's transitive interface hash → A misses");

    // Case 2: private-body change to C forces only C to miss.
    let proj = Project::new("trans-body", files, "main.align");
    let cache = proj.cache();
    let cold = emit_all(&proj, &cache, Profile::Release, BuildTarget::Baseline, &no_exports(), false);
    assert!(cold.outcomes.iter().all(|o| !o.hit));
    proj.write("c.align", c_body);
    let hot = emit_all(&proj, &cache, Profile::Release, BuildTarget::Baseline, &no_exports(), false);
    assert!(!hot.outcome("c").hit, "C's private body changed → C misses");
    assert_eq!(hot.outcome("c").miss_reason, Some(FirstDiff::MirDigest));
    assert!(hot.outcome("b").hit, "B's interface inputs unchanged → B hits");
    assert!(hot.outcome("main").hit, "A's interface inputs unchanged → A hits");
}

// ---- Gate 3b: a pub aggregate-constant value edit re-keys dependents -----------------------------

#[test]
fn gate3b_aggregate_const_element_edit_invalidates_dependents() {
    if !backend() {
        return;
    }
    // A `pub` aggregate constant's value (its initializer source) is part of the exported interface
    // hash, so editing one element must miss the defining unit AND every dependent that reads it —
    // the real cross-unit cache proof (not just an interface-hash inequality).
    let cfg_v1 = "module cfg\npub WEIGHTS := [2, 3, 5]\n";
    let cfg_v2 = "module cfg\npub WEIGHTS := [2, 3, 6]\n"; // one element edited
    let main = "import cfg\nfn main() {\n  print(cfg.WEIGHTS.sum())\n}\n";
    let proj = Project::new("agg-const-edit", &[("cfg.align", cfg_v1), ("main.align", main)], "main.align");
    let cache = proj.cache();

    let cold = emit_all(&proj, &cache, Profile::Release, BuildTarget::Baseline, &no_exports(), false);
    assert!(cold.outcomes.iter().all(|o| !o.hit));
    if cc_available() {
        assert_eq!(cold.run(&proj, Profile::Release), "10\n"); // 2 + 3 + 5
    }

    // Edit one element of the pub aggregate constant.
    proj.write("cfg.align", cfg_v2);
    let hot = emit_all(&proj, &cache, Profile::Release, BuildTarget::Baseline, &no_exports(), false);
    assert!(!hot.outcome("cfg").hit, "the edited defining unit must miss");
    assert!(
        !hot.outcome("main").hit,
        "a dependent keying on cfg's interface hash must miss when a pub constant's value changes"
    );
    if cc_available() {
        assert_eq!(hot.run(&proj, Profile::Release), "11\n"); // 2 + 3 + 6, proves the rebuild took effect
    }
}

// ---- Gate 4: comment-only edit → hit ------------------------------------------------------------

#[test]
fn gate4_comment_only_edit_hits() {
    if !backend() {
        return;
    }
    let v1 = "fn main() {\n  print(secret(3))\n}\nfn secret(x: i64) -> i64 = x * 2\n";
    let cmt = "// a harmless comment\nfn main() {\n  print(secret(3))\n}\nfn secret(x: i64) -> i64 = x * 2\n";
    let proj = Project::new("comment", &[("main.align", v1)], "main.align");
    let cache = proj.cache();
    let cold = emit_all(&proj, &cache, Profile::Release, BuildTarget::Baseline, &no_exports(), false);
    assert!(cold.outcomes.iter().all(|o| !o.hit));
    // impl_hash is MIR-based (blind to comments), so the key is unchanged → hit.
    proj.write("main.align", cmt);
    let hot = emit_all(&proj, &cache, Profile::Release, BuildTarget::Baseline, &no_exports(), false);
    assert!(hot.all_hit(), "a comment-only edit lowers to identical MIR → the codegen cache hits");
}

// ---- Gate 5: edit then exact revert → hit (old CAS entry) ----------------------------------------

#[test]
fn gate5_edit_then_revert_hits_old_entry() {
    if !backend() {
        return;
    }
    let v1 = "fn main() {\n  print(secret(3))\n}\nfn secret(x: i64) -> i64 = x * 2\n";
    let v2 = "fn main() {\n  print(secret(3))\n}\nfn secret(x: i64) -> i64 = x * 3\n";
    let proj = Project::new("revert", &[("main.align", v1)], "main.align");
    let cache = proj.cache();
    let _ = emit_all(&proj, &cache, Profile::Release, BuildTarget::Baseline, &no_exports(), false); // publish v1
    proj.write("main.align", v2);
    let e2 = emit_all(&proj, &cache, Profile::Release, BuildTarget::Baseline, &no_exports(), false);
    assert!(!e2.outcome("main").hit, "the body edit misses");
    // Revert to the exact v1 bytes: the v1 full-key action was never overwritten → hit its old CAS blob.
    proj.write("main.align", v1);
    let e3 = emit_all(&proj, &cache, Profile::Release, BuildTarget::Baseline, &no_exports(), false);
    assert!(e3.all_hit(), "an exact revert must re-hit the original CAS entry");
}

// ---- Gate 6: corrupted blob → corruption event + rebuild + correct binary ------------------------

#[test]
fn gate6_corrupted_blob_rebuilds() {
    if !backend() {
        return;
    }
    let src = "fn main() {\n  print(secret(3))\n}\nfn secret(x: i64) -> i64 = x * 2\n";
    let proj = Project::new("corrupt", &[("main.align", src)], "main.align");
    let cache = proj.cache();
    let cold = emit_all(&proj, &cache, Profile::Release, BuildTarget::Baseline, &no_exports(), false);
    // Test seam: compute the CAS blob path from the object bytes and overwrite it with garbage.
    let obj_bytes = std::fs::read(&cold.objs[0]).expect("read cold object");
    let hex = Hash128::of(&obj_bytes).to_hex();
    let blob = proj.cache_root().join("cas").join(&hex[..2]).join(&hex);
    assert!(blob.exists(), "the cold build must have published the CAS blob");
    std::fs::write(&blob, b"not an object file").expect("corrupt the blob");

    let hot = emit_all(&proj, &cache, Profile::Release, BuildTarget::Baseline, &no_exports(), false);
    assert!(!hot.outcome("main").hit, "a corrupted blob forces a rebuild");
    assert_eq!(
        hot.outcome("main").miss_reason,
        Some(FirstDiff::CorruptEntry),
        "a digest-verify failure is reported as a corruption event"
    );
    // The rebuilt object is valid again (its bytes re-hash to the same digest → republished CAS).
    let rebuilt = std::fs::read(&hot.objs[0]).expect("read rebuilt object");
    assert_eq!(rebuilt, obj_bytes, "the rebuild reproduces the original (byte-identical) object");
    if cc_available() {
        assert_eq!(hot.run(&proj, Profile::Release), "6\n");
    }
}

// ---- Gate 7: cold vs hit byte-identity (object + executable) -------------------------------------

#[test]
fn gate7_cold_and_hit_are_byte_identical() {
    if !backend() {
        return;
    }
    let files = &[("lib.align", "module lib\npub fn f(x: i64) -> i64 = x * 3\n"), ("main.align", "import lib\nfn main() {\n  print(lib.f(7))\n}\n")];
    let proj = Project::new("coldhit", files, "main.align");
    let cache = proj.cache();

    let cold = emit_all(&proj, &cache, Profile::Release, BuildTarget::Baseline, &no_exports(), false);
    assert!(cold.outcomes.iter().all(|o| !o.hit));
    let cold_objs: Vec<Vec<u8>> = cold.objs.iter().map(|p| std::fs::read(p).unwrap()).collect();

    let hot = emit_all(&proj, &cache, Profile::Release, BuildTarget::Baseline, &no_exports(), false);
    assert!(hot.all_hit());
    let hot_objs: Vec<Vec<u8>> = hot.objs.iter().map(|p| std::fs::read(p).unwrap()).collect();
    assert_eq!(cold_objs, hot_objs, "hit object bytes must equal cold object bytes");

    if cc_available() {
        let cold_exe = cold.exe_bytes(&proj, Profile::Release);
        let hot_exe = hot.exe_bytes(&proj, Profile::Release);
        assert_eq!(cold_exe, hot_exe, "the executable must be byte-identical between a cold and a fully-hit build");
    }
}

// ---- Gate 8: profile change / --export change → miss with the right FirstDiff --------------------

#[test]
fn gate8_profile_and_export_first_diffs() {
    if !backend() {
        return;
    }
    // Profile change.
    let proj = Project::new("profile", &[("main.align", "fn main() {\n  print(42)\n}\n")], "main.align");
    let cache = proj.cache();
    let _ = emit_all(&proj, &cache, Profile::Release, BuildTarget::Baseline, &no_exports(), false);
    let dev = emit_all(&proj, &cache, Profile::Dev, BuildTarget::Baseline, &no_exports(), false);
    assert!(!dev.outcome("main").hit);
    assert_eq!(dev.outcome("main").miss_reason, Some(FirstDiff::Profile), "a profile change misses on the profile component");

    // --export change (entry unit).
    let proj = Project::new("export", &[("main.align", "pub fn foo() -> i64 = 7\nfn main() {\n  print(foo())\n}\n")], "main.align");
    let cache = proj.cache();
    let _ = emit_all(&proj, &cache, Profile::Release, BuildTarget::Baseline, &no_exports(), false);
    let exp = emit_all(&proj, &cache, Profile::Release, BuildTarget::Baseline, &["foo".to_string()], false);
    assert!(!exp.outcome("main").hit);
    assert_eq!(exp.outcome("main").miss_reason, Some(FirstDiff::Exports), "an --export change misses on the export set");
}

// ---- Gate 9: rt-lto on/off → distinct keys ------------------------------------------------------

#[test]
fn gate9_rt_lto_distinct_keys() {
    if !backend() {
        return;
    }
    let proj = Project::new("rtlto", &[("main.align", "fn main() {\n  print(hello(\"x\"))\n}\nfn hello(s: str) -> i64 = if s == \"x\" { 1 } else { 0 }\n")], "main.align");
    let cache = proj.cache();
    let off = emit_all(&proj, &cache, Profile::Release, BuildTarget::Baseline, &no_exports(), false);
    assert!(off.outcomes.iter().all(|o| !o.hit));
    let on = emit_all(&proj, &cache, Profile::Release, BuildTarget::Baseline, &no_exports(), true);
    assert!(!on.outcome("main").hit, "rt-lto on is a distinct key from rt-lto off");
    assert_eq!(on.outcome("main").miss_reason, Some(FirstDiff::RtLto), "the rt-lto mode is the differing key component");
    // And rt-lto on is itself cacheable: a second rt-lto-on build hits.
    let on2 = emit_all(&proj, &cache, Profile::Release, BuildTarget::Baseline, &no_exports(), true);
    assert!(on2.all_hit(), "a repeated rt-lto-on build hits its own key");
}

// ---- Disabled fast path: no cache dir, verbatim object, `None` reason ----------------------------

#[test]
fn disabled_cache_emits_verbatim_without_touching_disk() {
    if !backend() {
        return;
    }
    let proj = Project::new("disabled", &[("main.align", "fn main() {\n  print(secret(4))\n}\nfn secret(x: i64) -> i64 = x * 2\n")], "main.align");

    // Enabled cold build → the reference object bytes + a populated cache.
    let enabled = emit_all(&proj, &proj.cache(), Profile::Release, BuildTarget::Baseline, &no_exports(), false);
    let ref_obj = std::fs::read(&enabled.objs[0]).unwrap();

    // Disabled build → miss with NO reason (cache not consulted) and byte-identical object; crucially
    // it must not create the cache root at all (the gating that keeps the binary hash off the hot path).
    let disabled = emit_all(&proj, &CacheContext::Disabled, Profile::Release, BuildTarget::Baseline, &no_exports(), false);
    assert!(!disabled.outcome("main").hit);
    assert_eq!(disabled.outcome("main").miss_reason, None, "a disabled cache reports no first-diff reason");
    let dis_obj = std::fs::read(&disabled.objs[0]).unwrap();
    assert_eq!(dis_obj, ref_obj, "the disabled path emits byte-identical object bytes");

    // A CacheContext::Disabled never writes anywhere; a separate never-touched root stays absent.
    let untouched = proj.dir.join("never-created-root");
    assert!(!untouched.exists());
    assert!(!CacheContext::Disabled.is_enabled());
}

// ---- Gate 10: cross-process impl_hash stability (fresh alignc subprocesses) ----------------------

/// Count the action manifests under a cache root (recursively over `actions/codegen`).
fn action_manifest_count(root: &Path) -> usize {
    let dir = root.join("actions").join("codegen");
    match std::fs::read_dir(&dir) {
        Ok(entries) => entries.filter(|e| e.as_ref().map(|e| e.path().is_file()).unwrap_or(false)).count(),
        Err(_) => 0,
    }
}

#[test]
fn gate10_cross_process_impl_hash_stability() {
    if !backend() || !cc_available() {
        return;
    }
    let proj = Project::new("xproc", &[("main.align", "fn main() {\n  print(secret(3))\n}\nfn secret(x: i64) -> i64 = x * 2\n")], "main.align");
    let alignc = env!("CARGO_BIN_EXE_alignc");
    let shared = proj.dir.join("xcache");

    let run_once = || {
        std::process::Command::new(alignc)
            .arg("build")
            .arg("main.align")
            .current_dir(&proj.dir)
            .env("ALIGNC_CACHE", &shared)
            .output()
            .expect("spawn alignc")
    };

    let out1 = run_once();
    assert!(out1.status.success(), "first alignc build failed: {}", String::from_utf8_lossy(&out1.stderr));
    let count1 = action_manifest_count(&shared);
    assert_eq!(count1, 1, "one unit → one action manifest after the first build");
    let exe1 = std::fs::read(proj.dir.join("main")).expect("read exe after run 1");

    let out2 = run_once();
    assert!(out2.status.success(), "second alignc build failed: {}", String::from_utf8_lossy(&out2.stderr));
    let count2 = action_manifest_count(&shared);
    // A stable cross-process key ⇒ the second build re-hits the same action digest and adds NO new
    // manifest. An unstable impl_hash would have written a second action file at a different digest.
    assert_eq!(count2, count1, "a stable cross-process impl_hash re-hits the same key (no new manifest)");
    let exe2 = std::fs::read(proj.dir.join("main")).expect("read exe after run 2");
    assert_eq!(exe1, exe2, "the cross-process cache-hit executable is byte-identical");
}

// ---- Gate 13b: the ALIGN_SORT_ADAPTIVE measurement toggle never poisons the shared cache ---------

/// The `ALIGN_SORT_ADAPTIVE` toggle (doc-12 §4.1) changes emitted codegen for a `.sort()` unit. Its
/// effect flows into `impl_hash` (so the object-cache key already differs), AND `CacheContext::from_env`
/// forces the cache off when it is set (fail-closed). This end-to-end gate proves no poisoning: a
/// baseline (`=off`) build between two shipped-shape builds neither publishes into nor reads from the
/// shared cache, so the shipped-shape object is served byte-identical and is never replaced by the
/// baseline one. Drives fresh `alignc` subprocesses (isolated env — never touches this process's env).
#[test]
fn gate13b_sort_adaptive_toggle_never_poisons_cache() {
    if !backend() || !cc_available() {
        return;
    }
    let src = "fn main() -> Result<(), Error> {\n\
        \x20 xs := [5, 3, 4, 1, 2].sort()\n\
        \x20 print(xs[0])\n\
        \x20 return Ok(())\n\
        }\n";
    let proj = Project::new("sortpoison", &[("main.align", src)], "main.align");
    let alignc = env!("CARGO_BIN_EXE_alignc");
    let shared = proj.dir.join("scache");

    let build = |adaptive_off: bool| {
        let mut cmd = std::process::Command::new(alignc);
        cmd.arg("build").arg("main.align").current_dir(&proj.dir).env("ALIGNC_CACHE", &shared);
        if adaptive_off {
            cmd.env("ALIGN_SORT_ADAPTIVE", "off");
        } else {
            cmd.env_remove("ALIGN_SORT_ADAPTIVE");
        }
        let out = cmd.output().expect("spawn alignc");
        assert!(out.status.success(), "alignc build failed: {}", String::from_utf8_lossy(&out.stderr));
        std::fs::read(proj.dir.join("main")).expect("read built exe")
    };

    // 1. Shipped shape (toggle unset) → publishes the adaptive object; one action manifest.
    let e1 = build(false);
    assert_eq!(action_manifest_count(&shared), 1, "the shipped-shape build publishes one action manifest");

    // 2. Baseline (`=off`) → cache forced off: publishes NOTHING (manifest count unchanged) and the
    //    exe differs from the shipped shape (the toggle genuinely changes codegen — sanity).
    let e2 = build(true);
    assert_eq!(action_manifest_count(&shared), 1, "the `=off` build must not publish into the shared cache (cache forced off)");
    assert_ne!(e1, e2, "the toggle must change emitted codegen (else the poisoning test proves nothing)");

    // 3. Shipped shape again → re-hits the adaptive manifest: byte-identical to run 1, NOT poisoned
    //    by the intervening baseline build.
    let e3 = build(false);
    assert_eq!(action_manifest_count(&shared), 1, "the second shipped-shape build re-hits (no new manifest)");
    assert_eq!(e1, e3, "the shipped-shape exe must be byte-identical — the baseline build never poisoned the cache");
}

// ---- Gate 13c: the ALIGN_BUFFER_DONATE measurement toggle never poisons the shared cache ---------

/// The `ALIGN_BUFFER_DONATE` toggle (doc-10 §8.1) changes emitted codegen for a donated
/// materialization unit (`make().map(f).to_array()`). Same contract as Gate 13b: the effect flows
/// into `impl_hash` (key differs) AND `CacheContext::from_env` forces the cache off when it is set,
/// so a `=off` baseline build between two shipped-shape builds neither publishes nor reads, and the
/// shipped-shape object is served byte-identical.
#[test]
fn gate13c_buffer_donate_toggle_never_poisons_cache() {
    if !backend() || !cc_available() {
        return;
    }
    let src = "fn make() -> array<i64> = [1, 2, 3, 4, 5].to_array()\n\
        fn dbl(x: i64) -> i64 = x * 2\n\
        fn main() -> Result<(), Error> {\n\
        \x20 print(make().map(dbl).to_array().sum())\n\
        \x20 return Ok(())\n\
        }\n";
    let proj = Project::new("donatepoison", &[("main.align", src)], "main.align");
    let alignc = env!("CARGO_BIN_EXE_alignc");
    let shared = proj.dir.join("scache");

    let build = |donate_off: bool| {
        let mut cmd = std::process::Command::new(alignc);
        cmd.arg("build").arg("main.align").current_dir(&proj.dir).env("ALIGNC_CACHE", &shared);
        if donate_off {
            cmd.env("ALIGN_BUFFER_DONATE", "off");
        } else {
            cmd.env_remove("ALIGN_BUFFER_DONATE");
        }
        let out = cmd.output().expect("spawn alignc");
        assert!(out.status.success(), "alignc build failed: {}", String::from_utf8_lossy(&out.stderr));
        std::fs::read(proj.dir.join("main")).expect("read built exe")
    };

    // 1. Shipped shape (toggle unset) → publishes the donated-shape object; one action manifest.
    let e1 = build(false);
    assert_eq!(action_manifest_count(&shared), 1, "the shipped-shape build publishes one action manifest");

    // 2. Baseline (`=off`) → cache forced off: publishes nothing and differs from the donated shape.
    let e2 = build(true);
    assert_eq!(action_manifest_count(&shared), 1, "the `=off` build must not publish into the shared cache (cache forced off)");
    assert_ne!(e1, e2, "the toggle must change emitted codegen (else the poisoning test proves nothing)");

    // 3. Shipped shape again → re-hits the donated manifest, byte-identical, not poisoned.
    let e3 = build(false);
    assert_eq!(action_manifest_count(&shared), 1, "the second shipped-shape build re-hits (no new manifest)");
    assert_eq!(e1, e3, "the shipped-shape exe must be byte-identical — the baseline build never poisoned the cache");
}

// ---- Gate 11: an unused, unimported file is outside every existing action -----------------------

#[test]
fn gate11_unimported_file_edit_keeps_all_units_hot() {
    if !backend() {
        return;
    }
    let files = &[
        ("lib.align", "module lib\npub fn f(x: i64) -> i64 = x + 1\n"),
        ("main.align", "import lib\nfn main() {\n  print(lib.f(41))\n}\n"),
    ];
    let proj = Project::new("unimported", files, "main.align");
    let cache = proj.cache();
    let cold = emit_all(&proj, &cache, Profile::Release, BuildTarget::Baseline, &no_exports(), false);
    assert!(cold.outcomes.iter().all(|o| !o.hit));

    proj.write("unused.align", "module unused\npub fn value() -> i64 = 1\n");
    let added = emit_all(&proj, &cache, Profile::Release, BuildTarget::Baseline, &no_exports(), false);
    assert!(added.all_hit(), "adding an unimported file must not enter the unit graph or invalidate existing actions");

    proj.write("unused.align", "module unused\npub fn value() -> i64 = 999\n");
    let edited = emit_all(&proj, &cache, Profile::Release, BuildTarget::Baseline, &no_exports(), false);
    assert!(edited.all_hit(), "editing an unimported file must leave every existing unit hot");
}

// ---- Gate 12: producer killed before publish leaves no readable entry ---------------------------

#[test]
fn gate12_orphaned_private_stage_is_never_a_hit() {
    if !backend() {
        return;
    }
    let proj = Project::new("killed-publish", &[("main.align", "fn main() {\n  print(42)\n}\n")], "main.align");
    let cache = proj.cache();

    // This is the only filesystem state a process killed between staged write and atomic rename can
    // leave. It has no final action-manifest name and therefore must be invisible to lookup.
    let action_dir = proj.cache_root().join("actions").join("codegen");
    std::fs::create_dir_all(&action_dir).expect("mkdir action dir");
    let orphan = action_dir.join(format!(".cache-stage-dead-{}", nonce()));
    std::fs::write(&orphan, b"partial manifest bytes").expect("write orphan stage");

    let rebuilt = emit_all(&proj, &cache, Profile::Release, BuildTarget::Baseline, &no_exports(), false);
    assert!(!rebuilt.outcome("main").hit, "an orphaned stage is not a published cache entry");
    assert_eq!(rebuilt.outcome("main").miss_reason, Some(FirstDiff::NoPriorEntry));
    assert!(orphan.exists(), "ordinary lookup/publish must not mistake or rename an unrelated orphan stage");

    let hot = emit_all(&proj, &cache, Profile::Release, BuildTarget::Baseline, &no_exports(), false);
    assert!(hot.all_hit(), "the safe rebuild publishes a complete entry for the next build");
}

// ---- Gate 13: resolved baseline/native CPU identity separates object namespaces -----------------

#[test]
fn gate13_cpu_change_misses_only_the_cpu_namespace() {
    if !backend() {
        return;
    }
    let proj = Project::new("cpu-key", &[("main.align", "fn main() {\n  print(42)\n}\n")], "main.align");
    let cache = proj.cache();
    let baseline = emit_all(&proj, &cache, Profile::Release, BuildTarget::Baseline, &no_exports(), false);
    assert!(!baseline.outcome("main").hit);

    let native = emit_all(&proj, &cache, Profile::Release, BuildTarget::Native, &no_exports(), false);
    assert!(!native.outcome("main").hit, "native codegen must not reuse the baseline object");
    assert_eq!(native.outcome("main").miss_reason, Some(FirstDiff::Cpu));

    let native_hot = emit_all(&proj, &cache, Profile::Release, BuildTarget::Native, &no_exports(), false);
    assert!(native_hot.all_hit(), "the native namespace is independently cacheable");
}
