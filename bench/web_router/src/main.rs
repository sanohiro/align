//! `bench/web_router` — the pkg.web W5 dispatch gate.
//!
//! Times the shipped framework router's per-request dispatch against a hand-written match over the
//! same six-route table, on four request shapes (static hit, `:param` hit, `*wildcard` hit, and a
//! miss). Both sides are Align, compiled from the same kernel with the same `--target-cpu`, so the
//! only difference is the dispatch mechanism.
//!
//! The scaling gate compares identical paths at identical depths across 6- and 128-route tables,
//! at both ends of their sibling chains. Each ratio is the median of adjacent, counterbalanced
//! pairs so clock drift and measurement position cannot masquerade as table-size cost.

use std::time::Instant;

/// CI ceilings include 28–31% headroom over the measured baseline/native maxima (1.07x / 2.11x).
/// The head ceiling rejects the old depth-mismatched row (~1.5x); the overall ceiling rejects a
/// return to route-table scanning or reversed sibling registration order while tolerating runner
/// noise. These are regression bounds, not a claim that contract item 3's ideal 1.00x is met.
const MAX_HEAD_SCALE: f64 = 1.35;
const MAX_ANY_SCALE: f64 = 2.75;

unsafe extern "C" {
    /// The framework router: `shape` selects the request path (0 static, 1 `:param`, 2 `*wildcard`,
    /// 3 miss) at RUNTIME, so neither side can constant-fold the path away.
    fn fw(shape: i64, n: i64) -> i64;
    /// The hand-written control over the same table and the same runtime-selected path.
    fn hw(shape: i64, n: i64) -> i64;
    /// The framework dispatch over the depth-matched six-route scaling table.
    fn fw_scale_small(shape: i64, n: i64) -> i64;
    /// The same framework dispatch over a 128-route table — the scaling gate.
    fn fw_big(shape: i64, n: i64) -> i64;
}

type Kernel = unsafe extern "C" fn(i64, i64) -> i64;

fn one_sample(f: Kernel, shape: i64, n: i64) -> f64 {
    let t0 = Instant::now();
    let acc = unsafe { f(shape, n) };
    let ns = t0.elapsed().as_nanos() as f64;
    std::hint::black_box(acc);
    ns / n as f64
}

fn median(xs: &mut [f64]) -> f64 {
    xs.sort_by(|a, b| a.partial_cmp(b).unwrap());
    let mid = xs.len() / 2;
    if xs.len().is_multiple_of(2) {
        (xs[mid - 1] + xs[mid]) / 2.0
    } else {
        xs[mid]
    }
}

/// Measure adjacent pairs, reversing their order every trial. The reported ratio is the median of
/// per-pair ratios, not a ratio of unrelated minima; both arms therefore share instantaneous drift.
fn paired(left: (Kernel, i64), right: (Kernel, i64), n: i64, trials: usize) -> (f64, f64, f64) {
    let mut ls = Vec::with_capacity(trials);
    let mut rs = Vec::with_capacity(trials);
    let mut ratios = Vec::with_capacity(trials);
    for trial in 0..trials {
        let (l, r) = if trial % 2 == 0 {
            (
                one_sample(left.0, left.1, n),
                one_sample(right.0, right.1, n),
            )
        } else {
            let r = one_sample(right.0, right.1, n);
            let l = one_sample(left.0, left.1, n);
            (l, r)
        };
        ls.push(l);
        rs.push(r);
        ratios.push(r / l);
    }
    (median(&mut ls), median(&mut rs), median(&mut ratios))
}

