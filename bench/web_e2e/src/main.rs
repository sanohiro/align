//! `bench/web_e2e` — the pkg.web W5 end-to-end gate.
//!
//! Drives two real compiled Align servers over loopback TCP with keep-alive'd connections and
//! reports requests/second:
//!
//!   `framework` — a route table handed to `web.serve`
//!   `raw`       — the same responses written directly against the `std.http` server primitive:
//!                 one accept loop, an if/else on the path, one `respond`
//!
//! Identical protocol work on both sides (same keep-alive, same framing, same runtime), so the
//! difference IS the framework's cost. The contract (`docs/impl/pkg-design/web.md`) asks for
//! ≈ zero overhead; this is the measurement that says what "≈ zero" is worth in context — a
//! dispatch that costs tens of nanoseconds against a request that costs tens of microseconds is
//! not where the time goes.
//!
//! The load generator is deliberately simple: `C` connections spread over `T` threads, each looping
//! "write one request, read one Content-Length-framed response" for `D` seconds. No pipelining —
//! that would measure the server's read loop, not its request path.

use std::io::{Read, Write};
use std::net::TcpStream;
use std::process::{Child, Command};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

/// A server child killed on drop — these never return on their own.
struct Server(Child);
impl Drop for Server {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

fn free_port() -> u16 {
    let probe = std::net::TcpListener::bind("127.0.0.1:0").expect("bind probe");
    let port = probe.local_addr().unwrap().port();
    drop(probe);
    port
}

/// Read exactly one `Content-Length`-framed response, leaving any later bytes in `buf`.
fn read_response(sock: &mut TcpStream, buf: &mut Vec<u8>, chunk: &mut [u8]) -> Result<usize, ()> {
    loop {
        if let Some(hp) = buf.windows(4).position(|w| w == b"\r\n\r\n") {
            let head_end = hp + 4;
            let cl = String::from_utf8_lossy(&buf[..head_end])
                .lines()
                .find_map(|l| {
                    l.to_ascii_lowercase()
                        .strip_prefix("content-length:")
                        .map(|v| v.trim().parse::<usize>().unwrap_or(0))
                })
                .unwrap_or(0);
            if buf.len() >= head_end + cl {
                buf.drain(..head_end + cl);
                return Ok(head_end + cl);
            }
        }
        match sock.read(chunk) {
            Ok(0) | Err(_) => return Err(()),
            Ok(n) => buf.extend_from_slice(&chunk[..n]),
        }
    }
}

/// Hammer `port` with `conns` keep-alive connections over `threads` threads for `secs`, returning
/// (requests, errors).
fn load(port: u16, threads: usize, conns: usize, secs: u64) -> (u64, u64) {
    let req: &[u8] = b"GET /plaintext HTTP/1.1\r\nHost: h\r\n\r\n";
    let done = Arc::new(AtomicBool::new(false));
    let total = Arc::new(AtomicU64::new(0));
    let errors = Arc::new(AtomicU64::new(0));
    let per_thread = conns.div_ceil(threads);
    let mut handles = Vec::new();
    for _ in 0..threads {
        let (done, total, errors) = (done.clone(), total.clone(), errors.clone());
        handles.push(std::thread::spawn(move || {
            // Each connection is driven in turn — one in-flight request per connection, which is
            // what a keep-alive client does.
            let mut socks: Vec<(TcpStream, Vec<u8>)> = Vec::new();
            for _ in 0..per_thread {
                match TcpStream::connect(("127.0.0.1", port)) {
                    Ok(s) => {
                        s.set_nodelay(true).ok();
                        s.set_read_timeout(Some(Duration::from_secs(10))).ok();
                        socks.push((s, Vec::new()));
                    }
                    Err(_) => {
                        errors.fetch_add(1, Ordering::Relaxed);
                    }
                };
            }
            let mut chunk = [0u8; 4096];
            let mut n = 0u64;
            while !done.load(Ordering::Relaxed) {
                for (sock, buf) in socks.iter_mut() {
                    if sock.write_all(req).is_err() || read_response(sock, buf, &mut chunk).is_err() {
                        errors.fetch_add(1, Ordering::Relaxed);
                        continue;
                    }
                    n += 1;
                }
            }
            total.fetch_add(n, Ordering::Relaxed);
        }));
    }
    std::thread::sleep(Duration::from_secs(secs));
    done.store(true, Ordering::Relaxed);
    for h in handles {
        let _ = h.join();
    }
    (total.load(Ordering::Relaxed), errors.load(Ordering::Relaxed))
}

/// Spawn a server and wait until it answers, so no connect-retry noise lands in the measurement.
fn start(exe: &str, port: u16, workers: usize) -> Server {
    let child = Command::new(exe)
        .args(["--port", &port.to_string(), "--workers", &workers.to_string()])
        .spawn()
        .unwrap_or_else(|e| panic!("spawn {exe}: {e}"));
    let deadline = Instant::now() + Duration::from_secs(30);
    loop {
        if let Ok(mut s) = TcpStream::connect(("127.0.0.1", port)) {
            s.set_read_timeout(Some(Duration::from_secs(5))).ok();
            if s.write_all(b"GET /plaintext HTTP/1.1\r\nHost: h\r\nConnection: close\r\n\r\n").is_ok() {
                let mut v = Vec::new();
                let _ = s.read_to_end(&mut v);
                let text = String::from_utf8_lossy(&v);
                assert!(text.contains("Hello, World!"), "{exe} must answer /plaintext: {text:?}");
                break;
            }
        }
        assert!(Instant::now() < deadline, "{exe} never came up");
        std::thread::sleep(Duration::from_millis(25));
    }
    Server(child)
}

fn main() {
    let fw_exe = std::env::var("ALIGN_FRAMEWORK_EXE").expect("set via run.sh");
    let raw_exe = std::env::var("ALIGN_RAW_EXE").expect("set via run.sh");
    let secs: u64 = std::env::var("SECS").ok().and_then(|s| s.parse().ok()).unwrap_or(3);
    let conns: usize = std::env::var("CONNS").ok().and_then(|s| s.parse().ok()).unwrap_or(16);
    let threads: usize = std::env::var("THREADS").ok().and_then(|s| s.parse().ok()).unwrap_or(4);
    let cores = std::thread::available_parallelism().map_or(1, |n| n.get());

    println!("load: {conns} keep-alive connections over {threads} threads, {secs}s per run\n");
    println!("  {:<28} {:>12} {:>14} {:>12}", "server", "workers", "req/s", "µs/req");

    let mut rows: Vec<(String, f64)> = Vec::new();
    for workers in [1usize, cores.min(8)] {
        for (name, exe) in [("framework (pkg.web)", &fw_exe), ("raw (std.http loop)", &raw_exe)] {
            let port = free_port();
            let _srv = start(exe, port, workers);
            let (n, errs) = load(port, threads, conns, secs);
            let rps = n as f64 / secs as f64;
            let us = if n > 0 { 1e6 / rps * conns as f64 } else { f64::NAN };
            assert!(errs * 20 < n.max(1), "{name} at {workers} workers: {errs} errors in {n} requests");
            println!("  {name:<28} {workers:>12} {rps:>14.0} {us:>12.1}");
            rows.push((format!("{name} w{workers}"), rps));
        }
        if workers == 1 && cores.min(8) == 1 {
            break;
        }
    }

    // The headline: what the framework costs against the same protocol work done by hand.
    for w in [0usize, 2] {
        if rows.len() > w + 1 {
            let (fw, raw) = (rows[w].1, rows[w + 1].1);
            println!(
                "\n  {} vs {}: {:.3}x  ({:.2} µs/req of framework overhead)",
                rows[w].0,
                rows[w + 1].0,
                fw / raw,
                (1e6 / fw - 1e6 / raw) * conns as f64
            );
        }
    }
}
