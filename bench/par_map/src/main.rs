//! par_map duel: Align `s.par_map(work).sum()` (persistent worker pool) vs Rust sequential and Rust
//! `rayon` (work-stealing pool). `work` is a moderately heavy per-element function so parallelism
//! can actually pay off. We vary N down to small sizes — the old per-call OS-thread spawn made
//! small `par_map` far slower than sequential; the pool should fix that.

use rayon::prelude::*;
use std::time::Instant;

#[repr(C)]
#[derive(Clone, Copy)]
struct Slice {
    ptr: *const i64,
    len: i64,
}

extern "C" {
    /// `pub fn pmap(s: slice<i64>) -> i64` — `s.par_map(work).sum()`.
    fn pmap(s: Slice) -> i64;
}

/// Must match the Align kernel's `work` (wrapping arithmetic = Align's defined i64 overflow).
#[inline]
fn work(x: i64) -> i64 {
    let mut a = x;
    a = a.wrapping_mul(2654435761).wrapping_add(12345);
    a = a.wrapping_mul(a).wrapping_add(7);
    a = a.wrapping_mul(40503).wrapping_sub(99);
    a
}

fn rust_seq(s: &[i64]) -> i64 {
    s.iter().map(|&x| work(x)).fold(0i64, i64::wrapping_add)
}

fn rust_rayon(s: &[i64]) -> i64 {
    s.par_iter().map(|&x| work(x)).reduce(|| 0i64, i64::wrapping_add)
}

fn gen(n: usize) -> Vec<i64> {
    let mut v = vec![0i64; n];
    let mut s: u64 = 0x9E3779B97F4A7C15;
    for d in v.iter_mut() {
        s = s.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
        *d = (s >> 33) as i64;
    }
    v
}

fn main() {
    let rounds = 50;
    println!("par_map(work).sum() — Align (pool) vs Rust sequential / rayon");
    println!("{:>9}  {:>10}  {:>10}  {:>10}  {:>9}  {:>9}", "n", "align ms", "seq ms", "rayon ms", "vs seq", "vs rayon");
    for &n in &[1_000usize, 10_000, 100_000, 1_000_000] {
        let data = gen(n);
        let sl = Slice { ptr: data.as_ptr(), len: n as i64 };

        // Correctness: Align (pool, parallel) must equal the sequential fold (no races / lost work).
        let a0 = unsafe { pmap(Slice { ptr: sl.ptr, len: sl.len }) };
        assert_eq!(a0, rust_seq(&data), "align vs sequential");
        assert_eq!(a0, rust_rayon(&data), "align vs rayon");

        let (mut am, mut sm, mut rm) = (f64::MAX, f64::MAX, f64::MAX);
        for _ in 0..rounds {
            let t = Instant::now();
            std::hint::black_box(unsafe { pmap(Slice { ptr: sl.ptr, len: sl.len }) });
            am = am.min(t.elapsed().as_secs_f64() * 1e3);

            let t = Instant::now();
            std::hint::black_box(rust_seq(&data));
            sm = sm.min(t.elapsed().as_secs_f64() * 1e3);

            let t = Instant::now();
            std::hint::black_box(rust_rayon(&data));
            rm = rm.min(t.elapsed().as_secs_f64() * 1e3);
        }
        println!("{:>9}  {:>10.3}  {:>10.3}  {:>10.3}  {:>8.2}x  {:>8.2}x", n, am, sm, rm, sm / am, rm / am);
    }
}
