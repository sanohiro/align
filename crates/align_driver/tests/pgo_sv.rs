//! Instrument-PGO SV — the verification bundle that CLOSES the instrument-PGO arc (S0–SV). It pins what
//! S1 (`pgo.rs`) and S2 (`pgo_cache.rs`) did not: build-twice determinism in BOTH modes, the
//! stale/wrong-profile mutation matrix, an explicit compile-time bound, and the PAYOFF GATE — a MEASURED
//! runtime win from a real profile, PGO's whole justification. The `thin_lto_sv.rs` bundle is the
//! structural template (build-twice determinism / mutation gates / compile-time bound), reused here.
//!
//!  1. **Build-twice determinism, both modes** — two independent COLD builds with SEPARATE cache roots
//!     produce a byte-identical executable, for `--pgo-instrument` (`gate_sv1a`) AND `--pgo-use` with the
//!     same profdata (`gate_sv1b`). The exe is deleted before each build and each build's coldness is
//!     asserted via `--cache-stats` (`0 hit, 1 miss`), so the comparison can never pass on a stale artifact.
//!  2. **Stale / wrong-profile mutation matrix** — (a) a profile from a DIFFERENT program → a prominent
//!     "matched 0 of N — is this profile from this program?" WARNING, exit 0, and a CORRECT exe (parity
//!     with flag-off), because a mismatched profile is performance-only, never a correctness bug
//!     (`gate_sv2a`); (b) a profile from an OLDER source version (the program is edited after profiling,
//!     but a surviving function still matches) → the aggregated staleness report on stderr, the build
//!     PROCEEDS, and the exe is correct (`gate_sv2b`); (c) a profdata whose body is corrupted PAST the
//!     magic (a flipped version field) so it slips the driver's magic pre-check but libLLVM's reader
//!     rejects it → the Error-severity diagnostic-handler HARD ERROR (`gate_sv2c`).
//!  3. **Compile-time bound** — a COLD `--pgo-instrument` and a COLD `--pgo-use` build's wall-time vs the
//!     flag-off cold build stays under a generous cap (interleaved per round, best-of-N min; `gate_sv3`,
//!     the `thin_lto_sv::gate_sv3` shape).
//!  4. **The PAYOFF GATE** (`gate_sv4`) — on a hot loop whose body PGO can lay out from the profile, a
//!     `--pgo-use` build beats the flag-off build by a MEASURED margin (interleaved A/B, min-of-N,
//!     black-box output, subprocess runs). Asserts a conservative floor well below the measured win.
//!
//! POLICY (2a): a wrong/stale profile is a WARNING, not a hard error. A `/code-review` pass falsified the
//! initial "0%-match = hard error" attempt — there is no reliable match signal (the post-pipeline
//! `Function::getEntryCount` tally undercounts inlined+DCE'd matches and, with `--rt-lto`, overcounts
//! baked runtime primitives that match any program; per-unit cache hits structurally bypass a build-level
//! gate). So the settled policy is amended: hard errors stay at the RELIABLE layer (missing/unreadable/
//! empty/bad-magic profdata + Error-severity libLLVM diagnostics — gate_sv2c), while a 0%/partial MATCH
//! ships as a prominent Align-voice warning (clang parity: profile mismatch is performance-only). The
//! tally still feeds that report. See the roadmap "Instrument-PGO SV SHIPPED" 2a paragraph.
//!
//! Tool-gated exactly like `pgo.rs`: the instrument link needs clang's profile runtime archive, the round
//! trip needs `llvm-profdata` (via the product's version-matched resolver), and the exe link needs `cc`.
//! Every gate skips (passes) when a required tool is absent.

mod common;
use common::*;

use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::Instant;

fn backend() -> bool {
    backend_available()
}

// ================================================================================================
// Corpus + harness
// ================================================================================================

