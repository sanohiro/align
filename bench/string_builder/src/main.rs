//! String-builder duel: Align's `builder` reduce-append pattern (the tool the Gap A error points to)
//! vs idiomatic Rust — naive (`String::new()` + `x.to_string()` per int) and optimized
//! (`String::with_capacity` + manual itoa into a stack buffer, no allocation). Per element we append
//! `"item-" + int + "-status "`, so the integer write path (the hand-rolled runtime itoa) is exercised.

use std::time::Instant;

#[repr(C)]
#[derive(Clone, Copy)]
struct Slice {
    ptr: *const i64,
    len: i64,
}

extern "C" {
    /// `pub fn build(s: slice<i64>) -> i64` — builder reduce-append, returns the final string length.
    fn build(s: Slice) -> i64;
}

fn rust_naive(s: &[i64]) -> i64 {
    let mut b = String::new();
    for &x in s {
        b.push_str("item-");
        b.push_str(&x.to_string());
        b.push_str("-status ");
    }
    b.len() as i64
}

fn rust_opt(s: &[i64]) -> i64 {
    let mut b = String::with_capacity(s.len() * 20);
    let mut buf = [0u8; 20];
    for &x in s {
        b.push_str("item-");
        b.push_str(itoa(x, &mut buf));
        b.push_str("-status ");
    }
    b.len() as i64
}

/// Manual itoa into `buf`, returning the formatted slice (no allocation).
fn itoa(v: i64, buf: &mut [u8; 20]) -> &str {
    let mut i = buf.len();
    let mut n = v.unsigned_abs();
    loop {
        i -= 1;
        buf[i] = b'0' + (n % 10) as u8;
        n /= 10;
        if n == 0 {
            break;
        }
    }
    if v < 0 {
        i -= 1;
        buf[i] = b'-';
    }
    std::str::from_utf8(&buf[i..]).unwrap()
}

fn gen(n: usize) -> Vec<i64> {
    let mut v = vec![0i64; n];
    let mut s: u64 = 0x9E3779B97F4A7C15;
    for d in v.iter_mut() {
        s = s.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
        *d = ((s >> 33) as i64) % 201 - 100;
    }
    v
}

fn main() {
    let rounds = 50;
    println!("builder reduce-append (\"item-\" + int + \"-status \") — Align vs Rust String");
    println!("{:>9}  {:>10}  {:>10}  {:>10}  {:>9}  {:>9}", "n", "align ms", "naive ms", "opt ms", "vs naive", "vs opt");
    for &n in &[1_000usize, 10_000, 100_000] {
        let data = gen(n);
        let sl = Slice { ptr: data.as_ptr(), len: n as i64 };

        // Correctness: all three produce the same length.
        let a0 = unsafe { build(Slice { ptr: sl.ptr, len: sl.len }) };
        assert_eq!(a0, rust_naive(&data), "align vs naive length");
        assert_eq!(a0, rust_opt(&data), "align vs opt length");

        let (mut am, mut nm, mut om) = (f64::MAX, f64::MAX, f64::MAX);
        for _ in 0..rounds {
            let t = Instant::now();
            std::hint::black_box(unsafe { build(Slice { ptr: sl.ptr, len: sl.len }) });
            am = am.min(t.elapsed().as_secs_f64() * 1e3);

            let t = Instant::now();
            std::hint::black_box(rust_naive(&data));
            nm = nm.min(t.elapsed().as_secs_f64() * 1e3);

            let t = Instant::now();
            std::hint::black_box(rust_opt(&data));
            om = om.min(t.elapsed().as_secs_f64() * 1e3);
        }
        println!("{:>9}  {:>10.3}  {:>10.3}  {:>10.3}  {:>8.2}x  {:>8.2}x", n, am, nm, om, nm / am, om / am);
    }
}
