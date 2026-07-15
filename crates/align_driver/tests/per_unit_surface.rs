//! M15 S2b — the per-unit CLI surface after the default flip.
//!
//! S2 proved the per-unit *mechanism* (`per_unit_codegen.rs`). S2b makes it the **only** build path
//! (`build`/`run`/`size`/`emit-obj`/`emit-llvm`/`emit-mir` all lower per unit) and deletes the
//! whole-program build path + the `build-per-unit` dev verb. These gates pin the user-visible
//! contract of that flip:
//!
//!  1. **N=1 byte-identity across every profile** — a single-file program's per-unit object equals
//!     its whole-program object for `dev`/`release`/`fast`/`small`/`tiny` (+ `--rt-lto` where legal),
//!     and the linked executables match. The migration keystone, widened past `gate_a`.
//!  2. **CLI equivalence** — the real `alignc build`/`run`/`size` on a single file produce an
//!     executable byte-identical to a library whole-program reference, identical program output, a
//!     size report; the removed `build-per-unit` verb is now an unknown command.
//!  3. **`emit-obj` multi-file** — one object per unit at `<module>.o` (incl. a dotted path), a
//!     non-entry `pub` fn external in ITS object, `--export` entry-unit-only (applied / wrong-unit /
//!     unknown), and a rejected `[out.o]` positional.
//!  4. **`emit-llvm` multi-file** — a per-unit banner, deterministic output, N=1 unbannered +
//!     byte-identical to the whole-program IR, and the cross-unit `pub` call left opaque under
//!     `--stage optimized` while the intra-unit call inlines.
//!  5. **`explain-opt` multi-file** — per-unit sections attributed to the right files, in the
//!     deterministic bottom-up order; N=1 has no section header.
//!  6. **`size` multi-file** — builds per unit and reports on the single final executable (the report
//!     total equals the built executable's on-disk size).
//!  7. **Multi-unit determinism** — a ≥3-unit DAG built twice yields byte-identical objects and
//!     executable (unit order, capability union, staging all deterministic).
//!  8. **Capability union at the CLI** — a compress unit + a plain unit link an executable that needs
//!     `libz` and only `libz`.
//!  9. **`--rt-lto` multi-file** — a hot `x == "literal"` in a non-entry unit inlines the runtime
//!     primitive under a per-unit `--rt-lto`; the flag is still rejected on `dev`/non-build verbs and
//!     its rejection text no longer mentions `build-per-unit`.

mod common;
use common::*;

use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use std::sync::atomic::{AtomicU64, Ordering};

fn backend() -> bool {
    backend_available()
}

fn alignc() -> &'static str {
    env!("CARGO_BIN_EXE_alignc")
}

/// Strip a Mach-O leading underscore so a symbol comparison is object-format-portable.
fn norm(sym: &str) -> &str {
    sym.strip_prefix('_').unwrap_or(sym)
}

static PROJ_NONCE: AtomicU64 = AtomicU64::new(0);

/// A fresh temp directory written with `(relative-path, source)` files (subdirectories created as
/// needed, so a dotted module's `util/math.align` lands in place), removed on drop. The per-unit CLI
/// verbs are run with this as the working directory so `import`s resolve and objects/executables land
/// here.
struct Proj {
    dir: PathBuf,
}

impl Proj {
    fn new(tag: &str, files: &[(&str, &str)]) -> Proj {
        let nonce = PROJ_NONCE.fetch_add(1, Ordering::Relaxed);
        let dir = std::env::temp_dir().join(format!("align-s2b-{}-{tag}-{nonce}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("mkdir proj");
        for (rel, src) in files {
            let path = dir.join(rel);
            if let Some(parent) = path.parent() {
                std::fs::create_dir_all(parent).expect("mkdir module subdir");
            }
            std::fs::write(&path, src).expect("write module file");
        }
        Proj { dir }
    }

    /// Run `alignc <args>` with this project as the working directory. `ALIGNC_CACHE=off` isolates
    /// these byte-identity/visibility assertions from the (now default-ON) user cache.
    fn run(&self, args: &[&str]) -> Output {
        Command::new(alignc()).args(args).env("ALIGNC_CACHE", "off").current_dir(&self.dir).output().expect("spawn alignc")
    }
}

impl Drop for Proj {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.dir);
    }
}

// ---- 1. N=1 byte-identity across every profile ---------------------------------------------------

