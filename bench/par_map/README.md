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
With the pool, `par_map` at 100k is now in the same order as the sequential loop (the spawn cost is
gone). A `PAR_MIN_CHUNK` floor also keeps trivially-small maps on the caller (no pool round-trip).

## Result (2026-06-29, native, `PAR_MIN_CHUNK = 32768`)

```
        n    align ms      seq ms    rayon ms     vs seq   vs rayon
     1000       0.003       0.001       0.032      0.25x     11.42x
    10000       0.021       0.007       0.047      0.31x      2.21x
   100000       0.107       0.065       0.891      0.61x      8.35x
  1000000       0.600       0.654       0.986      1.09x      1.64x
```

- **Pool removed the spawn regression**: 100k went from ~7× slower (old) to the same order as the
  sequential loop; the exact ratio is workload- and host-sensitive.
- **Chunking matters for this runtime shape.** Raising `PAR_MIN_CHUNK` from 4096 to 32768 reduced
  job-submission overhead for the cheap thunk-heavy kernel (100k improved from ~0.24 ms in this
  environment to ~0.11 ms). This is a tuning fix, not the endgame.
- **The ceiling is still the per-element indirect `thunk` call.** `par_map` invokes the lifted element
  function via a function pointer *per element*, so it doesn't inline/vectorize like the sequential
  Rust loop. `par_map` is best for heavier, non-vectorizable per-element work; for cheap arithmetic,
  thunk inlining/specialization is the real lever.

## Profile finding (2026-06-29, native)

`ALIGN_BENCH_PROFILE=1 bench/par_map/run.sh native` also times an Align sequential
`s.map(work).sum()` kernel. On this cheap arithmetic workload:

```
n=1000000: align-seq 0.295 ms; par_map 0.659 ms
```

So the remaining issue is not lack of workers: the sequential fused loop is already vectorized and
cheap. `par_map` is useful for heavier/non-vectorizable work; cheap maps should either stay
sequential or get a specialized/inlined parallel body rather than the per-element thunk.

## Remaining lever (recorded): inline the per-element thunk
Same class as the builder's per-write FFI overhead — to beat vectorized Rust on cheap maps, the
element function must inline into the loop (cross-object/LTO or a specialized monomorphic emit),
enabling vectorization. The pool was the spawn fix; thunk inlining is the per-element fix.
