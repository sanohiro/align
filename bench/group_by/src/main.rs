//! group_by duel: Align `s.group_by(.k).sum(.v)` (column-oriented primitive-key hash-aggregate) vs
//! idiomatic Rust grouped sum with `std::collections::HashMap` (SipHash) and a fast `ahash` map.
//!
//! Per the design mandate (and the json→soa lesson): the "beats Rust" claim is only honest against
//! the *fast* baseline (`ahash`), so we time both. We vary the number of distinct groups at a fixed
//! row count — a hash aggregate's cost is dominated by table size / collisions, so the few-groups vs
//! many-groups regimes behave very differently. Rounds alternate; we take the per-kernel min.

use std::collections::HashMap;
use std::time::Instant;

/// Align passes a `soa<KV>` as a `{ ptr, len }` over a column-major buffer (`[all k | all v]`).
#[repr(C)]
struct Soa {
    ptr: *const i64,
    len: i64,
}

extern "C" {
    /// `pub fn group(s: soa<KV>) -> i64` — `group_by(.k).sum(.v)`, returns the distinct-key count.
    fn group(s: Soa) -> i64;
}

/// Build a column-major soa buffer for `n` rows: `[k0..k_{n-1}, v0..v_{n-1}]`, key = LCG % groups.
/// Returns the buffer and the expected distinct-key count.
fn gen(n: usize, groups: i64) -> (Vec<i64>, usize) {
    let mut buf = vec![0i64; 2 * n];
    let mut state: u64 = 0x9e3779b97f4a7c15;
    for i in 0..n {
        state = state.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
        buf[i] = ((state >> 33) % groups as u64) as i64; // key column
        buf[n + i] = ((state >> 20) % 1000) as i64; // value column
    }
    // Distinct keys actually present (≤ groups; for n ≫ groups it's exactly `groups`).
    let distinct = buf[..n].iter().copied().collect::<std::collections::HashSet<_>>().len();
    (buf, distinct)
}

fn rust_std(keys: &[i64], vals: &[i64]) -> usize {
    let mut m: HashMap<i64, i64> = HashMap::new();
    for (k, v) in keys.iter().zip(vals) {
        *m.entry(*k).or_insert(0) += *v;
    }
    m.len()
}

fn rust_ahash(keys: &[i64], vals: &[i64]) -> usize {
    let mut m: HashMap<i64, i64, ahash::RandomState> = HashMap::with_hasher(ahash::RandomState::new());
    for (k, v) in keys.iter().zip(vals) {
        *m.entry(*k).or_insert(0) += *v;
    }
    m.len()
}

fn main() {
    let n = 1_000_000usize;
    let group_counts = [100i64, 10_000, 1_000_000];
    let rounds = 20;
    println!("group_by(.k).sum(.v) over {n} rows — Align vs Rust HashMap (std SipHash / ahash)");
    println!(
        "{:>9}  {:>9}  {:>10}  {:>10}  {:>10}  {:>10}  {:>10}",
        "groups", "distinct", "align ms", "std ms", "ahash ms", "vs std", "vs ahash"
    );
    for &g in &group_counts {
        let (buf, distinct) = gen(n, g);
        let (keys, vals) = buf.split_at(n);
        let soa = Soa { ptr: buf.as_ptr(), len: n as i64 };

        // Correctness: all three agree on the distinct-key count.
        let a0 = unsafe { group(Soa { ptr: soa.ptr, len: soa.len }) } as usize;
        assert_eq!(a0, distinct, "align group count");
        assert_eq!(rust_std(keys, vals), distinct, "std group count");
        assert_eq!(rust_ahash(keys, vals), distinct, "ahash group count");

        let (mut am, mut sm, mut hm) = (f64::MAX, f64::MAX, f64::MAX);
        for _ in 0..rounds {
            let t = Instant::now();
            std::hint::black_box(unsafe { group(Soa { ptr: soa.ptr, len: soa.len }) });
            am = am.min(t.elapsed().as_secs_f64() * 1e3);

            let t = Instant::now();
            std::hint::black_box(rust_std(keys, vals));
            sm = sm.min(t.elapsed().as_secs_f64() * 1e3);

            let t = Instant::now();
            std::hint::black_box(rust_ahash(keys, vals));
            hm = hm.min(t.elapsed().as_secs_f64() * 1e3);
        }
        println!(
            "{:>9}  {:>9}  {:>10.3}  {:>10.3}  {:>10.3}  {:>9.2}x  {:>9.2}x",
            g, distinct, am, sm, hm, sm / am, hm / am
        );
    }
}
