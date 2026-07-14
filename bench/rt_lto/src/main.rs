//! `--rt-lto` duel: the SAME Align kernel object, timed once built WITHOUT `--rt-lto` and once WITH
//! it (`run.sh` rebuilds the object between passes). This harness only times the kernels and prints
//! machine-parseable `<name> <ns>` lines; `run.sh` runs it against both objects and computes the
//! ratio (OFF / ON, >1 = `--rt-lto` faster).
//!
//! `eq_count` is the constant-length `x == "hello"` filter (the probe's 2.1x win); `sum_sq_pos` is
//! the numeric non-regression control. Data is generated at runtime (an LCG) so nothing folds away.

use std::hint::black_box;
use std::time::Instant;

#[repr(C)]
#[derive(Clone, Copy)]
struct AlignStr {
    ptr: *const u8,
    len: i64,
}

#[repr(C)]
#[derive(Clone, Copy)]
struct StrSlice {
    ptr: *const AlignStr,
    len: i64,
}

#[repr(C)]
#[derive(Clone, Copy)]
struct I64Slice {
    ptr: *const i64,
    len: i64,
}

extern "C" {
    /// `pub fn eq_count(s: slice<str>) -> i64` — count of elements byte-equal to "hello".
    fn eq_count(s: StrSlice) -> i64;
    /// `pub fn sum_sq_pos(s: slice<i64>) -> i64` — the vectorized numeric control.
    fn sum_sq_pos(s: I64Slice) -> i64;
}

/// Generate `n` short strings: ~1 in 8 is "hello" (length 5, matches the fast path), the rest are
/// non-matching runs of lengths 3..=7 (so the length-5 non-matches still reach the byte compare).
/// Returns the backing byte buffer (kept alive) and the `AlignStr` view array pointing into it.
fn gen_strings(n: usize) -> (Vec<u8>, Vec<AlignStr>) {
    let words: [&[u8]; 8] = [
        b"hello", b"abc", b"worl", b"align", b"hi", b"foobar", b"lambda", b"hey",
    ];
    // Flatten all backing bytes into one buffer, recording (offset, len) per element first so the
    // buffer never reallocates under the recorded pointers.
    let mut spans: Vec<(usize, usize)> = Vec::with_capacity(n);
    let mut bytes: Vec<u8> = Vec::new();
    let mut s: u64 = 0x9E3779B97F4A7C15;
    for _ in 0..n {
        s = s.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
        let w = words[((s >> 29) as usize) & 7];
        let off = bytes.len();
        bytes.extend_from_slice(w);
        spans.push((off, w.len()));
    }
    let base = bytes.as_ptr();
    let views: Vec<AlignStr> = spans
        .iter()
        .map(|&(off, len)| AlignStr {
            // SAFETY: `off` is within `bytes`; `bytes` is returned and outlives every use.
            ptr: unsafe { base.add(off) },
            len: len as i64,
        })
        .collect();
    (bytes, views)
}

fn gen_i64(n: usize) -> Vec<i64> {
    let mut v = vec![0i64; n];
    let mut s: u64 = 0xD1B54A32D192ED03;
    for d in v.iter_mut() {
        s = s.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
        *d = ((s >> 33) as i64) % 201 - 100;
    }
    v
}

/// Minimum wall-clock of `rounds` calls to `f` (min = the honest estimator under scheduler noise).
fn bench_min(rounds: usize, mut f: impl FnMut() -> i64) -> u128 {
    let mut best = u128::MAX;
    for _ in 0..rounds {
        let t = Instant::now();
        let r = f();
        let dt = t.elapsed().as_nanos();
        black_box(r);
        best = best.min(dt);
    }
    best
}

fn main() {
    let n: usize = std::env::var("N").ok().and_then(|s| s.parse().ok()).unwrap_or(1_000_000);
    let rounds: usize = std::env::var("ROUNDS").ok().and_then(|s| s.parse().ok()).unwrap_or(300);

    let (_bytes, views) = gen_strings(n);
    let str_slice = StrSlice { ptr: views.as_ptr(), len: views.len() as i64 };
    let nums = gen_i64(n);
    let i64_slice = I64Slice { ptr: nums.as_ptr(), len: nums.len() as i64 };

    // Warm up + sanity.
    let hits = unsafe { eq_count(str_slice) };
    let ctrl = unsafe { sum_sq_pos(i64_slice) };
    eprintln!("# n={n} rounds={rounds} eq_count_hits={hits} sum_sq_pos={ctrl}");

    let eq_ns = bench_min(rounds, || unsafe { eq_count(str_slice) });
    let ss_ns = bench_min(rounds, || unsafe { sum_sq_pos(i64_slice) });

    println!("eq_count {eq_ns}");
    println!("sum_sq_pos {ss_ns}");
}
