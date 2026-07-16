//! ThinLTO S1 (`--thin-lto`) — the serial cross-unit-optimizing build. Gates:
//!
//!  1. **Cross-unit inline, mutation-checked both directions** — a two-unit program (unit B: small
//!     `pub` fns; unit A: calls them). Under `--thin-lto` the entry object's undefined cross-unit
//!     references (`U lib$add1`, `U lib$mul2`) VANISH (imported + inlined); without the flag they
//!     remain. And the built program runs with identical output.
//!  2. **M13 Slice-5 wide-tuple `sret` positive** — a cross-unit fn returning a 4- / 8-`i64` tuple
//!     (the recorded cross-unit-aggregate gate). Under `--thin-lto` the boundary (and thus the `sret`
//!     store/load round trip) vanishes: the entry object no longer references the tuple fns; without
//!     the flag the cross-unit ABI is retained. Run-parity confirms correctness.
//!  3. **N=1 byte-identity** — `--thin-lto` on a single-unit program produces a byte-identical
//!     executable to the no-flag build (N=1 skips all three ThinLTO phases → today's object).
//!  4. **Run-parity corpus** — representative multi-file programs run under `--thin-lto` with stdout
//!     and exit code identical to the whole-program build.
//!  5. **Preserve survival** — `main` stays the external entry, and a non-entry `pub` fn stays an
//!     external DEFINE in its unit's ThinLTO object (fail-closed v1 preserve set); the linked exe runs.
//!  6. **Profile / verb rejection** — `--thin-lto` on `dev` (and on a non-build verb) is a diagnostic,
//!     not a silent no-op (subprocess, mirrors `rt_lto.rs`).
//!  7. **Flag-off unchanged** — the flag-off per-unit object path is byte-deterministic and untouched
//!     by this slice (the whole existing suite pins its exact bytes; this adds one explicit check).

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

/// Build a program's per-unit objects the ThinLTO way (all three phases) into `dir/thin<i>.o`. Uses a
/// DISABLED cache + `-j 1` so these S1 symbol-shape gates see a fresh, deterministic build.
fn thin_objects(per: &PerUnitBuilt) -> Vec<PathBuf> {
    let n = per.walk.units.len();
    let objs: Vec<PathBuf> = (0..n).map(|i| per.dir.join(format!("thin{i}.o"))).collect();
    align_driver::build_thin_lto(
        &per.walk.units,
        &objs,
        &align_driver::CacheContext::Disabled,
        &BuildTarget::Baseline,
        Profile::Release,
        &[],
        false,
        &per.dir,
        1,
    )
    .expect("thin-lto build");
    objs
}

fn entry_idx(per: &PerUnitBuilt) -> usize {
    per.walk.units.iter().position(|u| u.is_entry).expect("an entry unit")
}

/// Whether `syms` has an UNDEFINED reference to a symbol named `want`.
fn has_undef(syms: &[(char, String)], want: &str) -> bool {
    syms.iter().any(|(k, n)| *k == 'U' && norm(n) == want)
}

/// Whether `syms` has an external DEFINE of `want`.
fn has_ext_def(syms: &[(char, String)], want: &str) -> bool {
    syms.iter().any(|(k, n)| *k == 'T' && norm(n) == want)
}

// ---- Gate 1: cross-unit inline, mutation-checked both directions --------------------------------

const LIB1: &str = "module lib\npub fn add1(x: i64) -> i64 = x + 1\npub fn mul2(x: i64) -> i64 = x * 2\n";
const MAIN1: &str = "\
import lib
fn main() {
  mut s := 0
  s = s + lib.add1(41)
  s = s + lib.mul2(21)
  print(s)
}
";

