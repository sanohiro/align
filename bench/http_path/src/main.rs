//! `http_path` — what one request costs INSIDE the server, in allocations and in CPU nanoseconds.
//!
//! **Why this exists.** `bench/web_e2e` prices the whole request over loopback and reports "Align's
//! protocol path above the floor". That number is a *difference of two ~70 µs measurements*, so it
//! carries the noise of both: three adjacent baseline runs on this box gave **3.3 / 3.9 / 4.8
//! µs/req** — a 1.5 µs spread on a 4.0 µs signal. The allocation work the roadmap points at is worth
//! less than that spread, so `web_e2e` cannot price it, whatever the per-run req/s stability
//! suggests. (Its `CONNS=1` lesson still holds for what it was about: throughput moves 18% run to
//! run, req/s at one connection ~1%. It is the *derived difference* that is unusable here.)
//!
//! So this harness measures the server side directly, in one process, on two clocks that do not
//! have that problem:
//!
//! - **allocations and bytes per request**, counted exactly by a `#[global_allocator]` — no
//!   statistics at all. `align_runtime` is linked as a Rust lib rather than through its C ABI
//!   precisely so its own `Vec`/`String` traffic is visible here.
//! - **server CPU ns per request**, from `CLOCK_THREAD_CPUTIME_ID`. Wall time is the wrong clock:
//!   `accept` blocks in `poll` until the client's next request, so a wall-clock loop measures the
//!   ~65 µs loopback round-trip and buries the server's own few µs inside it. Thread CPU time
//!   accrues only while the thread is on-CPU, so blocking simply does not count.
//!
//! The syscalls (`poll`, `read`, `write`) are inside the measured region and identical across arms,
//! so a difference between arms is the CPU-side change.
//!
//! Run: `bench/http_path/run.sh` (or `cargo run --release -- [iters]`). Linux-specific.

use std::alloc::{GlobalAlloc, Layout, System};
use std::io::{Read, Write};
use std::net::TcpStream;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};

// --- the counting allocator ---------------------------------------------------------------------

static ALLOCS: AtomicU64 = AtomicU64::new(0);
static BYTES: AtomicU64 = AtomicU64::new(0);
/// Counting is off until the measured loop starts, so setup traffic is not attributed to requests.
static COUNTING: AtomicBool = AtomicBool::new(false);

struct Counting;

unsafe impl GlobalAlloc for Counting {
    unsafe fn alloc(&self, l: Layout) -> *mut u8 {
        if COUNTING.load(Ordering::Relaxed) {
            ALLOCS.fetch_add(1, Ordering::Relaxed);
            BYTES.fetch_add(l.size() as u64, Ordering::Relaxed);
        }
        unsafe { System.alloc(l) }
    }
    unsafe fn dealloc(&self, p: *mut u8, l: Layout) {
        unsafe { System.dealloc(p, l) }
    }
    unsafe fn realloc(&self, p: *mut u8, l: Layout, new: usize) -> *mut u8 {
        // A `Vec` growing counts as its own event: that is exactly the cost a right-sized or reused
        // buffer removes, and hiding it would make a growing buffer look free.
        if COUNTING.load(Ordering::Relaxed) {
            ALLOCS.fetch_add(1, Ordering::Relaxed);
            BYTES.fetch_add(new.saturating_sub(l.size()) as u64, Ordering::Relaxed);
        }
        unsafe { System.realloc(p, l, new) }
    }
}

#[global_allocator]
static A: Counting = Counting;

// --- the server thread's own CPU time ------------------------------------------------------------

#[repr(C)]
#[derive(Clone, Copy, Default)]
struct Timespec {
    tv_sec: i64,
    tv_nsec: i64,
}

const CLOCK_THREAD_CPUTIME_ID: i32 = 3;

unsafe extern "C" {
    fn clock_gettime(clk: i32, tp: *mut Timespec) -> i32;
}

/// This thread's consumed CPU time, in nanoseconds.
fn thread_cpu_ns() -> u64 {
    let mut ts = Timespec::default();
    let rc = unsafe { clock_gettime(CLOCK_THREAD_CPUTIME_ID, &mut ts) };
    assert_eq!(rc, 0, "clock_gettime(CLOCK_THREAD_CPUTIME_ID) failed");
    (ts.tv_sec as u64) * 1_000_000_000 + ts.tv_nsec as u64
}

// --- the request path ----------------------------------------------------------------------------

use align_runtime::{
    align_rt_http_accept, align_rt_http_rb_body, align_rt_http_rb_header, align_rt_http_respond,
    align_rt_http_response_new, align_rt_http_serve, align_rt_http_server_free, HttpServer,
};

const REQUEST: &[u8] =
    b"GET /plaintext HTTP/1.1\r\nHost: 127.0.0.1\r\nAccept: */*\r\nUser-Agent: http-path-bench\r\n\r\n";
const BODY: &[u8] = b"Hello, World!";
const CT_NAME: &[u8] = b"Content-Type";
const CT_VALUE: &[u8] = b"text/plain; charset=utf-8";

/// One server-side request: accept (poll + read + parse), build the response `bench/web_e2e`'s
/// `/plaintext` route builds, respond (serialize + one write). False if any step failed.
unsafe fn serve_one(srv: *mut HttpServer) -> bool {
    let mut ctx = std::ptr::null_mut();
    if unsafe { align_rt_http_accept(srv, &mut ctx) } != 0 {
        return false;
    }
    let rb = align_rt_http_response_new(200);
    unsafe {
        align_rt_http_rb_header(
            rb,
            CT_NAME.as_ptr(),
            CT_NAME.len() as i64,
            CT_VALUE.as_ptr(),
            CT_VALUE.len() as i64,
        );
        align_rt_http_rb_body(rb, BODY.as_ptr(), BODY.len() as i64);
        align_rt_http_respond(ctx, rb) == 0
    }
}

