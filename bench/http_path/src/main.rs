//! `http_path` — pricing the server's own request path in allocations and CPU nanoseconds.
//!
//! **Why this exists.** `bench/web_e2e` reports "Align's protocol path above the floor" as the
//! difference of two ~70 µs end-to-end measurements, so it carries both their noises: three adjacent
//! baseline runs gave 3.3 / 3.9 / 4.8 µs/req — a 1.5 µs spread on a 4.0 µs signal, larger than the
//! whole allocation budget that difference is supposed to price. (Its `CONNS=1` lesson still holds
//! for what it measured: throughput moves 18% run to run, req/s at one connection ~1%. It is the
//! *derived difference* that is unusable.)
//!
//! This harness measures the same path in one process, on metrics that survive:
//!
//! - **allocations per request — exact, integral, zero noise**, from a counting
//!   `#[global_allocator]`. `align_runtime` is a dependency as a Rust lib rather than through its C
//!   ABI so its own `Vec`/`String` traffic is visible. **Scope:** that is the runtime's *Rust*
//!   allocations only. Align-language allocation (`align_rt_alloc`) calls libc `malloc` directly and
//!   is invisible here — fine while the measured handler is this file's pure-Rust `serve_one`, and a
//!   trap for anyone who extends this to a real Align handler or to `pkg.web`.
//! - **server CPU ns per request**, from `CLOCK_THREAD_CPUTIME_ID`. Wall time is the wrong clock:
//!   `accept` blocks in `poll` until the client's next request, so a wall-clock loop reads the ~65 µs
//!   loopback round-trip around ~4 µs of work.
//!
//! **Two floors, because Align does one syscall more.** Align's path is `poll({parked, listener})` →
//! `read` → `send`; a plain read/write floor does one fewer syscall, and that `poll` costs ~0.9 µs
//! on this box — 21% of the naive difference. Reporting only against a plain floor would charge that
//! syscall to Align's CPU work and overstate what allocation removal can reach. So both floors run:
//! the plain one keeps the number comparable with `web_e2e`, and the poll floor is the honest budget
//! for CPU-side work.
//!
//! **Interleaved blocks, median, not one long pass.** Within a run the harness is tight, but the box
//! drifts *between* runs by more than its own σ (~480 ns observed on an unchanged binary), so more
//! iterations do not converge past that. The arms therefore alternate in blocks inside one process
//! and each reports the median of its blocks — `bench/README.md`'s balanced-order rule, applied.
//!
//! Run: `bench/http_path/run.sh` (or `cargo run --release -- [requests-per-arm] [blocks]`).
//! Linux-specific.

use std::alloc::{GlobalAlloc, Layout, System};
use std::io::{Read, Write};
use std::net::TcpStream;
use std::os::fd::AsRawFd;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};

// --- the counting allocator ---------------------------------------------------------------------

static ALLOCS: AtomicU64 = AtomicU64::new(0);
/// Bytes requested by *fresh* allocations. Kept apart from growth (below) on purpose: pre-reserving
/// a buffer moves bytes between these two, and a single summed figure would read as a regression.
static FRESH_BYTES: AtomicU64 = AtomicU64::new(0);
/// Bytes added by `realloc` growth (`new - old`).
static GROWTH_BYTES: AtomicU64 = AtomicU64::new(0);
/// Counting is off until a measured block starts, so setup traffic is never attributed to requests.
static COUNTING: AtomicBool = AtomicBool::new(false);

struct Counting;

unsafe impl GlobalAlloc for Counting {
    unsafe fn alloc(&self, l: Layout) -> *mut u8 {
        if COUNTING.load(Ordering::Relaxed) {
            ALLOCS.fetch_add(1, Ordering::Relaxed);
            FRESH_BYTES.fetch_add(l.size() as u64, Ordering::Relaxed);
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
            GROWTH_BYTES.fetch_add(new.saturating_sub(l.size()) as u64, Ordering::Relaxed);
        }
        unsafe { System.realloc(p, l, new) }
    }
}

#[global_allocator]
static A: Counting = Counting;

// --- clocks and syscalls -------------------------------------------------------------------------

#[repr(C)]
#[derive(Clone, Copy, Default)]
struct Timespec {
    tv_sec: i64,
    tv_nsec: i64,
}

#[repr(C)]
#[derive(Clone, Copy)]
struct PollFd {
    fd: i32,
    events: i16,
    revents: i16,
}

