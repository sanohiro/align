//! doc-13 §6.6 / §11 P3 — repeated-needle plan hoisting: REAL-PIPELINE adoption probe.
//!
//! `#[ignore]` (run with `cargo test -p align_driver --test needle_plan_hoist_probe -- --ignored
//! --nocapture`). This is the adoption gate: the win the doc-13 §6.6 runtime kernels measured must
//! reproduce through the REAL alignc-compiled pipeline.
//!
//! Method. For each (needle bytes, haystack bytes, element count) config, ONE Align program
//! (`xs.where(fn s { s.contains(n) }).count()` in a timed reps loop) is compiled twice by the current
//! alignc, differing ONLY in the `ALIGN_NEEDLE_HOIST` MIR toggle: ON (default) hoists one
//! `str_finder` plan per invocation and reuses it across the `count` elements; OFF lowers the same
//! fused loop but reconstructs the `memchr` searcher per element (`str_contains`). Because both come
//! from the identical source and the toggle only flips the per-element search, the comparison
//! isolates searcher reuse — no loop-shape / bounds-check / fusion confound. Each kernel times an
//! internal reps loop with `time.instant()` and prints the best (min) nanoseconds over TRIALS; the
//! harness runs the two executables interleaved and takes the min. The needle is a no-match string
//! (worst case: the full haystack is scanned).
//!
//! Adoption gate (reported, softly asserted): a material speedup in the 32–128 B region and roughly
//! neutral (~1.0×) at 16 KiB. The toggle is read at MIR lowering (the object-cache key fingerprints
//! it and `CacheContext::from_env` force-disables the cache when it is set).

mod common;
use common::*;
use std::time::Duration;

const TRIALS: u32 = 7; // internal min-of-N trials per kernel run
const PROC_RUNS: u32 = 5; // interleaved process runs per kernel (min taken)

/// Distinct `hsize`-byte strings, none containing the all-'z' needle (content is 'a'..'y').
fn haystacks(count: usize, hsize: usize) -> Vec<String> {
    (0..count)
        .map(|i| {
            let mut s = String::with_capacity(hsize);
            let mut n = i;
            for _ in 0..4 {
                s.push((b'a' + (n % 25) as u8) as char);
                n /= 25;
            }
            while s.len() < hsize {
                s.push((b'a' + ((s.len() + i) % 25) as u8) as char);
            }
            s.truncate(hsize);
            s
        })
        .collect()
}

fn array_literal(hs: &[String]) -> String {
    let mut out = String::from("[");
    for (i, s) in hs.iter().enumerate() {
        if i > 0 {
            out.push_str(", ");
        }
        out.push('"');
        out.push_str(s); // 'a'..'y' only — no escaping needed
        out.push('"');
    }
    out.push(']');
    out
}

fn reps_for(count: usize, hsize: usize) -> u64 {
    let per_ns = (count as u64) * (100 + hsize as u64);
    (3_000_000 / per_ns.max(1)).clamp(20, 20_000)
}

/// The single hoisted where-pipeline kernel — compiled twice, once per toggle state.
fn pipeline_src(hs: &[String], needle: &str, reps: u64) -> String {
    format!(
        r#"import std.time
pub fn main() -> Result<(), Error> {{
  xs := {arr}
  n := "{needle}"
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
      acc = acc + xs.where(fn s {{ s.contains(n) }}).count()
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
        arr = array_literal(hs),
        needle = needle,
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

/// Build the given source with `ALIGN_NEEDLE_HOIST` forced to a state (`hoist_on` leaves it unset).
/// The toggle is read in-process at MIR lowering, so set it around the synchronous `build_exe`.
/// SAFETY: this `#[ignore]` probe runs serially and alone; no other thread reads the env here.
fn build_with_toggle(name: &str, src: &str, hoist_on: bool) -> BuiltExe {
    unsafe {
        if hoist_on {
            std::env::remove_var("ALIGN_NEEDLE_HOIST");
        } else {
            std::env::set_var("ALIGN_NEEDLE_HOIST", "off");
        }
    }
    let exe = build_exe(name, src);
    unsafe {
        std::env::remove_var("ALIGN_NEEDLE_HOIST");
    }
    exe
}

#[test]
#[ignore = "measurement probe; run with --ignored --nocapture"]
fn real_pipeline_adoption_probe() {
    if !backend_available() {
        eprintln!("backend unavailable; skipping probe");
        return;
    }
    let needles: [(&str, usize); 2] = [("zzzz", 4), ("zzzzzzzzzzzzzzzz", 16)];
    let hsizes = [32usize, 64, 128, 1024, 16384];
    let counts = [16usize]; // amortized regime — the core adoption claim; bounds embedded source size

    println!("\nneedle  hay    count   reps    hoist_ns   percall_ns  speedup(off/on)");
    println!("--------------------------------------------------------------------------");
    let mut short_speedups: Vec<f64> = Vec::new();
    let mut large_speedups: Vec<f64> = Vec::new();

    for (needle, nb) in needles {
        for &hsize in &hsizes {
            for &count in &counts {
                let reps = reps_for(count, hsize);
                let hs = haystacks(count, hsize);
                let src = pipeline_src(&hs, needle, reps);
                // Identical source, two toggle states.
                let on_exe = build_with_toggle(&format!("nphp-on-{nb}-{hsize}-{count}"), &src, true);
                let off_exe = build_with_toggle(&format!("nphp-off-{nb}-{hsize}-{count}"), &src, false);

                let mut on_best = u64::MAX;
                let mut off_best = u64::MAX;
                for _ in 0..PROC_RUNS {
                    on_best = on_best.min(run_best_ns(&on_exe.exe));
                    off_best = off_best.min(run_best_ns(&off_exe.exe));
                    std::thread::sleep(Duration::from_millis(2));
                }
                let speedup = off_best as f64 / on_best.max(1) as f64;
                println!(
                    "{nb:>4}B  {hsize:>5}  {count:>5}  {reps:>6}  {on_best:>10}  {off_best:>11}  {speedup:>8.2}x"
                );
                if hsize <= 128 {
                    short_speedups.push(speedup);
                } else if hsize >= 16384 {
                    large_speedups.push(speedup);
                }
            }
        }
    }

    let gmean = |v: &[f64]| -> f64 {
        if v.is_empty() {
            return 1.0;
        }
        (v.iter().map(|x| x.ln()).sum::<f64>() / v.len() as f64).exp()
    };
    let short = gmean(&short_speedups);
    let large = gmean(&large_speedups);
    println!("\nshort-region (<=128B) geomean speedup: {short:.2}x");
    println!("large-region (16KiB)  geomean speedup: {large:.2}x");
    println!(
        "ADOPTION: {}",
        if short >= 1.15 {
            "REPRODUCED — material short-input win through the real pipeline (keep the compiler wiring)"
        } else {
            "NOT REPRODUCED (<~15% short win) — report; consider reverting the compiler wiring"
        }
    );

    assert!(
        short >= 1.10,
        "short-input (<=128B) win did not reproduce through the identical-pipeline toggle: geomean {short:.2}x"
    );
    assert!(large >= 0.90, "16KiB region regressed beyond noise: geomean {large:.2}x");
}
