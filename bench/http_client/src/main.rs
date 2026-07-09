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

use align_runtime::{
    align_rt_http_client_free, align_rt_http_client_get, align_rt_http_client_new, align_rt_http_resp_free,
    align_rt_http_resp_status, HttpResponse,
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
}