const CLOCK_THREAD_CPUTIME_ID: i32 = 3;
const POLLIN: i16 = 0x001;

unsafe extern "C" {
    fn clock_gettime(clk: i32, tp: *mut Timespec) -> i32;
    fn poll(fds: *mut PollFd, nfds: u64, timeout: i32) -> i32;
}

/// This thread's consumed CPU time, in nanoseconds.
fn thread_cpu_ns() -> u64 {
    let mut ts = Timespec::default();
    let rc = unsafe { clock_gettime(CLOCK_THREAD_CPUTIME_ID, &mut ts) };
    assert_eq!(rc, 0, "clock_gettime(CLOCK_THREAD_CPUTIME_ID) failed");
    (ts.tv_sec as u64) * 1_000_000_000 + ts.tv_nsec as u64
}

// --- the request path -----------------------------------------------------------------------------

use align_runtime::{
    align_rt_http_accept, align_rt_http_rb_body, align_rt_http_rb_header, align_rt_http_respond,
    align_rt_http_response_new, align_rt_http_serve, align_rt_http_server_free, HttpServer,
};

/// **`bench/web_e2e`'s exact request bytes.** Keeping them identical is what lets the two harnesses'
/// numbers be compared at all; a heavier request measurably changes the parse cost (a 3-header
/// variant read ~340 ns higher here, with the allocation count unchanged at 14).
const REQUEST: &[u8] = b"GET /plaintext HTTP/1.1\r\nHost: h\r\n\r\n";
const BODY: &[u8] = b"Hello, World!";
const CT_NAME: &[u8] = b"Content-Type";
const CT_VALUE: &[u8] = b"text/plain; charset=utf-8";

/// The response both floors write — byte-for-byte what the Align path produces for this request, so
/// every arm moves the same bytes through the same `write`. The client asserts this on its first
/// response in **every** arm, which is what keeps the arms comparable rather than merely similar.
const RESPONSE: &[u8] = b"HTTP/1.1 200 OK\r\nContent-Type: text/plain; charset=utf-8\r\nContent-Length: 13\r\n\r\nHello, World!";

/// One server-side request through Align: accept (poll + read + parse), build the response
/// `bench/web_e2e`'s `/plaintext` route builds, respond (serialize + one write).
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

// --- the harness ------------------------------------------------------------------------------------

/// One block's measurement for one arm.
#[derive(Clone, Copy)]
struct Sample {
    allocs: f64,
    fresh_bytes: f64,
    growth_bytes: f64,
    cpu_ns: f64,
}

/// A free port, the way the driver tests pick one: bind a probe on 0, read its port, drop it.
fn free_loopback_port() -> u16 {
    let probe = std::net::TcpListener::bind("127.0.0.1:0").expect("bind probe");
    let p = probe.local_addr().unwrap().port();
    drop(probe);
    p
}

/// Progress counter, read by the watchdog. A server arm blocks in `poll`/`accept` forever if its
/// client dies, and the client's own `expect`s are then never observed — so without this the harness
/// hangs silently instead of failing.
static SERVED: AtomicU64 = AtomicU64::new(0);

fn start_watchdog() {
    std::thread::spawn(|| {
        let mut last = 0;
        let mut stalled = 0;
        loop {
            std::thread::sleep(std::time::Duration::from_secs(1));
            let now = SERVED.load(Ordering::Relaxed);
            stalled = if now == last { stalled + 1 } else { 0 };
            last = now;
            if stalled >= 20 {
                eprintln!("http_path: no progress for 20s after {now} requests — aborting");
                std::process::abort();
            }
        }
    });
}

/// The client half: one keep-alive connection, strictly ping-pong, driving `total` requests.
///
/// **No channel back to the server half, on purpose:** the socket already sequences them (the
/// server's next accept blocks until the client sends again), and a channel would allocate on this
/// thread — which the counting allocator would then charge to the request path. Removing it is what
/// makes allocations/request exactly integral.
fn spawn_client(port: u16, total: u64) -> std::thread::JoinHandle<()> {
    std::thread::spawn(move || {
        let mut s = TcpStream::connect(("127.0.0.1", port)).expect("connect");
        s.set_nodelay(true).ok();
        // **No read timeout on purpose.** While one arm runs a block, the other arms' clients sit in
        // `read` for the whole block — at 200k requests per block that is well over any timeout worth
        // setting, and a timeout here aborted ordinary large-block invocations. The watchdog is the
        // right guard: `SERVED` advances during *any* arm's block, so it fires only on a true global
        // stall, which is exactly the condition worth failing on.
        let mut resp = vec![0u8; 512];
        for i in 0..total {
            s.write_all(REQUEST).expect("send request");
            let n = s.read(&mut resp).expect("read response");
            assert!(n > 0, "server closed the connection early");
            if i == 0 {
                assert_eq!(
                    &resp[..n],
                    RESPONSE,
                    "this arm does not emit the shared response byte-for-byte — the arms are not comparable"
                );
            }
        }
    })
}

