//! R6 benchmark (http.md) — the std.http keepalive connection pool measured against a plain-Rust
//! `std::net` baseline over a localhost plaintext server. It drives the Align pool's C-ABI entry
//! points directly (`align_rt_http_client_*`), so it measures the *shipped* runtime code, and times
//! four request loops against one in-process keepalive HTTP/1.1 server:
//!
//!   1. `align-pool`    — one `http.client()` reused for N GETs      (keepalive, the Slice-3 default)
//!   2. `align-nopool`  — a fresh client per GET                     (a fresh conn each request)
//!   3. `rust-keepalive`— one `TcpStream` reused for N GETs          (hand-written HTTP/1.1)
//!   4. `rust-fresh`    — a fresh `TcpStream` per GET
//!
//! The headline is R3: `align-nopool / align-pool` is the measured keepalive speedup, whose floor is
//! **1.48×** (http.md R3). `align-pool / rust-keepalive` shows Align is competitive with hand-rolled
//! Rust on the reuse path. Best-of-N trials (min — the least-noise estimator for a microbench).

use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::sync::Arc;
use std::thread;
use std::time::{Duration, Instant};

use std::sync::atomic::{AtomicUsize, Ordering};

use align_runtime::{
    align_rt_free_response_array, align_rt_http_client_free, align_rt_http_client_get, align_rt_http_client_new,
    align_rt_http_get_many, align_rt_http_resp_free, align_rt_http_resp_status, AlignStr, HttpResponse,
};

/// Index just past the `\r\n\r\n` that ends a message head, or `None` if not yet present.
fn head_end(buf: &[u8]) -> Option<usize> {
    buf.windows(4).position(|w| w == b"\r\n\r\n").map(|p| p + 4)
}

/// Parse `Content-Length` (0 if absent) from a head slice (`buf[..head_end]`).
fn content_length(head: &[u8]) -> usize {
    let head = String::from_utf8_lossy(head).to_ascii_lowercase();
    head.lines()
        .find_map(|l| l.strip_prefix("content-length:").map(|v| v.trim().parse::<usize>().unwrap_or(0)))
        .unwrap_or(0)
}

/// A persistent loopback keepalive HTTP/1.1 server: one handler thread per accepted connection, each
/// serving every request on its conn (GETs, no request body) with `response` until the client closes.
/// Returns the ephemeral port. The thread runs for the whole process (the bench exits when `main`
/// returns).
fn spawn_server(response: Arc<Vec<u8>>) -> u16 {
    let listener = TcpListener::bind("127.0.0.1:0").expect("bind loopback");
    let port = listener.local_addr().unwrap().port();
    thread::spawn(move || {
        for stream in listener.incoming() {
            let Ok(mut sock) = stream else { continue };
            let resp = response.clone();
            thread::spawn(move || {
                sock.set_nodelay(true).ok();
                let mut buf: Vec<u8> = Vec::new();
                let mut tmp = [0u8; 4096];
                loop {
                    let end = loop {
                        if let Some(p) = head_end(&buf) {
                            break Some(p);
                        }
                        match sock.read(&mut tmp) {
                            Ok(0) | Err(_) => break None,
                            Ok(n) => buf.extend_from_slice(&tmp[..n]),
                        }
                    };
                    let Some(p) = end else { break };
                    if sock.write_all(&resp).is_err() {
                        break;
                    }
                    buf.drain(..p); // GETs carry no body; the head is the whole request
                }
            });
        }
    });
    port
}

/// Read exactly one Content-Length-framed response off `s` (draining it from `buf`).
fn read_one(s: &mut TcpStream, buf: &mut Vec<u8>, tmp: &mut [u8]) {
    loop {
        if let Some(p) = head_end(buf) {
            let total = p + content_length(&buf[..p]);
            if buf.len() >= total {
                buf.drain(..total);
                return;
            }
        }
        match s.read(tmp) {
            Ok(0) | Err(_) => return,
            Ok(n) => buf.extend_from_slice(&tmp[..n]),
        }
    }
}

