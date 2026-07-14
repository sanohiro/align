//! Build profiles + `alignc size` (M13 Slice 4). Three layers:
//!   1. `Profile` enum unit tests — the pipeline-string / strip / parse mapping is the whole
//!      mechanism, so pin it exactly (no aliases, exact names, correct `default<O*>` strings).
//!   2. An in-process codegen-differs check — proving the profile's pipeline string actually reaches
//!      LLVM (a `dev`/O0 object differs from a `fast`/O3 object for a vectorizable kernel).
//!   3. Subprocess CLI tests for `alignc size` (report shape, the strip note under `tiny`) and the
//!      bad-`--profile` diagnostic.

use std::path::{Path, PathBuf};
use std::process::Command;

use align_driver::{check, emit_object_file, lower_to_mir, BuildTarget, ObjectFormat, Profile};

// ---------------------------------------------------------------------------
// 1. Profile enum — the mapping is the mechanism.
// ---------------------------------------------------------------------------

#[test]
fn profile_pipeline_strings_are_the_stock_default_set() {
    assert_eq!(Profile::Dev.pipeline(), "default<O0>");
    assert_eq!(Profile::Release.pipeline(), "default<O2>");
    assert_eq!(Profile::Fast.pipeline(), "default<O3>");
    assert_eq!(Profile::Small.pipeline(), "default<Os>");
    assert_eq!(Profile::Tiny.pipeline(), "default<Oz>");
}

#[test]
fn default_profile_is_release_no_behavior_change_without_the_flag() {
    // Today's behavior is `default<O2>`; the default profile must reproduce it exactly.
    assert_eq!(Profile::default(), Profile::Release);
    assert_eq!(Profile::default().pipeline(), "default<O2>");
}

#[test]
fn only_size_profiles_strip() {
    assert!(!Profile::Dev.strip());
    assert!(!Profile::Release.strip());
    assert!(!Profile::Fast.strip());
    assert!(Profile::Small.strip());
    assert!(Profile::Tiny.strip());
}

#[test]
fn parse_accepts_exact_names_only_and_roundtrips() {
    for p in [Profile::Dev, Profile::Release, Profile::Fast, Profile::Small, Profile::Tiny] {
        assert_eq!(Profile::parse(p.name()), Some(p), "{} must roundtrip", p.name());
    }
    // No aliases, no prefixes, no casing, no empty string.
    for bad in ["", "Dev", "DEV", "rel", "release ", "o2", "O2", "debug", "opt", "small-print"] {
        assert_eq!(Profile::parse(bad), None, "`{bad}` must be rejected");
    }
}

// ---------------------------------------------------------------------------
// 2. The pipeline string actually reaches LLVM.
// ---------------------------------------------------------------------------

/// A vectorizable reduction kernel — O0 and O3 lower it very differently, so the two objects must
/// differ byte-for-byte if the profile's pipeline is really threaded to `run_passes`.
const KERNEL: &str = "fn dbl(x: i64) -> i64 = x * 2\n\
     fn run(xs: slice<i64>) -> i64 = xs.map(dbl).sum()\n\
     fn main(args: array<str>) -> Result<(), Error> {\n  \
       a := [1, 2, 3, 4, 5, 6, 7, 8]\n  \
       s : slice<i64> := a[0..args.len()]\n  \
       print(run(s))\n  \
       return Ok(())\n\
     }\n";

#[test]
fn dev_and_fast_produce_different_objects() {
    if !align_driver::backend_available() {
        return;
    }
    let mut sm = align_span::SourceMap::new();
    let checked = check(&mut sm, "prof-kernel", KERNEL);
    assert!(!checked.diags.has_errors(), "kernel compiles");
    let mir = lower_to_mir(&checked.hir);

    // RAII temp paths so a panic mid-test still cleans up the objects (per-test tag keeps the two
    // filenames distinct under parallel test threads).
    let o0 = temp_obj("o0");
    let o3 = temp_obj("o3");
    emit_object_file(&mir, o0.path(), BuildTarget::Baseline, Profile::Dev, &[], false).expect("emit O0");
    emit_object_file(&mir, o3.path(), BuildTarget::Baseline, Profile::Fast, &[], false).expect("emit O3");
    let b0 = std::fs::read(o0.path()).expect("read O0");
    let b3 = std::fs::read(o3.path()).expect("read O3");
    assert_ne!(b0, b3, "O0 and O3 objects must differ — the profile pipeline must reach LLVM");
}

#[test]
fn small_and_release_objects_differ() {
    // Sister to the O0/O3 check: `small` adds `optsize` fn attrs + a `default<Os>` pipeline on top
    // of `release`'s `default<O2>`, so the two objects must differ byte-for-byte. Proves the size
    // dimension of the profile actually reaches the backend, not just the speed dimension.
    if !align_driver::backend_available() {
        return;
    }
    let mut sm = align_span::SourceMap::new();
    let checked = check(&mut sm, "prof-kernel", KERNEL);
    assert!(!checked.diags.has_errors(), "kernel compiles");
    let mir = lower_to_mir(&checked.hir);

    // RAII temp paths so a panic mid-test still cleans up the objects (per-test tag keeps the two
    // filenames distinct under parallel test threads).
    let os = temp_obj("os");
    let o2 = temp_obj("o2");
    emit_object_file(&mir, os.path(), BuildTarget::Baseline, Profile::Small, &[], false).expect("emit small");
    emit_object_file(&mir, o2.path(), BuildTarget::Baseline, Profile::Release, &[], false).expect("emit release");
    let bs = std::fs::read(os.path()).expect("read small");
    let b2 = std::fs::read(o2.path()).expect("read release");
    assert_ne!(bs, b2, "small and release objects must differ — the size dimension must reach LLVM");
}