/// A structurally DIFFERENT single-unit program (different helper name, different `main` CFG) — its
/// functions do NOT match `BRANCHY`'s profile records, so applying `BRANCHY`'s profile to it is the
/// 0%-match wrong-program case.
const DIFFERENT_PROG: &str = "\
fn transmogrify(z: i64) -> i64 {\n\
  mut r := z\n\
  if z % 7 == 0 { r = r + 100 }\n\
  if z % 11 == 0 { r = r - 50 }\n\
  return r\n\
}\n\
fn main() -> i32 {\n\
  mut k := 0\n\
  mut total := 0\n\
  loop {\n\
    if k >= 5000 { break }\n\
    total = total + transmogrify(k)\n\
    k = k + 1\n\
  }\n\
  print(total)\n\
  return 0\n\
}\n";

/// A program with a RECURSIVE `fib` (which survives O2 as its own function) plus a small `tag`. Profiled
/// as-is; the edited variant [`RECUR_V2`] changes only `tag`, so `fib` still matches the profile — the
/// partial-staleness case (some functions match, some do not → report + proceed).
const RECUR_V1: &str = "\
fn fib(n: i64) -> i64 {\n\
  if n < 2 { return n }\n\
  return fib(n - 1) + fib(n - 2)\n\
}\n\
fn tag(n: i64) -> i64 {\n\
  if n % 3 == 0 { return 7 }\n\
  return 9\n\
}\n\
fn main() -> i32 {\n\
  mut i := 0\n\
  mut acc := 0\n\
  loop {\n\
    if i >= 26 { break }\n\
    acc = acc + fib(i) + tag(i)\n\
    i = i + 1\n\
  }\n\
  print(acc)\n\
  return 0\n\
}\n";

/// [`RECUR_V1`] with `tag` edited (a new branch → its structural hash changes) and `fib`/`main` intact:
/// applying the V1 profile leaves `fib` matched (recursion keeps it a distinct function) and the edited
/// code mismatched → the partially-stale "proceed with an aggregated report" path.
const RECUR_V2: &str = "\
fn fib(n: i64) -> i64 {\n\
  if n < 2 { return n }\n\
  return fib(n - 1) + fib(n - 2)\n\
}\n\
fn tag(n: i64) -> i64 {\n\
  if n % 3 == 0 { return 8 }\n\
  if n % 6 == 0 { return 5 }\n\
  return 9\n\
}\n\
fn main() -> i32 {\n\
  mut i := 0\n\
  mut acc := 0\n\
  loop {\n\
    if i >= 26 { break }\n\
    acc = acc + fib(i) + tag(i)\n\
    i = i + 1\n\
  }\n\
  print(acc)\n\
  return 0\n\
}\n";

/// The PAYOFF kernel: a hot loop that, on ~90% of iterations (`r < 58` of `% 64`), runs a medium
/// straight-line `step` body, then always does a cheap tail. `step` is internalized + inlined in BOTH
/// builds, so the profile's win is the branch-weight-driven LAYOUT of the hot loop body (hot path placed
/// for fallthrough, the ~10% skip split out) — a stable ~1.1–1.16x measured on the dev box. Deterministic
/// skew (a multiplicative hash `% 64`) so the profile is representative, and a black-box `print(acc)` the
/// optimizer cannot fold away.
const PAYOFF: &str = "\
fn step(acc: i64, i: i64) -> i64 {\n\
  mut r := acc\n\
  r = r - (i % 4)\n\
  r = r * (i % 5)\n\
  r = r + (i % 6)\n\
  r = r - (i % 7)\n\
  r = r * (i % 8)\n\
  r = r + (i % 9)\n\
  r = r - (i % 10)\n\
  r = r * (i % 11)\n\
  r = r + (i % 12)\n\
  r = r - (i % 13)\n\
  r = r * (i % 14)\n\
  r = r + (i % 15)\n\
  return r\n\
}\n\
fn main() -> i32 {\n\
  mut i := 0\n\
  mut acc := 0\n\
  loop {\n\
    if i >= 40000000 { break }\n\
    r := (i * 2654435761) % 64\n\
    if r < 58 {\n\
      acc = step(acc, i)\n\
    }\n\
    acc = acc + (i & 3)\n\
    i = i + 1\n\
  }\n\
  print(acc)\n\
  return 0\n\
}\n";