#[test]
fn n1_object_and_exe_identical_across_all_profiles() {
    if !backend() {
        return;
    }
    // A tiny single-file program (keeps the emit×profile matrix fast). Its `pub` fn stays internal in
    // the entry unit (nothing imports the entry), so the per-unit object equals the whole-program one.
    let src = "pub fn helper(x: i64) -> i64 = x + 1\nfn main() -> i64 {\n  return helper(41)\n}\n";

    let mut sm_wp = SourceMap::new();
    let checked = check(&mut sm_wp, "n1.align", src);
    assert!(!checked.diags.has_errors());
    let wp_mir = lower_to_mir(&checked.hir);

    let mut sm_pu = SourceMap::new();
    let walk = build_per_unit(&mut sm_pu, "n1.align", src);
    assert!(!walk.diags.has_errors());
    assert_eq!(walk.units.len(), 1, "a single file is exactly one unit");
    let pu_mir = &walk.units[0].mir;

    let proj = Proj::new("n1-matrix", &[]);
    // (profile, whether --rt-lto is legal for it — release/fast only; the flag needs an inlining
    // pipeline, so `dev`/`small`/`tiny` reject it).
    let matrix = [
        (Profile::Dev, false),
        (Profile::Release, true),
        (Profile::Fast, true),
        (Profile::Small, false),
        (Profile::Tiny, false),
    ];
    for (profile, rt_lto_legal) in matrix {
        let rt_ltos: &[bool] = if rt_lto_legal { &[false, true] } else { &[false] };
        for &rt_lto in rt_ltos {
            let tag = format!("{}-{}", profile.name(), rt_lto);
            let wp = proj.dir.join(format!("wp-{tag}.o"));
            let pu = proj.dir.join(format!("pu-{tag}.o"));
            emit_object_file(&wp_mir, &wp, BuildTarget::Baseline, profile, &[], rt_lto).expect("wp emit");
            emit_object_file(pu_mir, &pu, BuildTarget::Baseline, profile, &[], rt_lto).expect("pu emit");
            assert_eq!(
                std::fs::read(&wp).unwrap(),
                std::fs::read(&pu).unwrap(),
                "N=1 per-unit object must equal the whole-program object (profile {}, rt_lto {rt_lto})",
                profile.name()
            );

            // And the linked executables match (deterministic link of identical objects).
            let wp_exe = proj.dir.join(format!("wp-{tag}"));
            let pu_exe = proj.dir.join(format!("pu-{tag}"));
            link_objects(&[wp.as_path()], &wp_exe, &wp_mir.link_libs, profile).expect("wp link");
            link_objects(&[pu.as_path()], &pu_exe, &pu_mir.link_libs, profile).expect("pu link");
            assert_eq!(
                std::fs::read(&wp_exe).unwrap(),
                std::fs::read(&pu_exe).unwrap(),
                "N=1 executables must match (profile {}, rt_lto {rt_lto})",
                profile.name()
            );
        }
    }
}

// ---- 2. CLI equivalence to a library whole-program reference -------------------------------------

#[test]
fn cli_build_run_size_match_library_reference() {
    if !backend() {
        return;
    }
    let src = "fn main() {\n  print(1234)\n}\n";

    // Library whole-program reference executable (lower_to_mir -> emit_object -> link_executable at
    // the CLI's default release/baseline).
    let reference = build_exe("s2b-ref", src);
    let ref_bytes = std::fs::read(&reference.exe).expect("read reference exe");

    let proj = Proj::new("cli-equiv", &[("app.align", src)]);

    // `build`: the produced `<stem>` executable is byte-identical to the library reference.
    let built = proj.run(&["build", "app.align"]);
    assert!(built.status.success(), "build failed: {}", String::from_utf8_lossy(&built.stderr));
    let cli_bytes = std::fs::read(proj.dir.join("app")).expect("read cli exe");
    assert_eq!(ref_bytes, cli_bytes, "CLI per-unit build must byte-match the whole-program reference");

    // `run`: program output is identical.
    let ran = proj.run(&["run", "app.align"]);
    assert!(ran.status.success(), "run failed: {}", String::from_utf8_lossy(&ran.stderr));
    assert_eq!(ran.stdout, b"1234\n", "run output must match the program");

    // `size`: a report is produced for the built executable.
    let sized = proj.run(&["size", "app.align"]);
    assert!(sized.status.success(), "size failed: {}", String::from_utf8_lossy(&sized.stderr));
    assert!(
        String::from_utf8_lossy(&sized.stdout).contains("total size:"),
        "size must print a report: {}",
        String::from_utf8_lossy(&sized.stdout)
    );

    // The deleted `build-per-unit` verb is now an unknown command (usage error, failure exit).
    let removed = proj.run(&["build-per-unit", "app.align"]);
    assert!(!removed.status.success(), "the removed verb must fail");
    assert!(
        String::from_utf8_lossy(&removed.stderr).contains("usage: alignc"),
        "the removed verb must print the usage error: {}",
        String::from_utf8_lossy(&removed.stderr)
    );
}