// ---------------------------------------------------------------------------
// 3. `alignc size` CLI + the bad-profile diagnostic (subprocess, real binary).
// ---------------------------------------------------------------------------

struct TempFile(PathBuf);
impl TempFile {
    fn path(&self) -> &Path {
        &self.0
    }
}
impl Drop for TempFile {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.0);
    }
}

/// An RAII path for an object file emitted by a test (created by `emit_object_file`, not here); the
/// `tag` keeps sibling objects in the same test distinct, and `Drop` removes it even on panic.
fn temp_obj(tag: &str) -> TempFile {
    TempFile(std::env::temp_dir().join(format!("align-prof-{}-{}.o", tag, std::process::id())))
}

fn write_src(tag: &str, body: &str) -> TempFile {
    let path = std::env::temp_dir().join(format!("align-size-test-{}-{}.align", std::process::id(), tag));
    std::fs::write(&path, body).expect("write src");
    TempFile(path)
}

fn alignc() -> Command {
    Command::new(env!("CARGO_BIN_EXE_alignc"))
}

const HELLO: &str = "fn main() -> i32 {\n  print(\"hello, align\")\n  return 0\n}\n";

#[test]
fn size_report_release_has_expected_shape() {
    if !align_driver::backend_available() {
        return;
    }
    let format = align_driver::target_object_format().expect("classifiable build target");
    let src = write_src("release", HELLO);
    let out = alignc().arg("size").arg(src.path()).output().expect("run alignc size");
    assert_eq!(out.status.code(), Some(0), "size exits 0:\n{}", String::from_utf8_lossy(&out.stderr));
    let s = String::from_utf8_lossy(&out.stdout);
    // Header names the profile + its pipeline.
    assert!(s.contains("profile:    release (default<O2>)"), "profile header:\n{s}");
    assert!(s.contains("total size:"), "total size line:\n{s}");
    // The other three report blocks are present and populated.
    assert!(s.contains("largest symbols"), "symbol block:\n{s}");
    assert!(s.contains("relocations:"), "relocation count:\n{s}");
    assert!(s.contains("dynamic dependencies"), "dependency block:\n{s}");
    // A release binary is NOT stripped, so the "no symbols" note must be absent.
    assert!(!s.contains("the symbol table is absent"), "release must list symbols:\n{s}");
    // The format-specific expectations: section naming, libc spelling, and the Mach-O
    // derived-symbol-size annotation.
    match format {
        ObjectFormat::Elf => {
            assert!(s.contains(".text"), "expected a .text section:\n{s}");
            assert!(s.contains(".symtab"), "release keeps the symbol table:\n{s}");
            assert!(s.contains("(DT_NEEDED)"), "ELF dependency heading:\n{s}");
            assert!(s.contains("libc.so.6"), "libc is a DT_NEEDED:\n{s}");
        }
        ObjectFormat::MachO => {
            assert!(s.contains("__TEXT,__text"), "expected the __TEXT,__text section:\n{s}");
            assert!(s.contains("(LC_LOAD_DYLIB)"), "Mach-O dependency heading:\n{s}");
            assert!(s.contains("libSystem"), "libSystem is an LC_LOAD_DYLIB:\n{s}");
            assert!(
                s.contains("derived from symbol address deltas"),
                "Mach-O sizes are annotated as derived:\n{s}"
            );
        }
    }
}

#[test]
fn size_report_tiny_is_stripped() {
    if !align_driver::backend_available() {
        return;
    }
    let format = align_driver::target_object_format().expect("classifiable build target");
    let src = write_src("tiny", HELLO);
    let out = alignc().args(["size"]).arg(src.path()).args(["--profile", "tiny"]).output().expect("run alignc size");
    assert_eq!(out.status.code(), Some(0), "size exits 0:\n{}", String::from_utf8_lossy(&out.stderr));
    let s = String::from_utf8_lossy(&out.stdout);
    assert!(s.contains("profile:    tiny (default<Oz>)"), "tiny profile header:\n{s}");
    // Stripped: the symbol block shows the absent-table note; on ELF the .symtab section is gone
    // too (a Mach-O symbol table lives in load commands, not a section, so there is no section to
    // check there).
    assert!(s.contains("the symbol table is absent"), "tiny shows the stripped note:\n{s}");
    if format == ObjectFormat::Elf {
        assert!(!s.contains(".symtab"), "tiny strips the symbol table:\n{s}");
    }
}

#[test]
fn size_report_honors_profile_eq_form() {
    if !align_driver::backend_available() {
        return;
    }
    let src = write_src("fasteq", HELLO);
    let out = alignc().arg("size").arg(src.path()).arg("--profile=fast").output().expect("run alignc size");
    assert_eq!(out.status.code(), Some(0));
    let s = String::from_utf8_lossy(&out.stdout);
    assert!(s.contains("profile:    fast (default<O3>)"), "--profile=fast header:\n{s}");
}

#[test]
fn bad_profile_is_a_diagnostic_not_a_panic() {
    let src = write_src("badprof", HELLO);
    let out = alignc().arg("build").arg(src.path()).args(["--profile", "turbo"]).output().expect("run alignc");
    assert_eq!(out.status.code(), Some(1), "bad profile fails with exit 1");
    let err = String::from_utf8_lossy(&out.stderr);
    assert!(err.contains("unknown --profile 'turbo'"), "diagnostic names the bad value:\n{err}");
    assert!(err.contains("dev, release, fast, small, tiny"), "diagnostic lists the valid names:\n{err}");
}
