//! `bench/web_e2e` — the pkg.web W5 end-to-end gate, and the load generator W7 reuses.
//!
//! Drives HTTP servers over loopback with keep-alive'd connections and reports throughput and
//! latency. Two targets are built and run by `run.sh`:
//!
//!   `framework` — a route table handed to `web.serve`
//!   `raw`       — the same responses written directly against the `std.http` server primitive:
//!                 one accept loop, an if/else on the path, one `respond`
//!
//! Identical protocol work on both sides (same keep-alive, same framing, same runtime), so the
//! difference IS the framework's cost. `EXTERNAL=host:port` drives an already-running server
//! instead — that is how the same harness measures a Go control for W7, so both sides are held to
//! one generator.
//!
//! **One thread per connection.** The first version drove `C` connections round-robin from `T`
//! threads, which caps throughput at `T / RTT` no matter how many connections are open — it
//! measured the generator, not the server (~33k req/s, quoted nowhere now). A connection is a
//! blocking write/read loop, so it gets its own thread; the box has more cores than the default
//! connection count.

use std::io::{Read, Write};
use std::net::TcpStream;
use std::process::{Child, Command};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
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

/// What one connection's thread reports back.
#[derive(Default)]
struct ConnStats {
    requests: u64,
    errors: u64,
    /// Per-request latencies in nanoseconds. Sampled every `SAMPLE`th request so the vector stays
    /// small at millions of requests without biasing the distribution.
    samples: Vec<u32>,
}

const SAMPLE: u64 = 16;

/// Read exactly one `Content-Length`-framed response, leaving any later bytes in `buf`.
fn read_response(sock: &mut TcpStream, buf: &mut Vec<u8>, chunk: &mut [u8]) -> Result<(), ()> {
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
                return Ok(());
            }
        }
        match sock.read(chunk) {
            Ok(0) | Err(_) => return Err(()),
            Ok(n) => buf.extend_from_slice(&chunk[..n]),
        }
    }
}

struct Report {
    rps: f64,
    p50_us: f64,
    p99_us: f64,
    errors: u64,
}

/// Drive `conns` keep-alive connections against `addr` for `secs`, one thread each.
fn load(addr: (&str, u16), conns: usize, secs: u64, path: &str) -> Report {
    let req = format!("GET {path} HTTP/1.1\r\nHost: h\r\n\r\n").into_bytes();
    let done = Arc::new(AtomicBool::new(false));
    let out: Arc<Mutex<Vec<ConnStats>>> = Arc::new(Mutex::new(Vec::new()));
    let mut handles = Vec::new();
    for _ in 0..conns {
        let (done, out, req) = (done.clone(), out.clone(), req.clone());
        let (host, port) = (addr.0.to_string(), addr.1);
        handles.push(std::thread::spawn(move || {
            let mut st = ConnStats::default();
            let Ok(mut sock) = TcpStream::connect((host.as_str(), port)) else {
                st.errors += 1;
                out.lock().unwrap().push(st);
                return;
            };
            sock.set_nodelay(true).ok();
            sock.set_read_timeout(Some(Duration::from_secs(10))).ok();
            let (mut buf, mut chunk) = (Vec::new(), [0u8; 4096]);
            while !done.load(Ordering::Relaxed) {
                let t0 = Instant::now();
                if sock.write_all(&req).is_err() || read_response(&mut sock, &mut buf, &mut chunk).is_err() {
                    st.errors += 1;
                    break;
                }
                st.requests += 1;
                if st.requests % SAMPLE == 0 {
                    st.samples.push(t0.elapsed().as_nanos().min(u32::MAX as u128) as u32);
                }
            }
            out.lock().unwrap().push(st);
        }));
    }
    let start = Instant::now();
    std::thread::sleep(Duration::from_secs(secs));
    done.store(true, Ordering::Relaxed);
    for h in handles {
        let _ = h.join();
    }
    let elapsed = start.elapsed().as_secs_f64();
    let stats = out.lock().unwrap();
    let (requests, errors): (u64, u64) = stats.iter().fold((0, 0), |(r, e), s| (r + s.requests, e + s.errors));
    let mut samples: Vec<u32> = stats.iter().flat_map(|s| s.samples.iter().copied()).collect();
    samples.sort_unstable();
    let pct = |p: f64| -> f64 {
        if samples.is_empty() {
            return f64::NAN;
        }
        samples[((samples.len() as f64 * p) as usize).min(samples.len() - 1)] as f64 / 1000.0
    };
    Report { rps: requests as f64 / elapsed, p50_us: pct(0.50), p99_us: pct(0.99), errors }
}

/// Spawn a server and wait until it answers, so no start-up noise lands in the measurement.
fn start(exe: &str, port: u16, workers: usize) -> Server {
    let child = Command::new(exe)
        .args(["--port", &port.to_string(), "--workers", &workers.to_string()])
        .spawn()
        .unwrap_or_else(|e| panic!("spawn {exe}: {e}"));
    await_ready("127.0.0.1", port, exe);
    Server(child)
}

fn await_ready(host: &str, port: u16, what: &str) {
    let deadline = Instant::now() + Duration::from_secs(30);
    loop {
        if let Ok(mut s) = TcpStream::connect((host, port)) {
            s.set_read_timeout(Some(Duration::from_secs(5))).ok();
            if s.write_all(b"GET /plaintext HTTP/1.1\r\nHost: h\r\nConnection: close\r\n\r\n").is_ok() {
                let mut v = Vec::new();
                let _ = s.read_to_end(&mut v);
                let text = String::from_utf8_lossy(&v);
                assert!(text.contains("Hello, World!"), "{what} must answer /plaintext: {text:?}");
                return;
            }
        }
        assert!(Instant::now() < deadline, "{what} never came up");
        std::thread::sleep(Duration::from_millis(25));
    }
}