/// 1. Align, pooled: one client reused for N GETs (keepalive — the Slice-3 default).
fn run_align_pool(port: u16, n: usize) {
    let url = format!("http://127.0.0.1:{port}/");
    let ub = url.as_bytes();
    unsafe {
        let client = align_rt_http_client_new();
        for _ in 0..n {
            let mut out: *mut HttpResponse = std::ptr::null_mut();
            let rc = align_rt_http_client_get(client, ub.as_ptr(), ub.len() as i64, &mut out);
            assert_eq!(rc, 0, "align-pool GET failed");
            debug_assert_eq!(align_rt_http_resp_status(out), 200);
            align_rt_http_resp_free(out);
        }
        align_rt_http_client_free(client);
    }
}

/// 2. Align, no pool: a fresh client (hence a fresh conn) per GET.
fn run_align_nopool(port: u16, n: usize) {
    let url = format!("http://127.0.0.1:{port}/");
    let ub = url.as_bytes();
    unsafe {
        for _ in 0..n {
            let client = align_rt_http_client_new();
            let mut out: *mut HttpResponse = std::ptr::null_mut();
            let rc = align_rt_http_client_get(client, ub.as_ptr(), ub.len() as i64, &mut out);
            assert_eq!(rc, 0, "align-nopool GET failed");
            align_rt_http_resp_free(out);
            align_rt_http_client_free(client);
        }
    }
}

/// 3. Rust, keepalive: one `TcpStream` reused for N hand-written GETs.
fn run_rust_keepalive(port: u16, n: usize) {
    let mut s = TcpStream::connect(("127.0.0.1", port)).expect("connect");
    s.set_nodelay(true).unwrap();
    let req = format!("GET / HTTP/1.1\r\nHost: 127.0.0.1:{port}\r\n\r\n");
    let mut buf: Vec<u8> = Vec::new();
    let mut tmp = [0u8; 4096];
    for _ in 0..n {
        s.write_all(req.as_bytes()).unwrap();
        read_one(&mut s, &mut buf, &mut tmp);
    }
}

/// 4. Rust, fresh: a fresh `TcpStream` per GET.
fn run_rust_fresh(port: u16, n: usize) {
    let req = format!("GET / HTTP/1.1\r\nHost: 127.0.0.1:{port}\r\n\r\n");
    let mut tmp = [0u8; 4096];
    for _ in 0..n {
        let mut s = TcpStream::connect(("127.0.0.1", port)).expect("connect");
        s.set_nodelay(true).unwrap();
        s.write_all(req.as_bytes()).unwrap();
        let mut buf: Vec<u8> = Vec::new();
        read_one(&mut s, &mut buf, &mut tmp);
    }
}

// --- R5: cl.get_many bounded-concurrency scaling ------------------------------------------------

/// A keepalive server that injects `latency` per request before responding (a localhost RTT ≈ 0 would
/// mask the I/O-overlap win `get_many` exists for — http.md item 6's bench directive). One handler
/// thread per connection, keepalive until the client closes.
fn spawn_latency_server(latency: Duration) -> u16 {
    let listener = TcpListener::bind("127.0.0.1:0").expect("bind loopback");
    let port = listener.local_addr().unwrap().port();
    thread::spawn(move || {
        for stream in listener.incoming() {
            let Ok(mut sock) = stream else { continue };
            thread::spawn(move || {
                sock.set_nodelay(true).ok();
                let mut buf: Vec<u8> = Vec::new();
                let mut tmp = [0u8; 4096];
                loop {
                    let end = loop {
                        if let Some(p) = head_end(&buf) {
                            break Some(p);
                        }
                        match sock.read(&mut tmp) {
                            Ok(0) | Err(_) => break None,
                            Ok(n) => buf.extend_from_slice(&tmp[..n]),
                        }
                    };
                    let Some(p) = end else { break };
                    thread::sleep(latency); // injected per-request latency (the overlap lever)
                    if sock.write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 2\r\n\r\nhi").is_err() {
                        break;
                    }
                    buf.drain(..p);
                }
            });
        }
    });
    port
}

