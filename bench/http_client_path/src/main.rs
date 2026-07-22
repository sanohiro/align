//! `http_client_path` — pricing `http.get`'s own client-side path in allocations and CPU nanoseconds.
//!
//! **Why this exists.** `bench/http_client` is the R6 throughput gate: it reports ~65 µs/req over
//! loopback, end to end, which is the right instrument for "does the pool beat reconnecting" (2.86×)
//! and the wrong one for anything smaller. The client path's remaining items are ~0.5–1 µs each —
//! under 2% of that figure, and derived as a difference of two large numbers, exactly the mistake
//! `bench/http_path/README.md` records for `web_e2e`. This harness is the client-side twin of
//! `bench/http_path`: same metrics, same statistics, one process.
//!
//! **One floor, not two.** `http_path` needs two because Align's *server* does one syscall more than
//! a plain read/write loop (the keep-alive `poll`). The client does not: `http_socket_exchange` is
//! `write_all` then a `read` loop, which is exactly what the floor arm does. So the single floor is
//! the honest zero, and the reported difference is CPU work — request build, response parse, the
//! owned `http_response`, and the pool lookup.
//!
//! **The server half must not allocate.** The counting allocator is global and this harness's peer
//! server runs in the same process, so any per-request allocation there would be charged to the
//! client path. It is written to allocate nothing per request (fixed buffers, a const response), and
//! the floor arm reading exactly `0.00` allocations/req is the assertion that proves it — the same
//! zero-check `http_path` uses in the mirror direction.
//!
//! **The arms must move identical bytes.** The peer asserts, on the first request of each arm, that
//! the request bytes are byte-for-byte what Align sends. A floor that sent a shorter request would
//! be measuring a different exchange.
//!
//! Run: `bench/http_client_path/run.sh` (or `cargo run --release -- [requests-per-arm] [blocks]`).
//! Linux-specific (`CLOCK_THREAD_CPUTIME_ID`).

use std::alloc::{GlobalAlloc, Layout, System};
use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
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

// --- clocks --------------------------------------------------------------------------------------

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

// --- the exchange ---------------------------------------------------------------------------------

use align_runtime::{
    align_rt_http_client_free, align_rt_http_client_get, align_rt_http_client_new,
    align_rt_http_resp_free, align_rt_http_resp_status, HttpClient,
};

/// `bench/http_path`'s response, so the two harnesses price the same message from the two ends.
const RESPONSE: &[u8] =
    b"HTTP/1.1 200 OK\r\nContent-Type: text/plain; charset=utf-8\r\nContent-Length: 13\r\n\r\nHello, World!";

/// The response both arms move. `BODY=n` swaps the 13-byte default for an `n`-byte body — the shape
/// a gateway actually reads back, and the one where per-byte copying (rather than per-request
/// overhead) dominates. A change can win at one size and lose at the other, so both are measured.
fn response_bytes() -> Vec<u8> {
    match std::env::var("BODY").ok().and_then(|v| v.parse::<usize>().ok()) {
        None => RESPONSE.to_vec(),
        Some(n) => {
            let mut r = format!("HTTP/1.1 200 OK\r\nContent-Type: text/plain; charset=utf-8\r\nContent-Length: {n}\r\n\r\n").into_bytes();
            r.extend(std::iter::repeat(b'x').take(n));
            r
        }
    }
}

/// Exactly what `align_rt_http_client_get` puts on the wire for `http://127.0.0.1:{port}/plaintext`
/// — asserted by the peer on the first request of every arm, so the floor cannot quietly measure a
/// cheaper exchange. A mismatch means the runtime's request serializer changed and this must follow
/// it; the assertion prints both, so the fix is mechanical. (Note what is NOT here: no
/// `Connection: keep-alive`. Align relies on 1.1's persistent default, which is the leanest bytes.)
///
/// The `Host` carries the ephemeral port, so this cannot be a constant — and the FLOOR arm sends the
/// bytes built for the ALIGN port, not its own. Its peer never parses the request, and byte-identical
/// arms matter more than a `Host` naming the socket it actually arrived on.
fn align_request_bytes(port: u16) -> Vec<u8> {
    format!("GET /plaintext HTTP/1.1\r\nHost: 127.0.0.1:{port}\r\n\r\n").into_bytes()
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

/// Progress counter, read by the watchdog. A client arm blocks in `read` forever if its peer dies,
/// so without this the harness hangs silently instead of failing.
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
                eprintln!("http_client_path: no progress for 20s after {now} requests — aborting");
                std::process::abort();
            }
        }
    });
}

