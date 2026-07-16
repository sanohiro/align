//! Adaptive total-order stable-sort probe (doc-12 §4.1). Measures the three refinements — whole-input
//! ordered early exit, ordered run-boundary straight-copy, and delayed merge-only scratch — by linking
//! **both** the post-change (`after`) and the main-worktree (`before`) Align kernels into one process
//! and comparing them with balanced AB/BA ordering and medians (reported twice).
//!
//! Matrix: six input states (sorted / tail-swap / 1% adjacent swaps / random / reverse / low-16
//! cardinality) × {1_024, 100_000, 1_000_000} `u64`. The gate (per the doc):
//!  - already-sorted, tail-swap, 1% swaps must improve materially;
//!  - random / reverse / low-cardinality must stay within 3% of the before build (no regression).
//!
//! Plus the short-size scratch matrix ({2,8,16,32}), proving via the runtime's `alloc-count` counters
//! that a `len <= 32` sort allocates only the materialize buffer(s) — 1 for plain sort, 2 for
//! `sort_by_key` — after the change, versus 2 / 3 before.
//!
//! This is a MANUAL probe (`./run.sh`), not a CI assertion. All inputs are LCG-generated at runtime.

use std::hint::black_box;
use std::time::Instant;

/// `slice<u64>` = `{ u64* ptr, i64 len }` (must match Align's slice ABI).
#[repr(C)]
#[derive(Clone, Copy)]
struct SliceU64 {
    ptr: *const u64,
    len: i64,
}

/// `str` = `{ u8* ptr, i64 len }`; `slice<str>` = `{ AlignStr* ptr, i64 len }`.
#[repr(C)]
#[derive(Clone, Copy)]
struct AlignStr {
    ptr: *const u8,
    len: i64,
}
#[repr(C)]
#[derive(Clone, Copy)]
struct SliceStr {
    ptr: *const AlignStr,
    len: i64,
}

extern "C" {
    fn sort_u64(xs: SliceU64) -> u64;
    fn sort_u64_before(xs: SliceU64) -> u64;
    fn sort_by_key_u64(xs: SliceU64) -> u64;
    fn sort_by_key_u64_before(xs: SliceU64) -> u64;
    fn sort_str(xs: SliceStr) -> i64;
    fn sort_str_before(xs: SliceStr) -> i64;
    // Present because run.sh builds the runtime with `--features alloc-count`.
    fn align_rt_alloc_count() -> i64;
    fn align_rt_free_count() -> i64;
}

const STATES: [&str; 6] = ["sorted", "tailswap", "onepct", "random", "reverse", "lowcard"];
const SIZES: [usize; 3] = [1_024, 100_000, 1_000_000];

/// Generate the `u64` input for a state. Deterministic; a splitmix64-style LCG for the random states.
fn gen(state: &str, n: usize) -> Vec<u64> {
    let mut v = vec![0u64; n];
    match state {
        "sorted" => {
            for (i, x) in v.iter_mut().enumerate() {
                *x = i as u64;
            }
        }
        "reverse" => {
            for (i, x) in v.iter_mut().enumerate() {
                *x = (n - 1 - i) as u64;
            }
        }
        "tailswap" => {
            for (i, x) in v.iter_mut().enumerate() {
                *x = i as u64;
            }
            if n >= 2 {
                v.swap(n - 1, n - 2);
            }
        }
        "onepct" => {
            for (i, x) in v.iter_mut().enumerate() {
                *x = i as u64;
            }
            // Swap every 100th adjacent pair → ~1% adjacent swaps on a sorted base.
            let mut k = 0;
            while k + 1 < n {
                v.swap(k, k + 1);
                k += 100;
            }
        }
        "random" => {
            let mut s: u64 = 0x9e3779b97f4a7c15;
            for x in v.iter_mut() {
                s = s.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
                *x = s >> 1;
            }
        }
        "lowcard" => {
            let mut s: u64 = 0x123456789abcdef;
            for x in v.iter_mut() {
                s = s.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
                *x = (s >> 33) % 16;
            }
        }
        other => panic!("unknown state {other}"),
    }
    v
}

fn median(v: &mut [f64]) -> f64 {
    v.sort_by(|a, b| a.partial_cmp(b).unwrap());
    v[v.len() / 2]
}