/// An RAII scratch directory holding one source `prog.align` (stem `prog`). Each test gets its own so the
/// stem-named executable never collides; removed on drop even if the test panics.
struct Kit {
    dir: PathBuf,
}

impl Kit {
    fn new(tag: &str, src: &str) -> Kit {
        let dir = std::env::temp_dir().join(format!("align_pgosv_{tag}_{}_{}", std::process::id(), thin_nonce()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("create scratch dir");
        std::fs::write(dir.join("prog.align"), src).expect("write source");
        Kit { dir }
    }
    fn write(&self, src: &str) {
        std::fs::write(self.dir.join("prog.align"), src).expect("rewrite source");
    }
    fn exe(&self) -> PathBuf {
        self.dir.join("prog")
    }
    fn path(&self, name: &str) -> PathBuf {
        self.dir.join(name)
    }
}

impl Drop for Kit {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.dir);
    }
}

/// Run `alignc <flags> --profile release build prog.align` in the kit dir with `ALIGNC_CACHE=cache`
/// (`"off"` or a path). The produced `prog` executable lands in the kit dir.
fn build(kit: &Kit, cache: &str, flags: &[&str]) -> std::process::Output {
    let mut c = Command::new(env!("CARGO_BIN_EXE_alignc"));
    c.current_dir(&kit.dir).env("ALIGNC_CACHE", cache);
    for f in flags {
        c.arg(f);
    }
    c.args(["--profile", "release", "build", "prog.align"]);
    c.output().expect("run alignc build")
}

/// stderr of a build as an owned `String` (for message assertions).
fn err_of(out: &std::process::Output) -> String {
    String::from_utf8_lossy(&out.stderr).into_owned()
}

// ================================================================================================
// Gate SV1: build-twice determinism, both modes
// ================================================================================================

/// `--pgo-instrument`: two independent COLD builds (separate cache roots, exe deleted between) produce a
/// byte-identical executable, each asserted cold via `--cache-stats` (`0 hit, 1 miss`).
#[test]
fn gate_sv1a_instrument_build_twice_byte_identical() {
    if !backend() || !cc_available() || !profile_rt_available() {
        return;
    }
    let kit = Kit::new("det-instr", BRANCHY);
    let cold_build = |root: &str| -> Vec<u8> {
        let _ = std::fs::remove_file(kit.exe()); // never let a stale exe satisfy the comparison
        let out = build(&kit, root, &["--pgo-instrument", "--cache-stats"]);
        assert_eq!(out.status.code(), Some(0), "instrument build failed:\n{}", err_of(&out));
        assert!(err_of(&out).contains("0 hit, 1 miss"), "the build must be cold (all miss):\n{}", err_of(&out));
        assert!(kit.exe().exists(), "the build produced the exe");
        std::fs::read(kit.exe()).expect("read exe")
    };
    let a = cold_build(kit.path("ra").to_str().unwrap());
    let b = cold_build(kit.path("rb").to_str().unwrap());
    assert_eq!(a, b, "two cold --pgo-instrument builds (distinct cache roots) must be byte-identical");
}

