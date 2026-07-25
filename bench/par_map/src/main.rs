//! par_map duel: Align `s.par_map(work).sum()` (persistent worker pool) vs Rust sequential and Rust
//! `rayon` (work-stealing pool). `bench/par_map/run.sh threshold` separately probes the
//! caller-only/pool boundary with a balanced median ratio after warming the pool.

use rayon::prelude::*;
use std::time::Instant;

#[repr(C)]
#[derive(Clone, Copy)]
struct Slice {
    ptr: *const i64,
    len: i64,
}

extern "C" {
    /// `pub fn pmap_cheap(s: slice<i64>) -> i64` — cheap vectorizable body.
    fn pmap_cheap(s: Slice) -> i64;
    /// `pub fn smap_cheap(s: slice<i64>) -> i64` — sequential cheap body.
    fn smap_cheap(s: Slice) -> i64;
    /// `pub fn pmap(s: slice<i64>) -> i64` — `s.par_map(work).sum()`.
    fn pmap(s: Slice) -> i64;
    /// `pub fn smap(s: slice<i64>) -> i64` — sequential `s.map(work).sum()`.
    fn smap(s: Slice) -> i64;
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
    s.par_iter()
        .map(|&x| work(x))
        .reduce(|| 0i64, i64::wrapping_add)
}

fn gen(n: usize) -> Vec<i64> {
    let mut v = vec![0i64; n];
    let mut s: u64 = 0x9E3779B97F4A7C15;
    for d in v.iter_mut() {
        s = s
            .wrapping_mul(6364136223846793005)
            .wrapping_add(1442695040888963407);
        *d = (s >> 33) as i64;
    }
    v
}

type Kernel = unsafe extern "C" fn(Slice) -> i64;

#[derive(Clone, Copy)]
struct ThresholdCase {
    name: &'static str,
    par: Kernel,
    seq: Kernel,
}

fn call(kernel: Kernel, slice: Slice) -> i64 {
    // The benchmark exports are checked Align functions with the same C ABI; the harness owns the
    // backing Vec for the full duration of every call.
    unsafe { kernel(slice) }
}

fn elapsed_ms(kernel: Kernel, slice: Slice, reps: usize) -> f64 {
    let start = Instant::now();
    for _ in 0..reps {
        std::hint::black_box(call(kernel, slice));
    }
    start.elapsed().as_secs_f64() * 1e3
}

fn percentile(sorted: &[f64], p: f64) -> f64 {
    let index = ((sorted.len() - 1) as f64 * p).round() as usize;
    sorted[index]
}

fn run_standard() {
    let rounds = 50;
    let profile = std::env::var_os("ALIGN_BENCH_PROFILE").is_some();
    println!("par_map(work).sum() — Align (pool) vs Rust sequential / rayon");
    println!(
        "{:>9}  {:>10}  {:>10}  {:>10}  {:>9}  {:>9}",
        "n", "align ms", "seq ms", "rayon ms", "vs seq", "vs rayon"
    );
    for &n in &[1_000usize, 10_000, 100_000, 1_000_000] {
        let data = gen(n);
        let sl = Slice {
            ptr: data.as_ptr(),
            len: n as i64,
        };

        // Correctness: Align (pool, parallel) must equal the sequential fold (no races / lost work).
        let a0 = unsafe {
            pmap(Slice {
                ptr: sl.ptr,
                len: sl.len,
            })
        };
        assert_eq!(a0, rust_seq(&data), "align vs sequential");
        assert_eq!(a0, rust_rayon(&data), "align vs rayon");
        assert_eq!(
            unsafe {
                smap(Slice {
                    ptr: sl.ptr,
                    len: sl.len,
                })
            },
            a0,
            "align sequential vs par_map"
        );

        let (mut am, mut sm, mut rm) = (f64::MAX, f64::MAX, f64::MAX);
        let mut align_seq = f64::MAX;
        for _ in 0..rounds {
            let t = Instant::now();
            std::hint::black_box(unsafe {
                pmap(Slice {
                    ptr: sl.ptr,
                    len: sl.len,
                })
            });
            am = am.min(t.elapsed().as_secs_f64() * 1e3);

            if profile {
                let t = Instant::now();
                std::hint::black_box(unsafe {
                    smap(Slice {
                        ptr: sl.ptr,
                        len: sl.len,
                    })
                });
                align_seq = align_seq.min(t.elapsed().as_secs_f64() * 1e3);
            }

            let t = Instant::now();
            std::hint::black_box(rust_seq(&data));
            sm = sm.min(t.elapsed().as_secs_f64() * 1e3);

            let t = Instant::now();
            std::hint::black_box(rust_rayon(&data));
            rm = rm.min(t.elapsed().as_secs_f64() * 1e3);
        }
        println!(
            "{:>9}  {:>10.3}  {:>10.3}  {:>10.3}  {:>8.2}x  {:>8.2}x",
            n,
            am,
            sm,
            rm,
            sm / am,
            rm / am
        );
        if profile {
            println!(
                "profile n={n}: align-seq {:8.3} ms; pmap is {:5.2}x align-seq",
                align_seq,
                am / align_seq
            );
        }
    }
}

