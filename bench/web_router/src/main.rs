//! `bench/web_router` — the pkg.web W5 dispatch gate.
//!
//! Times the shipped framework router's per-request dispatch against a hand-written match over the
//! same six-route table, on four request shapes (static hit, `:param` hit, `*wildcard` hit, and a
//! miss). Both sides are Align, compiled from the same kernel with the same `--target-cpu`, so the
//! only difference is the dispatch mechanism.
//!
//! The gate (`docs/impl/pkg-design/web.md` item 3): dispatch is O(path segments) — flat in table
//! size — and the framework is within noise of the hand-written control while resolving strictly
//! more (priority order, param capture, the method table).

use std::time::Instant;

unsafe extern "C" {
    /// The framework router: `shape` selects the request path (0 static, 1 `:param`, 2 `*wildcard`,
    /// 3 miss) at RUNTIME, so neither side can constant-fold the path away.
    fn fw(shape: i64, n: i64) -> i64;
    /// The hand-written control over the same table and the same runtime-selected path.
    fn hw(shape: i64, n: i64) -> i64;
    /// The same framework dispatch over a 128-route table — the scaling gate.
    fn fw_big(shape: i64, n: i64) -> i64;
}

/// Best-of-`trials` wall time for `n` dispatches, in nanoseconds per dispatch. The minimum is the
/// least-noise estimator for a microbench (it is the run least interrupted by the scheduler).
fn ns_per_op(f: unsafe extern "C" fn(i64, i64) -> i64, shape: i64, n: i64, trials: usize) -> f64 {
    let mut best = f64::MAX;
    for _ in 0..trials {
        let t0 = Instant::now();
        let acc = unsafe { f(shape, n) };
        let ns = t0.elapsed().as_nanos() as f64;
        std::hint::black_box(acc);
        best = best.min(ns / n as f64);
    }
    best
}

fn main() {
    let n: i64 = std::env::var("N").ok().and_then(|s| s.parse().ok()).unwrap_or(200_000);
    let trials: usize = std::env::var("TRIALS").ok().and_then(|s| s.parse().ok()).unwrap_or(7);

    // Agreement first: a benchmark of two functions that disagree measures nothing. One dispatch
    // each — the accumulator is `n` copies of the route index, so `f(1)` IS the index.
    let shapes: [(&str, i64, i64); 4] = [
        ("static   /v1/models", 0, 0),
        ("param    /v1/models/42", 1, 2),
        ("wildcard /assets/css/site.css", 2, 5),
        ("miss     /v2/nope", 3, -1),
    ];
    for (name, shape, want) in shapes {
        let (a, b) = unsafe { (fw(shape, 1), hw(shape, 1)) };
        assert_eq!(a, want, "framework dispatch for {name} must resolve to route {want}, got {a}");
        assert_eq!(b, want, "hand-written dispatch for {name} must resolve to route {want}, got {b}");
    }

    // The scaling shapes need the same anchor: a `fw_big` that started missing early would read as
    // an IMPROVED scaling ratio with nothing to catch it.
    for (shape, want) in [(0i64, 0i64), (1, 127), (2, -1)] {
        let got = unsafe { fw_big(shape, 1) };
        assert_eq!(got, want, "fw_big shape {shape} must resolve to route {want}, got {got}");
    }

    // Warm both sides (first-call page-ins, the tree build) before any measurement.
    for (_, shape, _) in shapes {
        unsafe {
            std::hint::black_box(fw(shape, 1000));
            std::hint::black_box(hw(shape, 1000));
        }
    }

    println!("dispatches: {n}, best of {trials}\n");
    println!("  {:<32} {:>12} {:>12} {:>10}", "shape", "framework", "hand-written", "ratio");
    let mut worst = 0.0f64;
    for (name, shape, _) in shapes {
        // A and B are measured adjacently per shape (not all shapes of A, then all of B), so the
        // two sides of a printed ratio share their clock state. Within a side the 7 trials are
        // consecutive — full per-trial A/B alternation is the stricter form `bench/README.md`
        // describes, and is worth doing if these ratios ever get close enough to argue about.
        let a = ns_per_op(fw, shape, n, trials);
        let b = ns_per_op(hw, shape, n, trials);
        let ratio = a / b;
        worst = worst.max(ratio);
        println!("  {name:<32} {a:>9.1} ns {b:>9.1} ns {ratio:>9.2}x");
    }
    println!("\n  worst framework/hand-written ratio: {worst:.2}x");

    // The scaling gate: contract item 3 says dispatch is O(path segments) and FLAT in table size.
    // Compare the SAME shapes over 6 routes and over 128 — a per-route slope would show up here as
    // a ratio far from 1.0, which is exactly how the pre-chain implementation behaved (~0.85 ns per
    // route: 44 ns at 8 routes, 453 ns at 512).
    println!("\n  scaling (contract item 3 — dispatch must be flat in table size)");
    println!("  {:<32} {:>12} {:>12} {:>10}", "shape", "6 routes", "128 routes", "ratio");
    let scaling: [(&str, i64, i64); 3] = [
        ("static hit", 0, 0),
        ("param  hit", 1, 1),
        ("miss", 3, 2),
    ];
    let mut worst_scale = 0.0f64;
    for (name, small_shape, big_shape) in scaling {
        let small = ns_per_op(fw, small_shape, n, trials);
        let big = ns_per_op(fw_big, big_shape, n, trials);
        let ratio = big / small;
        worst_scale = worst_scale.max(ratio);
        println!("  {name:<32} {small:>9.1} ns {big:>9.1} ns {ratio:>9.2}x");
    }
    println!("\n  worst 128-route / 6-route ratio: {worst_scale:.2}x  (1.00x = flat; linear scan would be ~20x)");
}
