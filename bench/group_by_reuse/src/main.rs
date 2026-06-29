//! A2 dictionary-reuse duel: multi-aggregation over a string key. Four grouped aggregates
//! (sum .a, sum .b, max .c, min .d) keyed by the same `.name` column of an AoS `array<Row>`.
//!
//! - **a1** (Align, naive): four independent `group_by(.name)` calls — the runtime re-interns the
//!   string key column four times (one string hash per row per aggregate).
//! - **a2** (Align, reuse): `dict_encode(.name)` interns the key column *once* into a dense-id
//!   column, then the four aggregates run on integer ids. The string hashing is amortized.
//! - **Rust** (the honest baseline): the same four aggregates, each a `HashMap<&str, _>` built from
//!   scratch (re-hashing the string keys four times) — with both `std` SipHash and fast `ahash`.
//!
//! Per the design mandate (and the json→soa lesson): the "reuse wins" claim is only honest against
//! the *fast* baseline (`ahash`), so we time it too. We vary the distinct-group count at a fixed row
//! count and take the per-kernel min over many rounds. The headline numbers are **a1/a2** (the reuse
//! speedup within Align) and **ahash/a2** (vs idiomatic fast Rust).

use std::collections::HashMap;
use std::hash::BuildHasher;
use std::time::Instant;

/// `str` in Align is a `{ u8* ptr, i64 len }` view — must match the runtime's `AlignStr`.
#[repr(C)]
#[derive(Clone, Copy)]
struct AlignStr {
    ptr: *const u8,
    len: i64,
}

/// `Row { name: str, a: i64, b: i64, c: i64, d: i64 }` — AoS, declared-order C layout.
#[repr(C)]
#[derive(Clone, Copy)]
struct Row {
    name: AlignStr,
    a: i64,
    b: i64,
    c: i64,
    d: i64,
}

/// An `array<Row>` is passed (and returned) as a `{ Row* ptr, i64 len }`.
#[repr(C)]
#[derive(Clone, Copy)]
struct Slice {
    ptr: *const Row,
    len: i64,
}

extern "C" {
    /// `pub fn a1(us: array<Row>) -> array<Row>` — four naive str-key group_bys; threads `us` back.
    fn a1(us: Slice) -> Slice;
    /// `pub fn a2(us: array<Row>) -> array<Row>` — dict_encode once, four reused id group_bys; threads back.
    fn a2(us: Slice) -> Slice;
    fn a2_encode(us: Slice) -> Slice;
    fn a2_one(us: Slice) -> Slice;
    fn a2_two(us: Slice) -> Slice;
    fn a2_three(us: Slice) -> Slice;
}

/// Build `groups` distinct string keys (kept alive by the returned `Vec<String>`) and `n` rows whose
/// `.name` borrows them (key = LCG % groups). Returns the key storage, the AoS rows, the per-row group
/// index + the four value columns (for the Rust baseline), and the distinct-key count.
#[allow(clippy::type_complexity)]
fn gen(n: usize, groups: usize) -> (Vec<String>, Vec<Row>, Vec<usize>, [Vec<i64>; 4], usize) {
    let keys: Vec<String> = (0..groups).map(|g| format!("key_{g:06}")).collect();
    let mut rows = Vec::with_capacity(n);
    let mut gidx = Vec::with_capacity(n);
    let (mut ca, mut cb, mut cc, mut cd) = (Vec::with_capacity(n), Vec::with_capacity(n), Vec::with_capacity(n), Vec::with_capacity(n));
    let mut state: u64 = 0x9e3779b97f4a7c15;
    for _ in 0..n {
        state = state.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
        let g = (state >> 33) as usize % groups;
        let (a, b, c, d) = (
            ((state >> 20) % 1000) as i64,
            ((state >> 24) % 1000) as i64,
            ((state >> 28) % 1000) as i64,
            ((state >> 32) % 1000) as i64,
        );
        rows.push(Row { name: AlignStr { ptr: keys[g].as_ptr(), len: keys[g].len() as i64 }, a, b, c, d });
        gidx.push(g);
        ca.push(a);
        cb.push(b);
        cc.push(c);
        cd.push(d);
    }
    let distinct = gidx.iter().copied().collect::<std::collections::HashSet<_>>().len();
    (keys, rows, gidx, [ca, cb, cc, cd], distinct)
}