fn main() {
    let n: i64 = std::env::var("N")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(200_000);
    let trials: usize = std::env::var("TRIALS")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(8);
    assert!(n > 0, "N must be positive");
    assert!(
        trials >= 4 && trials.is_multiple_of(2),
        "TRIALS must be even and at least 4"
    );

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
        assert_eq!(
            a, want,
            "framework dispatch for {name} must resolve to route {want}, got {a}"
        );
        assert_eq!(
            b, want,
            "hand-written dispatch for {name} must resolve to route {want}, got {b}"
        );
    }

    // The scaling shapes need the same anchor: a `fw_big` that started missing early would read as
    // an IMPROVED scaling ratio with nothing to catch it.
    for (shape, small_want, big_want) in [
        (0i64, 0i64, 0i64),
        (1, 1, 1),
        (2, 4, 126),
        (4, 5, 127),
        (3, -1, -1),
    ] {
        let (small, big) = unsafe { (fw_scale_small(shape, 1), fw_big(shape, 1)) };
        assert_eq!(
            small, small_want,
            "small scaling shape {shape} must resolve to route {small_want}, got {small}"
        );
        assert_eq!(
            big, big_want,
            "large scaling shape {shape} must resolve to route {big_want}, got {big}"
        );
    }

    // Warm both sides (first-call page-ins, the tree build) before any measurement.
    for (_, shape, _) in shapes {
        unsafe {
            std::hint::black_box(fw(shape, 1000));
            std::hint::black_box(hw(shape, 1000));
        }
    }
    for shape in 0..5 {
        unsafe {
            std::hint::black_box(fw_scale_small(shape, 1000));
            std::hint::black_box(fw_big(shape, 1000));
        }
    }

    println!("dispatches: {n}, median of {trials} adjacent counterbalanced pairs\n");
    println!(
        "  {:<32} {:>12} {:>12} {:>10}",
        "shape", "framework", "hand-written", "ratio"
    );
    let mut worst = 0.0f64;
    for (name, shape, _) in shapes {
        // Each ratio uses adjacent A/B pairs with alternating order, so its two sides share their
        // clock state and neither side always benefits from running first.
        let (b, a, ratio) = paired((hw, shape), (fw, shape), n, trials);
        worst = worst.max(ratio);
        println!("  {name:<32} {a:>9.1} ns {b:>9.1} ns {ratio:>9.2}x");
    }
    println!("\n  worst framework/hand-written ratio: {worst:.2}x");

    // The scaling gate: contract item 3 says dispatch is O(path segments) and FLAT in table size.
    // Compare the SAME runtime paths at the SAME depths over 6 and 128 routes. Head rows isolate
    // fixed depth cost; tail/miss rows expose the remaining per-node sibling scan.
    println!("\n  scaling (contract item 3 — dispatch must be flat in table size)");
    println!(
        "  {:<32} {:>12} {:>12} {:>10}",
        "shape", "6 routes", "128 routes", "ratio"
    );
    let scaling: [(&str, i64); 5] = [
        ("static hit / chain head", 0),
        ("param  hit / chain head", 1),
        ("static hit / chain tail", 2),
        ("param  hit / chain tail", 4),
        ("miss", 3),
    ];
    let mut worst_scale = 0.0f64;
    let mut worst_head_scale = 0.0f64;
    for (name, shape) in scaling {
        let (small, big, ratio) = paired((fw_scale_small, shape), (fw_big, shape), n, trials);
        if shape == 0 || shape == 1 {
            worst_head_scale = worst_head_scale.max(ratio);
        }
        worst_scale = worst_scale.max(ratio);
        println!("  {name:<32} {small:>9.1} ns {big:>9.1} ns {ratio:>9.2}x");
    }
    println!("\n  worst 128-route / 6-route ratio: {worst_scale:.2}x  (1.00x = flat; linear scan would be ~20x)");

    if std::env::var_os("WEB_ROUTER_GATE").is_some() {
        assert!(
            worst_head_scale <= MAX_HEAD_SCALE,
            "web_router head scaling regression: {worst_head_scale:.2}x exceeds {MAX_HEAD_SCALE:.2}x"
        );
        assert!(
            worst_scale <= MAX_ANY_SCALE,
            "web_router scaling regression: {worst_scale:.2}x exceeds {MAX_ANY_SCALE:.2}x"
        );
        println!(
            "  WEB_ROUTER_GATE: PASS (head <= {MAX_HEAD_SCALE:.2}x, every shape <= {MAX_ANY_SCALE:.2}x)"
        );
    }
}