// ---- 3. emit-obj multi-file (filenames, visibility, --export) ------------------------------------

const MATH_UNIT: &str = "module util.math\nfn sq(x: i64) -> i64 = x * x\npub fn cube(x: i64) -> i64 = sq(x) * x\n";
const APP_UNIT: &str = "module main\nimport util.math\nfn helper(x: i64) -> i64 = x + 1\nfn main() -> i64 {\n  return util.math.cube(3) + helper(1)\n}\n";

fn math_app_proj(tag: &str) -> Proj {
    Proj::new(tag, &[("util/math.align", MATH_UNIT), ("main.align", APP_UNIT)])
}

#[test]
fn emit_obj_multi_file_writes_one_object_per_unit() {
    if !backend() {
        return;
    }
    let proj = math_app_proj("emit-obj-names");
    let out = proj.run(&["emit-obj", "main.align"]);
    assert!(out.status.success(), "emit-obj failed: {}", String::from_utf8_lossy(&out.stderr));
    // Exact expected filenames: `<module-path>.o`, including the dotted dependency.
    assert!(proj.dir.join("util.math.o").exists(), "expected util.math.o");
    assert!(proj.dir.join("main.o").exists(), "expected main.o");

    // The non-entry `pub` fn is a defined external (`T`) in ITS object (mangled `util.math$cube`).
    if let Some(dep_syms) = nm_symbols(&proj.dir.join("util.math.o")) {
        assert!(
            dep_syms.iter().any(|(k, n)| *k == 'T' && norm(n) == "util.math$cube"),
            "util.math$cube must be external in util.math.o: {dep_syms:?}"
        );
        // The entry object exports `main`.
        let main_syms = nm_symbols(&proj.dir.join("main.o")).expect("nm main.o");
        assert!(
            main_syms.iter().any(|(k, n)| *k == 'T' && norm(n) == "main"),
            "main must be external in main.o: {main_syms:?}"
        );
    }
}

#[test]
fn emit_obj_export_is_entry_unit_only() {
    if !backend() {
        return;
    }
    let proj = math_app_proj("emit-obj-export");

    // `--export <entry fn>` keeps a private entry function external in the entry object.
    let ok = proj.run(&["emit-obj", "main.align", "--export", "helper"]);
    assert!(ok.status.success(), "entry-fn export must succeed: {}", String::from_utf8_lossy(&ok.stderr));
    if let Some(main_syms) = nm_symbols(&proj.dir.join("main.o")) {
        assert!(
            main_syms.iter().any(|(k, n)| *k == 'T' && norm(n) == "helper"),
            "--export helper must make it external in main.o: {main_syms:?}"
        );
    }

    // `--export <non-entry fn>` is a hard error naming the defining unit (never a silent no-op).
    let wrong = proj.run(&["emit-obj", "main.align", "--export", "cube"]);
    assert!(!wrong.status.success(), "exporting a non-entry fn must fail");
    let werr = String::from_utf8_lossy(&wrong.stderr);
    assert!(werr.contains("util.math"), "must name the defining unit: {werr}");
    assert!(werr.contains("Mark it `pub`"), "must guide toward `pub`: {werr}");

    // `--export bogus` is the listed unknown-export error.
    let bogus = proj.run(&["emit-obj", "main.align", "--export", "bogus"]);
    assert!(!bogus.status.success(), "an unknown export must fail");
    assert!(
        String::from_utf8_lossy(&bogus.stderr).contains("unknown export(s): bogus"),
        "must be the unknown-export error: {}",
        String::from_utf8_lossy(&bogus.stderr)
    );

    // A single `[out.o]` positional is ambiguous for N>1 — a hard error.
    let out_pos = proj.run(&["emit-obj", "main.align", "out.o"]);
    assert!(!out_pos.status.success(), "an output path with N>1 must fail");
    assert!(
        String::from_utf8_lossy(&out_pos.stderr).contains("one object per unit"),
        "must explain the one-object-per-unit rule: {}",
        String::from_utf8_lossy(&out_pos.stderr)
    );
}

