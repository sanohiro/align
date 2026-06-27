# `par_map` — data-parallel map (Align pool vs Rust sequential / rayon)

Measures `s.par_map(work).sum()` against Rust sequential and `rayon` (work-stealing pool), varying N.
`work` is a few wrapping-int ops per element. The Align runtime now uses a **persistent worker pool**
(`par_pool`) instead of spawning OS threads on every call.

```sh
bench/par_map/run.sh [baseline|v3|native]   # default native
```

Same plumbing as the other cargo benches; `rayon` is a dep, runtime linked as the cdylib.

## What the pool fixed

Gemini's M2 report measured the *old* `par_map` (raw `std::thread::scope` spawn per call) at **~7×
slower than sequential at N=100k** — the OS-thread creation (~tens of µs × N threads) dwarfed the work.
With the pool, `par_map` at 100k is now **≈parity with sequential** (the spawn cost is gone). A
`PAR_MIN_CHUNK` floor also keeps trivially-small maps on the caller (no pool round-trip).

## Result (2026-06-27, native)

```
        n    align ms      seq ms    rayon ms     vs seq   vs rayon
     1000       0.003       0.002       0.013      0.55x      4.58x
    10000       0.024       0.015       0.022      0.62x      0.89x
   100000       0.117       0.108       0.075      0.92x      0.64x
  1000000       1.31        1.07        0.52       0.81x      0.39x
```

- **Pool removed the spawn regression**: 100k went from ~7× slower (old) → ≈parity.
- **Still ≈sequential parity, behind `rayon` (0.4–0.6×) for this cheap work.** The ceiling is the
  **per-element indirect `thunk` call**: `par_map` invokes the lifted element function via a function
  pointer *per element*, so it doesn't inline/vectorize, where sequential/rayon inline `work` and the
  compiler vectorizes the loop. So the ~8× core parallelism is cancelled by the per-element slowdown.
  par_map wins on *heavy, non-vectorizable* per-element work (where the thunk cost dominates the call
  overhead); for cheap vectorizable arithmetic it can't beat a vectorized sequential loop.

## Remaining lever (recorded): inline the per-element thunk
Same class as the builder's per-write FFI overhead — to beat vectorized Rust on cheap maps, the
element function must inline into the loop (cross-object/LTO or a specialized monomorphic emit),
enabling vectorization. The pool was the spawn fix; thunk inlining is the per-element fix.
