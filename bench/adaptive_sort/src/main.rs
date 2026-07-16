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
#![allow(dead_code)]

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
    fn sort_u64_ctrl(xs: SliceU64) -> u64;
    fn sort_by_key_u64(xs: SliceU64) -> u64;
    fn sort_by_key_u64_before(xs: SliceU64) -> u64;
    fn sort_by_key_u64_ctrl(xs: SliceU64) -> u64;
    fn sort_str(xs: SliceStr) -> i64;
    fn sort_str_before(xs: SliceStr) -> i64;
    fn sort_str_ctrl(xs: SliceStr) -> i64;
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


/// Drift-immune ratio: measure `after` and `other` **adjacent** (same instantaneous CPU frequency —
/// WSL2 has no frequency control, so the block-sequential method is corrupted by ±25% drift between
/// blocks), record `other/after` per adjacent pair, and return the **median ratio** over `pairs`
/// pairs. > 1 ⇒ after faster. The median of adjacent ratios cancels slow-frequency episodes.
unsafe fn ratio_adjacent(
    after: unsafe extern "C" fn(SliceU64) -> u64,
    other: unsafe extern "C" fn(SliceU64) -> u64,
    input: &[u64],
    pairs: usize,
) -> f64 {
    let sl = SliceU64 { ptr: input.as_ptr(), len: input.len() as i64 };
    for _ in 0..5 {
        black_box(after(black_box(sl)));
        black_box(other(black_box(sl)));
    }
    let mut r = Vec::with_capacity(pairs);
    for _ in 0..pairs {
        let t = Instant::now();
        black_box(after(black_box(sl)));
        let ta = t.elapsed().as_nanos() as f64;
        let t = Instant::now();
        black_box(other(black_box(sl)));
        let tb = t.elapsed().as_nanos() as f64;
        r.push(tb / ta);
    }
    median(&mut r)
}

/// Print, per state/size: the real after-vs-before ratio (before = `ALIGN_SORT_ADAPTIVE=off`
/// baseline), the after-vs-ctrl **control** (both the shipped code, so any deviation from 1.00 is
/// pure cross-kernel i-cache/position bias), and the bias-corrected ratio (real / control). The
/// corrected column is the verdict: > 1 ⇒ after (shipped) faster.
fn print_ratio_table(
    kind: &str,
    after: unsafe extern "C" fn(SliceU64) -> u64,
    before: unsafe extern "C" fn(SliceU64) -> u64,
    ctrl: unsafe extern "C" fn(SliceU64) -> u64,
) {
    println!("\n== {kind} drift-immune median-of-adjacent-ratios (after=shipped, before=baseline, ctrl=shipped) ==");
    println!("{:<10} {:>10} {:>10} {:>10} {:>12}", "state", "n", "real", "ctrl", "corrected");
    for &n in &[100_000usize, 1_000_000] {
        let pairs = if n >= 500_000 { 81 } else { 201 };
        for state in STATES {
            let input = gen(state, n);
            let real = unsafe { ratio_adjacent(after, before, &input, pairs) };
            let ctl = unsafe { ratio_adjacent(after, ctrl, &input, pairs) };
            println!("{:<10} {:>10} {:>9.3}x {:>9.3}x {:>11.3}x", state, n, real, ctl, real / ctl);
        }
    }
}

/// Backing strings for `sort_str` — one distinct string per LCG state value; the key is the whole
/// byte-lexicographic string. Returns the storage (keep alive) and the `AlignStr` views.
fn gen_str(state: &str, n: usize) -> (Vec<String>, Vec<AlignStr>) {
    let vals = gen(state, n);
    let backing: Vec<String> = vals.iter().map(|v| format!("k{:012}", v % 100_000)).collect();
    let views: Vec<AlignStr> = backing.iter().map(|s| AlignStr { ptr: s.as_ptr(), len: s.len() as i64 }).collect();
    (backing, views)
}

unsafe fn ratio_adjacent_str(
    after: unsafe extern "C" fn(SliceStr) -> i64,
    other: unsafe extern "C" fn(SliceStr) -> i64,
    views: &[AlignStr],
    pairs: usize,
) -> f64 {
    let sl = SliceStr { ptr: views.as_ptr(), len: views.len() as i64 };
    for _ in 0..5 {
        black_box(after(black_box(sl)));
        black_box(other(black_box(sl)));
    }
    let mut r = Vec::with_capacity(pairs);
    for _ in 0..pairs {
        let t = Instant::now();
        black_box(after(black_box(sl)));
        let ta = t.elapsed().as_nanos() as f64;
        let t = Instant::now();
        black_box(other(black_box(sl)));
        let tb = t.elapsed().as_nanos() as f64;
        r.push(tb / ta);
    }
    median(&mut r)
}

fn print_str_ratio() {
    println!("\n== sort_str (byte-lex key) drift-immune median-of-adjacent-ratios, n=100_000 ==");
    println!("{:<10} {:>10} {:>10} {:>10} {:>12}", "state", "n", "real", "ctrl", "corrected");
    let n = 100_000usize;
    for state in ["sorted", "random", "reverse"] {
        let (_backing, views) = gen_str(state, n);
        let real = unsafe { ratio_adjacent_str(sort_str, sort_str_before, &views, 121) };
        let ctl = unsafe { ratio_adjacent_str(sort_str, sort_str_ctrl, &views, 121) };
        println!("{:<10} {:>10} {:>9.3}x {:>9.3}x {:>11.3}x", state, n, real, ctl, real / ctl);
    }
}

fn main() {
    println!("adaptive_sort probe — after = shipped (w64) shape, before = ALIGN_SORT_ADAPTIVE=off baseline, ctrl = shipped");
    print_scratch_matrix();
    print_ratio_table("sort_u64 (plain)", sort_u64, sort_u64_before, sort_u64_ctrl);
    print_ratio_table("sort_by_key_u64 (identity key)", sort_by_key_u64, sort_by_key_u64_before, sort_by_key_u64_ctrl);
    print_str_ratio();
}
