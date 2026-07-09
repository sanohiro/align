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