/// The FLOOR: a minimal Rust HTTP/1.1 server on the same box, same generator — read a request,
/// write a canned response, keep the connection. It does no routing, no parse beyond finding the
/// blank line, and no allocation per request. Whatever it costs is the loopback + generator +
/// kernel floor; anything an Align server costs ABOVE it is Align's protocol path, which is the
/// only way to price that path without `strace`/`perf` (neither is on this box).
fn spawn_floor(port: u16, threads: usize) -> Arc<AtomicBool> {
    let stop = Arc::new(AtomicBool::new(false));
    let listener = std::net::TcpListener::bind(("127.0.0.1", port)).expect("floor bind");
    for _ in 0..threads.max(1) {
        let listener = listener.try_clone().expect("clone listener");
        let stop = stop.clone();
        std::thread::spawn(move || {
            const RESP: &[u8] = b"HTTP/1.1 200 OK\r\nContent-Type: text/plain; charset=utf-8\r\nContent-Length: 13\r\n\r\nHello, World!";
            while !stop.load(Ordering::Relaxed) {
                let Ok((mut sock, _)) = listener.accept() else { break };
                sock.set_nodelay(true).ok();
                let stop = stop.clone();
                std::thread::spawn(move || {
                    let mut buf = [0u8; 4096];
                    while !stop.load(Ordering::Relaxed) {
                        // One read per request: the generator never pipelines, so a request always
                        // arrives whole in one segment on loopback.
                        match sock.read(&mut buf) {
                            Ok(0) | Err(_) => return,
                            Ok(_) => {}
                        }
                        if sock.write_all(RESP).is_err() {
                            return;
                        }
                    }
                });
            }
        });
    }
    stop
}

fn row(name: &str, workers: usize, r: &Report) {
    println!(
        "  {name:<28} {workers:>8} {:>12.0} {:>10.1} {:>10.1}",
        r.rps, r.p50_us, r.p99_us
    );
    assert!(r.errors * 20 < (r.rps as u64).max(1), "{name}: {} errors", r.errors);
}

fn main() {
    let secs: u64 = std::env::var("SECS").ok().and_then(|s| s.parse().ok()).unwrap_or(3);
    let conns: usize = std::env::var("CONNS").ok().and_then(|s| s.parse().ok()).unwrap_or(32);
    let path = std::env::var("PATH_").unwrap_or_else(|_| "/plaintext".to_string());
    let cores = std::thread::available_parallelism().map_or(1, |n| n.get());

    // `EXTERNAL=host:port` measures an already-running server (the W7 Go control) with the same
    // generator, rather than spawning Align's.
    if let Ok(ext) = std::env::var("EXTERNAL") {
        let (host, port) = ext.rsplit_once(':').expect("EXTERNAL=host:port");
        let port: u16 = port.parse().expect("EXTERNAL port");
        await_ready(host, port, &ext);
        println!("load: {conns} keep-alive connections, {secs}s, {path}\n");
        println!("  {:<28} {:>8} {:>12} {:>10} {:>10}", "server", "workers", "req/s", "p50 µs", "p99 µs");
        let r = load((host, port), conns, secs, &path);
        row(&ext, 0, &r);
        return;
    }

    let fw_exe = std::env::var("ALIGN_FRAMEWORK_EXE").expect("set via run.sh");
    let raw_exe = std::env::var("ALIGN_RAW_EXE").expect("set via run.sh");
    println!("load: {conns} keep-alive connections (one thread each), {secs}s per run, {path}\n");
    println!("  {:<28} {:>8} {:>12} {:>10} {:>10}", "server", "workers", "req/s", "p50 µs", "p99 µs");

    let mut rows: Vec<(usize, f64, f64)> = Vec::new(); // (workers, framework rps, raw rps)
    let worker_counts = if cores >= 2 { vec![1usize, cores.min(8)] } else { vec![1usize] };
    for workers in worker_counts {
        let port = free_port();
        let fw = {
            let _s = start(&fw_exe, port, workers);
            load(("127.0.0.1", port), conns, secs, &path)
        };
        row("framework (pkg.web)", workers, &fw);
        let port = free_port();
        let raw = {
            let _s = start(&raw_exe, port, workers);
            load(("127.0.0.1", port), conns, secs, &path)
        };
        row("raw (std.http loop)", workers, &raw);
        rows.push((workers, fw.rps, raw.rps));
    }

    // The floor, measured last so it cannot warm anything for the servers under test.
    let port = free_port();
    let floor_stop = spawn_floor(port, 1);
    await_ready("127.0.0.1", port, "floor");
    let floor = load(("127.0.0.1", port), conns, secs, &path);
    row("floor (minimal Rust)", 1, &floor);
    floor_stop.store(true, Ordering::Relaxed);

    println!("\n  framework / raw — the framework's cost, the W5 gate:");
    for (workers, fw, raw) in &rows {
        println!("    {workers} worker(s): {:.3}x", fw / raw);
    }
    // What a request costs ABOVE the floor, per worker — the protocol path's own budget.
    if let Some((_, fw1, raw1)) = rows.first() {
        let us = |rps: f64| 1e6 / rps;
        println!(
            "\n  per-request cost at 1 worker: floor {:.1} µs, raw {:.1} µs, framework {:.1} µs",
            us(floor.rps), us(*raw1), us(*fw1)
        );
        println!(
            "    Align's protocol path above the floor: {:.1} µs/req  ({:.0}% of the request)",
            us(*raw1) - us(floor.rps),
            (us(*raw1) - us(floor.rps)) / us(*raw1) * 100.0
        );
    }
}