/// `--pgo-use` with the SAME merged profile: two independent COLD builds (separate cache roots) produce a
/// byte-identical executable. Determinism must hold with a real profile threaded through codegen.
#[test]
fn gate_sv1b_use_build_twice_byte_identical() {
    if !backend() || !cc_available() {
        return;
    }
    let kit = Kit::new("det-use", BRANCHY);
    let Some(profdata) = make_profdata(&kit.dir, BRANCHY) else {
        return;
    };
    let prof = profdata.to_str().unwrap();
    let cold_build = |root: &str| -> Vec<u8> {
        let _ = std::fs::remove_file(kit.exe());
        let out = build(&kit, root, &["--pgo-use", prof, "--cache-stats"]);
        assert_eq!(out.status.code(), Some(0), "use build failed:\n{}", err_of(&out));
        assert!(err_of(&out).contains("0 hit, 1 miss"), "the build must be cold (all miss):\n{}", err_of(&out));
        std::fs::read(kit.exe()).expect("read exe")
    };
    let a = cold_build(kit.path("rc").to_str().unwrap());
    let b = cold_build(kit.path("rd").to_str().unwrap());
    assert_eq!(a, b, "two cold --pgo-use builds (same profdata, distinct cache roots) must be byte-identical");
}

// ================================================================================================
// Gate SV2: stale / wrong-profile mutation matrix
// ================================================================================================

/// 2a: a profile from a DIFFERENT program → a prominent 0%-match WARNING, exit 0, and a CORRECT exe.
/// `BRANCHY`'s profile is applied to [`DIFFERENT_PROG`], whose functions match none of the profile
/// records → 0 functions receive profile data → the driver emits the aggregated "is this profile from
/// this program?" warning and PROCEEDS. A mismatched profile is a performance concern only (clang
/// parity), so the build must succeed and the exe must be correct (identical stdout to a flag-off build).
#[test]
fn gate_sv2a_wrong_program_profile_warns_and_proceeds() {
    if !cc_available() {
        return;
    }
    let kit = Kit::new("wrongprog", BRANCHY);
    let Some(profdata) = make_profdata(&kit.dir, BRANCHY) else {
        return;
    };
    // Swap in a structurally different program; apply BRANCHY's profile to it.
    kit.write(DIFFERENT_PROG);
    let out = build(&kit, "off", &["--pgo-use", profdata.to_str().unwrap()]);
    assert_eq!(out.status.code(), Some(0), "a wrong-program profile must WARN + proceed, not hard-error:\n{}", err_of(&out));
    let err = err_of(&out);
    assert!(
        err.contains("matched 0 of") && err.contains("is this profile from this program"),
        "the prominent 0%-match warning must appear on stderr:\n{err}"
    );
    let use_stdout = Command::new(kit.exe()).output().expect("run use exe").stdout;

    // Correctness: a flag-off build of the same program must produce identical output (the ignored
    // profile changed nothing but performance).
    assert_eq!(build(&kit, "off", &[]).status.code(), Some(0), "flag-off build of the wrong-program source failed");
    let off_stdout = Command::new(kit.exe()).output().expect("run off exe").stdout;
    assert_eq!(use_stdout, off_stdout, "the profile-ignored --pgo-use exe must be correct (== flag-off output)");
}

/// 2b: a profile from an OLDER source version → partial staleness. The program is profiled, then `tag` is
/// edited while the recursive `fib` (a distinct surviving function) stays put. Applying the stale profile
/// leaves `fib` matched and the edited code mismatched, so the build must PROCEED with an aggregated
/// staleness report on stderr AND produce a CORRECT exe (identical stdout to a flag-off build of the
/// edited source).
#[test]
fn gate_sv2b_stale_source_partial_reports_and_proceeds() {
    if !cc_available() {
        return;
    }
    let kit = Kit::new("stalesrc", RECUR_V1);
    let Some(profdata) = make_profdata(&kit.dir, RECUR_V1) else {
        return;
    };
    // Edit the program (the profile is now partially stale); build with the stale profile.
    kit.write(RECUR_V2);
    let out = build(&kit, "off", &["--pgo-use", profdata.to_str().unwrap()]);
    assert_eq!(out.status.code(), Some(0), "partial staleness must PROCEED, not hard-error:\n{}", err_of(&out));
    let err = err_of(&out);
    assert!(
        err.contains("proceeding despite") && err.contains("profile-use warning"),
        "the aggregated staleness report must appear on stderr:\n{err}"
    );
    let use_stdout = Command::new(kit.exe()).output().expect("run use exe").stdout;

    // Correctness: a flag-off build of the SAME (edited) source must produce identical output.
    assert_eq!(build(&kit, "off", &[]).status.code(), Some(0), "flag-off build of the edited source failed");
    let off_stdout = Command::new(kit.exe()).output().expect("run off exe").stdout;
    assert_eq!(use_stdout, off_stdout, "the partially-stale --pgo-use exe must be correct (== flag-off output)");
}