/// Run one measured block of `reqs` requests, with counting armed only for its duration.
fn measure_block(reqs: u64, mut serve: impl FnMut()) -> Sample {
    ALLOCS.store(0, Ordering::SeqCst);
    FRESH_BYTES.store(0, Ordering::SeqCst);
    GROWTH_BYTES.store(0, Ordering::SeqCst);
    COUNTING.store(true, Ordering::SeqCst);
    let cpu0 = thread_cpu_ns();
    for _ in 0..reqs {
        serve();
        SERVED.fetch_add(1, Ordering::Relaxed);
    }
    let cpu_ns = thread_cpu_ns() - cpu0;
    COUNTING.store(false, Ordering::SeqCst);
    let n = reqs as f64;
    Sample {
        allocs: ALLOCS.load(Ordering::SeqCst) as f64 / n,
        fresh_bytes: FRESH_BYTES.load(Ordering::SeqCst) as f64 / n,
        growth_bytes: GROWTH_BYTES.load(Ordering::SeqCst) as f64 / n,
        cpu_ns: cpu_ns as f64 / n,
    }
}

fn median(mut xs: Vec<f64>) -> f64 {
    xs.sort_by(|a, b| a.partial_cmp(b).unwrap());
    let n = xs.len();
    if n % 2 == 1 {
        xs[n / 2]
    } else {
        (xs[n / 2 - 1] + xs[n / 2]) / 2.0
    }
}