/// The Rust baseline: the same four aggregates (sum a, sum b, max c, min d), each a fresh
/// `HashMap<&str, i64>` keyed by the string — so the string keys are hashed four times (the cost the
/// A2 rail amortizes). Generic over the hasher so we can run std SipHash and fast ahash. Returns the
/// summed distinct counts (= 4 × group count), matching the Align kernels.
fn rust_multi<S: BuildHasher + Default>(keys: &[String], gidx: &[usize], cols: &[Vec<i64>; 4]) -> usize {
    let mut total = 0usize;
    // sum .a
    let mut ma: HashMap<&str, i64, S> = HashMap::default();
    for (i, &g) in gidx.iter().enumerate() {
        *ma.entry(keys[g].as_str()).or_insert(0) += cols[0][i];
    }
    total += ma.len();
    // sum .b
    let mut mb: HashMap<&str, i64, S> = HashMap::default();
    for (i, &g) in gidx.iter().enumerate() {
        *mb.entry(keys[g].as_str()).or_insert(0) += cols[1][i];
    }
    total += mb.len();
    // max .c
    let mut mc: HashMap<&str, i64, S> = HashMap::default();
    for (i, &g) in gidx.iter().enumerate() {
        let e = mc.entry(keys[g].as_str()).or_insert(i64::MIN);
        *e = (*e).max(cols[2][i]);
    }
    total += mc.len();
    // min .d
    let mut md: HashMap<&str, i64, S> = HashMap::default();
    for (i, &g) in gidx.iter().enumerate() {
        let e = md.entry(keys[g].as_str()).or_insert(i64::MAX);
        *e = (*e).min(cols[3][i]);
    }
    total += md.len();
    total
}

/// The *smart* Rust baseline — the true competition for A2. A single pass building one
/// `HashMap<&str, [i64; 4]>`: the string key is hashed **once** per row (like A2's `dict_encode`),
/// and all four aggregates update in that one probe. Returns 4 × the distinct count (one map, counted
/// four times) to match the others' checksum.
fn rust_single<S: BuildHasher + Default>(keys: &[String], gidx: &[usize], cols: &[Vec<i64>; 4]) -> usize {
    let mut m: HashMap<&str, [i64; 4], S> = HashMap::default();
    for (i, &g) in gidx.iter().enumerate() {
        let e = m.entry(keys[g].as_str()).or_insert([0, 0, i64::MIN, i64::MAX]);
        e[0] += cols[0][i];
        e[1] += cols[1][i];
        e[2] = e[2].max(cols[2][i]);
        e[3] = e[3].min(cols[3][i]);
    }
    4 * m.len()
}