/// 2c: a profdata whose body is corrupted PAST the 8-byte magic (the version field flipped) → it passes
/// the driver's magic pre-check (`validate_profdata`) but libLLVM's reader rejects it, and the USE-run
/// diagnostic handler turns that Error-severity diagnostic into a HARD ERROR (never a silent
/// diagnose-and-exit). The magic bytes are asserted intact so the failure is provably the deep reader
/// path, not the pre-check.
#[test]
fn gate_sv2c_corrupt_profile_valid_magic_hard_errors() {
    let kit = Kit::new("corrupt", BRANCHY);
    let Some(profdata) = make_profdata(&kit.dir, BRANCHY) else {
        return;
    };
    let mut bytes = std::fs::read(&profdata).expect("read profdata");
    assert!(bytes.len() > 16, "a merged profdata has at least a header");
    // Magic = bytes[0..8] (left intact); version = bytes[8..16] (flipped to an unsupported value).
    let magic_before = bytes[0..8].to_vec();
    for b in bytes[8..16].iter_mut() {
        *b ^= 0xFF;
    }
    assert_eq!(&bytes[0..8], &magic_before[..], "the magic must stay intact so validate_profdata passes");
    let corrupt = kit.path("corrupt.profdata");
    std::fs::write(&corrupt, &bytes).expect("write corrupt profdata");

    let out = build(&kit, "off", &["--pgo-use", corrupt.to_str().unwrap()]);
    assert_ne!(out.status.code(), Some(0), "a corrupt (valid-magic) profdata must hard-error");
    let err = err_of(&out);
    assert!(
        err.contains("PGO profile use reported error"),
        "the failure must be the Error-severity diagnostic-handler path:\n{err}"
    );
}

// ================================================================================================
// Gate SV3: compile-time bound (both PGO modes)
// ================================================================================================

/// A generous, non-flaky compile-time bound: a COLD `--pgo-instrument` build AND a COLD `--pgo-use` build
/// must each stay under `CAP`× the flag-off cold build over the same source. The three configs are
/// INTERLEAVED per round (a mid-test load spike hits all sides symmetrically) and the per-config min is
/// kept (`gate_sv3` in `thin_lto_sv` explains why this cannot flake). `ALIGNC_CACHE=off` forces every run
/// cold.
#[test]
fn gate_sv3_compile_time_bound_both_modes() {
    if !backend() || !cc_available() || !profile_rt_available() {
        return;
    }
    let kit = Kit::new("ctbound", BRANCHY);
    let Some(profdata) = make_profdata(&kit.dir, BRANCHY) else {
        return;
    };
    let prof = profdata.to_str().unwrap().to_string();
    const CAP: f64 = 3.0;
    const ROUNDS: usize = 3;
    let one = |flags: &[&str]| -> f64 {
        let t0 = Instant::now();
        let out = build(&kit, "off", flags);
        let dt = t0.elapsed().as_secs_f64();
        assert!(out.status.success(), "build failed (flags={flags:?}):\n{}", err_of(&out));
        dt
    };
    let (mut off, mut instr, mut usen) = (f64::INFINITY, f64::INFINITY, f64::INFINITY);
    for _ in 0..ROUNDS {
        off = off.min(one(&[]));
        instr = instr.min(one(&["--pgo-instrument"]));
        usen = usen.min(one(&["--pgo-use", &prof]));
    }
    let (ri, ru) = (instr / off, usen / off);
    assert!(ri < CAP, "--pgo-instrument cold build {ri:.2}x exceeds the {CAP:.1}x cap (off={off:.3}s, instr={instr:.3}s)");
    assert!(ru < CAP, "--pgo-use cold build {ru:.2}x exceeds the {CAP:.1}x cap (off={off:.3}s, use={usen:.3}s)");
}

