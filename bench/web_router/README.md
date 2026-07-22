# `web_router` — pkg.web dispatch vs a hand-written match (W5)

The **W5 dispatch gate** for `pkg.web` (`docs/impl/pkg-design/web.md`, performance contract item 3).
Times the shipped framework router — the radix tree built once, matched per request, exactly as
`serve` drives it — against what an app would write by hand with no framework at all, over the same
six-route table and the same four request shapes.

```sh
bench/web_router/run.sh            # 200k dispatches, median of 8 adjacent AB/BA pairs
N=1000000 TRIALS=10 bench/web_router/run.sh
WEB_ROUTER_GATE=1 N=100000 TRIALS=8 bench/web_router/run.sh baseline
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

## Result (2026-07-22, native, 200k dispatches, median of 8 adjacent counterbalanced pairs)

```
  shape                               framework hand-written      ratio
  static   /v1/models                   34.8 ns       2.8 ns     12.61x
  param    /v1/models/42                49.8 ns       9.8 ns      5.08x
  wildcard /assets/css/site.css         44.6 ns      15.7 ns      2.85x
  miss     /v2/nope                     25.3 ns      10.1 ns      2.50x

  scaling (6-route vs 128-route table)
  shape                                6 routes   128 routes      ratio
  static hit / chain head               48.5 ns      51.1 ns      1.06x
  param  hit / chain head               64.1 ns      67.2 ns      1.05x
  static hit / chain tail               60.7 ns     127.6 ns      2.09x
  param  hit / chain tail               74.6 ns     139.2 ns      1.87x
  miss                                  47.4 ns      76.1 ns      1.61x
```

**The CI regression gate is met. Contract item 3's ideal 1.00x is not: the honest row now isolates
the remaining sibling-chain slope instead of mixing it with depth and chain position.**

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

### What is left, and what CI pins

- **The sibling scan is the remaining slope.** A node's static edges are a linked chain walked with
  a string compare per sibling, so dispatch is O(segments × siblings), not O(segments). A miss on a
  flat 128-route namespace still costs ~0.4 µs, and the per-route slope on a miss (~2.8 ns/route)
  is **unchanged** by the chains — the chain IS the node's children. The fix is a sibling index (a
  first-byte bucket, or a sorted edge run with a binary search): build-time work, per-request O(1)
  or O(log siblings).
- **The row now changes only table size.** Both tables contain the exact same head and tail paths at
  the same depth, and static/param are reported separately at both ends. Each number is measured as
  an adjacent pair, alternating small→large and large→small; the ratio is the median of those paired
  ratios rather than a ratio of unrelated minima. Chain-head rows stay at **1.05–1.07x**; tail rows
  expose the real scan at **1.87–2.11x**.
- **CI pins two explicit regression ceilings on Linux x86_64 baseline:** chain-head shapes must stay
  at or below **1.35x**, and every shape at or below **2.75x**. Those bounds leave 28–31% headroom
  over six native/baseline 200k runs while rejecting the old depth-mismatched row (~1.5x), a return
  to O(table) route scans (~20x), and reversed sibling order. A mutation restoring the old two-vs-
  three-segment mismatch reaches 1.59x and fails the head ceiling. The first hosted CI run measured
  1.14x at the head and 2.23x worst, within those ceilings.
- **The per-edge `Route` struct copy.** Resolving a label needs one field, but `routes[i].pattern`
  is rejected through a `slice<struct>` (`'arr[i].pattern' needs a struct array or soa`), so the
  walk binds the whole `Route` (four 16-byte views) per sibling. This multiplies the sibling scan;
  it is the compiler-side lever.
- **On the hand-written ratio:** a six-route table flatters the control, which answers a static hit
  with one string compare. The framework's job is to stay flat where a chain degrades linearly, so
  the ratio column is context, not the gate.
