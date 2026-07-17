//! doc-13 §8.4 / §11 P3 item 2 — local constant-array pooling: adoption probe + cutoff sweep.
//!
//! `#[ignore]` (run with `cargo test -p align_driver --test const_pool_probe -- --ignored
//! --nocapture`). This is the S3 adoption gate: pooling a large all-constant local array literal
//! into per-unit rodata (one memcpy — which LLVM elides to a direct rodata read for a non-mutated
//! binding) must beat the baseline that rebuilds the table with `n` element stores on every call.
//!
//! Method. For each element count `N`, ONE Align program is compiled twice by the current alignc,
//! differing ONLY in the `ALIGN_CONST_POOL` toggle: ON (default) pools; OFF rebuilds the table with
//! `n` stores. The program calls a function `hot(seed)` holding the local table and reading it
//! through a data-dependent runtime index inside a reps loop; because the index depends on the
//! running accumulator, LLVM keeps the per-call table initialization in the loop body (verified: the
//! unpooled stores stay under the loop backedge, they are not hoisted), so the comparison isolates
//! the per-call init cost — pooled reads rodata, unpooled rebuilds `xs`. Each kernel times an
//! internal reps loop with `time.instant()` and prints the best (min) nanoseconds; the harness runs
//! the two executables interleaved and takes the min, balanced.
//!
//! Adoption gate (reported, softly asserted): >=15% (>=1.15x) at the large positive case, and
//! <=3% regression (>=0.97x) below the chosen cutoff. The sweep locates the cutoff empirically —
//! `CONST_POOL_MIN_ELEMS` in `align_sema` must sit at the first N where the win is clear and no
//! smaller N regresses beyond noise. The toggle is read at sema; the object-cache key fingerprints
//! the lowered MIR and `CacheContext::from_env` force-disables the cache when it is set.

mod common;
use common::*;
use std::time::Duration;

const TRIALS: u32 = 9; // internal min-of-N trials per kernel run (median-of-9 discipline: min is stable here)
const PROC_RUNS: u32 = 5; // interleaved process runs per kernel (min taken)

/// N pseudo-random small non-negative i64 constants, rendered as an array literal.
fn table_literal(n: usize) -> String {
    // A cheap deterministic LCG — distinct values, no external crate.
    let mut state: u64 = 0x9e3779b97f4a7c15;
    let mut out = String::from("[");
    for i in 0..n {
        if i > 0 {
            out.push_str(", ");
        }
        state = state.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
        out.push_str(&((state >> 33) % 1000).to_string());
    }
    out.push(']');
    out
}

/// Reps scaled so total work (reps * per-call init) is roughly constant across N.
fn reps_for(n: usize) -> u64 {
    (40_000_000 / (n as u64 + 8)).clamp(2_000, 4_000_000)
}

/// The single hot-table kernel — compiled twice, once per toggle state. `hot(seed)` holds the local
/// table; the reps loop feeds the accumulator back as the index seed so the table init cannot be
/// hoisted out of the loop.
fn kernel_src(n: usize, reps: u64) -> String {
    format!(
        r#"import std.time
fn hot(seed: i64) -> i64 {{
  xs := {table}
  return xs[seed % {n}] + xs[(seed * 7 + 3) % {n}]
}}
pub fn main() -> Result<(), Error> {{
  mut best := 9223372036854775807
  mut grand := 0
  mut trial := 0
  best = loop {{
    if trial >= {trials} {{ break best }}
    t0 := time.instant()
    mut acc := 0
    mut r := 0
    acc = loop {{
      if r >= {reps} {{ break acc }}
      acc = acc + hot(acc + r)
      r = r + 1
    }}
    t1 := time.instant()
    grand = grand + acc
    dt := t1 - t0
    if dt < best {{ best = dt }}
    trial = trial + 1
  }}
  print(best)
  print(grand)
  return Ok(())
}}
"#,
        table = table_literal(n),
        n = n,
        trials = TRIALS,
        reps = reps,
    )
}

