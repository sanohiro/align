# `web_e2e` — what pkg.web costs, end to end (W5)

The **W5 end-to-end gate**. Two real compiled Align servers, driven over loopback with keep-alive'd
connections:

- `framework` — a three-route table handed to `web.serve`.
- `raw` — the same responses written directly on the `std.http` server primitive: one accept loop,
  an if/else on the path, one `respond`. Same prefork shape, so both are comparable at any worker
  count.

Identical protocol work on both sides (same keep-alive, same framing, same runtime, same compiler
flags), so the difference **is** the framework's cost.

```sh
bench/web_e2e/run.sh                       # 16 connections, 4 threads, 3s per run
SECS=10 CONNS=64 THREADS=8 bench/web_e2e/run.sh
```

## Result (2026-07-21, native, 16 keep-alive connections over 4 threads, 3s per run)

```
  server                            workers          req/s       µs/req
  framework (pkg.web)                     1          32956        485.5
  raw (std.http loop)                     1          31924        501.2
  framework (pkg.web)                     8          47940        333.8
  raw (std.http loop)                     8          49373        324.1

  framework w1 vs raw w1: 1.032x
  framework w8 vs raw w8: 0.971x
```

**The gate is MET: framework overhead is within ±3%, i.e. inside the noise of this harness.** At one
worker the framework even measures nominally *faster*, which is the clearest possible statement that
what it adds — radix dispatch, the method table, the handler indirection, the automatic 404/405/500
— is not visible against the cost of a request.

## What this measurement settles, and what it does not

**It settles where the time goes.** A request costs ~30 µs here; dispatch (`bench/web_router`) costs
35 ns. **The router is 0.1% of a request.** The remaining router levers recorded in that bench —
the per-node sibling index, the per-edge `Route` struct copy — are therefore worth ~0.1% at this
table size, and should not be prioritised over anything on the protocol path. That is the whole
reason to build this bench before optimising the other one further.

**It does not settle Align's absolute throughput.** ~33k req/s on loopback is low for a plaintext
server, and the load generator is the first suspect: it drives its connections round-robin from 4
threads, so the number is bounded by client-side round-trip handling, not by the server. Read the
*ratio*, which is what both sides share; do not quote the req/s as a capacity figure. The W7 Fiber
comparison needs a real load generator (`wrk`/`oha`) against both, and that is where an honest
absolute number comes from.

Two things worth measuring next on the protocol path itself, both of which this harness would show:
the `poll` syscall the keep-alive `accept` adds per request, and whether the response write can
share a buffer with the parse rather than allocating per response.