// ================================================================================================
// Gate SV4: the PAYOFF GATE — a measured PGO runtime win
// ================================================================================================

/// PGO's whole justification: on the [`PAYOFF`] kernel a `--pgo-use` build must beat the flag-off build by
/// a MEASURED margin. Methodology (house discipline): a representative profile collected via the real CLI
/// round trip, off/use binaries built into distinct paths, black-box output parity asserted, then
/// interleaved A/B timing with the per-side min over N rounds (min discards one-time page-in). The floor
/// (1.03×) is far below the ~1.1–1.16× measured on the dev box; a machine where PGO genuinely does not
/// help this kernel would (correctly) fail, signalling the payoff claim does not hold there.
///
/// `#[ignore]`: this is a WALL-TIME assertion, and it flaked twice under the full threaded suite's
/// parallel build load (passes in isolation every time). Per the repo's perf-probe discipline
/// (compaction/base64/sort precedents), timing claims live in manual probes, never CI asserts. Run:
/// `cargo test -p align_driver --test pgo_sv gate_sv4 -- --ignored --nocapture`
#[test]
#[ignore = "manual payoff probe: wall-time assert, run in isolation (see doc comment)"]
fn gate_sv4_payoff_pgo_use_beats_flag_off() {
    if !backend() || !cc_available() {
        return;
    }
    let kit = Kit::new("payoff", PAYOFF);
    let Some(profdata) = make_profdata(&kit.dir, PAYOFF) else {
        return;
    };
    let prof = profdata.to_str().unwrap();

    // Flag-off binary.
    assert_eq!(build(&kit, "off", &[]).status.code(), Some(0), "flag-off payoff build failed");
    let off_exe = kit.path("prog_off");
    std::fs::rename(kit.exe(), &off_exe).expect("stash off exe");

    // Profile-use binary.
    let use_out = build(&kit, "off", &["--pgo-use", prof]);
    assert_eq!(use_out.status.code(), Some(0), "--pgo-use payoff build failed:\n{}", err_of(&use_out));
    let use_exe = kit.path("prog_use");
    std::fs::rename(kit.exe(), &use_exe).expect("stash use exe");

    // Black-box output parity (the profile must not change results).
    let o_off = Command::new(&off_exe).output().expect("run off");
    let o_use = Command::new(&use_exe).output().expect("run use");
    assert_eq!(o_off.stdout, o_use.stdout, "off/use payoff output must be identical");

    const FLOOR: f64 = 1.03;
    const ROUNDS: usize = 5;
    let timed = |exe: &Path| -> f64 {
        let t0 = Instant::now();
        let st = Command::new(exe).stdout(Stdio::null()).status().expect("run kernel");
        assert!(st.success(), "kernel run failed");
        t0.elapsed().as_secs_f64()
    };
    let (mut off, mut usen) = (f64::INFINITY, f64::INFINITY);
    for _ in 0..ROUNDS {
        off = off.min(timed(&off_exe));
        usen = usen.min(timed(&use_exe));
    }
    let speedup = off / usen;
    assert!(
        speedup >= FLOOR,
        "PGO payoff {speedup:.3}x is below the {FLOOR:.2}x floor (off={off:.3}s, use={usen:.3}s) — \
         the profile-use build did not measurably beat flag-off on this kernel"
    );
}