/// Balanced AB/BA timing of `after` vs `before` over `input`, `samples` each. Returns
/// `(after_median_ns, before_median_ns)`.
unsafe fn bench_pair(
    after: unsafe extern "C" fn(SliceU64) -> u64,
    before: unsafe extern "C" fn(SliceU64) -> u64,
    input: &[u64],
    samples: usize,
) -> (f64, f64) {
    let sl = SliceU64 { ptr: input.as_ptr(), len: input.len() as i64 };
    for _ in 0..3 {
        black_box(after(black_box(sl)));
        black_box(before(black_box(sl)));
    }
    let mut a = Vec::with_capacity(samples);
    let mut b = Vec::with_capacity(samples);
    for r in 0..samples {
        if r % 2 == 0 {
            let t = Instant::now();
            black_box(after(black_box(sl)));
            a.push(t.elapsed().as_nanos() as f64);
            let t = Instant::now();
            black_box(before(black_box(sl)));
            b.push(t.elapsed().as_nanos() as f64);
        } else {
            let t = Instant::now();
            black_box(before(black_box(sl)));
            b.push(t.elapsed().as_nanos() as f64);
            let t = Instant::now();
            black_box(after(black_box(sl)));
            a.push(t.elapsed().as_nanos() as f64);
        }
    }
    (median(&mut a), median(&mut b))
}

fn samples_for(n: usize) -> usize {
    match n {
        0..=2_000 => 401,
        2_001..=200_000 => 51,
        _ => 21,
    }
}

fn print_u64_table(kind: &str, after: unsafe extern "C" fn(SliceU64) -> u64, before: unsafe extern "C" fn(SliceU64) -> u64) {
    println!("\n== {kind}: before→after speedup (before_ns / after_ns), medians reported twice ==");
    println!("{:<10} {:>10} {:>14} {:>14} {:>10} {:>10}", "state", "n", "after ns", "before ns", "spd#1", "spd#2");
    for &n in &SIZES {
        let s = samples_for(n);
        for state in STATES {
            let input = gen(state, n);
            // Report the median twice (two independent passes) as a stability check.
            let (a1, b1) = unsafe { bench_pair(after, before, &input, s) };
            let (a2, b2) = unsafe { bench_pair(after, before, &input, s) };
            println!(
                "{:<10} {:>10} {:>14.0} {:>14.0} {:>9.2}x {:>9.2}x",
                state, n, a1, b1, b1 / a1, b2 / a2
            );
        }
    }
}

/// Short-size scratch matrix: prove the `len <= 32` sort allocates only the materialize buffer(s).
fn print_scratch_matrix() {
    println!("\n== short-size scratch matrix: align_rt_alloc delta per call (want after=1 plain / 2 keyed) ==");
    println!("{:<20} {:>6} {:>14} {:>14} {:>14} {:>14}", "kernel", "n", "after allocs", "after frees", "before allocs", "before frees");
    for &n in &[2usize, 8, 16, 32] {
        let input = gen("random", n);
        let sl = SliceU64 { ptr: input.as_ptr(), len: input.len() as i64 };
        // Plain sort.
        let (aa, af) = unsafe { alloc_delta_u64(sort_u64, sl) };
        let (ba, bf) = unsafe { alloc_delta_u64(sort_u64_before, sl) };
        println!("{:<20} {:>6} {:>14} {:>14} {:>14} {:>14}", "sort_u64", n, aa, af, ba, bf);
        // Keyed sort.
        let (aa, af) = unsafe { alloc_delta_u64(sort_by_key_u64, sl) };
        let (ba, bf) = unsafe { alloc_delta_u64(sort_by_key_u64_before, sl) };
        println!("{:<20} {:>6} {:>14} {:>14} {:>14} {:>14}", "sort_by_key_u64", n, aa, af, ba, bf);
    }
}

unsafe fn alloc_delta_u64(f: unsafe extern "C" fn(SliceU64) -> u64, sl: SliceU64) -> (i64, i64) {
    let a0 = align_rt_alloc_count();
    let f0 = align_rt_free_count();
    black_box(f(black_box(sl)));
    (align_rt_alloc_count() - a0, align_rt_free_count() - f0)
}

