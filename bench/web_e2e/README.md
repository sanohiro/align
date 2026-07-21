# `web_e2e` — what pkg.web costs, and how it compares (W5 + W7)

The **W5 end-to-end gate** and the load generator **W7** reuses. Servers are driven over loopback
with keep-alive'd connections, one thread per connection:

- `framework` — a three-route table handed to `web.serve`.
- `raw` — the same responses written directly on the `std.http` server primitive: one accept loop,
  an if/else on the path, one `respond`. Same prefork shape, comparable at any worker count.
- `floor` — a minimal Rust server (read, write a canned response, keep the connection). Not a
  competitor: it is the loopback + kernel + generator floor, so Align's cost ABOVE it is Align's
  own protocol path. This box has neither `strace` nor `perf`, and this is how the path gets priced
  without them.
- `go/` — the external control (`EXTERNAL=host:port` drives any already-running server with the
  same generator).

```sh
bench/web_e2e/run.sh                      # 32 connections, 3s per run
CONNS=1 bench/web_e2e/run.sh              # pure ping-pong: latency, no queueing
SECS=10 CONNS=64 bench/web_e2e/run.sh
EXTERNAL=127.0.0.1:8080 cargo run -q --release   # measure someone else's server
```

**One thread per connection matters.** The first version drove `C` connections round-robin from `T`
threads, capping throughput at `T / RTT` however many were open — it measured the generator (~33k
req/s) and nothing else. That number is quoted nowhere now.

## Result (2026-07-21, native, WSL2, 32 cores)

### The W5 gate — what the framework costs

```
CONNS=1 (pure ping-pong: one request in flight, no queueing)
  server                        workers        req/s     p50 µs     p99 µs
  floor (minimal Rust)                1        15294       63.3      102.1
  raw (std.http loop)                 1        14400       67.0      106.6
  framework (pkg.web)                 1        14239       67.5      111.7

  per-request: floor 65.4 µs, raw 69.4 µs, framework 70.2 µs
  Align's protocol path above the floor: 4.1 µs/req
  pkg.web above raw std.http:            0.8 µs/req
```

**The gate is met with a number, not a ratio in the noise: the framework costs ~0.8 µs per request**
— radix dispatch, the method table, the handler indirection and the automatic 404/405/500 together.
At 32 connections the two are 0.98–1.00× of each other.

**And it prices everything else.** Align's whole protocol path is **4.1 µs/req**; dispatch
(`bench/web_router`) is **35 ns**, i.e. **0.9% of Align's own path** and 0.05% of a request. The
router levers recorded in that bench are worth ~0.1%; the 4.1 µs is the real budget, and the
candidates inside it are the `poll` syscall keep-alive adds per request and sharing one buffer
between the parse and the response write.

The 65 µs floor is WSL2 loopback round-trip plus scheduling — it dwarfs every server here, which is
exactly why the ping-pong *differences*, not the absolute req/s, are the result at `CONNS=1`.

### W7 — the external comparison

Same box, same generator, same request, same 3-route table shape. Fiber is the reference `pkg.web`
was designed against; **both sides run their prefork configuration**, which is Fiber's own
recommendation for throughput and the direct analogue of `web.serve(..., workers)`.

```
                                     1 conn                  32 conns
  server                        req/s   p50 µs        req/s   p50 µs   p99 µs
  pkg.web       (32 workers)    14239     70.2       491505     44.8    210.1
  Fiber prefork (32 procs)      11945     81.2       374393     52.9    533.4
  Fiber         (goroutines)    11900     83.7        86582    127.5   6564.1
  Go net/http   (32 cores)       9702    102.5        82910    141.0   1837.0
  floor (minimal Rust)          15294     63.3            —        —        —
```

**pkg.web is 1.31× Fiber-prefork on throughput, 1.19× on single-connection latency, and 2.5× better
at the p99 tail** (210 µs vs 533 µs) — and 5.9× Go's `net/http`. Against the floor: Align's whole
protocol path costs 4.1 µs per request where Fiber's costs 17.9 µs and `net/http`'s 37.7 µs.

Fiber's non-prefork number is included because it is what `fiber.New()` gives you by default, and
the 4.3× gap between its two configurations is the more useful lesson than either number alone.

Caveats, because a benchmark without them is advertising:

- **The generator is ours** — `wrk`/`oha` are not installed on this box. It is held identically
  against every server here and drives one thread per connection, but an independent generator is
  worth re-running under before these numbers are quoted externally.
- **WSL2 loopback** inflates the floor (63 µs round-trip) for everyone, which compresses the
  1-connection ratios. The 32-connection column is where the servers, not the transport, dominate.
- **32 connections is a small load.** Fiber's published figures use hundreds; the ordering here may
  not hold at that scale, and neither side was tuned (no `GOGC`, no socket tuning, default backlog).
- Go 1.26.5, Fiber v2. Align at `--target-cpu native`, release profile.

## Negative result: the speculative read does not pay (2026-07-21)

Align's request path does exactly ONE syscall more than the floor server — `poll({parked, listener})`
before the read — so the obvious first cut at the 4.1 µs was to skip it: try a non-blocking
`recv(MSG_DONTWAIT)` on the most-recently-served connection first, feed whatever it returns straight
into the parse, and only `poll` on a miss. Implemented, tested, measured, **reverted**.

- **At `CONNS=1` it cannot win by construction.** The client is synchronous, so when the server
  returns to `accept` the next request has not been sent yet: the speculative read always misses and
  costs an extra syscall before the `poll` it was meant to replace. Measured 4.1 → 3.9 µs, inside
  the noise.
- **Under load it did not win either, and the harness cannot currently prove a 5% effect.** Adjacent
  A/B at 32 workers / 32 connections: with 371.8k, without 392.5k, then with again 437.9k — the same
  build varying by 18% run to run swamps the difference. (An earlier 491.5k reading, taken when the
  box was quieter, would have made this look like a 24% regression had it been used as the baseline;
  it was not, because the comparison was re-taken adjacently. Do that.)

Two things to carry forward rather than re-derive:

1. **`CONNS=1` ping-pong is the tool for protocol-path work here, not throughput.** Its numbers were
   stable to ~1% across runs while the 32-connection figures moved by 18%. Price a change against
   the floor at one connection; use throughput only for effects large enough to survive that noise.
2. **The remaining 4.1 µs is not one syscall.** With the poll unremovable this way, what is left to
   attack is allocation and copying: `http_read_request` starts a fresh `Vec` per request, the
   header spans are a fresh `Vec`, the builder holds `String`s, and the response is serialized into
   another fresh buffer. That is four-plus allocations on a path whose whole budget is 4.1 µs, and
   `bench/web_e2e` at `CONNS=1` can price each one.
