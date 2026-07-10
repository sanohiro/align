//! M12 Slice A8 — per-request arena reuse benchmark (the measure-first ship gate).
//!
//! The gateway shape: a long-running server loop that resets per request —
//! `loop { arena { …transients… } }`. Each iteration opens an arena, carves a handful of KB-class
//! allocations (a ~2 KiB rendered-template buffer + several small parse-table-ish allocs, ~2.5 KiB
//! total, so one 64 KiB chunk per arena), then ends the arena. We measure per-iteration wall time
//! across five variants of that identical allocation shape:
//!
//!   (a) align arena, pre-pool     — build with `--no-default-features` (pool compiled out)
//!   (b) align arena, POOLED+rezero— the A8 slice (default build)
//!   (c) align arena, POOLED no-rezero — an upper bound; measured via a temporary runtime edit
//!                                       (comment out `chunk.fill(0)` in `arena_pool_take`), then
//!                                       reverted. Not reproducible from this harness alone by design.
//!   (d) Rust `bumpalo` reset loop — the same shape on a keep-largest-chunk bump allocator
//!   (e) plain Rust malloc/free    — the same shape with `Vec` allocations freed each iteration
//!
//! The gate: ship the pool iff (b) >= ~1.15x over (a) on this realistic shape. (a)/(b) come from two
//! `run.sh` invocations with the pool feature on/off; (d)/(e) are feature-independent and print in
//! both runs as a stable sanity check. All numbers are best-of-TRIALS (min — the least-noise
//! estimator for a microbench). Drives the runtime's C-ABI directly, so it times shipped code.

use std::hint::black_box;
use std::time::{Duration, Instant};

use align_runtime::{align_rt_arena_alloc, align_rt_arena_begin, align_rt_arena_end};

/// Per-iteration allocation shape (KB-class, one 64 KiB chunk per arena).
const BIG: usize = 2048; // a rendered-template output buffer
const SMALL: usize = 48; // a parse-table-ish small alloc
const SMALL_COUNT: usize = 8; // several of them

#[cfg(feature = "pool")]
const ARENA_LABEL: &str = "(b) align arena  — POOLED + re-zero";
#[cfg(not(feature = "pool"))]
const ARENA_LABEL: &str = "(a) align arena  — BASELINE (pre-pool)";

/// The align-arena variant ((a) or (b) depending on the compiled `pool` feature). One `arena { … }`
/// per iteration, driving the runtime's C-ABI begin/alloc/end.
fn run_arena(iters: usize) -> (Duration, u64) {
    let mut sum = 0u64;
    let t = Instant::now();
    for i in 0..iters {
        let a = align_rt_arena_begin();
        // The big template buffer — write a byte pattern and fold a couple of bytes into the checksum
        // so the allocation + touch cannot be optimized away.
        let big = unsafe { align_rt_arena_alloc(a, BIG as i64, 8) };
        unsafe {
            core::ptr::write_bytes(big, (i & 0xff) as u8, BIG);
            sum = sum.wrapping_add(*big as u64).wrapping_add(*big.add(BIG - 1) as u64);
        }
        for k in 0..SMALL_COUNT {
            let p = unsafe { align_rt_arena_alloc(a, SMALL as i64, 8) };
            unsafe {
                core::ptr::write_bytes(p, (k & 0xff) as u8, SMALL);
                sum = sum.wrapping_add(*p as u64);
            }
        }
        unsafe { align_rt_arena_end(a) };
    }
    (t.elapsed(), sum)
}

/// (d) The same shape on a `bumpalo` bump allocator reset each iteration (keeps its largest chunk).
fn run_bumpalo(iters: usize) -> (Duration, u64) {
    let mut bump = bumpalo::Bump::new();
    let mut sum = 0u64;
    let t = Instant::now();
    for i in 0..iters {
        bump.reset();
        let big = bump.alloc_slice_fill_copy(BIG, (i & 0xff) as u8);
        sum = sum.wrapping_add(big[0] as u64).wrapping_add(big[BIG - 1] as u64);
        for k in 0..SMALL_COUNT {
            let p = bump.alloc_slice_fill_copy(SMALL, (k & 0xff) as u8);
            sum = sum.wrapping_add(p[0] as u64);
        }
    }
    (t.elapsed(), sum)
}

/// (e) The same shape with plain `Vec` allocations freed each iteration (malloc/free).
// The heap allocation is the point of this baseline — a stack array would defeat it, so the
// `useless_vec` lint (const-sized `vec![…]`) does not apply here.
#[allow(clippy::useless_vec)]
fn run_malloc(iters: usize) -> (Duration, u64) {
    let mut sum = 0u64;
    let t = Instant::now();
    for i in 0..iters {
        let big = vec![(i & 0xff) as u8; BIG];
        sum = sum.wrapping_add(big[0] as u64).wrapping_add(big[BIG - 1] as u64);
        for k in 0..SMALL_COUNT {
            let p = vec![(k & 0xff) as u8; SMALL];
            sum = sum.wrapping_add(p[0] as u64);
        }
        // `big` / the small vecs drop here (free).
    }
    (t.elapsed(), sum)
}

/// Best-of-`trials` per-iteration nanoseconds for `f`. Returns (ns_per_iter, checksum).
fn best_of(trials: usize, iters: usize, mut f: impl FnMut(usize) -> (Duration, u64)) -> (f64, u64) {
    let mut best = Duration::MAX;
    let mut sum = 0u64;
    for _ in 0..trials {
        let (d, s) = f(black_box(iters));
        sum = black_box(s);
        if d < best {
            best = d;
        }
    }
    (best.as_nanos() as f64 / iters as f64, sum)
}

fn main() {
    let iters: usize = std::env::var("ITERS").ok().and_then(|v| v.parse().ok()).unwrap_or(300_000);
    let trials: usize = std::env::var("TRIALS").ok().and_then(|v| v.parse().ok()).unwrap_or(9);

    // Warm up (first-chunk malloc, page faults, the pool priming).
    let _ = run_arena(black_box(10_000));

    let (arena_ns, _) = best_of(trials, iters, run_arena);
    let (bump_ns, _) = best_of(trials, iters, run_bumpalo);
    let (malloc_ns, _) = best_of(trials, iters, run_malloc);

    println!("arena-pool bench — gateway shape ({BIG} B + {SMALL_COUNT}x{SMALL} B per iteration)");
    println!("iterations: {iters}, best of {trials} trials (min ns/iter)\n");
    println!("  {ARENA_LABEL:38} {arena_ns:8.1} ns/iter");
    println!("  (d) rust bumpalo — reset loop          {bump_ns:8.1} ns/iter");
    println!("  (e) rust malloc/free — Vec per iter    {malloc_ns:8.1} ns/iter");
}
