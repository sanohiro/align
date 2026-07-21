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
  static   /v1/models                   42.6 ns       2.3 ns     18.49x
  param    /v1/models/42                57.3 ns      10.1 ns      5.69x
  wildcard /assets/css/site.css         35.9 ns      15.6 ns      2.29x
  miss     /v2/nope                     26.6 ns       9.9 ns      2.70x

  scaling (contract item 3 — dispatch must be flat in table size)
  shape                                6 routes   128 routes      ratio
  static hit                            42.8 ns     121.4 ns      2.84x
  param  hit                            57.7 ns      63.9 ns      1.11x
  miss                                  26.3 ns      74.2 ns      2.82x
```

**The gate is NOT met yet**, and the bench now says so precisely rather than in prose. What has been
fixed and what is left:

- **Fixed: the per-request tree build.** Dispatch used to rebuild the whole radix structure on every
  request — 1319 ns/op at six routes, 708 ns/op at two, i.e. scaling with TABLE SIZE. Hoisting the
  build into `serve` took it to 57 ns/op.
- **Fixed: the O(table) scans inside dispatch.** The walk scanned every edge in the table at every
  node, and the method phase scanned every route with a string compare per row. Both are chains
  built once now — `efirst`/`enext` (this node's static edges) and `rnext` (the next route on the
  same pattern). An adversarial review measured the pre-chain code at **~0.85 ns per route** (44 ns
  at 8 routes, 453 ns at 512); the chains removed that slope.
- **Left: the sibling scan.** Dispatch is O(segments × **siblings at each node**), not O(segments):
  a node's static edges are a linked chain, walked with a string compare per sibling. The 128-route
  table has 8 children under `/api` and 8 under each group, which is the whole of the remaining
  **2.84×**. Making the contract's O(segments) literally true wants a sibling index (a first-byte
  bucket, or a sorted edge run with a binary search) — build-time work, per-request O(1) or
  O(log siblings). The `param hit` row is already 1.11× because a `:param` edge is a single slot,
  not a chain: that row is what the static rows should look like.
- **Left: the per-edge `Route` struct copy.** Resolving an edge label needs one field —
  `routes[i].pattern` — but that projection is rejected through a `slice<struct>`
  (`'arr[i].pattern' needs a struct array or soa`), so the walk binds the whole `Route` (four
  16-byte views) per sibling visited. This multiplies the sibling scan above; it is the recorded
  compiler-side lever.
- **On the hand-written ratio:** a six-route table flatters the control, which answers a static hit
  with one string compare. That ratio is not the gate — the scaling row is. The framework's job is
  to stay flat where a chain degrades linearly, and the honest comparison for the ratio column is a
  hand-written chain over 128 routes (recorded, not built: it would be 128 lines of `==`).