fn main() {
    let mut args = std::env::args().skip(1);
    let parse = |s: Option<String>, dflt: u64, what: &str| -> u64 {
        match s {
            None => dflt,
            Some(v) => v.parse().unwrap_or_else(|_| panic!("{what}: not a number: {v:?}")),
        }
    };
    let reqs_per_arm = parse(args.next(), 100_000, "requests-per-arm");
    let blocks = parse(args.next(), 6, "blocks");
    assert!(blocks >= 4 && blocks % 2 == 0, "blocks must be even and >= 4 (counterbalanced order)");
    let per_block = reqs_per_arm / blocks;
    assert!(per_block >= 1_000, "at least 1000 requests per block (got {per_block})");
    let warm = per_block.min(2_000);
    let total = warm + per_block * blocks;

    start_watchdog();

    // --- arm 1: the plain floor — read, write, keep the connection. One syscall FEWER than Align.
    let plain_port = free_loopback_port();
    let plain_listener = std::net::TcpListener::bind(("127.0.0.1", plain_port)).expect("bind plain");
    let plain_client = spawn_client(plain_port, total);
    let (mut plain_conn, _) = plain_listener.accept().expect("plain accept");
    plain_conn.set_nodelay(true).ok();

    // --- arm 2: the poll floor — the same, plus the `poll({conn, listener})` Align does before its
    // read. This is the honest zero for CPU-side work; the difference between the two floors IS the
    // syscall Align's keep-alive wait costs.
    let poll_port = free_loopback_port();
    let poll_listener = std::net::TcpListener::bind(("127.0.0.1", poll_port)).expect("bind poll");
    let poll_client = spawn_client(poll_port, total);
    let (mut poll_conn, _) = poll_listener.accept().expect("poll accept");
    poll_conn.set_nodelay(true).ok();

    // --- arm 3: Align.
    let align_port = free_loopback_port();
    let mut srv: *mut HttpServer = std::ptr::null_mut();
    let host = b"127.0.0.1";
    let rc =
        unsafe { align_rt_http_serve(host.as_ptr(), host.len() as i64, align_port as i64, &mut srv) };
    assert_eq!(rc, 0, "http.serve failed: {rc}");
    let align_client = spawn_client(align_port, total);

    let mut req_buf = [0u8; 2048];
    // One dispatcher rather than three closures, so the arm ORDER can be varied per block below.
    let mut serve = |arm: usize, buf: &mut [u8; 2048]| match arm {
        0 => {
            let n = plain_conn.read(buf).expect("plain read");
            assert!(n > 0, "plain floor: client closed");
            plain_conn.write_all(RESPONSE).expect("plain write");
        }
        1 => {
            let mut fds = [
                PollFd { fd: poll_conn.as_raw_fd(), events: POLLIN, revents: 0 },
                PollFd { fd: poll_listener.as_raw_fd(), events: POLLIN, revents: 0 },
            ];
            let r = unsafe { poll(fds.as_mut_ptr(), 2, -1) };
            assert!(r > 0, "poll floor: poll failed");
            let n = poll_conn.read(buf).expect("poll floor read");
            assert!(n > 0, "poll floor: client closed");
            poll_conn.write_all(RESPONSE).expect("poll floor write");
        }
        _ => assert!(unsafe { serve_one(srv) }, "align request failed"),
    };

    // Warm-up per arm: the first requests pay one-time costs (the parked set's first insert,
    // allocator arena growth) that are not per-request costs.
    for _ in 0..warm {
        for arm in 0..3 {
            serve(arm, &mut req_buf);
        }
    }

    // **Counterbalanced interleaving.** Alternating A,B,C every block is not enough: the SLOT itself
    // carries a systematic bias on this box — with three identical floors in the three slots, slot 2
    // reads ~115 ns above slot 1 and slot 3 ~109 ns below slot 2. That is twice the headline's own σ
    // and half the size of the effects this harness exists to price, and it does not shrink with more
    // blocks. Reversing the order on odd blocks cancels it, which is what `bench/README.md`'s
    // "balanced order" actually asks for; `blocks` is even so every arm gets each slot equally often.
    let mut samples: [Vec<Sample>; 3] = [Vec::new(), Vec::new(), Vec::new()];
    for b in 0..blocks {
        for slot in 0..3usize {
            let arm = if b % 2 == 0 { slot } else { 2 - slot };
            samples[arm].push(measure_block(per_block, || serve(arm, &mut req_buf)));
        }
    }
    let [sp, sq, sa] = samples;

    drop(plain_conn);
    drop(poll_conn);
    unsafe { align_rt_http_server_free(srv) };
    for c in [plain_client, poll_client, align_client] {
        c.join().expect("client thread");
    }

    let med = |xs: &[Sample], f: fn(&Sample) -> f64| median(xs.iter().map(f).collect());
    let (pa, qa, aa) = (med(&sp, |s| s.allocs), med(&sq, |s| s.allocs), med(&sa, |s| s.allocs));
    let (pc, qc, ac) = (med(&sp, |s| s.cpu_ns), med(&sq, |s| s.cpu_ns), med(&sa, |s| s.cpu_ns));
    // The zero that proves the client thread is not polluting the Align count: a floor arm runs the
    // identical client and must allocate nothing at all.
    assert_eq!(pa, 0.0, "the plain floor allocated — the counter is seeing the client thread");
    assert_eq!(qa, 0.0, "the poll floor allocated — the counter is seeing the client thread");

    let spread = |xs: &[Sample]| {
        let v: Vec<f64> = xs.iter().map(|s| s.cpu_ns).collect();
        let (lo, hi) = v.iter().fold((f64::MAX, f64::MIN), |(l, h), &x| (l.min(x), h.max(x)));
        hi - lo
    };
    println!("{blocks} interleaved blocks x {per_block} requests per arm (after {warm} warm-up)");
    println!("  arm            allocs/req   fresh B/req   growth B/req   CPU ns/req   block spread");
    for (name, s, a, c) in [
        ("floor (plain)", &sp, pa, pc),
        ("floor (+poll)", &sq, qa, qc),
        ("align", &sa, aa, ac),
    ] {
        println!(
            "  {name:<14} {a:>10.2}  {:>12.1}  {:>13.1}  {c:>11.0}  {:>12.0}",
            med(s, |x| x.fresh_bytes),
            med(s, |x| x.growth_bytes),
            spread(s)
        );
    }
    println!(
        "\n  the keep-alive `poll` costs:                    {:.0} ns/req",
        qc - pc
    );
    println!(
        "  Align above the plain floor (web_e2e's figure):  {:.0} ns/req",
        ac - pc
    );
    println!(
        "  Align's CPU work above the poll floor:          {:.0} ns/req, {aa:.2} allocations",
        ac - qc
    );
}
