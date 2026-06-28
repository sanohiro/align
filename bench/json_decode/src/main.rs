//! JSON decode-throughput duel: Align `json.decode` vs idiomatic Rust `serde_json`.
//!
//! Two Align shapes, both folding `where(.active).pay.sum()` so the parser dominates:
//!   - `decode_full` — decode all 4 fields into `array<Full>`.
//!   - `decode_proj` — decode only `active`+`pay` into `array<Proj>`, the decoder skipping the
//!     undeclared `score`/`extra` (the projection rail).
//! Rust baselines mirror each: `serde_json` into a `Vec<Full4>` (all fields) and a `Vec<Proj2>`
//! (serde ignores unknown fields by default — the same projection).
//!
//! This is the regression tracker for the parser rewrite (recursive-descent → two-stage SIMD): run
//! it before/after each change and watch the `align/serde` ratios. Both sides parse the SAME
//! runtime-generated JSON (not a constant). Rounds alternate; we take the per-kernel min.

use serde::Deserialize;
use std::time::Instant;

/// Align passes a `str` as a `{ ptr, len }` value (SysV two-register), matching this `repr(C)`.
#[derive(Clone, Copy)]
#[repr(C)]
struct AlignStr {
    ptr: *const u8,
    len: i64,
}

extern "C" {
    /// `pub fn decode_full(d: str) -> i64` — decode all 4 fields → `where(.active).pay.sum()`.
    fn decode_full(data: AlignStr) -> i64;
    /// `pub fn decode_proj(d: str) -> i64` — decode 2 fields (skip score/extra) → same aggregate.
    fn decode_proj(data: AlignStr) -> i64;
}

#[derive(Deserialize)]
#[allow(dead_code)]
struct Full4 {
    active: bool,
    pay: i64,
    score: i64,
    extra: i64,
}

#[derive(Deserialize)]
struct Proj2 {
    active: bool,
    pay: i64,
}

fn rust_full(data: &str) -> i64 {
    let rows: Vec<Full4> = serde_json::from_str(data).expect("valid JSON");
    rows.iter().filter(|r| r.active).map(|r| r.pay).sum()
}

fn rust_proj(data: &str) -> i64 {
    // serde ignores unknown fields by default, so this skips score/extra — the same projection.
    let rows: Vec<Proj2> = serde_json::from_str(data).expect("valid JSON");
    rows.iter().filter(|r| r.active).map(|r| r.pay).sum()
}

/// Build a JSON array of `n` 4-field records with LCG-varied values (nothing folds), ~half active.
/// Returns the JSON text and the expected `where(.active).pay.sum()`.
fn gen_json(n: usize) -> (String, i64) {
    use std::fmt::Write;
    let mut s = String::with_capacity(n * 56);
    let mut state: u64 = 0x9e3779b97f4a7c15;
    let mut expected: i64 = 0;
    s.push('[');
    for i in 0..n {
        state = state.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
        let pay = ((state >> 33) % 1000) as i64;
        let score = ((state >> 20) % 500) as i64;
        let active = (state >> 40) & 1 == 0;
        if active {
            expected += pay;
        }
        if i > 0 {
            s.push(',');
        }
        write!(s, "{{\"active\":{active},\"pay\":{pay},\"score\":{score},\"extra\":{i}}}").unwrap();
    }
    s.push(']');
    (s, expected)
}

fn main() {
    let sizes = [10_000usize, 100_000, 1_000_000];
    let rounds = 40;
    println!("JSON decode throughput — Align json.decode vs serde_json (both fold where(.active).pay.sum())");
    println!(
        "{:>9} {:>8} | {:>9} {:>9} {:>9} | {:>9} {:>9} {:>9}",
        "records", "json KB", "A-full", "rs-full", "full×", "A-proj", "rs-proj", "proj×"
    );
    for &n in &sizes {
        let (json, expected) = gen_json(n);
        let astr = AlignStr { ptr: json.as_ptr(), len: json.len() as i64 };

        // Correctness: every path agrees with the generator before we trust the timing.
        assert_eq!(unsafe { decode_full(astr) }, expected, "align full wrong");
        assert_eq!(unsafe { decode_proj(astr) }, expected, "align proj wrong");
        assert_eq!(rust_full(&json), expected, "rust full wrong");
        assert_eq!(rust_proj(&json), expected, "rust proj wrong");

        let (mut af, mut ap, mut rf, mut rp) = (f64::MAX, f64::MAX, f64::MAX, f64::MAX);
        for _ in 0..rounds {
            let t = Instant::now();
            std::hint::black_box(unsafe { decode_full(astr) });
            af = af.min(t.elapsed().as_secs_f64() * 1e3);

            let t = Instant::now();
            std::hint::black_box(rust_full(&json));
            rf = rf.min(t.elapsed().as_secs_f64() * 1e3);

            let t = Instant::now();
            std::hint::black_box(unsafe { decode_proj(astr) });
            ap = ap.min(t.elapsed().as_secs_f64() * 1e3);

            let t = Instant::now();
            std::hint::black_box(rust_proj(&json));
            rp = rp.min(t.elapsed().as_secs_f64() * 1e3);
        }
        println!(
            "{:>9} {:>8} | {:>8.3} {:>8.3} {:>8.2}x | {:>8.3} {:>8.3} {:>8.2}x",
            n,
            json.len() / 1024,
            af,
            rf,
            rf / af,
            ap,
            rp,
            rp / ap
        );
    }
}