// ---- 4. emit-llvm multi-file (banner, determinism, N=1, opaque boundary) -------------------------

#[test]
fn emit_llvm_multi_file_banner_and_determinism() {
    if !backend() {
        return;
    }
    let proj = math_app_proj("emit-llvm-banner");
    let a = proj.run(&["emit-llvm", "main.align"]);
    assert!(a.status.success(), "emit-llvm failed: {}", String::from_utf8_lossy(&a.stderr));
    let text = String::from_utf8_lossy(&a.stdout);
    // One banner per unit, in bottom-up order (dependency first).
    let dep_at = text.find("; ==== unit: util.math ====").expect("dep banner");
    let main_at = text.find("; ==== unit: main ====").expect("entry banner");
    assert!(dep_at < main_at, "the dependency unit must be emitted before the entry unit");

    // Deterministic: a second run is byte-equal.
    let b = proj.run(&["emit-llvm", "main.align"]);
    assert_eq!(a.stdout, b.stdout, "emit-llvm must be deterministic across runs");
}

#[test]
fn emit_llvm_n1_has_no_banner_and_matches_whole_program() {
    if !backend() {
        return;
    }
    let src = "fn main() -> i64 {\n  return [1, 2, 3].sum()\n}\n";
    let proj = Proj::new("emit-llvm-n1", &[("solo.align", src)]);
    let cli = proj.run(&["emit-llvm", "solo.align"]);
    assert!(cli.status.success(), "emit-llvm failed: {}", String::from_utf8_lossy(&cli.stderr));
    let cli_ir = String::from_utf8_lossy(&cli.stdout);
    assert!(!cli_ir.contains("==== unit:"), "a single-unit program must have no banner:\n{cli_ir}");

    // Byte-identical to the pre-flip whole-program IR (`emit_llvm` helper = whole-program lowering).
    let reference = emit_llvm(src);
    assert_eq!(cli_ir, reference, "N=1 emit-llvm must equal the whole-program IR");
}

#[test]
fn emit_llvm_optimized_leaves_cross_unit_call_opaque() {
    if !backend() {
        return;
    }
    // The dependency's private `sq` is intra-unit (must inline into `cube`); the entry's call to
    // `cube` is cross-unit (must stay an opaque call — units are optimized in isolation).
    let per = build_per_unit_multi("opt-boundary", &[("util/math.align", MATH_UNIT), ("main.align", APP_UNIT)], "main.align");
    let dep = per.unit("util.math");
    let entry = per.unit("main");

    let dep_ir = emit_llvm_ir(&dep.mir, BuildTarget::Baseline, true, &[], false).expect("dep opt ir");
    assert!(
        !dep_ir.contains("call") || !dep_ir.contains("$sq"),
        "the intra-unit private `sq` must inline into `cube` (no surviving call):\n{dep_ir}"
    );

    let entry_ir = emit_llvm_ir(&entry.mir, BuildTarget::Baseline, true, &[], false).expect("entry opt ir");
    assert!(
        entry_ir.contains("util.math$cube"),
        "the cross-unit `pub` call must stay an opaque call to the extern:\n{entry_ir}"
    );
}

// ---- 5. explain-opt multi-file -------------------------------------------------------------------

const SCALE_UNIT: &str = "module util.math\npub fn scale(xs: array<i64>) -> i64 = xs.map(dbl).sum()\nfn dbl(x: i64) -> i64 = x * 2\n";
const SCALE_MAIN: &str = "module main\nimport util.math\nfn main() -> i64 {\n  return util.math.scale([1, 2, 3, 4, 5, 6, 7, 8].to_array())\n}\n";

#[test]
fn explain_opt_multi_file_has_per_unit_sections() {
    if !backend() {
        return;
    }
    let proj = Proj::new("explain-multi", &[("util/math.align", SCALE_UNIT), ("main.align", SCALE_MAIN)]);
    let a = proj.run(&["explain-opt", "main.align"]);
    assert!(a.status.success(), "explain-opt failed: {}", String::from_utf8_lossy(&a.stderr));
    let text = String::from_utf8_lossy(&a.stdout);
    // A section per unit, attributed to the right file, in bottom-up order.
    let dep_at = text.find("==== unit: util.math (math.align) ====").expect("dep section");
    let main_at = text.find("==== unit: main (main.align) ====").expect("entry section");
    assert!(dep_at < main_at, "the dependency section must come first");

    // Deterministic order across runs.
    let b = proj.run(&["explain-opt", "main.align"]);
    assert_eq!(a.stdout, b.stdout, "explain-opt must be deterministic");
}

