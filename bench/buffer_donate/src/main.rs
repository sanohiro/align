//! doc-10 §8.1 / doc-13 §8.5 — unique-buffer donation: the REAL leak/double-free gate + balanced
//! AB/BA timing probe (measure-first adoption gate).
//!
//! The default-suite `buffer_donate` MIR/exec tests prove donation fires on the positive shape and
//! not on the negatives, and that donation-on and donation-off compute identical results (which
//! catches a double free — it aborts — and a use-after-donate — it corrupts the result). They CANNOT
//! detect a pure leak. This harness closes that gap and measures the adoption gate:
//!
//!  1. Leak/double-free: it links the donation-ON and donation-OFF kernels against the runtime built
//!     with `--features alloc-count` and reads `align_rt_alloc_count`/`align_rt_free_count` around
//!     `alloc_probe`. It asserts BOTH variants are perfectly balanced (alloc == free — no leak, no
//!     double-free) and that donation-ON allocates exactly `reps` FEWER buffers than OFF.
//!  2. Timing: it runs `time_probe` for ON and OFF in a balanced AB/BA order, several rounds, and
//!     reports the min wall-clock per variant across a working-set sweep. A material win on the
//!     positive case with identical results is the adoption signal.
//!
//! MANUAL probe: `./run.sh` (not a CI assertion — it needs the alloc-count runtime + native tuning).

use std::time::Instant;

extern "C" {
    // Donation ON build (default alignc).
    fn alloc_probe(reps: i64) -> i64;
    fn time_probe(reps: i64, n: i64) -> i64;
    // Donation OFF build (ALIGN_BUFFER_DONATE=off), entry symbols renamed `*_off` by run.sh.
    fn alloc_probe_off(reps: i64) -> i64;
    fn time_probe_off(reps: i64, n: i64) -> i64;
    // Present because run.sh builds the runtime with `--features alloc-count`.
    fn align_rt_alloc_count() -> i64;
    fn align_rt_free_count() -> i64;
}

fn counts() -> (i64, i64) {
    unsafe { (align_rt_alloc_count(), align_rt_free_count()) }
}

/// Min wall-clock over `rounds` calls of `f(reps, n)`, warming once first.
fn bench(rounds: u32, reps: i64, n: i64, f: unsafe extern "C" fn(i64, i64) -> i64) -> (u128, i64) {
    let mut got = unsafe { f(reps, n) }; // warm
    let mut best = u128::MAX;
    for _ in 0..rounds {
        let t = Instant::now();
        got = unsafe { f(reps, n) };
        best = best.min(t.elapsed().as_nanos());
    }
    (best, got)
}

fn main() {
    let mut failures = 0;

    // ── (1) Leak / double-free + alloc-reduction gate ────────────────────────────────────────────
    println!("== leak / double-free + alloc-reduction gate (alloc_probe) ==");
    for &reps in &[1i64, 2, 16, 1000, 20_000] {
        // ON
        let (a0, f0) = counts();
        let on = unsafe { alloc_probe(reps) };
        let (a1, f1) = counts();
        let (da_on, df_on) = (a1 - a0, f1 - f0);
        // OFF
        let (a2, f2) = counts();
        let off = unsafe { alloc_probe_off(reps) };
        let (a3, f3) = counts();
        let (da_off, df_off) = (a3 - a2, f3 - f2);

        // sum(inc(0..8)) = sum(1..9) = 36 per invocation.
        let want = reps * 36;
        let on_balanced = da_on == df_on;
        let off_balanced = da_off == df_off;
        // ON allocates exactly `reps` fewer buffers (one donated per invocation); OFF allocs 2x ON.
        let alloc_saved = da_off - da_on == reps && da_off == 2 * da_on;
        let ok = on == want && off == want && on_balanced && off_balanced && alloc_saved;
        println!(
            "  reps={reps:>6}: result on={on} off={off} (want {want})  alloc on={da_on} off={da_off} \
             free on={df_on} off={df_off}  saved={}  {}",
            da_off - da_on,
            if ok { "OK" } else { "FAIL" }
        );
        if !ok {
            failures += 1;
        }
    }

    // ── (2) Balanced AB/BA timing sweep across the cache hierarchy ────────────────────────────────
    // n * 8 bytes is the working set of ONE buffer; the 3-chain touches a few of them. Sweep n from
    // L1-resident to well past LLC. Each entry is (reps, n) chosen so total work is comparable.
    println!("\n== balanced AB/BA timing (time_probe, min of rounds; lower is better) ==");
    println!("  {:>10} {:>8} {:>14} {:>14} {:>9}", "n", "bytes/buf", "donate_on_ns", "donate_off_ns", "speedup");
    let sweep: &[(i64, i64)] = &[
        (2000, 512),      // ~4 KiB/buf  — L1
        (2000, 4096),     // 32 KiB/buf  — L1/L2 edge
        (1000, 65_536),   // 512 KiB/buf — L2/LLC
        (200, 1_048_576), // 8 MiB/buf   — LLC/DRAM
        (40, 8_388_608),  // 64 MiB/buf  — DRAM
    ];
    let rounds = 7;
    for &(reps, n) in sweep {
        // Balanced order: alternate ON/OFF first each half-round to cancel warmup/frequency drift.
        let (on_a, r_on_a) = bench(rounds, reps, n, time_probe);
        let (off_a, r_off_a) = bench(rounds, reps, n, time_probe_off);
        let (off_b, r_off_b) = bench(rounds, reps, n, time_probe_off);
        let (on_b, r_on_b) = bench(rounds, reps, n, time_probe);
        let on = on_a.min(on_b);
        let off = off_a.min(off_b);
        let results = [r_on_a, r_on_b, r_off_a, r_off_b];
        let identical = results.iter().all(|&r| r == results[0]);
        if !identical {
            eprintln!("  RESULT MISMATCH at n={n}: {results:?}");
            failures += 1;
        }
        let speedup = off as f64 / on as f64;
        println!("  {n:>10} {:>8} {on:>14} {off:>14} {speedup:>8.3}x", n * 8);
    }

    if failures == 0 {
        println!("\nBUFFER-DONATION GATE: PASS — balanced, one fewer alloc per donation, identical results");
    } else {
        eprintln!("\nBUFFER-DONATION GATE: FAIL ({failures} check(s))");
        std::process::exit(1);
    }
}