#[test]
fn gate_cross_unit_inline_mutation_checked() {
    if !backend() {
        return;
    }
    let per = build_per_unit_multi("tl-inline", &[("lib.align", LIB1), ("main.align", MAIN1)], "main.align");
    let ei = entry_idx(&per);

    // Baseline (flag-off) objects: the entry references the callees as undefined cross-unit symbols.
    let base = per.emit_objects(false);
    // ThinLTO objects: the callees are imported into the entry and inlined.
    let thin = thin_objects(&per);

    let Some(base_syms) = nm_symbols(&base[ei]) else {
        return; // llvm-nm unavailable — skip the binary-inspection assertions
    };
    let thin_syms = nm_symbols(&thin[ei]).expect("nm thin entry");

    // Direction A (flag OFF): the cross-unit call is an undefined reference.
    assert!(
        has_undef(&base_syms, "lib$add1") && has_undef(&base_syms, "lib$mul2"),
        "flag-off entry object must reference lib$add1 / lib$mul2 as undefined externs:\n{base_syms:?}"
    );
    // Direction B (flag ON): the reference is gone (imported + inlined).
    assert!(
        !has_undef(&thin_syms, "lib$add1") && !has_undef(&thin_syms, "lib$mul2"),
        "under --thin-lto the cross-unit calls must inline (no undefined ref left):\n{thin_syms:?}"
    );

    // And the ThinLTO-built program runs with the expected output.
    let obj_refs: Vec<&std::path::Path> = thin.iter().map(|p| p.as_path()).collect();
    let exe = per.dir.join(format!("tl-inline{}", std::env::consts::EXE_SUFFIX));
    align_driver::link_objects(&obj_refs, &exe, &per.link_libs_union(), Profile::Release).expect("link");
    let out = std::process::Command::new(&exe).output().expect("run");
    assert_eq!(String::from_utf8_lossy(&out.stdout), "84\n", "thin-lto program output");
}

// ---- Gate 2: M13 Slice-5 wide-tuple sret positive ----------------------------------------------

const LIB2: &str = "\
module lib
pub fn quad(x: i64) -> (i64, i64, i64, i64) = (x, x + 1, x + 2, x + 3)
pub fn oct(x: i64) -> (i64, i64, i64, i64, i64, i64, i64, i64) = (x, x + 1, x + 2, x + 3, x + 4, x + 5, x + 6, x + 7)
";
const MAIN2: &str = "\
import lib
fn main() {
  t := lib.quad(10)
  print(t.0 + t.1 + t.2 + t.3)
  u := lib.oct(20)
  print(u.0 + u.1 + u.2 + u.3 + u.4 + u.5 + u.6 + u.7)
}
";

#[test]
fn gate_wide_tuple_sret_vanishes_when_inlined() {
    if !backend() {
        return;
    }
    let per = build_per_unit_multi("tl-tuple", &[("lib.align", LIB2), ("main.align", MAIN2)], "main.align");
    let ei = entry_idx(&per);

    let base = per.emit_objects(false);
    let thin = thin_objects(&per);

    let Some(base_syms) = nm_symbols(&base[ei]) else {
        return;
    };
    let thin_syms = nm_symbols(&thin[ei]).expect("nm thin tuple entry");

    // Without the flag: the 4-/8-i64 tuple returns cross the unit boundary — the producer stores
    // every field into the sret buffer and the consumer loads them, so the entry references the fns.
    assert!(
        has_undef(&base_syms, "lib$quad") && has_undef(&base_syms, "lib$oct"),
        "flag-off entry must reference the tuple-returning fns (the retained cross-unit sret ABI):\n{base_syms:?}"
    );
    // Under --thin-lto: the fns are imported + inlined, so the boundary — and the sret store/load
    // round trip with it — vanishes (no reference remains).
    assert!(
        !has_undef(&thin_syms, "lib$quad") && !has_undef(&thin_syms, "lib$oct"),
        "under --thin-lto the wide-tuple sret round trip must vanish (fns inlined, no undefined ref):\n{thin_syms:?}"
    );

    // Run-parity: the ThinLTO build produces the same result as the whole-program build.
    let obj_refs: Vec<&std::path::Path> = thin.iter().map(|p| p.as_path()).collect();
    let exe = per.dir.join(format!("tl-tuple{}", std::env::consts::EXE_SUFFIX));
    align_driver::link_objects(&obj_refs, &exe, &per.link_libs_union(), Profile::Release).expect("link");
    let thin_out = std::process::Command::new(&exe).output().expect("run");
    let whole = build_and_run_multi("tl-tuple-wp", &[("lib.align", LIB2), ("main.align", MAIN2)], "main.align");
    assert_eq!(thin_out.stdout, whole.stdout, "thin-lto vs whole-program output");
    assert_eq!(String::from_utf8_lossy(&thin_out.stdout), "46\n188\n");
}

