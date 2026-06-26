//! JSON → SoA analytics duel: Align (`json.decode` straight into a column-major `soa<Row>`, then
//! `where(.active).pay.sum()`) vs idiomatic Rust (`serde_json` → `Vec<Row>` AoS → filter/map/sum).
//!
//! The workload touches 2 of 4 fields. Align lands the data column-major and the scan reads only the
//! `active` + `pay` columns; Rust's `serde` deserializes every field into a `Vec<Row>` (AoS) and the
//! filter drags whole 4-field records through cache. Both sides parse the SAME runtime-generated
//! JSON (not a constant, so nothing folds). Rounds alternate and we take the min (the standard trap:
//! never time all of A then all of B over a >cache working set — see `bench/README.md`).

use serde::Deserialize;
use std::time::Instant;

/// Align passes a `str` as a `{ ptr, len }` value (SysV two-register), matching this `repr(C)`.
#[repr(C)]
struct AlignStr {
    ptr: *const u8,
    len: i64,
}

extern "C" {
    /// `pub fn agg(data: str) -> i64` — decode → `soa<Row>` → `where(.active).pay.sum()`, or -1 on
    /// a parse error.
    fn agg(data: AlignStr) -> i64;
}

// `score`/`extra` are deserialized for fidelity (a fair 4-field record) but not read by the
// aggregate — the realistic "decode the whole record, use a few fields" analytics shape.
#[derive(Deserialize)]
#[allow(dead_code)]
struct Row {
    active: bool,
    pay: i64,
    score: i64,
    extra: i64,
}

/// Idiomatic Rust: deserialize the whole array into a `Vec<Row>` (AoS), then filter + sum.
fn rust_agg(data: &str) -> i64 {
    let rows: Vec<Row> = serde_json::from_str(data).expect("valid JSON");
    rows.iter().filter(|r| r.active).map(|r| r.pay).sum()
}

/// Build a JSON array of `n` records with LCG-varied values (so neither parser can constant-fold),
/// ~half `active`. Returns the JSON text and the expected `where(.active).pay.sum()`.
fn gen_json(n: usize) -> (String, i64) {
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
        // Field order varied a little would also exercise the perfect-hash; keep it fixed + realistic.
        s.push_str(&format!(
            "{{\"active\":{active},\"pay\":{pay},\"score\":{score},\"extra\":{i}}}"
        ));
    }
    s.push(']');
    (s, expected)
}

fn main() {
    let sizes = [10_000usize, 100_000, 1_000_000];
    let rounds = 30;
    println!("JSON → SoA analytics: Align (decode→soa→where(.active).pay.sum) vs Rust (serde_json→Vec→filter/sum)");
    println!("{:>10}  {:>10}  {:>12}  {:>12}  {:>7}", "records", "json KB", "align ms", "rust ms", "speedup");
    for &n in &sizes {
        let (json, expected) = gen_json(n);
        let astr = AlignStr { ptr: json.as_ptr(), len: json.len() as i64 };

        // Correctness: both must agree with the generator before we trust the timing.
        let a0 = unsafe { agg(AlignStr { ptr: json.as_ptr(), len: json.len() as i64 }) };
        let r0 = rust_agg(&json);
        assert_eq!(a0, expected, "align result wrong");
        assert_eq!(r0, expected, "rust result wrong");

        let (mut align_min, mut rust_min) = (f64::MAX, f64::MAX);
        for _ in 0..rounds {
            let t = Instant::now();
            let av = unsafe { agg(AlignStr { ptr: astr.ptr, len: astr.len }) };
            let ad = t.elapsed().as_secs_f64() * 1e3;
            std::hint::black_box(av);
            align_min = align_min.min(ad);

            let t = Instant::now();
            let rv = rust_agg(&json);
            let rd = t.elapsed().as_secs_f64() * 1e3;
            std::hint::black_box(rv);
            rust_min = rust_min.min(rd);
        }
        println!(
            "{:>10}  {:>10}  {:>12.3}  {:>12.3}  {:>6.2}x",
            n,
            json.len() / 1024,
            align_min,
            rust_min,
            rust_min / align_min
        );
    }
}
