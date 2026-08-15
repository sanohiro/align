//! JSON → SoA analytics duel: Align (`json.decode` straight into a column-major `soa<Row>`, then
//! `where(.active).pay.sum()`) vs idiomatic Rust (`serde_json` → `Vec<Row>` AoS → filter/map/sum).
//!
//! The workload touches 2 of 4 fields. Align lands the data column-major and the scan reads only the
//! `active` + `pay` columns; Rust's `serde` deserializes every field into a `Vec<Row>` (AoS) and the
//! filter drags whole 4-field records through cache. Both sides parse the SAME runtime-generated
//! JSON (not a constant, so nothing folds). Rounds alternate and we take the exact integer median.

use serde::Deserialize;
use std::time::Instant;

#[path = "../../json_escape/evidence/statistics.rs"]
mod statistics;

use statistics::{elapsed_nanoseconds, median_microseconds, milliseconds_token};

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
    /// `pub fn agg_len(data: str) -> i64` — decode → `soa<Row>`, return row count.
    fn agg_len(data: AlignStr) -> i64;
    /// `pub fn agg_aos(data: str) -> i64` — decode → `array<Row>` (AoS, no transpose) → same
    /// aggregate. Isolates the transpose cost (vs `agg`) and the parser (vs the Rust baseline).
    fn agg_aos(data: AlignStr) -> i64;
    /// `pub fn agg_aos_len(data: str) -> i64` — decode → `array<Row>`, return row count.
    fn agg_aos_len(data: AlignStr) -> i64;
    /// `pub fn agg_proj(data: str) -> i64` — decode the SAME 4-field JSON into a NARROW `soa<Row2>`
    /// (only `active`+`pay` declared → the decoder skips `score`/`extra`), then the same aggregate.
    /// The projection rail: decode-projection (skip unqueried fields) + columnar scan.
    fn agg_proj(data: AlignStr) -> i64;
    /// `pub fn agg_proj_len(data: str) -> i64` — decode → narrow `soa<Row2>`, return row count.
    fn agg_proj_len(data: AlignStr) -> i64;
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

/// The fair projection baseline: serde into a NARROW 2-field struct — serde skips the two undeclared
/// keys (`score`/`extra`) just as Align's decoder does, so both sides "declare 2, skip 2".
#[derive(Deserialize)]
struct Row2 {
    active: bool,
    pay: i64,
}

fn rust_agg_proj(data: &str) -> i64 {
    let rows: Vec<Row2> = serde_json::from_str(data).expect("valid JSON");
    rows.iter().filter(|r| r.active).map(|r| r.pay).sum()
}

/// Build a JSON array of `n` records with LCG-varied values (so neither parser can constant-fold),
/// ~half `active`. Returns the JSON text and the expected `where(.active).pay.sum()`.
fn gen_json(n: usize) -> (String, i64) {
    use std::fmt::Write;
    let mut s = String::with_capacity(n * 56);
    let mut state: u64 = 0x9e3779b97f4a7c15;
    let mut expected: i64 = 0;
    s.push('[');
    for i in 0..n {
        state = state
            .wrapping_mul(6364136223846793005)
            .wrapping_add(1442695040888963407);
        let pay = ((state >> 33) % 1000) as i64;
        let score = ((state >> 20) % 500) as i64;
        let active = (state >> 40) & 1 == 0;
        if active {
            expected += pay;
        }
        if i > 0 {
            s.push(',');
        }
        // `write!` straight into the buffer — avoids a temporary `String` alloc per record.
        write!(
            s,
            "{{\"active\":{active},\"pay\":{pay},\"score\":{score},\"extra\":{i}}}"
        )
        .unwrap();
    }
    s.push(']');
    (s, expected)
}