/// The peer: a keep-alive HTTP/1.1 server answering `RESPONSE` to every request, one thread per
/// connection, serving until the client closes.
///
/// **It allocates nothing per request** — a fixed read buffer and a `const` response — because the
/// counting allocator is global and this thread shares the process with the measured one. The floor
/// arm's `0.00 allocs/req` is the assertion that keeps this honest.
fn spawn_peer(listener: TcpListener, arm: &'static str, expected: Vec<u8>, response: Vec<u8>) -> std::thread::JoinHandle<()> {
    std::thread::spawn(move || {
        let (mut conn, _) = listener.accept().expect("peer accept");
        conn.set_nodelay(true).ok();
        let mut buf = [0u8; 4096];
        let mut first = true;
        loop {
            // One request per read: both arms are strict ping-pong over loopback with `TCP_NODELAY`,
            // so a request never arrives split. A short read would show up as the assertion below.
            let n = match conn.read(&mut buf) {
                Ok(0) | Err(_) => return, // the client finished and closed
                Ok(n) => n,
            };
            if first {
                assert_eq!(
                    &buf[..n],
                    &expected[..],
                    "\n{arm}: this arm does not send the shared request byte-for-byte, so the arms \
                     are not comparable.\n  sent:     {:?}\n  expected: {:?}\n",
                    String::from_utf8_lossy(&buf[..n]),
                    String::from_utf8_lossy(&expected)
                );
                first = false;
            }
            if conn.write_all(&response).is_err() {
                return;
            }
        }
    })
}

/// A free port, the way the driver tests pick one: bind a probe on 0, read its port, drop it.
/// Returns the bound listener so nothing can race in between.
fn bind_loopback() -> (TcpListener, u16) {
    let l = TcpListener::bind("127.0.0.1:0").expect("bind loopback");
    let p = l.local_addr().unwrap().port();
    (l, p)
}