/// Align `cl.get_many(urls, degree)` over `n` URLs at bounded concurrency `degree` — the shipped
/// runtime batch path via its C-ABI. Returns after freeing the owned `array<response>` + the client.
fn run_align_get_many(port: u16, n: usize, degree: usize) {
    let url = format!("http://127.0.0.1:{port}/");
    let views: Vec<AlignStr> = (0..n).map(|_| AlignStr { ptr: url.as_ptr(), len: url.len() as i64 }).collect();
    unsafe {
        let client = align_rt_http_client_new();
        let mut out = AlignStr { ptr: std::ptr::null(), len: 0 };
        let rc = align_rt_http_get_many(client, views.as_ptr(), views.len() as i64, degree as i64, &mut out);
        assert_eq!(rc, 0, "get_many batch failed");
        assert_eq!(out.len as usize, n, "one response per URL");
        align_rt_free_response_array(out.ptr as *mut u8, out.len);
        align_rt_http_client_free(client);
    }
}

/// Align sequential baseline: one client, `n` GETs one after another (no overlap — the pool reuses a
/// single conn, so every request pays the full injected latency serially).
fn run_align_sequential(port: u16, n: usize) {
    let url = format!("http://127.0.0.1:{port}/");
    let ub = url.as_bytes();
    unsafe {
        let client = align_rt_http_client_new();
        for _ in 0..n {
            let mut out: *mut HttpResponse = std::ptr::null_mut();
            let rc = align_rt_http_client_get(client, ub.as_ptr(), ub.len() as i64, &mut out);
            assert_eq!(rc, 0, "sequential GET failed");
            align_rt_http_resp_free(out);
        }
        align_rt_http_client_free(client);
    }
}

/// Rust baseline at **equal degree**: a fixed pool of `degree` threads claims URL indices off a shared
/// counter and runs hand-written keepalive HTTP/1.1 GETs (each thread reuses one `TcpStream`). The
/// same bounded-concurrency shape as `get_many`, so `align-getmany / rust-pool` is the parity number.
fn run_rust_pool(port: u16, n: usize, degree: usize) {
    let next = AtomicUsize::new(0);
    let req = format!("GET / HTTP/1.1\r\nHost: 127.0.0.1:{port}\r\n\r\n");
    thread::scope(|scope| {
        for _ in 0..degree.min(n) {
            let next = &next;
            let req = req.as_bytes();
            scope.spawn(move || {
                let mut s = TcpStream::connect(("127.0.0.1", port)).expect("connect");
                s.set_nodelay(true).unwrap();
                let mut buf: Vec<u8> = Vec::new();
                let mut tmp = [0u8; 4096];
                loop {
                    let i = next.fetch_add(1, Ordering::Relaxed);
                    if i >= n {
                        break;
                    }
                    s.write_all(req).unwrap();
                    read_one(&mut s, &mut buf, &mut tmp);
                }
            });
        }
    });
}

/// Best (min) wall time of `trials` runs of `f(port, n)`, after a warmup.
fn best_of(port: u16, n: usize, trials: usize, mut f: impl FnMut(u16, usize)) -> Duration {
    f(port, (n / 20).max(1)); // warmup
    let mut best = Duration::MAX;
    for _ in 0..trials {
        let t = Instant::now();
        f(port, n);
        best = best.min(t.elapsed());
    }
    best
}

