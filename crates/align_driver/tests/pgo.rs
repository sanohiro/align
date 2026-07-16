//! Instrument-PGO S1 driver gates (`--pgo-instrument` / `--pgo-use`).
//!
//! CLI-level gates (subprocess `alignc`) for the user-facing behavior: a gen build
//! links / runs / writes a `.profraw` and prints its destination; the full
//! gen→run→merge→use round trip succeeds; the instrumented binary carries the profile
//! ELF sections (absent otherwise); off/instrument/use are run-parity; the rejection
//! matrix and fail-loud profdata errors; flag-off determinism.
//!
//! The IR-level `!prof branch_weights` + `__profc_`/`llvm.used` mutation-checks live in
//! the codegen crate's `pgo` unit tests (retargeted spike tests), where LLVM IR text is
//! accessible; here the equivalent is checked at the linked-binary level (the profile
//! sections `__llvm_prf_cnts` / `__llvm_prf_data`).
//!
//! Tool-gated: the instrument link needs clang's profile runtime archive; the round trip
//! needs `llvm-profdata`. All tools are resolved through the product's own version-matched
//! resolver (`align_driver::llvm_tool` / `common::llvm_readobj`, honoring `LLVM_SYS_221_PREFIX`
//! → `<tool>-22` → `<tool>`), never hardcoded names. Tests skip (pass) when a required tool is
//! absent, matching the repo's existing external-tool gates.

mod common;

use std::path::{Path, PathBuf};
use std::process::Command;

use align_driver::{backend_available, llvm_tool};
// `BRANCHY` (the branch-heavy corpus) and `profile_rt_available` are hoisted into `common` — shared
// verbatim with the S2 `pgo_cache` gates (one source of truth for the PGO test corpus).
use common::{profile_rt_available, BRANCHY};

fn alignc() -> Command {
    // ALIGNC_CACHE=off is load-bearing test ISOLATION: a PGO build now uses the default-ON object cache
    // (S2), so these subprocess gates must not read/write the developer's real cache — pin it off for a
    // clean, deterministic run. (The cache-composition behavior itself is gated in `pgo_cache.rs`.)
    let mut c = Command::new(env!("CARGO_BIN_EXE_alignc"));
    c.env("ALIGNC_CACHE", "off");
    c
}

/// The version-matched `llvm-profdata`, if discoverable (the driver's own resolver) — the round-trip
/// tests need it to `merge` a `.profraw`.
fn llvm_profdata() -> Option<PathBuf> {
    llvm_tool("llvm-profdata")
}

/// An RAII temp directory holding a test's source + emitted executable. Each test gets its
/// own so the `stem`-named executable (written relative to the build cwd) never collides.
struct Scratch(PathBuf);