/// Run one measured block of `reqs` requests, with counting armed only for its duration.
fn measure_block(reqs: u64, mut exchange: impl FnMut()) -> Sample {
    ALLOCS.store(0, Ordering::SeqCst);
    FRESH_BYTES.store(0, Ordering::SeqCst);
    GROWTH_BYTES.store(0, Ordering::SeqCst);
    COUNTING.store(true, Ordering::SeqCst);
    let cpu0 = thread_cpu_ns();
    for _ in 0..reqs {
        exchange();
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

    start_watchdog();

    // --- arm 1 bound first: its port decides the request bytes BOTH arms send (see
    // `align_request_bytes`). Align — `http.client()` reused across every request, so the pool is
    // warm and the measured path is the keep-alive one (a fresh conn per request would price
    // `connect`, not this).
    let (align_listener, align_port) = bind_loopback();
    let request = align_request_bytes(align_port);
    let response = response_bytes();
    let default_response = response == RESPONSE;
    let align_peer = spawn_peer(align_listener, "align", request.clone(), response.clone());

    // --- arm 0: the floor — write the request, read the response, keep the connection. Exactly the
    // syscalls Align's exchange makes, and nothing else.
    let (floor_listener, floor_port) = bind_loopback();
    let floor_peer = spawn_peer(floor_listener, "floor", request.clone(), response.clone());
    let mut floor_conn = TcpStream::connect(("127.0.0.1", floor_port)).expect("floor connect");
    floor_conn.set_nodelay(true).ok();
    let client: *mut HttpClient = align_rt_http_client_new();
    let url = format!("http://127.0.0.1:{align_port}/plaintext");

    let mut resp_buf = vec![0u8; response.len() + 64];
    // One dispatcher rather than two closures, so the arm ORDER can be varied per block below.
    let mut exchange = |arm: usize, buf: &mut Vec<u8>| match arm {
        0 => {
            floor_conn.write_all(&request).expect("floor write");
            // Read to the framed length, exactly as Align's exchange does — a single `read` would
            // stop at the first segment and make the floor cheaper than the arm it is the zero for.
            let mut got = 0;
            while got < response.len() {
                let n = floor_conn.read(&mut buf[got..]).expect("floor read");
                assert!(n > 0, "floor: peer closed early");
                got += n;
            }
            assert_eq!(&buf[..got], &response[..], "floor: peer sent a different response");
        }
        _ => {
            let mut resp = std::ptr::null_mut();
            let rc = unsafe {
                align_rt_http_client_get(client, url.as_ptr(), url.len() as i64, &mut resp)
            };
            assert_eq!(rc, 0, "align: http.get failed ({rc})");
            assert_eq!(unsafe { align_rt_http_resp_status(resp) }, 200, "align: wrong status");
            unsafe { align_rt_http_resp_free(resp) };
        }
    };

    // Warm-up per arm: the first requests pay one-time costs (the pool's first insert, allocator
    // arena growth) that are not per-request costs.
    for _ in 0..warm {
        for arm in 0..2 {
            exchange(arm, &mut resp_buf);
        }
    }

    // **Counterbalanced interleaving.** Alternating A,B every block is not enough on its own: the
    // SLOT itself carries a systematic bias on this box (`bench/http_path/README.md` measured slot 2
    // reading ~115 ns above slot 1 with three *identical* arms). Reversing the order on odd blocks
    // cancels it; `blocks` is even so each arm gets each slot equally often.
    let mut samples: [Vec<Sample>; 2] = [Vec::new(), Vec::new()];
    for b in 0..blocks {
        for slot in 0..2usize {
            let arm = if b % 2 == 0 { slot } else { 1 - slot };
            samples[arm].push(measure_block(per_block, || exchange(arm, &mut resp_buf)));
        }
    }
    let [sf, sa] = samples;

    drop(floor_conn);
    unsafe { align_rt_http_client_free(client) };
    for p in [floor_peer, align_peer] {
        p.join().expect("peer thread");
    }

    let med = |xs: &[Sample], f: fn(&Sample) -> f64| median(xs.iter().map(f).collect());
    let (fa, aa) = (med(&sf, |s| s.allocs), med(&sa, |s| s.allocs));
    let (fc, ac) = (med(&sf, |s| s.cpu_ns), med(&sa, |s| s.cpu_ns));
    // The zero that proves neither peer thread is polluting the Align count: the floor arm runs the
    // identical peer and must allocate nothing at all.
    assert_eq!(fa, 0.0, "the floor allocated — the counter is seeing the peer thread");
    // Seven is the common 13-byte response's ceiling. Larger BODY= probes intentionally exercise
    // buffer growth, so their allocation count is data for the size comparison rather than this
    // small-response regression gate.
    if default_response {
        for sample in &sa {
            assert!(
                sample.allocs <= 7.0,
                "http.get allocation regression: {:.2}/request exceeds the pinned 7-allocation ceiling",
                sample.allocs
            );
        }
    }

    let spread = |xs: &[Sample]| {
        let v: Vec<f64> = xs.iter().map(|s| s.cpu_ns).collect();
        let (lo, hi) = v.iter().fold((f64::MAX, f64::MIN), |(l, h), &x| (l.min(x), h.max(x)));
        hi - lo
    };
    println!("{blocks} interleaved blocks x {per_block} requests per arm (after {warm} warm-up)");
    println!("  arm            allocs/req   fresh B/req   growth B/req   CPU ns/req   block spread");
    for (name, s, a, c) in [("floor", &sf, fa, fc), ("align", &sa, aa, ac)] {
        println!(
            "  {name:<14} {a:>10.2}  {:>12.1}  {:>13.1}  {c:>11.0}  {:>12.0}",
            med(s, |x| x.fresh_bytes),
            med(s, |x| x.growth_bytes),
            spread(s)
        );
    }
    println!(
        "\n  http.get's CPU work above the floor:  {:.0} ns/req, {aa:.2} allocations",
        ac - fc
    );
}