fn main() {
    let n: usize = std::env::var("N").ok().and_then(|s| s.parse().ok()).unwrap_or(30_000);
    let trials: usize = std::env::var("TRIALS").ok().and_then(|s| s.parse().ok()).unwrap_or(7);

    // A small keepalive 200 (no `Connection: close` → the pool reuses the conn).
    let response = Arc::new(b"HTTP/1.1 200 OK\r\nContent-Length: 13\r\n\r\nHello, world!".to_vec());
    let port = spawn_server(response);
    // Let the listener come up.
    thread::sleep(Duration::from_millis(50));

    let ap = best_of(port, n, trials, run_align_pool);
    let an = best_of(port, n, trials, run_align_nopool);
    let rk = best_of(port, n, trials, run_rust_keepalive);
    let rf = best_of(port, n, trials, run_rust_fresh);

    let row = |name: &str, d: Duration| {
        let per = d.as_secs_f64() / n as f64;
        println!("  {name:<16} {:>8.1} ms   {:>7.2} µs/req   {:>10.0} req/s", d.as_secs_f64() * 1e3, per * 1e6, 1.0 / per);
    };
    println!("std.http R6 — {n} GETs over localhost, best of {trials}\n");
    row("align-pool", ap);
    row("align-nopool", an);
    row("rust-keepalive", rk);
    row("rust-fresh", rf);

    let keepalive_speedup = an.as_secs_f64() / ap.as_secs_f64();
    let vs_rust = ap.as_secs_f64() / rk.as_secs_f64();
    println!("\n  keepalive speedup (align-nopool / align-pool) = {keepalive_speedup:.2}x   (R3 floor: 1.48x)");
    println!("  align-pool / rust-keepalive                   = {vs_rust:.2}x   (<1 = Align faster)");
    println!(
        "\n  R3 1.48x keepalive floor: {}",
        if keepalive_speedup >= 1.48 { "MET" } else { "NOT MET (see README analysis)" }
    );

    // --- R5: cl.get_many bounded-concurrency scaling ------------------------------------------
    // The overlap win only shows against real per-request latency (localhost RTT ≈ 0), so the server
    // injects a fixed sleep per request. `get_many` at degree D overlaps ~D requests; the sequential
    // baseline pays every latency serially. Honest reporting: the MEASURED overlap factor + this
    // machine's core count + parity vs an equal-degree Rust thread pool — NOT a hardware-independent
    // 12.8x claim.
    let gm_n: usize = std::env::var("GM_N").ok().and_then(|s| s.parse().ok()).unwrap_or(64);
    let gm_degree: usize = std::env::var("GM_DEGREE").ok().and_then(|s| s.parse().ok()).unwrap_or(16);
    let gm_latency_ms: u64 = std::env::var("GM_LATENCY_MS").ok().and_then(|s| s.parse().ok()).unwrap_or(12);
    let gm_trials: usize = std::env::var("GM_TRIALS").ok().and_then(|s| s.parse().ok()).unwrap_or(5);
    let cores = std::thread::available_parallelism().map(|c| c.get()).unwrap_or(0);
    let lat_port = spawn_latency_server(Duration::from_millis(gm_latency_ms));
    thread::sleep(Duration::from_millis(50));

    let best_of_gm = |trials: usize, mut f: Box<dyn FnMut()>| -> Duration {
        f(); // warmup
        let mut best = Duration::MAX;
        for _ in 0..trials {
            let t = Instant::now();
            f();
            best = best.min(t.elapsed());
        }
        best
    };
    let gm = best_of_gm(gm_trials, Box::new(move || run_align_get_many(lat_port, gm_n, gm_degree)));
    let seq = best_of_gm(gm_trials, Box::new(move || run_align_sequential(lat_port, gm_n)));
    let rp = best_of_gm(gm_trials, Box::new(move || run_rust_pool(lat_port, gm_n, gm_degree)));

    println!(
        "\nstd.http R5 — cl.get_many: {gm_n} GETs, degree {gm_degree}, {gm_latency_ms} ms injected latency/req, best of {gm_trials}"
    );
    println!("  (machine: {cores} logical cores — I/O-bound overlap can exceed core count)\n");
    let gmrow = |name: &str, d: Duration| println!("  {name:<20} {:>8.1} ms", d.as_secs_f64() * 1e3);
    gmrow("align-getmany", gm);
    gmrow("align-sequential", seq);
    gmrow("rust-pool", rp);
    let overlap = seq.as_secs_f64() / gm.as_secs_f64();
    let vs_rust = gm.as_secs_f64() / rp.as_secs_f64();
    println!("\n  overlap factor (align-sequential / align-getmany) = {overlap:.1}x   (ideal ≈ degree {gm_degree})");
    println!("  align-getmany / rust-pool (equal degree)          = {vs_rust:.2}x   (<1 = Align faster; ~1 = parity)");
}
