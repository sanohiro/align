//! M15 S2 — per-unit codegen + N-object link gates.
//!
//! The separate-compilation build path (`build_per_unit` → one object per unit → link N objects)
//! proved against the whole-program path it will replace at S2b:
//!
//!  a. **N=1 byte-identity** — a single-file program's per-unit object is byte-for-byte the
//!     whole-program object (the keystone the migration rests on), and both run identically.
//!  b. **Multi-file end-to-end** — cross-unit calls / imported types / a Move return / an
//!     in-consumer generic build, link, and RUN with output identical to the whole-program build.
//!  c. **Visibility** — a non-entry unit's object exports its `pub` fns and hides its privates /
//!     the consumer-side monomorphs; the entry object exports `main`; a cross-unit call site is an
//!     undefined reference to the mangled extern.
//!  d. **Capability union** — a unit using `std.compress` contributes `libz` and only `libz` to the
//!     deterministic union; a capability-free unit contributes nothing.
//!  e. **Per-unit rt-lto** — an `x == "literal"` hot loop in the NON-entry unit inlines the runtime
//!     primitive under `--rt-lto` (absence check), and the flag-off path still calls it.
//!  f. **impl_hash** — a private-body edit flips impl_hash but not interface_hash; a comment-only
//!     edit that lowers to identical MIR flips neither (the MIR-based impl_hash win over source bytes).

mod common;
use common::*;

use std::path::PathBuf;

fn backend() -> bool {
    backend_available()
}

/// Strip a Mach-O leading underscore so a symbol comparison is object-format-portable.
fn norm(sym: &str) -> &str {
    sym.strip_prefix('_').unwrap_or(sym)
}

// ---- Gate a: N=1 byte-identity ------------------------------------------------------------------

#[test]
fn gate_a_single_file_object_is_byte_identical_to_whole_program() {
    if !backend() {
        return;
    }
    // A single-file program with a `pub` fn: in the entry unit it must stay internal (nothing
    // imports the entry), so the per-unit object matches the whole-program object exactly.
    let src = "pub fn helper(x: i64) -> i64 = x + 1\nfn main() {\n  print(helper(41))\n}\n";

    let mut sm_wp = SourceMap::new();
    let checked = check(&mut sm_wp, "n1.align", src);
    assert!(!checked.diags.has_errors());
    let wp_mir = lower_to_mir(&checked.hir);

    let mut sm_pu = SourceMap::new();
    let walk = build_per_unit(&mut sm_pu, "n1.align", src);
    assert!(!walk.diags.has_errors());
    assert_eq!(walk.units.len(), 1, "a single file is exactly one unit");
    let pu_mir = &walk.units[0].mir;
    assert!(walk.units[0].is_entry);

    let dir = std::env::temp_dir();
    let pid = std::process::id();
    let obj_wp = dir.join(format!("align-n1-wp-{pid}.o"));
    let obj_pu = dir.join(format!("align-n1-pu-{pid}.o"));
    struct Cleanup(Vec<PathBuf>);
    impl Drop for Cleanup {
        fn drop(&mut self) {
            for p in &self.0 {
                let _ = std::fs::remove_file(p);
            }
        }
    }
    let _g = Cleanup(vec![obj_wp.clone(), obj_pu.clone()]);

    emit_object_file(&wp_mir, &obj_wp, BuildTarget::Baseline, Profile::Release, &[], false).expect("wp codegen");
    emit_object_file(pu_mir, &obj_pu, BuildTarget::Baseline, Profile::Release, &[], false).expect("pu codegen");

    let bytes_wp = std::fs::read(&obj_wp).expect("read wp obj");
    let bytes_pu = std::fs::read(&obj_pu).expect("read pu obj");
    assert_eq!(
        bytes_wp, bytes_pu,
        "N=1 per-unit object must be byte-identical to the whole-program object (settlement keystone)"
    );

    // And the linked executable runs identically (whole-program vs per-unit).
    let wp_out = build_and_run("n1-wp", src);
    let pu_out = build_per_unit_multi("n1-pu", &[("main.align", src)], "main.align").link_and_run();
    assert_eq!(wp_out.stdout, pu_out.stdout, "N=1 run output must match");
}

// ---- Gate b: multi-file end-to-end --------------------------------------------------------------

const LIB_B: &str = "\
module lib
pub Vec2 { x: i64, y: i64 }
pub fn mk(x: i64, y: i64) -> Vec2 = Vec2 { x: x, y: y }
pub fn sumv(v: Vec2) -> i64 = v.x + v.y
pub fn id<T>(a: T) -> T = a
pub fn triple() -> array<i64> = [1, 2, 3].to_array()
";

const MAIN_B: &str = "\
import lib
fn main() {
  v := lib.mk(10, 20)
  print(lib.sumv(v))
  print(lib.id(5))
  a := lib.triple()
  print(a.len())
}
";