#[test]
fn explain_opt_single_file_has_no_section_header() {
    if !backend() {
        return;
    }
    let src = "fn main() -> i64 {\n  return [1, 2, 3].sum()\n}\n";
    let proj = Proj::new("explain-solo", &[("solo.align", src)]);
    let out = proj.run(&["explain-opt", "solo.align"]);
    assert!(out.status.success(), "explain-opt failed: {}", String::from_utf8_lossy(&out.stderr));
    assert!(
        !String::from_utf8_lossy(&out.stdout).contains("==== unit:"),
        "a single-unit explain-opt must have no section header"
    );
}

// ---- 6. size multi-file --------------------------------------------------------------------------

#[test]
fn size_multi_file_reports_the_final_executable() {
    if !backend() {
        return;
    }
    let proj = Proj::new("size-multi", &[("util/math.align", SCALE_UNIT), ("main.align", SCALE_MAIN)]);
    // Build the executable so we can compare its on-disk size to the report's total.
    let built = proj.run(&["build", "main.align"]);
    assert!(built.status.success(), "build failed: {}", String::from_utf8_lossy(&built.stderr));
    let exe_size = std::fs::metadata(proj.dir.join("main")).expect("stat exe").len();

    let sized = proj.run(&["size", "main.align"]);
    assert!(sized.status.success(), "size failed: {}", String::from_utf8_lossy(&sized.stderr));
    let report = String::from_utf8_lossy(&sized.stdout);
    // The report's `total size` (grouped with commas) equals the built executable's stat size —
    // proving the per-unit `size` reports on the single final executable, not per unit.
    let line = report.lines().find(|l| l.contains("total size:")).expect("total size line");
    let digits: String = line.chars().filter(|c| c.is_ascii_digit()).collect();
    assert_eq!(digits.parse::<u64>().unwrap(), exe_size, "size total must equal the executable's on-disk size");
}

// ---- 7. Multi-unit determinism (>=3 units) -------------------------------------------------------

const DAG_C: &str = "module c\npub fn base() -> i64 = 1\n";
const DAG_A: &str = "module a\nimport c\npub fn av() -> i64 = c.base() + 1\n";
const DAG_B: &str = "module b\nimport c\npub fn bv() -> i64 = c.base() + 2\n";
const DAG_MAIN: &str = "module main\nimport a\nimport b\nfn main() -> i64 {\n  return a.av() + b.bv()\n}\n";

#[test]
fn multi_unit_dag_builds_byte_identically_twice() {
    if !backend() {
        return;
    }
    let files = &[("a.align", DAG_A), ("b.align", DAG_B), ("c.align", DAG_C), ("main.align", DAG_MAIN)];
    let x = build_per_unit_multi("dag-x", files, "main.align");
    let y = build_per_unit_multi("dag-y", files, "main.align");
    assert!(x.walk.units.len() >= 3, "at least three units");
    assert_eq!(x.walk.units.len(), y.walk.units.len());

    let ox = x.emit_objects(false);
    let oy = y.emit_objects(false);
    for i in 0..ox.len() {
        // Unit order is pinned (the same bottom-up walk both times).
        assert_eq!(x.walk.units[i].unit, y.walk.units[i].unit, "unit order must be deterministic");
        assert_eq!(
            std::fs::read(&ox[i]).unwrap(),
            std::fs::read(&oy[i]).unwrap(),
            "object for unit `{}` must be byte-identical across builds",
            x.walk.units[i].unit
        );
    }

    // The linked executable is byte-identical too (capability union order + staging deterministic).
    let link = |b: &PerUnitBuilt, objs: &[PathBuf]| -> Vec<u8> {
        let refs: Vec<&Path> = objs.iter().map(|p| p.as_path()).collect();
        let exe = b.dir.join(format!("dag-exe{}", std::env::consts::EXE_SUFFIX));
        link_objects(&refs, &exe, &b.link_libs_union(), Profile::Release).expect("link");
        std::fs::read(&exe).expect("read exe")
    };
    assert_eq!(link(&x, &ox), link(&y, &oy), "the linked executable must be byte-identical across builds");
}

// ---- 8. Capability union at the CLI --------------------------------------------------------------

