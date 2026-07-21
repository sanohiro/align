# `web_router` — pkg.web dispatch vs a hand-written match (W5)

The **W5 dispatch gate** for `pkg.web` (`docs/impl/pkg-design/web.md`, performance contract item 3).
Times the shipped framework router — the radix tree built once, matched per request, exactly as
`serve` drives it — against what an app would write by hand with no framework at all, over the same
six-route table and the same four request shapes.

```sh
bench/web_router/run.sh            # 200k dispatches per shape, best of 7
N=1000000 TRIALS=9 bench/web_router/run.sh
```

## How it works

The router lives in `pkg.web.internal.*`, which the pkg-foundation D7 rule makes importable only
from within `pkg.web`. So `run.sh` **assembles a module tree** in a temp dir: the shipped
`apps/web/pkg/**` sources, plus `align/bench_window.align` (a `pkg.web.bench` module that forwards
to the internal router) and `align/kernel.align` (the entry unit, which holds the `--export`ed
functions — `--export` is entry-unit only). Nothing is added to the shipped package.

Both dispatchers are Align, compiled from one kernel at one `--target-cpu`, so the only difference
measured is the dispatch mechanism:

- `fw` — the framework: priority order (static > `:param` > `*wildcard`), backtracking, then the
  method table.
- `hw` — the control: a whole-path `==` chain with two `starts_with` prefixes. It resolves **less**
  than the router does (no capture, no priority resolution, no `Allow` set), so it is the fastest
  honest control for a table this small.

**The request path is runtime data.** The first version spelled it as a literal at the call site and
read 2.3 ns/op for the static shape — the loop counter alone, because the whole comparison chain
constant-folded (`bench/README.md`: "Runtime data, never literals"). Both sides now take an opaque
`shape` and index a path table with it.

## Result (2026-07-21, native, 200k dispatches, best of 7)

```
  shape                               framework hand-written      ratio
  static   /v1/models                   35.4 ns       2.3 ns     15.35x
  param    /v1/models/42                48.8 ns      10.0 ns      4.86x
  wildcard /assets/css/site.css         43.9 ns      15.6 ns      2.81x
  miss     /v2/nope                     24.8 ns       9.9 ns      2.51x

  scaling (6-route vs 128-route table)
  shape                                6 routes   128 routes      ratio
  static hit                            35.2 ns      52.1 ns      1.48x
  param  hit                            49.3 ns     144.2 ns      2.93x
  miss                                  24.9 ns      78.9 ns      3.17x
```

**The gate is NOT met, and — read the caveat below — this scaling row is not yet a clean measurement
of the property.**

### What is fixed

- **The per-request tree build.** Dispatch used to rebuild the whole radix structure on every
  request: 1319 ns/op at six routes, 708 ns/op at two. Hoisting the build into `serve` took it to
  ~57 ns/op (35 ns after the ordering fix below).
- **The method phase.** It scanned every route with a string compare per row; it now walks a
  same-claim route chain (`rnext`) built once, and compares no patterns at all.
- **Sibling order (a regression this bench's own review caught).** Edges were first appended to a
  node's chain by PREPENDING, which walks a node's children newest-first — so the first-registered
  route became the last candidate. Measured on a flat 128-route namespace: `/r0` went 23.7 →
  **394.5 ns/op (16.6× slower)** while `/r127` got 34× faster. Since the first-registered routes are
  the ones an app writes first (`/health`, `/metrics`, …), the chain now appends at the TAIL, in
  registration order. Every small-table shape got faster with it (42.6 → 35.4 ns static).

### What is left, and why this row is not yet the gate

- **The sibling scan is the remaining slope.** A node's static edges are a linked chain walked with
  a string compare per sibling, so dispatch is O(segments × siblings), not O(segments). A miss on a
  flat 128-route namespace still costs ~0.4 µs, and the per-route slope on a miss (~2.8 ns/route)
  is **unchanged** by the chains — the chain IS the node's children. The fix is a sibling index (a
  first-byte bucket, or a sorted edge run with a binary search): build-time work, per-request O(1)
  or O(log siblings).
- **This scaling row conflates three variables**, which the review demonstrated by decomposing it:
  the small table's static path is 2 segments and the large one's is 3 (**1.35× of pure depth**),
  and each shape lands at a different position in its chain. Depth-matched and head-positioned, the
  128-route table answers in **49.9 ns vs 57.0 ns** for the 6-route table at the same depth — i.e.
  *flat*. The honest measurement is the SAME path against a small and a large table, reported at
  both chain ends; that redesign is the next change here. Until then read the row as "there is a
  slope", not as its size.
- **It is a report, not a gate.** Nothing fails, nothing exits non-zero, and it is not in CI.
  Wiring a documented ceiling is part of the same follow-up.
- **The per-edge `Route` struct copy.** Resolving a label needs one field, but `routes[i].pattern`
  is rejected through a `slice<struct>` (`'arr[i].pattern' needs a struct array or soa`), so the
  walk binds the whole `Route` (four 16-byte views) per sibling. This multiplies the sibling scan;
  it is the compiler-side lever.
- **On the hand-written ratio:** a six-route table flatters the control, which answers a static hit
  with one string compare. The framework's job is to stay flat where a chain degrades linearly, so
  the ratio column is context, not the gate.
