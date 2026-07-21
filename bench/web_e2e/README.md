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

```
                                 conns      req/s     p50 µs
  pkg.web        (32 workers)       32     491505       44.8
  Go net/http    (32 cores)         32      82910      141.0

  pkg.web        (1 worker)          1      14239       70.2
  Go net/http    (all cores)         1       9702      102.5
```

**pkg.web is 5.9× Go's `net/http` on throughput and 1.47× on single-connection latency**, same box,
same generator, same request. Align's protocol path costs 4.1 µs above the floor; Go's costs 37.7 µs.

Caveats, because a benchmark without them is advertising:

- **This is `net/http`, not Fiber.** Fiber is the reference `pkg.web` was designed against, and it
  needs Go ≥ 1.16 (`io/fs`); this box has 1.15.8, so Fiber does not build here. `net/http` is a fair
  and widely-deployed control, but the W7 line in `pkg-design/web.md` is not closed until Fiber runs.
- **Go got all 32 cores; Align was given 32 workers** for the throughput row (and 8 in `run.sh`'s
  default sweep, where it still beat Go's all-core number by 2.2×).
- **The generator is ours**, not `wrk`/`oha` — neither is installed. It is held identically against
  both sides, and one thread per connection removes the cap the first version had, but an
  independent generator is worth re-running under before quoting these numbers anywhere external.
- **WSL2 loopback** inflates the floor for everyone.