// ---- Gate 3: N=1 byte-identity (CLI, exercises the real driver branch) ---------------------------

const SINGLE: &str = "pub fn helper(x: i64) -> i64 = x + 1\nfn main() {\n  print(helper(41))\n}\n";

#[test]
fn gate_n1_exe_byte_identical() {
    if !backend() {
        return;
    }
    let dir = std::env::temp_dir().join(format!("align-tl-n1-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    struct Cleanup(PathBuf);
    impl Drop for Cleanup {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }
    let _g = Cleanup(dir.clone());
    let src = dir.join("single.align");
    std::fs::write(&src, SINGLE).unwrap();

    let build = |exe_name: &str, thin: bool| -> Vec<u8> {
        let exe = dir.join(exe_name);
        let mut cmd = std::process::Command::new(env!("CARGO_BIN_EXE_alignc"));
        cmd.env("ALIGNC_CACHE", "off").arg("build").arg(&src);
        if thin {
            cmd.arg("--thin-lto");
        }
        // `build` writes `<stem>` in the CWD; run in `dir` and give a distinct name via a copy after.
        let status = cmd.current_dir(&dir).output().expect("run alignc build");
        assert!(status.status.success(), "build failed: {}", String::from_utf8_lossy(&status.stderr));
        let produced = dir.join("single");
        let bytes = std::fs::read(&produced).expect("read built exe");
        std::fs::rename(&produced, &exe).expect("rename exe");
        bytes
    };

    let noflag = build("s_noflag", false);
    let thin = build("s_thin", true);
    assert_eq!(
        noflag, thin,
        "N=1 --thin-lto must produce a byte-identical executable to the no-flag build (N=1 skips ThinLTO)"
    );
}

// ---- Gate 4: run-parity corpus ------------------------------------------------------------------

#[test]
fn gate_run_parity_corpus() {
    if !backend() {
        return;
    }
    // A representative slice of the per-unit corpus: imported struct + in-consumer generic + a Move
    // array return; a hot `x == "literal"` filter across a unit; and the wide-tuple program.
    let lib_b = "\
module lib
pub Vec2 { x: i64, y: i64 }
pub fn mk(x: i64, y: i64) -> Vec2 = Vec2 { x: x, y: y }
pub fn sumv(v: Vec2) -> i64 = v.x + v.y
pub fn id<T>(a: T) -> T = a
pub fn triple() -> array<i64> = [1, 2, 3].to_array()
";
    let main_b = "\
import lib
fn main() {
  v := lib.mk(10, 20)
  print(lib.sumv(v))
  print(lib.id(5))
  a := lib.triple()
  print(a.len())
}
";
    let hot = "\
module hot
fn is_hello(x: str) -> bool = x == \"hello\"
pub fn eq_count(s: array<str>) -> i64 = s.where(is_hello).count()
";
    let main_hot = "\
import hot
fn main() {
  data := [\"hello\", \"world\", \"hello\"].to_array()
  print(hot.eq_count(data))
}
";

    #[allow(clippy::type_complexity)]
    let corpus: &[(&str, &[(&str, &str)], &str)] = &[
        ("cp-structgen", &[("lib.align", lib_b), ("main.align", main_b)], "main.align"),
        ("cp-hotfilter", &[("hot.align", hot), ("main.align", main_hot)], "main.align"),
        ("cp-tuple", &[("lib.align", LIB2), ("main.align", MAIN2)], "main.align"),
    ];

    for (name, files, entry) in corpus {
        let whole = build_and_run_multi(name, files, entry);
        let per = build_per_unit_multi(name, files, entry);
        let thin = thin_objects(&per);
        let obj_refs: Vec<&std::path::Path> = thin.iter().map(|p| p.as_path()).collect();
        let exe = per.dir.join(format!("{name}{}", std::env::consts::EXE_SUFFIX));
        align_driver::link_objects(&obj_refs, &exe, &per.link_libs_union(), Profile::Release).expect("link");
        let thin_out = std::process::Command::new(&exe).output().expect("run");
        assert_eq!(
            whole.stdout, thin_out.stdout,
            "`{name}`: --thin-lto stdout must match the whole-program build\n whole: {:?}\n thin:  {:?}",
            String::from_utf8_lossy(&whole.stdout),
            String::from_utf8_lossy(&thin_out.stdout)
        );
        assert_eq!(whole.status.code(), thin_out.status.code(), "`{name}`: exit code must match");
    }
}

// ---- Gate 5: preserve survival ------------------------------------------------------------------

#[test]
fn gate_preserve_main_and_pub_survive() {
    if !backend() {
        return;
    }
    // A pub fn that main does NOT call, so nothing internal references it: only the preserve set keeps
    // it an external define in its unit's ThinLTO object.
    let lib = "module lib\npub fn used(x: i64) -> i64 = x + 1\npub fn unused(x: i64) -> i64 = x * 7\n";
    let main = "import lib\nfn main() {\n  print(lib.used(41))\n}\n";
    let per = build_per_unit_multi("tl-preserve", &[("lib.align", lib), ("main.align", main)], "main.align");
    let ei = entry_idx(&per);
    let li = per.walk.units.iter().position(|u| u.unit == "lib").expect("lib unit");
    let thin = thin_objects(&per);

    let Some(entry_syms) = nm_symbols(&thin[ei]) else {
        return;
    };
    let lib_syms = nm_symbols(&thin[li]).expect("nm thin lib");

    // main stays the external C entry.
    assert!(has_ext_def(&entry_syms, "main"), "main must be an external define in the entry object:\n{entry_syms:?}");
    // Both pub fns stay external defines in lib.o (preserve set — even `unused`, which nothing calls).
    assert!(
        has_ext_def(&lib_syms, "lib$used"),
        "the preserve set must keep lib$used an external define under --thin-lto:\n{lib_syms:?}"
    );
    assert!(
        has_ext_def(&lib_syms, "lib$unused"),
        "the fail-closed preserve set keeps EVERY pub fn (incl. the uncalled lib$unused) external:\n{lib_syms:?}"
    );

    // The linked program still runs correctly.
    let obj_refs: Vec<&std::path::Path> = thin.iter().map(|p| p.as_path()).collect();
    let exe = per.dir.join(format!("tl-preserve{}", std::env::consts::EXE_SUFFIX));
    align_driver::link_objects(&obj_refs, &exe, &per.link_libs_union(), Profile::Release).expect("link");
    let out = std::process::Command::new(&exe).output().expect("run");
    assert_eq!(String::from_utf8_lossy(&out.stdout), "42\n");
}

// ---- Extra: build-twice determinism (de-risks the eventual SV gate) -----------------------------

#[test]
fn thin_lto_objects_are_build_twice_deterministic() {
    if !backend() {
        return;
    }
    // ThinLTO's promoted `.llvm.<hash>` names + import/export decisions must be deterministic across
    // builds, or the eventual SV byte-identity gate (and any object cache) is impossible. Pin it now.
    let hot = "\
module hot
fn is_hello(x: str) -> bool = x == \"hello\"
pub fn eq_count(s: array<str>) -> i64 = s.where(is_hello).count()
pub fn add(a: i64, b: i64) -> i64 = a + b
";
    let main = "\
import hot
fn main() {
  data := [\"hello\", \"world\"].to_array()
  print(hot.eq_count(data))
  print(hot.add(2, 3))
}
";
    let per = build_per_unit_multi("tl-det", &[("hot.align", hot), ("main.align", main)], "main.align");
    let n = per.walk.units.len();
    let a: Vec<PathBuf> = (0..n).map(|i| per.dir.join(format!("da{i}.o"))).collect();
    let b: Vec<PathBuf> = (0..n).map(|i| per.dir.join(format!("db{i}.o"))).collect();
    let disabled = align_driver::CacheContext::Disabled;
    align_driver::build_thin_lto(&per.walk.units, &a, &disabled, &BuildTarget::Baseline, Profile::Release, &[], false, &per.dir, 1).expect("thin-lto a");
    align_driver::build_thin_lto(&per.walk.units, &b, &disabled, &BuildTarget::Baseline, Profile::Release, &[], false, &per.dir, 1).expect("thin-lto b");
    for i in 0..n {
        assert_eq!(
            std::fs::read(&a[i]).unwrap(),
            std::fs::read(&b[i]).unwrap(),
            "unit `{}` ThinLTO object must be byte-identical across builds",
            per.walk.units[i].unit
        );
    }
}

// ---- Gate 6: profile / verb rejection (subprocess) ----------------------------------------------

const HELLO: &str = "fn main() -> i32 {\n  print(\"hello, align\")\n  return 0\n}\n";

fn write_cli_src(tag: &str) -> PathBuf {
    let path = std::env::temp_dir().join(format!("align-tl-cli-{}-{}.align", std::process::id(), tag));
    std::fs::write(&path, HELLO).expect("write src");
    path
}

#[test]
fn cli_thin_lto_rejects_dev_profile() {
    let src = write_cli_src("dev");
    let out = std::process::Command::new(env!("CARGO_BIN_EXE_alignc"))
        .env("ALIGNC_CACHE", "off")
        .arg("build")
        .arg(&src)
        .args(["--thin-lto", "--profile", "dev"])
        .output()
        .expect("run alignc build");
    let _ = std::fs::remove_file(&src);
    let err = String::from_utf8_lossy(&out.stderr);
    assert_eq!(out.status.code(), Some(1), "--thin-lto + dev profile must fail:\n{err}");
    assert!(
        err.contains("alignc: --thin-lto is incompatible with the `dev` profile"),
        "diagnostic must name the profile incompatibility:\n{err}"
    );
}

#[test]
fn cli_thin_lto_rejects_non_build_verb() {
    let src = write_cli_src("emitobj");
    let out = std::process::Command::new(env!("CARGO_BIN_EXE_alignc"))
        .env("ALIGNC_CACHE", "off")
        .arg("emit-obj")
        .arg(&src)
        .arg("--thin-lto")
        .output()
        .expect("run alignc emit-obj");
    let _ = std::fs::remove_file(&src);
    let err = String::from_utf8_lossy(&out.stderr);
    assert_eq!(out.status.code(), Some(1), "--thin-lto on emit-obj must fail:\n{err}");
    assert!(
        err.contains("alignc: --thin-lto is only valid for `build`/`run`/`size`"),
        "diagnostic must name the valid verb set:\n{err}"
    );
}

// ---- Gate 7: flag-off path is deterministic + untouched -----------------------------------------

#[test]
fn gate_flag_off_object_unchanged_and_deterministic() {
    if !backend() {
        return;
    }
    // The flag-off per-unit object path is not on the ThinLTO code path at all; pin that it is
    // byte-deterministic (two emits of the same unit match) — the exact bytes are additionally pinned
    // by the pre-existing per-unit / cold-build byte gates, which this slice leaves green.
    let per = build_per_unit_multi("tl-flagoff", &[("lib.align", LIB1), ("main.align", MAIN1)], "main.align");
    for (i, u) in per.walk.units.iter().enumerate() {
        let a = per.dir.join(format!("off_a{i}.o"));
        let b = per.dir.join(format!("off_b{i}.o"));
        emit_object_file(&u.mir, &a, BuildTarget::Baseline, Profile::Release, &[], false).expect("emit a");
        emit_object_file(&u.mir, &b, BuildTarget::Baseline, Profile::Release, &[], false).expect("emit b");
        assert_eq!(
            std::fs::read(&a).unwrap(),
            std::fs::read(&b).unwrap(),
            "flag-off object for unit `{}` must be byte-deterministic (path untouched by ThinLTO)",
            u.unit
        );
    }
}
