//! doc-13 §6.6 — repeated-needle plan hoisting: the REAL leak / double-free assertion (finding 5).
//!
//! The default-suite `needle_plan_hoist` tests run the hoisted pipeline and confirm it does not crash
//! and returns the right count — that catches a double free / use-after-free but CANNOT detect a pure
//! leak (a leaked ~tens-of-bytes plan neither crashes nor drifts the count). This harness closes that
//! gap: it links the compiled `kernel.align` against the runtime built with `--features alloc-count`
//! and reads `align_rt_str_finder_new_count` / `align_rt_str_finder_free_count` around each call, so
//! it asserts the plan is freed EXACTLY once per invocation — after a reps loop and after an early
//! `?` error exit. A leak leaves `new > free`; a double free leaves `free > new`.
//!
//! MANUAL probe: `./run.sh` (not a CI assertion — it needs the alloc-count runtime).

extern "C" {
    // Exported from kernel.align (i64 in / i64 out — the needle is a literal inside the kernel).
    fn count_reps(reps: i64) -> i64;
    fn count_try(flag: i64) -> i64;
    // Present because run.sh builds the runtime with `--features alloc-count`.
    fn align_rt_str_finder_new_count() -> i64;
    fn align_rt_str_finder_free_count() -> i64;
}

fn counts() -> (i64, i64) {
    unsafe { (align_rt_str_finder_new_count(), align_rt_str_finder_free_count()) }
}

fn main() {
    let mut failures = 0;

    // (1) reps loop: N invocations must build and free exactly N plans, and balance.
    for &reps in &[1i64, 2, 16, 1000, 20_000] {
        let (n0, f0) = counts();
        let got = unsafe { count_reps(reps) };
        let (n1, f1) = counts();
        let (dnew, dfree) = (n1 - n0, f1 - f0);
        // Each element list has "al" in "alpha"/"alfalfa" → 2 hits per invocation.
        let expect_hits = reps * 2;
        let ok = dnew == reps && dfree == reps && got == expect_hits;
        println!(
            "count_reps(reps={reps:>6}): result={got:>8} (want {expect_hits})  finder_new+={dnew:>6} finder_free+={dfree:>6}  {}",
            if ok { "OK" } else { "FAIL" }
        );
        if !ok {
            failures += 1;
        }
    }

    // (2) early `?` error exit (flag=0 → Err propagated): the plan built before the `?` must still be
    // freed exactly once. And the normal path (flag != 0) frees exactly once too.
    for &flag in &[0i64, 5] {
        let (n0, f0) = counts();
        let got = unsafe { count_try(flag) };
        let (n1, f1) = counts();
        let (dnew, dfree) = (n1 - n0, f1 - f0);
        // flag=0 → inner returns Err(3) → `else -1` → -1; flag!=0 → 2 hits ("ab" in "ab","abc") +
        // flag → 2 + flag.
        let expect = if flag == 0 { -1 } else { 2 + flag };
        let ok = dnew == 1 && dfree == 1 && got == expect;
        println!(
            "count_try(flag={flag}):   result={got:>8} (want {expect})  finder_new+={dnew} finder_free+={dfree}  {}",
            if ok { "OK" } else { "FAIL" }
        );
        if !ok {
            failures += 1;
        }
    }

    // (3) Global balance at the end: every plan ever built has been freed.
    let (total_new, total_free) = counts();
    let balanced = total_new == total_free;
    println!("\ntotal finder_new={total_new} finder_free={total_free}  balanced={balanced}");
    if !balanced {
        failures += 1;
    }

    if failures == 0 {
        println!("\nLEAK/DOUBLE-FREE GATE: PASS — finder plans are freed exactly once on every exit path");
    } else {
        eprintln!("\nLEAK/DOUBLE-FREE GATE: FAIL ({failures} check(s))");
        std::process::exit(1);
    }
}