fn main() {
    let n = 1_000_000usize;
    let group_counts = [100usize, 10_000, 1_000_000];
    let rounds = 20;
    let profile = std::env::var_os("ALIGN_BENCH_PROFILE").is_some();
    println!("4 aggregates (sum a, sum b, max c, min d) over {n} rows by a str key — Align reuse vs Rust");
    println!("  a1 = Align naive (4 str group_bys)   a2 = Align dict_encode reuse");
    println!("  naive = Rust 4× HashMap<&str>   smart = Rust 1-pass HashMap<&str,[i64;4]> (ahash, hashes once)");
    println!(
        "{:>8}  {:>8}  {:>9}  {:>9}  {:>9}  {:>9}  {:>8}  {:>9}",
        "groups", "distinct", "a1 ms", "a2 ms", "naive ms", "smart ms", "a1/a2", "smart/a2"
    );
    for &g in &group_counts {
        let (keys, rows, gidx, cols, distinct) = gen(n, g);
        let input = Slice { ptr: rows.as_ptr(), len: n as i64 };
        let want = 4 * distinct;

        // Correctness: the Rust baselines compute 4 × the distinct-key count; the Align kernels thread
        // `us` back unchanged (a liveness check that the call ran without corrupting the buffer — the
        // aggregate *values* are proven by the merged `dict_encode_reuse_matches_a1_string_group_by`).
        assert_eq!(rust_multi::<ahash::RandomState>(&keys, &gidx, &cols), want, "naive count");
        assert_eq!(rust_single::<ahash::RandomState>(&keys, &gidx, &cols), want, "smart count");
        let r1 = unsafe { a1(input) };
        assert!(r1.ptr == input.ptr && r1.len == input.len, "a1 threaded the array back unchanged");
        let r2 = unsafe { a2(input) };
        assert!(r2.ptr == input.ptr && r2.len == input.len, "a2 threaded the array back unchanged");

        let (mut t1, mut t2, mut tn, mut tsm) = (f64::MAX, f64::MAX, f64::MAX, f64::MAX);
        for _ in 0..rounds {
            let t = Instant::now();
            std::hint::black_box(unsafe { a1(std::hint::black_box(input)) });
            t1 = t1.min(t.elapsed().as_secs_f64() * 1e3);

            let t = Instant::now();
            std::hint::black_box(unsafe { a2(std::hint::black_box(input)) });
            t2 = t2.min(t.elapsed().as_secs_f64() * 1e3);

            let t = Instant::now();
            std::hint::black_box(rust_multi::<ahash::RandomState>(&keys, &gidx, &cols));
            tn = tn.min(t.elapsed().as_secs_f64() * 1e3);

            let t = Instant::now();
            std::hint::black_box(rust_single::<ahash::RandomState>(&keys, &gidx, &cols));
            tsm = tsm.min(t.elapsed().as_secs_f64() * 1e3);
        }
        println!(
            "{:>8}  {:>8}  {:>9.3}  {:>9.3}  {:>9.3}  {:>9.3}  {:>7.2}x  {:>8.2}x",
            g, distinct, t1, t2, tn, tsm, t1 / t2, tsm / t2
        );

        if profile && g == 1_000_000 {
            for f in [
                unsafe { a2_encode(input) },
                unsafe { a2_one(input) },
                unsafe { a2_two(input) },
                unsafe { a2_three(input) },
            ] {
                assert!(f.ptr == input.ptr && f.len == input.len, "profile function threaded the array back unchanged");
            }
            let (mut te, mut t_one, mut t_two, mut t_three) = (f64::MAX, f64::MAX, f64::MAX, f64::MAX);
            for _ in 0..rounds {
                let t = Instant::now();
                std::hint::black_box(unsafe { a2_encode(std::hint::black_box(input)) });
                te = te.min(t.elapsed().as_secs_f64() * 1e3);

                let t = Instant::now();
                std::hint::black_box(unsafe { a2_one(std::hint::black_box(input)) });
                t_one = t_one.min(t.elapsed().as_secs_f64() * 1e3);

                let t = Instant::now();
                std::hint::black_box(unsafe { a2_two(std::hint::black_box(input)) });
                t_two = t_two.min(t.elapsed().as_secs_f64() * 1e3);

                let t = Instant::now();
                std::hint::black_box(unsafe { a2_three(std::hint::black_box(input)) });
                t_three = t_three.min(t.elapsed().as_secs_f64() * 1e3);
            }
            println!("profile 1M groups:");
            println!("  encode only {:8.3} ms", te);
            println!("  + 1 aggregate {:8.3} ms  delta {:8.3} ms", t_one, t_one - te);
            println!("  + 2 aggregate {:8.3} ms  delta {:8.3} ms", t_two, t_two - te);
            println!("  + 3 aggregate {:8.3} ms  delta {:8.3} ms", t_three, t_three - te);
            println!("  + 4 aggregate {:8.3} ms  delta {:8.3} ms", t2, t2 - te);
        }
    }
}