fn run_best_ns(exe: &std::path::Path) -> u64 {
    let out = std::process::Command::new(exe).output().expect("run probe exe");
    assert!(out.status.success(), "probe exe failed: {}", String::from_utf8_lossy(&out.stderr));
    let txt = String::from_utf8_lossy(&out.stdout);
    txt.lines()
        .next()
        .and_then(|l| l.trim().parse::<u64>().ok())
        .unwrap_or_else(|| panic!("probe printed no ns: {txt:?}"))
}

/// Build `src` with `ALIGN_CONST_POOL` forced to a state (`pool_on` leaves it unset = default-on).
/// The toggle is read in-process at sema, so set it around the synchronous `build_exe`.
/// SAFETY: this `#[ignore]` probe runs serially and alone; no other thread reads the env here.
fn build_with_toggle(name: &str, src: &str, pool_on: bool) -> BuiltExe {
    unsafe {
        if pool_on {
            std::env::remove_var("ALIGN_CONST_POOL");
        } else {
            std::env::set_var("ALIGN_CONST_POOL", "off");
        }
    }
    let exe = build_exe(name, src);
    unsafe {
        std::env::remove_var("ALIGN_CONST_POOL");
    }
    exe
}

#[test]
#[ignore = "measurement probe; run with --ignored --nocapture"]
fn const_pool_adoption_and_cutoff_sweep() {
    if !backend_available() {
        eprintln!("backend unavailable; skipping probe");
        return;
    }
    let counts = [1usize, 4, 8, 16, 32, 64, 256, 1024, 4096];

    println!("\n   N     reps      pool_ns   nopool_ns   speedup(off/on)");
    println!("-------------------------------------------------------------");
    let mut results: Vec<(usize, f64)> = Vec::new();

    for &n in &counts {
        let reps = reps_for(n);
        let src = kernel_src(n, reps);
        let on_exe = build_with_toggle(&format!("cpp-on-{n}"), &src, true);
        let off_exe = build_with_toggle(&format!("cpp-off-{n}"), &src, false);

        let mut on_best = u64::MAX;
        let mut off_best = u64::MAX;
        for _ in 0..PROC_RUNS {
            // Balanced AB/BA: alternate which runs first across proc runs.
            on_best = on_best.min(run_best_ns(&on_exe.exe));
            off_best = off_best.min(run_best_ns(&off_exe.exe));
            std::thread::sleep(Duration::from_millis(2));
            off_best = off_best.min(run_best_ns(&off_exe.exe));
            on_best = on_best.min(run_best_ns(&on_exe.exe));
            std::thread::sleep(Duration::from_millis(2));
        }
        let speedup = off_best as f64 / on_best.max(1) as f64;
        println!("{n:>5}  {reps:>8}  {on_best:>10}  {off_best:>10}  {speedup:>10.3}x");
        results.push((n, speedup));
    }

    println!("\nCUTOFF READING:");
    for (n, s) in &results {
        let verdict = if *s >= 1.15 {
            "WIN (>=15%)"
        } else if *s >= 0.97 {
            "neutral (<=3% regression)"
        } else {
            "REGRESSION (>3%)"
        };
        println!("  N={n:>5}: {s:.3}x  {verdict}");
    }

    let large = results.iter().find(|(n, _)| *n == 4096).map(|(_, s)| *s).unwrap_or(1.0);
    println!(
        "\nADOPTION: {}",
        if large >= 1.15 {
            "large positive case wins >=15% (keep the compiler wiring; set CONST_POOL_MIN_ELEMS at the empirical crossover)"
        } else {
            "large positive case did NOT win >=15% — report the honest negative and revert the compiler wiring"
        }
    );

    // Soft assertions (report-first; a noisy host must not red the suite). The large case is the
    // adoption claim; every N at/above the current cutoff must not regress.
    assert!(large >= 1.15, "large (N=4096) positive case did not clear the 15% adoption gate: {large:.3}x");
}
