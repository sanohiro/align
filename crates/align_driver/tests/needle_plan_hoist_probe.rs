//! doc-13 §6.6 / §11 P3 — repeated-needle plan hoisting: REAL-PIPELINE adoption probe.
//!
//! `#[ignore]` (run with `cargo test -p align_driver --test needle_plan_hoist_probe -- --ignored
//! --nocapture`). This is the adoption gate: the win the doc-13 §6.6 runtime kernels measured
//! (2.5–6.1× at 32–128 B, ~1.0× at 16 KiB) must reproduce through the REAL alignc-compiled pipeline,
//! not just a Rust micro-kernel.
//!
//! Method. For each (needle bytes, haystack bytes, element count) config, two Align programs are
//! compiled by the current alignc and linked against the runtime. HOISTED =
//! `xs.where(fn s { s.contains(n) }).count()` → one `finder_new` per invocation, reused across the
//! `count` elements (`finder_find`). PER-CALL = a hand-written `loop { if xs[i].contains(n) … }` →
//! `str_contains` per element, i.e. it reconstructs the `memchr` searcher for every element (the
//! unhoisted shape). Both are the real compiled pipeline; they differ only in searcher reuse. Each kernel times an
//! internal `REPS` loop with `time.instant()` and prints the best (min) reps-loop nanoseconds over
//! `TRIALS` internal trials; the harness runs the two executables interleaved and takes the min.
//! `finder_new` returns a fresh opaque allocation each invocation, so neither the searcher build nor
//! the per-element search can be hoisted/CSE'd across reps (no measurement collapse). The needle is a
//! no-match string (worst case: the full haystack is scanned).
//!
//! Adoption gate (reported, and softly asserted): a material speedup in the 32–128 B region and
//! roughly neutral (~1.0×) at 16 KiB. Counts sweep the amortization boundary (1 element = plan build
//! not yet amortized; growing counts amortize it).

mod common;
use common::*;
use std::time::Duration;

const TRIALS: u32 = 7; // internal min-of-N trials per kernel run
const PROC_RUNS: u32 = 5; // interleaved process runs per kernel (min taken)

/// Distinct `hsize`-byte strings, none containing the needle (needle is all 'z'; content is 'a'..'y'
/// plus a per-index varying prefix so no two elements are identical and none matches).
fn haystacks(count: usize, hsize: usize) -> Vec<String> {
    (0..count)
        .map(|i| {
            let mut s = String::with_capacity(hsize);
            // A short distinct prefix (base-25 of i over 'a'..'y'), then fill with a rotating
            // 'a'..'y' pattern. 'z' never appears, so an all-'z' needle never matches.
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
        out.push_str(s); // content is ASCII 'a'..'y' — no escaping needed
        out.push('"');
    }
    out.push(']');
    out
}

fn reps_for(count: usize, hsize: usize) -> u64 {
    // Target ~3 ms per reps-loop on the SLOW (per-call) path so timing is stable but the matrix
    // finishes fast. Per pipeline the per-call path pays, per element, a searcher reconstruction
    // (~100 ns) plus a scan (~1 ns/byte); bound reps by that estimate.
    let per_ns = (count as u64) * (100 + hsize as u64);
    (3_000_000 / per_ns.max(1)).clamp(20, 20_000)
}

fn hoisted_src(hs: &[String], needle: &str, reps: u64) -> String {
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

fn percall_src(hs: &[String], needle: &str, reps: u64) -> String {
    format!(
        r#"import std.time
pub fn main() -> Result<(), Error> {{
  xs := {arr}
  n := "{needle}"
  total := xs.len()
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
      mut i := 0
      c := loop {{
        if i >= total {{ break acc }}
        if xs[i].contains(n) {{ acc = acc + 1 }}
        i = i + 1
      }}
      acc = c
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

#[test]
#[ignore = "measurement probe; run with --ignored --nocapture"]
fn real_pipeline_adoption_probe() {
    if !backend_available() {
        eprintln!("backend unavailable; skipping probe");
        return;
    }
    let needles: [(&str, usize); 2] = [("zzzz", 4), ("zzzzzzzzzzzzzzzz", 16)];
    let hsizes = [32usize, 64, 128, 1024, 16384];
    // Fixed at the amortized regime (16 elements per plan) — the core adoption claim. The count=1
    // boundary is exercised by the runtime kernels in doc-13 §6.6; here we keep the compiled-source
    // size bounded (16 * 16 KiB ≈ 256 KiB max) so the matrix compiles in a sane time.
    let counts = [16usize];

    println!("\nneedle  hay    count   reps   hoisted_ns  percall_ns  speedup(percall/hoisted)");
    println!("--------------------------------------------------------------------------------");

    // Track the short-region and large-region behavior for the soft adoption assertion.
    let mut short_speedups: Vec<f64> = Vec::new();
    let mut large_speedups: Vec<f64> = Vec::new();

    for (needle, nb) in needles {
        for &hsize in &hsizes {
            for &count in &counts {
                let reps = reps_for(count, hsize);
                let hs = haystacks(count, hsize);
                let h_exe = build_exe(&format!("nphp-h-{nb}-{hsize}-{count}"), &hoisted_src(&hs, needle, reps));
                let p_exe = build_exe(&format!("nphp-p-{nb}-{hsize}-{count}"), &percall_src(&hs, needle, reps));

                // Interleave the two executables to average out any slow frequency drift.
                let mut hbest = u64::MAX;
                let mut pbest = u64::MAX;
                for _ in 0..PROC_RUNS {
                    hbest = hbest.min(run_best_ns(&h_exe.exe));
                    pbest = pbest.min(run_best_ns(&p_exe.exe));
                    std::thread::sleep(Duration::from_millis(2));
                }
                let speedup = pbest as f64 / hbest.max(1) as f64;
                println!(
                    "{nb:>4}B  {hsize:>5}  {count:>5}  {reps:>6}  {hbest:>10}  {pbest:>10}  {speedup:>8.2}x"
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

    // Soft gate: the short region must show a real win; the large region must not regress badly.
    assert!(
        short >= 1.10,
        "short-input (<=128B) win did not reproduce through the real pipeline: geomean {short:.2}x"
    );
    assert!(
        large >= 0.90,
        "16KiB region regressed beyond noise: geomean {large:.2}x"
    );
}