fn run_threshold() {
    // This mirrors the runtime constant intentionally. The probe is the evidence used before a
    // future change to the runtime threshold; it must not silently become a second tuning knob.
    const PAR_MIN_CHUNK: usize = 65_536;
    const ROUNDS: usize = 31;
    let counts = [
        16_384usize,
        32_768,
        32_769,
        49_152,
        65_535,
        65_536,
        65_537,
        73_728,
        81_920,
        98_304,
        131_072,
        196_608,
        262_144,
    ];
    let cases = [
        ThresholdCase {
            name: "cheap",
            par: pmap_cheap,
            seq: smap_cheap,
        },
        ThresholdCase {
            name: "heavy",
            par: pmap,
            seq: smap,
        },
    ];

    // Initialize the persistent pool once, so the table measures the steady-state choice at the
    // boundary. Cold-start behavior is pinned separately by the runtime integration test.
    let warm = gen(PAR_MIN_CHUNK * 2);
    let warm_slice = Slice {
        ptr: warm.as_ptr(),
        len: warm.len() as i64,
    };
    std::hint::black_box(call(pmap, warm_slice));

    println!("par_map threshold probe (warm pool, {ROUNDS} balanced ratio samples)");
    println!("n <= {PAR_MIN_CHUNK}: caller-only; n > {PAR_MIN_CHUNK}: pool eligible");
    println!(
        "{:>9}  {:>8}  {:>18}  {:>18}",
        "n", "case", "median par/seq", "p10..p90"
    );
    for &n in &counts {
        let data = gen(n);
        let slice = Slice {
            ptr: data.as_ptr(),
            len: n as i64,
        };
        // Batch small maps so each timing contains roughly one million body elements. Repeating
        // the same call preserves the per-call scheduler cost while making the clock resolution
        // and transient worker wakeups a small part of each sample.
        let reps = (1_048_576usize / n).max(1);
        for case in cases {
            let expected = call(case.seq, slice);
            assert_eq!(call(case.par, slice), expected, "{} n={n}", case.name);
            // Alternate par→seq and seq→par so the two adjacent timings see both positions in the
            // cycle. The median of ratios keeps correlated frequency drift in the pair.
            let mut ratios = Vec::with_capacity(ROUNDS);
            for round in 0..ROUNDS {
                let (par_ms, seq_ms) = if round % 2 == 0 {
                    (
                        elapsed_ms(case.par, slice, reps),
                        elapsed_ms(case.seq, slice, reps),
                    )
                } else {
                    let seq_ms = elapsed_ms(case.seq, slice, reps);
                    let par_ms = elapsed_ms(case.par, slice, reps);
                    (par_ms, seq_ms)
                };
                ratios.push(par_ms / seq_ms);
            }
            ratios.sort_by(f64::total_cmp);
            let median = percentile(&ratios, 0.5);
            let p10 = percentile(&ratios, 0.1);
            let p90 = percentile(&ratios, 0.9);
            println!(
                "{n:>9}  {:>8}  {median:>18.3}  {p10:.3}..{p90:.3}",
                case.name
            );
        }
    }
}

fn main() {
    if std::env::args().nth(1).as_deref() == Some("threshold") {
        run_threshold();
    } else {
        run_standard();
    }
}
