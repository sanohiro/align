//! Benchmark harness: links the Align kernel object (`alignc emit-obj kernels.align`) and compares
//! each Align kernel against an idiomatic hand-written Rust equivalent on identical runtime data.
//!
//! Methodology (important): the two kernels are timed in **alternating rounds** and we keep the
//! **minimum** per kernel. Timing all of A then all of B over a working set larger than cache gives
//! wildly wrong ratios (the second one runs against a warm-ish cache / settled clocks). Alternate +
//! min is the only honest way. Data is generated at runtime (an LCG) so neither compiler can
//! constant-fold the kernel away.
//!
//! Build + run via `bench/run.sh` (which picks the same `--target-cpu` for `alignc` and `rustc`).

use std::hint::black_box;
use std::time::Instant;

// `slice<i64>` is passed as `{ ptr, len }` by value — SysV puts that in two integer registers,
// matching a `#[repr(C)]` struct argument.
#[repr(C)]
struct Slice {
    ptr: *const i64,
    len: i64,
}

extern "C" {
    fn sum_sq_pos(s: Slice) -> i64; // Align: s.where(pos).map(sq).sum()
    fn col_sum(s: Slice) -> i64; // Align: ps.a.sum() over soa<Particle> (column-major {ptr,len})
}

/// 8-field, 64-byte struct — what idiomatic Rust stores as `Vec<Particle>` (array-of-structs).
#[derive(Clone, Copy)]
struct Particle {
    a: i64,
    b: i64,
    c: i64,
    d: i64,
    e: i64,
    f: i64,
    g: i64,
    h: i64,
}

fn rust_sum_sq_pos(s: &[i64]) -> i64 {
    s.iter().copied().filter(|&x| x > 0).map(|x| x.wrapping_mul(x)).sum()
}

/// Fill `n` i64s with a runtime LCG sequence in roughly [-100, 100] (non-constant-foldable).
fn gen(n: usize) -> Vec<i64> {
    let mut v = vec![0i64; n];
    let mut s: u64 = 0x9E3779B97F4A7C15;
    for d in v.iter_mut() {
        s = s.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
        *d = ((s >> 33) as i64) % 201 - 100;
    }
    v
}

/// Run `align`/`rust` closures in alternating rounds; return their min times in microseconds.
fn duel(rounds: usize, mut align: impl FnMut() -> i64, mut rust: impl FnMut() -> i64) -> (f64, f64) {
    assert_eq!(align(), rust(), "kernels disagree");
    let (mut amin, mut rmin) = (f64::MAX, f64::MAX);
    let mut sink = 0i64;
    for _ in 0..rounds {
        let t = Instant::now();
        sink = sink.wrapping_add(align());
        amin = amin.min(t.elapsed().as_secs_f64() * 1e6);
        let t = Instant::now();
        sink = sink.wrapping_add(rust());
        rmin = rmin.min(t.elapsed().as_secs_f64() * 1e6);
    }
    black_box(sink);
    (amin, rmin)
}

fn report(name: &str, n: usize, amin: f64, rmin: f64) {
    let verdict = if amin < rmin * 0.95 {
        "Align faster"
    } else if amin > rmin * 1.05 {
        "Rust faster"
    } else {
        "= parity"
    };
    println!("  {name:<14} n={n:>9}   align {amin:9.2} us   rust {rmin:9.2} us   ratio {:.3}  ({verdict})", amin / rmin);
}

fn main() {
    println!("Align vs idiomatic Rust (same --target-cpu, alternating-min timing):");
    for &(n, rounds) in &[(100_000usize, 2000usize), (1_000_000, 500), (50_000_000, 15)] {
        let data = gen(n);
        // `black_box` the inputs so the Rust kernel can't be hoisted out of the round loop (the data
        // is loop-invariant) — both sides must do the full work each call.
        let sl = || Slice { ptr: data.as_ptr(), len: n as i64 };
        let (a, r) = duel(rounds, || unsafe { sum_sq_pos(black_box(sl())) }, || rust_sum_sq_pos(black_box(&data)));
        report("sum_sq_pos", n, a, r);
    }

    // 2) Field-subset scan — Align `soa<T>` (column) vs idiomatic Rust `Vec<Struct>` (AoS). Align
    //    reads only the `a` column; idiomatic Rust drags whole 64-byte structs through cache. Align
    //    wins (≈7x on a memory-bound scan) — the layout lever.
    println!("  -- field-subset scan: Align soa<T> column vs Rust Vec<Struct> (AoS) --");
    for &(n, rounds) in &[(1_000_000usize, 500usize), (20_000_000, 20)] {
        let g = gen(n);
        let aos: Vec<Particle> = g.iter().map(|&a| Particle { a, b: 0, c: 0, d: 0, e: 0, f: 0, g: 0, h: 0 }).collect();
        // Align's soa<Particle> is one column-major buffer [all a | all b | … | all h]; the `a`
        // column is [0..n). Only column `a` is read by the kernel, so only it needs real data.
        let mut soa = vec![0i64; 8 * n];
        soa[..n].copy_from_slice(&g);
        let sl = || Slice { ptr: soa.as_ptr(), len: n as i64 };
        let (a, r) = duel(rounds, || unsafe { col_sum(black_box(sl())) }, || black_box(&aos).iter().map(|p| p.a).sum());
        report("col_sum(soa)", n, a, r);
    }
}