/// A light `str`-key timing (after vs before) at one mid size, plus one sorted state — the str path
/// is exercised for completeness; the u64 table above is the gated headline.
fn print_str_timing() {
    println!("\n== sort_str (str key): before→after speedup, n=100_000 ==");
    println!("{:<10} {:>10} {:>14} {:>14} {:>10}", "state", "n", "after ns", "before ns", "spd");
    let n = 100_000usize;
    for state in ["sorted", "random", "reverse"] {
        // Build distinct backing strings keyed by the u64 state values so ties/order match.
        let vals = gen(state, n);
        let backing: Vec<String> = vals.iter().map(|v| format!("k{:012}", v % 100_000)).collect();
        let views: Vec<AlignStr> = backing.iter().map(|s| AlignStr { ptr: s.as_ptr(), len: s.len() as i64 }).collect();
        let sl = SliceStr { ptr: views.as_ptr(), len: views.len() as i64 };
        for _ in 0..3 {
            unsafe {
                black_box(sort_str(black_box(sl)));
                black_box(sort_str_before(black_box(sl)));
            }
        }
        let s = 41usize;
        let mut a = Vec::with_capacity(s);
        let mut b = Vec::with_capacity(s);
        for r in 0..s {
            if r % 2 == 0 {
                let t = Instant::now();
                unsafe { black_box(sort_str(black_box(sl))) };
                a.push(t.elapsed().as_nanos() as f64);
                let t = Instant::now();
                unsafe { black_box(sort_str_before(black_box(sl))) };
                b.push(t.elapsed().as_nanos() as f64);
            } else {
                let t = Instant::now();
                unsafe { black_box(sort_str_before(black_box(sl))) };
                b.push(t.elapsed().as_nanos() as f64);
                let t = Instant::now();
                unsafe { black_box(sort_str(black_box(sl))) };
                a.push(t.elapsed().as_nanos() as f64);
            }
        }
        let (am, bm) = (median(&mut a), median(&mut b));
        println!("{:<10} {:>10} {:>14.0} {:>14.0} {:>9.2}x", state, n, am, bm, bm / am);
    }
}

/// Non-interleaved (sequential) measurement: all `after` samples in one uninterrupted block, then all
/// `before` — removes the cross-kernel i-cache/branch-history pollution that in-process AB/BA of two
/// differently-sized kernels introduces. Repeated `reps` times; the min block-median is reported.
unsafe fn bench_sequential(
    after: unsafe extern "C" fn(SliceU64) -> u64,
    before: unsafe extern "C" fn(SliceU64) -> u64,
    input: &[u64],
    samples: usize,
    reps: usize,
) -> (f64, f64) {
    let sl = SliceU64 { ptr: input.as_ptr(), len: input.len() as i64 };
    for _ in 0..5 {
        black_box(after(black_box(sl)));
        black_box(before(black_box(sl)));
    }
    let mut a_best = f64::INFINITY;
    let mut b_best = f64::INFINITY;
    for _ in 0..reps {
        let mut a = Vec::with_capacity(samples);
        for _ in 0..samples {
            let t = Instant::now();
            black_box(after(black_box(sl)));
            a.push(t.elapsed().as_nanos() as f64);
        }
        let mut b = Vec::with_capacity(samples);
        for _ in 0..samples {
            let t = Instant::now();
            black_box(before(black_box(sl)));
            b.push(t.elapsed().as_nanos() as f64);
        }
        a_best = a_best.min(median(&mut a));
        b_best = b_best.min(median(&mut b));
    }
    (a_best, b_best)
}

fn print_sequential_negatives() {
    println!("\n== sort_u64 SEQUENTIAL (non-interleaved) negatives — before/after, min of block-medians ==");
    println!("{:<10} {:>10} {:>14} {:>14} {:>10}", "state", "n", "after ns", "before ns", "spd");
    for &n in &SIZES {
        let s = samples_for(n).min(101);
        for state in ["random", "reverse", "lowcard"] {
            let input = gen(state, n);
            let (a, b) = unsafe { bench_sequential(sort_u64, sort_u64_before, &input, s, 5) };
            println!("{:<10} {:>10} {:>14.0} {:>14.0} {:>9.2}x", state, n, a, b, b / a);
        }
    }
}

fn main() {
    println!("adaptive_sort probe — in-process AB/BA (after = post-change, before = main worktree)");
    print_sequential_negatives();
    print_scratch_matrix();
    print_u64_table("sort_u64 (plain)", sort_u64, sort_u64_before);
    print_u64_table("sort_by_key_u64 (identity key)", sort_by_key_u64, sort_by_key_u64_before);
    print_str_timing();
}
