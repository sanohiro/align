# `http_client` — std.http keepalive pool vs Rust `std::net` (R6)

The **R6 completion gate** for `std.http` (see `docs/impl/std-design/http.md`). Measures the Slice-3
connection pool: consecutive `cl.get()`s to the same host:port reuse one live keepalive connection
(http.md R3) instead of reconnecting each time. The bench drives the **shipped** pool through its
C-ABI entry points (`align_rt_http_client_*`, an ordinary path dependency on `align_runtime`), so
what it times is the real runtime code, not a model.

```sh
bench/http_client/run.sh            # 30k GETs, best of 7 (defaults)
N=100000 TRIALS=9 bench/http_client/run.sh
```

## How it works

One in-process persistent **keepalive HTTP/1.1 server** on an ephemeral loopback port (one handler
thread per connection, serving every request on its conn until the client closes). Four request loops
run against it, each timed best-of-N (min — the least-noise estimator for a microbench):

- `align-pool` — one `http.client()` reused for N GETs (**keepalive — the Slice-3 default**).
- `align-nopool` — a fresh client per GET (a fresh TCP conn + handshake + teardown each request).
- `rust-keepalive` — one `std::net::TcpStream` reused for N hand-written HTTP/1.1 GETs.
- `rust-fresh` — a fresh `TcpStream` per GET.

The headline is **R3**: `align-nopool / align-pool` is the keepalive speedup, whose floor is
**1.48×** (the rail recorded in `open-questions.md`). `align-pool / rust-keepalive` shows Align is
competitive with hand-rolled Rust on the reuse path.

## Result (2026-07-10, native, loopback, N=30000, best of 7)

```
  align-pool         1964.9 ms     65.50 µs/req        15268 req/s
  align-nopool       5617.3 ms    187.24 µs/req         5341 req/s
  rust-keepalive     1920.2 ms     64.01 µs/req        15624 req/s
  rust-fresh         5509.6 ms    183.65 µs/req         5445 req/s

  keepalive speedup (align-nopool / align-pool) = 2.86x   (R3 floor: 1.48x)
  align-pool / rust-keepalive                   = 1.02x   (<1 = Align faster)

  R3 1.48x keepalive floor: MET
```

**Findings:**

- **R3 keepalive floor — MET, with margin.** The pool makes repeated GETs **2.86×** faster than
  reconnecting each time, comfortably over the 1.48× floor. On loopback the per-request cost drops
  from ~187 µs (connect + 3-way handshake + request/response + teardown) to ~66 µs (request/response
  only) — the pool eliminates exactly the connect/teardown round-trips, which is the whole point of
  R3. The floor is a conservative number from a real-network design-note measurement; loopback makes
  the connect overhead relatively larger, so the observed speedup exceeds it. (On a real network the
  absolute latencies are dominated by RTT, but the *ratio* stays ≥ the floor because a saved
  connection is a saved round-trip.)
- **Parity with hand-written Rust.** `align-pool` (65.5 µs/req) tracks `rust-keepalive` (64.0 µs/req)
  within ~2% — noise on loopback. Both are RTT/syscall-bound at this response size, and the pool adds
  only an O(1) locked `HashMap` take/put per request off the I/O path. `align-nopool` likewise tracks
  `rust-fresh`. Align's pool is not paying any structural overhead over the idiomatic Rust baseline.

Numbers are loopback-bound and machine-specific; re-run before quoting. The point is the two ratios,
not the absolute µs.

## R5 — `cl.get_many` bounded-concurrency scaling

The same harness also measures **R5** (`docs/impl/std-design/http.md` item 6): the batched
`cl.get_many(urls, max_concurrency)` path, which spawns `min(max_concurrency, urls.len())` scoped
blocking-I/O workers that claim URL indices off a shared counter and run the ordinary exchange against
the **shared** keepalive pool. The overlap win only appears against real per-request latency — a
localhost RTT ≈ 0 would mask it — so this server variant **injects a fixed sleep per request**
(default 12 ms). Three loops, best-of-N (min):

- `align-getmany` — one `cl.get_many(urls, degree)` over `GM_N` URLs at concurrency `GM_DEGREE`
  (the shipped runtime batch path via its C-ABI).
- `align-sequential` — one client, `GM_N` GETs one after another (no overlap — every request pays the
  full injected latency serially).
- `rust-pool` — a hand-written **equal-degree** Rust thread pool: `degree` threads claim URL indices
  off a shared counter, each reusing one keepalive `TcpStream`. The same bounded-concurrency shape as
  `get_many`, so the ratio against it is the honest parity number.

```sh
GM_N=64 GM_DEGREE=16 GM_LATENCY_MS=12 GM_TRIALS=5 bench/http_client/run.sh
```

### Result (2026-07-10, native, loopback, 64 GETs, degree 16, 12 ms injected latency/req, best of 3)

```
  align-getmany            50.7 ms
  align-sequential        779.9 ms
  rust-pool                50.3 ms

  overlap factor (align-sequential / align-getmany) = 15.4x   (ideal ≈ degree 16)
  align-getmany / rust-pool (equal degree)          = 1.01x   (<1 = Align faster; ~1 = parity)
```

**Findings (honest reporting — machine + degree dependent, NOT a hardware-independent claim):**

- **Machine: 32 logical cores.** The measured **overlap factor is 15.4×** at degree 16 — essentially
  the ideal `degree` (64 requests × 12 ms ÷ 16 ≈ 48 ms vs 64 × 12 ms ≈ 768 ms sequential). Because the
  work is **I/O-bound** (workers block on the injected sleep, not the CPU), the overlap tracks the
  concurrency degree and can exceed the core count — raising `GM_DEGREE` raises the factor until the
  server or the network saturates. This is why the mechanism uses a **dedicated bounded blocking-I/O
  worker pool**, not the CPU-sized `par_map` pool (which would cap overlap at core count — the wrong
  shape for I/O batching). Quote the factor *with* the degree and core count, never as a bare number.
- **Parity with hand-written Rust at equal degree.** `align-getmany` (50.7 ms) tracks `rust-pool`
  (50.3 ms) within ~1% — Align's batch path carries no structural overhead over an idiomatic Rust
  fixed thread pool. Both are latency-bound; the shared `Mutex`-guarded pool adds only an O(1) locked
  take/put per request off the I/O path.

The absolute overlap depends on `GM_DEGREE`, the injected latency, and the machine; re-run before
quoting. The point is the two ratios (overlap ≈ degree; parity ≈ 1×), not the absolute ms.