/// The canned response the floor arm writes — byte-for-byte what the Align path produces for
/// `/plaintext`, so the two arms move the same bytes through the same syscalls.
const FLOOR_RESPONSE: &[u8] = b"HTTP/1.1 200 OK\r\nContent-Type: text/plain; charset=utf-8\r\nContent-Length: 13\r\n\r\nHello, World!";

/// What one arm measured.
struct Arm {
    allocs: f64,
    bytes: f64,
    cpu_ns: f64,
}

/// A free port, the way the driver tests pick one: bind a probe on 0, read its port, drop it.
fn free_loopback_port() -> u16 {
    let probe = std::net::TcpListener::bind("127.0.0.1:0").expect("bind probe");
    let p = probe.local_addr().unwrap().port();
    drop(probe);
    p
}

/// Drive `iters` ping-pong requests against a server loop, on this thread, with one keep-alive
/// client on another. `serve` runs ONE request and must not return until it has answered.
///
/// **No channel between the halves, on purpose:** the socket already sequences them (the server's
/// next read blocks until the client sends again), and a channel would allocate on the client
/// thread — which this process's counting allocator would then charge to the request path.
fn run_arm(port: u16, iters: u64, mut serve: impl FnMut()) -> Arm {
    let client = std::thread::spawn(move || {
        let mut s = TcpStream::connect(("127.0.0.1", port)).expect("connect");
        s.set_nodelay(true).ok();
        let mut resp = [0u8; 512];
        for _ in 0..iters {
            s.write_all(REQUEST).expect("send request");
            let n = s.read(&mut resp).expect("read response");
            assert!(n > 0, "server closed the connection early");
        }
    });

    // Warm-up: the first requests pay one-time costs (the parked set's first insert, allocator
    // arena growth, the client's connect) that are not per-request costs.
    let warm = (iters / 10).clamp(1, 2_000);
    for _ in 0..warm {
        serve();
    }

    let measured = iters - warm;
    ALLOCS.store(0, Ordering::SeqCst);
    BYTES.store(0, Ordering::SeqCst);
    COUNTING.store(true, Ordering::SeqCst);
    let cpu0 = thread_cpu_ns();
    for _ in 0..measured {
        serve();
    }
    let cpu_ns = thread_cpu_ns() - cpu0;
    COUNTING.store(false, Ordering::SeqCst);
    client.join().expect("client thread");

    Arm {
        allocs: ALLOCS.load(Ordering::SeqCst) as f64 / measured as f64,
        bytes: BYTES.load(Ordering::SeqCst) as f64 / measured as f64,
        cpu_ns: cpu_ns as f64 / measured as f64,
    }
}

fn main() {
    let iters: u64 = std::env::args().nth(1).and_then(|s| s.parse().ok()).unwrap_or(20_000);

    // --- the floor: read the request, write a canned response, keep the connection. Same socket
    // setup, same client, same clock — so the CPU cost of the syscalls and of WSL2's accounting is
    // in BOTH arms and cancels in the difference. Without this control the absolute CPU number is
    // uninterpretable (it reads ~36 µs/req, which is virtualization overhead, not server work).
    let floor_port = free_loopback_port();
    let listener = std::net::TcpListener::bind(("127.0.0.1", floor_port)).expect("bind floor");
    let floor = {
        let mut conn: Option<std::net::TcpStream> = None;
        let mut req = [0u8; 2048];
        run_arm(floor_port, iters, || {
            let c = conn.get_or_insert_with(|| {
                let (s, _) = listener.accept().expect("floor accept");
                s.set_nodelay(true).ok();
                s
            });
            let n = c.read(&mut req).expect("floor read");
            assert!(n > 0, "floor: client closed");
            c.write_all(FLOOR_RESPONSE).expect("floor write");
        })
    };
    drop(listener);

    // --- Align's own path.
    let port = free_loopback_port();
    let mut srv: *mut HttpServer = std::ptr::null_mut();
    let host = b"127.0.0.1";
    let rc = unsafe { align_rt_http_serve(host.as_ptr(), host.len() as i64, port as i64, &mut srv) };
    assert_eq!(rc, 0, "http.serve failed: {rc}");
    let align = run_arm(port, iters, || {
        assert!(unsafe { serve_one(srv) }, "request failed");
    });
    unsafe { align_rt_http_server_free(srv) };

    let measured = iters - (iters / 10).clamp(1, 2_000);
    println!("requests measured per arm: {measured}");
    println!("  arm            allocs/req   bytes/req   CPU ns/req");
    println!(
        "  floor          {:>10.2}  {:>10.1}  {:>11.0}",
        floor.allocs, floor.bytes, floor.cpu_ns
    );
    println!(
        "  align          {:>10.2}  {:>10.1}  {:>11.0}",
        align.allocs, align.bytes, align.cpu_ns
    );
    println!(
        "\n  Align's server-side cost above the floor: {:.0} ns/req, {:.2} allocations, {:.0} bytes",
        align.cpu_ns - floor.cpu_ns,
        align.allocs - floor.allocs,
        align.bytes - floor.bytes
    );
}