#[test]
fn gate_b_multi_file_run_matches_whole_program() {
    if !backend() {
        return;
    }
    let files = &[("lib.align", LIB_B), ("main.align", MAIN_B)];
    let whole = build_and_run_multi("gb-whole", files, "main.align");
    let per = build_per_unit_multi("gb-peru", files, "main.align");
    let per_out = per.link_and_run();
    assert_eq!(
        String::from_utf8_lossy(&whole.stdout),
        String::from_utf8_lossy(&per_out.stdout),
        "per-unit multi-file output must match the whole-program build"
    );
    // Sanity: the expected values (imported struct sum, in-consumer generic, Move-array return).
    assert_eq!(String::from_utf8_lossy(&per_out.stdout), "30\n5\n3\n");
}

// ---- Gate c: visibility (llvm-nm) ---------------------------------------------------------------

const LIB_C: &str = "\
module lib
pub fn sumv(a: i64, b: i64) -> i64 = a + b
pub fn dbl(x: i64) -> i64 = secret(x)
pub fn id<T>(a: T) -> T = a
fn secret(x: i64) -> i64 = x * 2
";

const MAIN_C: &str = "\
import lib
fn main() {
  print(lib.sumv(1, 2))
  print(lib.id(9))
}
";

#[test]
fn gate_c_visibility_symbols() {
    if !backend() {
        return;
    }
    let per = build_per_unit_multi("gc", &[("lib.align", LIB_C), ("main.align", MAIN_C)], "main.align");
    // Emit at O0 so internal symbols (the private fn, the consumer-side monomorph) stay observable —
    // O2 would inline the trivial monomorph away, hiding the structure this gate checks.
    let objs = per.emit_objects_with(Profile::Dev, false);
    // Unit order is bottom-up: lib (dependency) first, main (entry) last.
    let lib_idx = per.walk.units.iter().position(|u| u.unit == "lib").expect("lib built");
    let main_idx = per.walk.units.iter().position(|u| u.unit == "main").expect("main built");

    let Some(lib_syms) = nm_symbols(&objs[lib_idx]) else {
        return; // llvm-nm unavailable — skip the binary-inspection assertions
    };
    let main_syms = nm_symbols(&objs[main_idx]).expect("nm main");

    let is_ext = |syms: &[(char, String)], want: &str| {
        syms.iter().any(|(k, n)| *k == 'T' && norm(n) == want)
    };
    let has_any = |syms: &[(char, String)], want: &str| syms.iter().any(|(_, n)| norm(n) == want);

    // Non-entry unit `lib`: its `pub` non-generic fns are external-defined...
    assert!(is_ext(&lib_syms, "lib$sumv"), "lib$sumv must be external in lib.o:\n{lib_syms:?}");
    assert!(is_ext(&lib_syms, "lib$dbl"), "lib$dbl must be external in lib.o");
    // ...its private fn is NOT external (internal linkage — never a 'T')...
    assert!(
        !is_ext(&lib_syms, "lib$secret"),
        "lib$secret is private; it must not be an external symbol:\n{lib_syms:?}"
    );
    // ...and the generic's monomorph is emitted consumer-side, so no `lib$id...` symbol here.
    assert!(
        !lib_syms.iter().any(|(_, n)| norm(n).starts_with("lib$id")),
        "a generic pub fn's monomorph must NOT live in the producer object:\n{lib_syms:?}"
    );

    // Entry unit `main`: `main` is external (the C entry)...
    assert!(is_ext(&main_syms, "main"), "main must be external in main.o:\n{main_syms:?}");
    // ...the cross-unit call is an undefined reference to the mangled extern...
    assert!(
        main_syms.iter().any(|(k, n)| *k == 'U' && norm(n) == "lib$sumv"),
        "main.o must reference lib$sumv as an undefined extern:\n{main_syms:?}"
    );
    // ...and the in-consumer monomorph lives here (as an internal, non-'T' symbol).
    assert!(
        has_any(&main_syms, "lib$id$i64") || main_syms.iter().any(|(_, n)| norm(n).starts_with("lib$id")),
        "the lib$id monomorph must be emitted into the consumer object:\n{main_syms:?}"
    );
}

// ---- Gate d: capability union -------------------------------------------------------------------

const ZIP_D: &str = "\
module zip
import std.compress
pub fn zlen(data: str) -> Result<i64, Error> {
  comp := compress.gzip_compress(data, 6)?
  return Ok(comp.bytes().len())
}
";

const PLAIN_D: &str = "\
module plain
pub fn bump(x: i64) -> i64 = x + 1
";

