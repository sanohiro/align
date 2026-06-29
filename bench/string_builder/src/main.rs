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

#[repr(C)]
#[derive(Clone, Copy)]
struct AlignStr {
    ptr: *const u8,
    len: i64,
}

extern "C" {
    /// `pub fn build(s: slice<i64>) -> i64` — builder reduce-append, returns the final string length.
    fn build(s: Slice) -> i64;
    /// `build` with a pre-sized builder (`builder(n*16)`) — the capacity (Gap C) variant.
    fn build_cap(s: Slice) -> i64;
    fn build_static_one(s: Slice) -> i64;
    fn build_static_two(s: Slice) -> i64;
    fn build_int_only(s: Slice) -> i64;
    fn align_rt_builder_new(arena: *mut std::ffi::c_void, capacity: i64) -> *mut std::ffi::c_void;
    fn align_rt_builder_write_str_int_str(
        b: *mut std::ffi::c_void,
        p1: *const u8,
        l1: i64,
        v: i64,
        p2: *const u8,
        l2: i64,
    );
    fn align_rt_builder_into_string(b: *mut std::ffi::c_void) -> AlignStr;
    fn align_rt_free(ptr: *mut u8);
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

fn runtime_batch(s: &[i64], capacity: i64) -> i64 {
    let p1 = b"item-";
    let p2 = b"-status ";
    unsafe {
        let b = align_rt_builder_new(std::ptr::null_mut(), capacity);
        for &x in s {
            align_rt_builder_write_str_int_str(b, p1.as_ptr(), p1.len() as i64, x, p2.as_ptr(), p2.len() as i64);
        }
        let out = align_rt_builder_into_string(b);
        let len = out.len;
        align_rt_free(out.ptr as *mut u8);
        len
    }
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
    let profile = std::env::var_os("ALIGN_BENCH_PROFILE").is_some();
    println!("builder reduce-append (\"item-\" + int + \"-status \") — Align vs Rust String");
    println!(
        "{:>9}  {:>9}  {:>9}  {:>9}  {:>9}  {:>9}  {:>9}",
        "n", "align ms", "+cap ms", "naive ms", "opt ms", "cap/opt", "cap/naive"
    );
    for &n in &[1_000usize, 10_000, 100_000] {
        let data = gen(n);
        let sl = Slice { ptr: data.as_ptr(), len: n as i64 };

        // Correctness: all four produce the same length.
        let a0 = unsafe { build(Slice { ptr: sl.ptr, len: sl.len }) };
        assert_eq!(a0, unsafe { build_cap(Slice { ptr: sl.ptr, len: sl.len }) }, "align cap length");
        assert_eq!(a0, rust_naive(&data), "align vs naive length");
        assert_eq!(a0, rust_opt(&data), "align vs opt length");
        assert_eq!(a0, runtime_batch(&data, 0), "runtime batch length");

        let (mut am, mut cm, mut nm, mut om) = (f64::MAX, f64::MAX, f64::MAX, f64::MAX);
        for _ in 0..rounds {
            let t = Instant::now();
            std::hint::black_box(unsafe { build(Slice { ptr: sl.ptr, len: sl.len }) });
            am = am.min(t.elapsed().as_secs_f64() * 1e3);

            let t = Instant::now();
            std::hint::black_box(unsafe { build_cap(Slice { ptr: sl.ptr, len: sl.len }) });
            cm = cm.min(t.elapsed().as_secs_f64() * 1e3);

            let t = Instant::now();
            std::hint::black_box(rust_naive(&data));
            nm = nm.min(t.elapsed().as_secs_f64() * 1e3);

            let t = Instant::now();
            std::hint::black_box(rust_opt(&data));
            om = om.min(t.elapsed().as_secs_f64() * 1e3);
        }
        println!("{:>9}  {:>9.3}  {:>9.3}  {:>9.3}  {:>9.3}  {:>8.2}x  {:>8.2}x", n, am, cm, nm, om, om / cm, nm / cm);

        if profile && n == 100_000 {
            let (mut s1, mut s2, mut io, mut rb, mut rbc) = (f64::MAX, f64::MAX, f64::MAX, f64::MAX, f64::MAX);
            for _ in 0..rounds {
                let t = Instant::now();
                std::hint::black_box(unsafe { build_static_one(Slice { ptr: sl.ptr, len: sl.len }) });
                s1 = s1.min(t.elapsed().as_secs_f64() * 1e3);

                let t = Instant::now();
                std::hint::black_box(unsafe { build_static_two(Slice { ptr: sl.ptr, len: sl.len }) });
                s2 = s2.min(t.elapsed().as_secs_f64() * 1e3);

                let t = Instant::now();
                std::hint::black_box(unsafe { build_int_only(Slice { ptr: sl.ptr, len: sl.len }) });
                io = io.min(t.elapsed().as_secs_f64() * 1e3);

                let t = Instant::now();
                std::hint::black_box(runtime_batch(&data, 0));
                rb = rb.min(t.elapsed().as_secs_f64() * 1e3);

                let t = Instant::now();
                std::hint::black_box(runtime_batch(&data, (n * 16) as i64));
                rbc = rbc.min(t.elapsed().as_secs_f64() * 1e3);
            }
            println!("profile 100k:");
            println!("  static one write/row {:8.3} ms", s1);
            println!("  static two writes/row {:8.3} ms  extra call delta {:8.3} ms", s2, s2 - s1);
            println!("  int only write/row   {:8.3} ms", io);
            println!("  full build           {:8.3} ms  static+int interaction {:8.3} ms", am, am - s2 - io);
            println!("  runtime batch        {:8.3} ms", rb);
            println!("  runtime batch +cap   {:8.3} ms", rbc);
        }
    }
}