fn main() {
    let sizes = [10_000usize, 100_000, 1_000_000];
    let rounds = 30;
    let profile = std::env::var_os("ALIGN_BENCH_PROFILE").is_some();
    println!("JSON decode + where(.active).pay.sum() — Align soa / Align AoS / Align proj (narrow soa) vs serde_json");
    println!(
        "{:>9}  {:>8}  {:>9}  {:>9}  {:>9}  {:>9}  {:>8}  {:>8}  {:>9}",
        "records",
        "json KB",
        "soa ms",
        "aos ms",
        "proj ms",
        "rust ms",
        "soa/rust",
        "aos/rust",
        "proj/rustP"
    );
    for &n in &sizes {
        let (json, expected) = gen_json(n);
        let astr = AlignStr {
            ptr: json.as_ptr(),
            len: json.len() as i64,
        };

        // Correctness: all four must agree with the generator before we trust the timing (the
        // projection variant reads the same two fields, so it must produce the same sum).
        assert_eq!(
            unsafe {
                agg(AlignStr {
                    ptr: astr.ptr,
                    len: astr.len,
                })
            },
            expected,
            "align soa wrong"
        );
        assert_eq!(
            unsafe {
                agg_aos(AlignStr {
                    ptr: astr.ptr,
                    len: astr.len,
                })
            },
            expected,
            "align aos wrong"
        );
        assert_eq!(
            unsafe {
                agg_proj(AlignStr {
                    ptr: astr.ptr,
                    len: astr.len,
                })
            },
            expected,
            "align proj wrong"
        );
        assert_eq!(rust_agg(&json), expected, "rust wrong");
        assert_eq!(rust_agg_proj(&json), expected, "rust proj wrong");

        let (mut soa_ns, mut aos_ns, mut proj_ns, mut rust_ns, mut rustp_ns) = (
            Vec::with_capacity(rounds),
            Vec::with_capacity(rounds),
            Vec::with_capacity(rounds),
            Vec::with_capacity(rounds),
            Vec::with_capacity(rounds),
        );
        for _ in 0..rounds {
            let t = Instant::now();
            std::hint::black_box(unsafe {
                agg(AlignStr {
                    ptr: astr.ptr,
                    len: astr.len,
                })
            });
            soa_ns.push(elapsed_nanoseconds(t.elapsed()).expect("soa duration overflow"));

            let t = Instant::now();
            std::hint::black_box(unsafe {
                agg_aos(AlignStr {
                    ptr: astr.ptr,
                    len: astr.len,
                })
            });
            aos_ns.push(elapsed_nanoseconds(t.elapsed()).expect("aos duration overflow"));

            let t = Instant::now();
            std::hint::black_box(unsafe {
                agg_proj(AlignStr {
                    ptr: astr.ptr,
                    len: astr.len,
                })
            });
            proj_ns.push(elapsed_nanoseconds(t.elapsed()).expect("projection duration overflow"));

            let t = Instant::now();
            std::hint::black_box(rust_agg(&json));
            rust_ns.push(elapsed_nanoseconds(t.elapsed()).expect("Rust duration overflow"));

            let t = Instant::now();
            std::hint::black_box(rust_agg_proj(&json));
            rustp_ns
                .push(elapsed_nanoseconds(t.elapsed()).expect("Rust projection duration overflow"));
        }
        let soa = median_microseconds(&mut soa_ns).expect("soa median overflow");
        let aos = median_microseconds(&mut aos_ns).expect("aos median overflow");
        let proj = median_microseconds(&mut proj_ns).expect("projection median overflow");
        let rust = median_microseconds(&mut rust_ns).expect("Rust median overflow");
        let rustp = median_microseconds(&mut rustp_ns).expect("Rust projection median overflow");
        println!(
            "{:>9}  {:>8}  {:>9}  {:>9}  {:>9}  {:>9}  {:>7.2}x  {:>7.2}x  {:>8.2}x",
            n,
            json.len() / 1024,
            milliseconds_token(soa),
            milliseconds_token(aos),
            milliseconds_token(proj),
            milliseconds_token(rust),
            rust as f64 / soa as f64,
            rust as f64 / aos as f64,
            rustp as f64 / proj as f64
        );

        if profile && n == 1_000_000 {
            assert_eq!(
                unsafe {
                    agg_len(AlignStr {
                        ptr: astr.ptr,
                        len: astr.len,
                    })
                },
                n as i64,
                "align soa len wrong"
            );
            assert_eq!(
                unsafe {
                    agg_aos_len(AlignStr {
                        ptr: astr.ptr,
                        len: astr.len,
                    })
                },
                n as i64,
                "align aos len wrong"
            );
            assert_eq!(
                unsafe {
                    agg_proj_len(AlignStr {
                        ptr: astr.ptr,
                        len: astr.len,
                    })
                },
                n as i64,
                "align proj len wrong"
            );
            let (mut soa_len_ns, mut aos_len_ns, mut proj_len_ns) = (
                Vec::with_capacity(rounds),
                Vec::with_capacity(rounds),
                Vec::with_capacity(rounds),
            );
            for _ in 0..rounds {
                let t = Instant::now();
                std::hint::black_box(unsafe {
                    agg_len(AlignStr {
                        ptr: astr.ptr,
                        len: astr.len,
                    })
                });
                soa_len_ns
                    .push(elapsed_nanoseconds(t.elapsed()).expect("soa profile duration overflow"));

                let t = Instant::now();
                std::hint::black_box(unsafe {
                    agg_aos_len(AlignStr {
                        ptr: astr.ptr,
                        len: astr.len,
                    })
                });
                aos_len_ns
                    .push(elapsed_nanoseconds(t.elapsed()).expect("aos profile duration overflow"));

                let t = Instant::now();
                std::hint::black_box(unsafe {
                    agg_proj_len(AlignStr {
                        ptr: astr.ptr,
                        len: astr.len,
                    })
                });
                proj_len_ns.push(
                    elapsed_nanoseconds(t.elapsed()).expect("projection profile duration overflow"),
                );
            }
            let soa_len =
                median_microseconds(&mut soa_len_ns).expect("soa profile median overflow");
            let aos_len =
                median_microseconds(&mut aos_len_ns).expect("aos profile median overflow");
            let proj_len =
                median_microseconds(&mut proj_len_ns).expect("projection profile median overflow");
            println!("profile 1M:");
            println!(
                "  soa decode-only (4 cols)  {:>8} ms; aggregate delta {:8.3} ms",
                milliseconds_token(soa_len),
                (soa as f64 - soa_len as f64) / 1_000.0
            );
            println!(
                "  aos decode-only           {:>8} ms; aggregate delta {:8.3} ms",
                milliseconds_token(aos_len),
                (aos as f64 - aos_len as f64) / 1_000.0
            );
            println!(
                "  proj decode-only (2 cols) {:>8} ms; aggregate delta {:8.3} ms",
                milliseconds_token(proj_len),
                (proj as f64 - proj_len as f64) / 1_000.0
            );
            println!(
                "  decode-projection saving (soa 4col -> proj 2col) {:8.3} ms",
                (soa_len as f64 - proj_len as f64) / 1_000.0
            );
        }
    }
}