const MAIN_D: &str = "\
import zip
import plain
fn main() -> Result<(), Error> {
  n := zip.zlen(\"hello world\")?
  print(n + plain.bump(0))
  return Ok(())
}
";

#[test]
fn gate_d_capability_union_is_libz_only() {
    if !backend() {
        return;
    }
    let per = build_per_unit_multi("gd", &[("zip.align", ZIP_D), ("plain.align", PLAIN_D), ("main.align", MAIN_D)], "main.align");
    let libs = per.link_libs_union();
    assert!(libs.contains(&"z".to_string()), "the compress unit must contribute libz: {libs:?}");
    for absent in ["zstd", "crypto", "ssl"] {
        assert!(!libs.contains(&absent.to_string()), "no unit uses {absent}: {libs:?}");
    }

    // End-to-end: the linked binary records exactly the libz dependency among gated libraries.
    let objs = per.emit_objects(false);
    let obj_refs: Vec<&std::path::Path> = objs.iter().map(|p| p.as_path()).collect();
    // Lives inside `per.dir` (the project's temp dir), so it is removed automatically when `per`
    // drops — no separate cleanup guard needed.
    let exe = per.dir.join(format!("align-gd{}", std::env::consts::EXE_SUFFIX));
    link_objects(&obj_refs, &exe, &libs, Profile::Release).expect("link");
    if let Some(readobj) = align_driver::llvm_tool("llvm-readobj") {
        let needed = needed_libs(&readobj, &exe);
        assert!(needed.iter().any(|l| is_lib(l, "z")), "binary must need libz: {needed:?}");
        for absent in ["zstd", "crypto", "ssl"] {
            assert!(!needed.iter().any(|l| is_lib(l, absent)), "binary must not need lib{absent}: {needed:?}");
        }
    }
}

// ---- Gate e: per-unit rt-lto --------------------------------------------------------------------

const HOT_E: &str = "\
module hot
fn is_hello(x: str) -> bool = x == \"hello\"
pub fn eq_count(s: slice<str>) -> i64 = s.where(is_hello).count()
";

const MAIN_E: &str = "\
import hot
fn main() {}
";

#[test]
fn gate_e_rt_lto_inlines_in_non_entry_unit() {
    if !backend() {
        return;
    }
    let per = build_per_unit_multi("ge", &[("hot.align", HOT_E), ("main.align", MAIN_E)], "main.align");
    let hot = per.unit("hot");
    // Off-path: the optimized IR of the NON-entry unit still calls the runtime primitive.
    let off = emit_llvm_ir(&hot.mir, BuildTarget::Baseline, true, &[], false).expect("emit off");
    assert!(
        off.contains("call i32 @align_rt_str_eq"),
        "flag-off optimized IR should still call align_rt_str_eq:\n{off}"
    );
    // On-path: `--rt-lto` merges the baked bitcode into THIS unit's raw module, so the primitive
    // inlines and the call is gone — the merge is per-unit (hot loops live in arbitrary units).
    let on = emit_llvm_ir(&hot.mir, BuildTarget::Baseline, true, &[], true).expect("emit on");
    assert!(
        !on.contains("call i32 @align_rt_str_eq"),
        "under per-unit --rt-lto align_rt_str_eq must inline in the non-entry unit:\n{on}"
    );
}

// ---- Gate f: impl_hash (MIR-based) --------------------------------------------------------------

#[test]
fn gate_f_impl_hash_flips_on_body_not_on_comment() {
    // A private-body edit changes the unit's MIR → impl_hash flips, interface_hash does not.
    let lib_v1 = "module lib\npub fn pubfn(x: i64) -> i64 = x + secret(x)\nfn secret(x: i64) -> i64 = x * 2\n";
    let lib_v2 = "module lib\npub fn pubfn(x: i64) -> i64 = x + secret(x)\nfn secret(x: i64) -> i64 = x * 3\n";
    // A comment-only edit lowers to identical MIR → neither hash moves (the win over source bytes).
    let lib_cmt = "module lib\n// a harmless comment\npub fn pubfn(x: i64) -> i64 = x + secret(x)\nfn secret(x: i64) -> i64 = x * 2\n";
    let main = "import lib\nfn main() {\n  print(lib.pubfn(3))\n}\n";

    let find = |b: &PerUnitBuilt, u: &str| b.unit(u).summary.clone();
    let v1 = build_per_unit_multi("gf1", &[("lib.align", lib_v1), ("main.align", main)], "main.align");
    let v2 = build_per_unit_multi("gf2", &[("lib.align", lib_v2), ("main.align", main)], "main.align");
    let cm = build_per_unit_multi("gfc", &[("lib.align", lib_cmt), ("main.align", main)], "main.align");

    let (s1, s2, sc) = (find(&v1, "lib"), find(&v2, "lib"), find(&cm, "lib"));
    assert_eq!(s1.interface_hash, s2.interface_hash, "a private-body edit must NOT change interface_hash");
    assert_ne!(s1.impl_hash, s2.impl_hash, "a private-body edit MUST change impl_hash");

    assert_eq!(s1.interface_hash, sc.interface_hash, "a comment-only edit must not change interface_hash");
    assert_eq!(
        s1.impl_hash, sc.impl_hash,
        "a comment-only edit lowers to identical MIR — impl_hash must not move (the MIR-based win)"
    );
}