impl Scratch {
    fn new(tag: &str) -> Scratch {
        let d = std::env::temp_dir().join(format!("align_pgo_gate_{}_{}", tag, std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).expect("create scratch dir");
        std::fs::write(d.join("prog.align"), BRANCHY).expect("write source");
        Scratch(d)
    }
    fn path(&self) -> &Path {
        &self.0
    }
    fn exe(&self) -> PathBuf {
        self.0.join("prog")
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

/// Run `alignc <flags> --profile release build prog.align` with the scratch dir as cwd, so the
/// `prog` executable lands inside it. Returns the process output.
fn build(scratch: &Scratch, flags: &[&str]) -> std::process::Output {
    let mut c = alignc();
    c.current_dir(scratch.path());
    for f in flags {
        c.arg(f);
    }
    c.args(["--profile", "release", "build", "prog.align"]);
    c.output().expect("run alignc build")
}

/// Run a built program with `LLVM_PROFILE_FILE` pointed at `profraw` (harmless for a
/// non-instrumented binary) and return its stdout as a string.
fn run_prog(exe: &Path, profraw: &Path) -> (Option<i32>, String) {
    let out = Command::new(exe)
        .env("LLVM_PROFILE_FILE", profraw)
        .output()
        .expect("run built program");
    (out.status.code(), String::from_utf8_lossy(&out.stdout).into_owned())
}

// ---------------------------------------------------------------------------
// Gate: a gen build links, runs, writes a non-empty .profraw; driver prints the dest.
// ---------------------------------------------------------------------------
#[test]
fn instrument_build_writes_profraw_and_prints_destination() {
    if !backend_available() || !profile_rt_available() {
        return;
    }
    let s = Scratch::new("gen");
    let out = build(&s, &["--pgo-instrument"]);
    assert_eq!(out.status.code(), Some(0), "instrument build failed:\n{}", String::from_utf8_lossy(&out.stderr));
    // Nothing-hidden: the profraw destination is surfaced on STDERR (stdout must stay clean for `run`
    // and `size`).
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(
        stderr.contains("--pgo-instrument:") && stderr.contains("profile to"),
        "instrument build did not print the profraw destination to stderr:\n{stderr}"
    );
    assert!(s.exe().exists(), "instrument build produced no executable");

    let profraw = s.path().join("gen.profraw");
    let (code, _) = run_prog(&s.exe(), &profraw);
    assert!(code.is_some(), "instrumented program crashed (signal), did not exit cleanly");
    let meta = std::fs::metadata(&profraw);
    assert!(
        meta.as_ref().map(|m| m.len() > 0).unwrap_or(false),
        "no non-empty .profraw written to {} — counters or runtime were stripped",
        profraw.display()
    );
}

// ---------------------------------------------------------------------------
// Gate: the instrumented binary carries the profile ELF sections; a plain build does not.
// (The linked-binary equivalent of the IR-shape __profc_/__profd_ counter check.)
// ---------------------------------------------------------------------------
#[test]
fn instrument_binary_has_profile_sections_plain_does_not() {
    if !backend_available() || !profile_rt_available() {
        return;
    }
    // `llvm-readobj` (version-matched, both ELF and Mach-O) — the product's binary-inspection tool.
    let Some(readobj) = common::llvm_readobj() else {
        return;
    };
    let section_dump = |exe: &Path| -> String {
        let out = Command::new(&readobj).arg("--sections").arg(exe).output().expect("llvm-readobj");
        String::from_utf8_lossy(&out.stdout).into_owned()
    };

    let si = Scratch::new("secinstr");
    assert_eq!(build(&si, &["--pgo-instrument"]).status.code(), Some(0));
    let instr = section_dump(&si.exe());
    assert!(instr.contains("__llvm_prf_cnts"), "instrumented binary missing __llvm_prf_cnts section");
    assert!(instr.contains("__llvm_prf_data"), "instrumented binary missing __llvm_prf_data section");

    let sp = Scratch::new("secplain");
    assert_eq!(build(&sp, &[]).status.code(), Some(0));
    let plain = section_dump(&sp.exe());
    assert!(
        !plain.contains("__llvm_prf_cnts"),
        "plain (no --pgo-instrument) binary unexpectedly has profile sections"
    );
}

// ---------------------------------------------------------------------------
// Gate: full round trip — gen -> run -> llvm-profdata merge -> use build succeeds & runs.
// ---------------------------------------------------------------------------
#[test]
fn full_round_trip_gen_merge_use() {
    let Some(profdata_tool) = llvm_profdata() else { return };
    if !backend_available() || !profile_rt_available() {
        return;
    }
    let s = Scratch::new("rt");
    assert_eq!(build(&s, &["--pgo-instrument"]).status.code(), Some(0), "gen build failed");
    let profraw = s.path().join("rt.profraw");
    let (code, gen_stdout) = run_prog(&s.exe(), &profraw);
    assert!(code.is_some(), "instrumented run crashed");
    assert!(std::fs::metadata(&profraw).map(|m| m.len() > 0).unwrap_or(false), "no profraw");

    let profdata = s.path().join("rt.profdata");
    let merged = Command::new(&profdata_tool)
        .args(["merge", "-o"])
        .arg(&profdata)
        .arg(&profraw)
        .status()
        .expect("llvm-profdata merge")
        .success();
    assert!(merged, "llvm-profdata merge failed");

    // The use build must succeed and produce a working binary with identical output.
    let use_out = build(&s, &["--pgo-use", profdata.to_str().unwrap()]);
    assert_eq!(
        use_out.status.code(),
        Some(0),
        "use build failed:\n{}",
        String::from_utf8_lossy(&use_out.stderr)
    );
    let (use_code, use_stdout) = run_prog(&s.exe(), &s.path().join("unused.profraw"));
    assert_eq!(use_code, Some(0), "use-built program did not exit 0");
    assert_eq!(use_stdout, gen_stdout, "use build changed program output");
}

// ---------------------------------------------------------------------------
// Gate: run-parity — off / instrument / use produce identical stdout on the branchy corpus.
// ---------------------------------------------------------------------------
#[test]
fn run_parity_off_instrument_use() {
    let Some(profdata_tool) = llvm_profdata() else { return };
    if !backend_available() || !profile_rt_available() {
        return;
    }
    // off
    let off = Scratch::new("paroff");
    assert_eq!(build(&off, &[]).status.code(), Some(0));
    let (_, off_out) = run_prog(&off.exe(), &off.path().join("x.profraw"));

    // instrument (also collects a profile for the use build)
    let instr = Scratch::new("parinstr");
    assert_eq!(build(&instr, &["--pgo-instrument"]).status.code(), Some(0));
    let profraw = instr.path().join("p.profraw");
    let (_, instr_out) = run_prog(&instr.exe(), &profraw);

    let profdata = instr.path().join("p.profdata");
    assert!(
        Command::new(&profdata_tool).args(["merge", "-o"]).arg(&profdata).arg(&profraw).status().expect("merge").success(),
        "merge failed"
    );

    // use
    let usue = Scratch::new("paruse");
    assert_eq!(build(&usue, &["--pgo-use", profdata.to_str().unwrap()]).status.code(), Some(0));
    let (_, use_out) = run_prog(&usue.exe(), &usue.path().join("x.profraw"));

    assert_eq!(off_out, instr_out, "instrument build changed program stdout vs off");
    assert_eq!(off_out, use_out, "use build changed program stdout vs off");
}

// ---------------------------------------------------------------------------
// Gate: rejection matrix.
// ---------------------------------------------------------------------------
#[test]
fn rejection_matrix() {
    let s = Scratch::new("reject");
    let src = s.path().join("prog.align");
    let src = src.to_str().unwrap();

    // Both flags together (mutually exclusive).
    let o = alignc().args(["--pgo-instrument", "--pgo-use", "x.profdata", "build", src]).output().unwrap();
    assert_ne!(o.status.code(), Some(0), "both PGO flags must be rejected");
    assert!(String::from_utf8_lossy(&o.stderr).contains("mutually exclusive"));

    // Flag-swallowing (both demonstrated orders): the space-form `--pgo-use` must NOT consume a
    // following flag/verb as its value. `--pgo-use --pgo-instrument` must not bypass mutual exclusion
    // with a misleading "file '--pgo-instrument' does not exist"; `--pgo-use --thin-lto build` must
    // not consume the verb. Both are "requires a value" (the likely-flag guard).
    let o = alignc().args(["--pgo-use", "--pgo-instrument", "--profile", "release", "build", src]).output().unwrap();
    assert_ne!(o.status.code(), Some(0), "--pgo-use --pgo-instrument must be rejected");
    let e = String::from_utf8_lossy(&o.stderr);
    assert!(e.contains("--pgo-use requires a value"), "must not swallow the following flag:\n{e}");
    assert!(!e.contains("does not exist"), "must not treat the following flag as a profile path:\n{e}");

    let o = alignc().args(["--pgo-use", "--thin-lto", "build", src]).output().unwrap();
    assert_ne!(o.status.code(), Some(0), "--pgo-use --thin-lto build must be rejected");
    assert!(String::from_utf8_lossy(&o.stderr).contains("--pgo-use requires a value"), "must not consume the verb");

    // --pgo-instrument on a wrong verb.
    let o = alignc().args(["--pgo-instrument", "check", src]).output().unwrap();
    assert_ne!(o.status.code(), Some(0));
    assert!(String::from_utf8_lossy(&o.stderr).contains("only valid for `build`/`run`/`size`"));

    // --pgo-instrument on a size profile (dev/small/tiny).
    for prof in ["dev", "small", "tiny"] {
        let o = alignc().args(["--pgo-instrument", "--profile", prof, "build", src]).output().unwrap();
        assert_ne!(o.status.code(), Some(0), "--pgo on profile {prof} must be rejected");
        assert!(String::from_utf8_lossy(&o.stderr).contains("incompatible with the"));
    }

    // Combined with --thin-lto.
    let o = alignc().args(["--pgo-instrument", "--thin-lto", "--profile", "release", "build", src]).output().unwrap();
    assert_ne!(o.status.code(), Some(0));
    assert!(String::from_utf8_lossy(&o.stderr).contains("cannot be combined with --thin-lto"));

    // Bare --pgo-use (no value) as the trailing token.
    let o = alignc().args(["build", src, "--pgo-use"]).output().unwrap();
    assert_ne!(o.status.code(), Some(0));
    assert!(String::from_utf8_lossy(&o.stderr).contains("--pgo-use requires a value"));

    // Empty --pgo-use= value.
    let o = alignc().args(["build", src, "--pgo-use="]).output().unwrap();
    assert_ne!(o.status.code(), Some(0));
    assert!(String::from_utf8_lossy(&o.stderr).contains("--pgo-use requires a value"));
}

// ---------------------------------------------------------------------------
// Gate: fail-loud profdata — missing / garbage (wrong magic) / empty all hard-error, name the path.
// ---------------------------------------------------------------------------
#[test]
fn fail_loud_profdata_errors() {
    if !backend_available() {
        return;
    }
    let s = Scratch::new("failloud");
    let src = s.path().join("prog.align");
    let src = src.to_str().unwrap();

    // Missing.
    let missing = s.path().join("nope.profdata");
    let o = alignc().args(["--pgo-use", missing.to_str().unwrap(), "--profile", "release", "build", src]).output().unwrap();
    assert_ne!(o.status.code(), Some(0), "missing profdata must be rejected");
    let e = String::from_utf8_lossy(&o.stderr);
    assert!(e.contains("does not exist") && e.contains("nope.profdata"), "missing error must name the path:\n{e}");

    // Garbage (wrong magic).
    let garbage = s.path().join("garbage.profdata");
    std::fs::write(&garbage, b"this is not a profile at all, just bytes").unwrap();
    let o = alignc().args(["--pgo-use", garbage.to_str().unwrap(), "--profile", "release", "build", src]).output().unwrap();
    assert_ne!(o.status.code(), Some(0), "garbage profdata must be rejected");
    let e = String::from_utf8_lossy(&o.stderr);
    assert!(e.contains("not a valid LLVM indexed profile") && e.contains("garbage.profdata"), "bad-magic error must name the path:\n{e}");

    // Empty.
    let empty = s.path().join("empty.profdata");
    std::fs::write(&empty, b"").unwrap();
    let o = alignc().args(["--pgo-use", empty.to_str().unwrap(), "--profile", "release", "build", src]).output().unwrap();
    assert_ne!(o.status.code(), Some(0), "empty profdata must be rejected");
    let e = String::from_utf8_lossy(&o.stderr);
    assert!(e.contains("is empty") && e.contains("empty.profdata"), "empty error must name the path:\n{e}");
}

// ---------------------------------------------------------------------------
// Gate: flag-off determinism — two plain builds are byte-identical (the flag-off path is
// unchanged; PGO adds no nondeterminism to ordinary builds).
// ---------------------------------------------------------------------------
#[test]
fn flag_off_builds_are_byte_identical() {
    if !backend_available() {
        return;
    }
    let a = Scratch::new("detA");
    let b = Scratch::new("detB");
    assert_eq!(build(&a, &[]).status.code(), Some(0));
    assert_eq!(build(&b, &[]).status.code(), Some(0));
    let ba = std::fs::read(a.exe()).expect("read A");
    let bb = std::fs::read(b.exe()).expect("read B");
    assert_eq!(ba, bb, "two flag-off builds are not byte-identical (determinism regression)");
}