const ZIP_UNIT: &str = "module zip\nimport std.compress\npub fn zlen(data: str) -> Result<i64, Error> {\n  comp := compress.gzip_compress(data, 6)?\n  return Ok(comp.bytes().len())\n}\n";
const PLAIN_UNIT: &str = "module plain\npub fn bump(x: i64) -> i64 = x + 1\n";
const CAP_MAIN: &str = "module main\nimport zip\nimport plain\nfn main() -> Result<(), Error> {\n  n := zip.zlen(\"hello world\")?\n  print(n + plain.bump(0))\n  return Ok(())\n}\n";

#[test]
fn cli_build_capability_union_needs_libz_only() {
    if !backend() {
        return;
    }
    let proj = Proj::new("cap-union", &[("zip.align", ZIP_UNIT), ("plain.align", PLAIN_UNIT), ("main.align", CAP_MAIN)]);
    let built = proj.run(&["build", "main.align"]);
    assert!(built.status.success(), "build failed: {}", String::from_utf8_lossy(&built.stderr));

    let Some(readobj) = llvm_readobj() else {
        return; // llvm-readobj unavailable — skip the binary-inspection assertion
    };
    let needed = needed_libs(&readobj, &proj.dir.join("main"));
    assert!(needed.iter().any(|l| is_lib(l, "z")), "the compress unit must pull in libz: {needed:?}");
    for absent in ["zstd", "crypto", "ssl"] {
        assert!(!needed.iter().any(|l| is_lib(l, absent)), "no unit uses lib{absent}: {needed:?}");
    }
}

// ---- 9. --rt-lto multi-file ----------------------------------------------------------------------

const HOT_UNIT: &str = "module hot\nfn is_hello(x: str) -> bool = x == \"hello\"\npub fn eq_count(s: slice<str>) -> i64 = s.where(is_hello).count()\n";
const HOT_MAIN: &str = "module main\nimport hot\nfn main() {}\n";

#[test]
fn cli_rt_lto_multi_file_inlines_in_non_entry_unit() {
    if !backend() {
        return;
    }
    let proj = Proj::new("rt-lto-multi", &[("hot.align", HOT_UNIT), ("main.align", HOT_MAIN)]);

    // The default `build --rt-lto` (release) links across the multi-unit program.
    let built = proj.run(&["build", "--rt-lto", "main.align"]);
    assert!(built.status.success(), "build --rt-lto failed: {}", String::from_utf8_lossy(&built.stderr));

    // Per-unit `--rt-lto` on `emit-llvm --stage optimized`: the runtime primitive inlines inside the
    // NON-entry `hot` unit's section (hot loops live in arbitrary units, so the merge is per unit).
    let on = proj.run(&["emit-llvm", "--rt-lto", "main.align", "--stage", "optimized"]);
    assert!(on.status.success(), "emit-llvm --rt-lto failed: {}", String::from_utf8_lossy(&on.stderr));
    let text = String::from_utf8_lossy(&on.stdout);
    let hot_start = text.find("; ==== unit: hot ====").expect("hot section");
    let hot_section = &text[hot_start..text[hot_start + 1..].find("; ==== unit:").map(|i| hot_start + 1 + i).unwrap_or(text.len())];
    assert!(
        !hot_section.contains("call i32 @align_rt_str_eq"),
        "under per-unit --rt-lto align_rt_str_eq must inline in the non-entry unit:\n{hot_section}"
    );

    // The flag is still rejected on `dev` (needs an inlining pipeline).
    let dev = proj.run(&["build", "--rt-lto", "--profile", "dev", "main.align"]);
    assert!(!dev.status.success(), "--rt-lto on dev must be rejected");
    assert!(
        String::from_utf8_lossy(&dev.stderr).contains("incompatible with the `dev` profile"),
        "dev rejection text: {}",
        String::from_utf8_lossy(&dev.stderr)
    );

    // ...and on a non-build verb, with a rejection text that no longer names the removed verb.
    let non_build = proj.run(&["emit-mir", "--rt-lto", "main.align"]);
    assert!(!non_build.status.success(), "--rt-lto on emit-mir must be rejected");
    let nb_err = String::from_utf8_lossy(&non_build.stderr);
    assert!(nb_err.contains("--rt-lto is only valid"), "non-build rejection text: {nb_err}");
    assert!(!nb_err.contains("build-per-unit"), "the rejection text must not mention the removed verb: {nb_err}");
}
