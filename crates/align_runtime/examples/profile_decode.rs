//! Profiling harness for `align_rt_json_decode_struct_array` — isolates the two-stage decode so a
//! sampling profiler (`sample`/dtrace) can attribute the full-vs-proj cost. Mirrors the
//! `bench/json_decode` workload (same generator, same Full/Proj shapes) but loops one kernel for a
//! fixed iteration count and links the runtime directly (full Rust symbols, no Align kernel).
//!
//!   cargo run --release --example profile_decode -- <full|proj> <records> <iters>

use align_runtime::{align_rt_free, align_rt_json_decode_struct_array, AlignStr, JsonField};
use std::time::Instant;

/// The canonical `wyhash`, seeded — the same `align_hash::wyhash` the runtime's `json_phf_hash` and
/// codegen's `build_phf` call, so the table this harness builds routes identically to a real decode.
fn phf_hash(bytes: &[u8], seed: u64) -> u64 {
    align_hash::wyhash(bytes, seed)
}

/// Brute-force a collision-free perfect-hash table (slot → field index, -1 empty) over `names`,
/// same shape codegen builds: a power-of-two table, one name compare confirms a hit. Returns
/// `(table, seed)`.
fn build_phf(names: &[&[u8]]) -> (Vec<i32>, u64) {
    let mut size = names.len().next_power_of_two().max(2);
    loop {
        for seed in 0u64..1_000_000 {
            let mut table = vec![-1i32; size];
            let mut ok = true;
            for (i, n) in names.iter().enumerate() {
                let slot = (phf_hash(n, seed) & (size as u64 - 1)) as usize;
                if table[slot] != -1 {
                    ok = false;
                    break;
                }
                table[slot] = i as i32;
            }
            if ok {
                return (table, seed);
            }
        }
        size *= 2; // no collision-free seed at this size — widen the table
    }
}

/// Same JSON the bench generates: `n` 4-field records, ~half active, nothing folds.
fn gen_json(n: usize) -> String {
    use std::fmt::Write;
    let mut s = String::with_capacity(n * 56);
    let mut state: u64 = 0x9e3779b97f4a7c15;
    s.push('[');
    for i in 0..n {
        state = state.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
        let pay = ((state >> 33) % 1000) as i64;
        let score = ((state >> 20) % 500) as i64;
        let active = (state >> 40) & 1 == 0;
        if i > 0 {
            s.push(',');
        }
        write!(s, "{{\"active\":{active},\"pay\":{pay},\"score\":{score},\"extra\":{i}}}").unwrap();
    }
    s.push(']');
    s
}

fn field(name: &'static [u8], tag: i32, offset: i64) -> JsonField {
    JsonField { name_ptr: name.as_ptr(), name_len: name.len() as i64, tag, offset, sub: core::ptr::null(), opt_tag: -1 }
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let shape = args.get(1).map(|s| s.as_str()).unwrap_or("full");
    let records: usize = args.get(2).and_then(|s| s.parse().ok()).unwrap_or(1_000_000);
    let iters: usize = args.get(3).and_then(|s| s.parse().ok()).unwrap_or(200);

    let json = gen_json(records);
    let src = AlignStr { ptr: json.as_ptr(), len: json.len() as i64 };

    // tag = (kind << 8) | width: bool = (1<<8)|1 = 257, i64 = (0<<8)|8 = 8.
    const BOOL: i32 = (1 << 8) | 1;
    const I64: i32 = 8;
    let (descs, names, esz): (Vec<JsonField>, Vec<&[u8]>, i64) = match shape {
        "proj" => (
            vec![field(b"active", BOOL, 0), field(b"pay", I64, 8)],
            vec![&b"active"[..], &b"pay"[..]],
            16,
        ),
        _ => (
            vec![field(b"active", BOOL, 0), field(b"pay", I64, 8), field(b"score", I64, 16), field(b"extra", I64, 24)],
            vec![&b"active"[..], &b"pay"[..], &b"score"[..], &b"extra"[..]],
            32,
        ),
    };
    let (phf, seed) = build_phf(&names);

    let decode = || -> (i64, *mut u8) {
        let mut out = AlignStr { ptr: std::ptr::null(), len: 0 };
        let rc = unsafe {
            align_rt_json_decode_struct_array(
                src.ptr,
                src.len,
                descs.as_ptr(),
                descs.len() as i64,
                esz,
                &mut out,
                phf.as_ptr(),
                phf.len() as i64,
                seed as i64,
            )
        };
        assert_eq!(rc, 0, "decode failed");
        (out.len, out.ptr as *mut u8)
    };

    // Warm up + correctness sanity (count == records).
    let (count, ptr) = decode();
    assert_eq!(count as usize, records, "decoded count mismatch");
    unsafe { align_rt_free(ptr) };

    eprintln!("profile_decode: shape={shape} records={records} iters={iters} json={} KB phf_size={} seed={seed}", json.len() / 1024, phf.len());

    let t = Instant::now();
    let mut sink = 0i64;
    for _ in 0..iters {
        let (count, ptr) = decode();
        sink = sink.wrapping_add(count);
        unsafe { align_rt_free(ptr) }; // free the materialized array each round (else it leaks)
    }
    let secs = t.elapsed().as_secs_f64();
    std::hint::black_box(sink);
    eprintln!("profile_decode: {iters} iters in {secs:.3}s → {:.3} ms/iter", secs / iters as f64 * 1e3);
}
