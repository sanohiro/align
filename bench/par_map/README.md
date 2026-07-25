# `par_map` — data-parallel map (Align pool vs Rust sequential / rayon)

Measures `s.par_map(work).sum()` against Rust sequential and `rayon` (work-stealing pool), varying N.
The Align runtime uses a persistent worker pool and one generated typed kernel per claimed range.

```sh
bench/par_map/run.sh [baseline|v3|native]   # headline comparison; default native
bench/par_map/run.sh threshold              # threshold probe on the native target
```

The runtime is linked as a cdylib and the harness supplies runtime-generated input. The Align and
Rust controls therefore execute the same data and the same wrapping-int body.

## Current result

Native Apple Silicon, 2026-07-25, after the whole-range kernel and `PAR_MIN_CHUNK = 65536`:

```
        n    align ms      seq ms    rayon ms     vs seq   vs rayon
     1000       0.002       0.002       0.015      0.84x      6.47x
    10000       0.023       0.019       0.031      0.82x      1.36x
   100000       0.108       0.116       0.068      1.07x      0.63x
  1000000       0.362       0.634       0.214      1.75x      0.59x
```

The exact ratio is host- and body-sensitive. The whole-range kernel removes the per-element
indirect callback from the hot loop; a direct cheap body can inline and vectorize. The pool is
useful once the body has enough work to amortize range submission and synchronization. A cheap
body can still lose to the sequential fused loop, so callers should measure before choosing the
explicit parallel form.

## Threshold probe

`run.sh threshold` warms the process-lifetime pool, repeats each size to cover roughly one million
body elements per timing, alternates `par_map`/sequential order, and reports the median of 31 paired
`par/seq` ratios plus its p10..p90 spread. It probes both a cheap vectorizable body and the heavier
headline body around the caller-only/pool boundary. Cold-start behavior remains pinned by
`crates/align_runtime/tests/par_map_cold_start.rs`.

Native Apple Silicon, one representative run after moving the boundary to 65536:

```
        n      case     median par/seq      p10..p90
    32768     cheap               1.305    1.303..1.312
    32768     heavy               1.167    1.167..1.170
    32769     cheap               1.309    1.304..1.315
    32769     heavy               1.164    1.163..1.166
    65535     cheap               1.262    1.260..1.264
    65535     heavy               1.148    1.140..1.150
    65536     cheap               1.261    1.259..1.278
    65536     heavy               1.149    1.146..1.155
    65537     cheap               1.391    1.380..1.430
    65537     heavy               1.216    1.209..1.226
    81920     cheap               1.259    1.234..1.283
    81920     heavy               1.008    1.004..1.011
    98304     cheap               1.270    1.233..1.309
    98304     heavy               0.961    0.948..0.971
   131072     cheap               1.326    1.323..1.333
   131072     heavy               0.878    0.870..0.887
```

The previous 32768 boundary entered the pool immediately at 32769 even though both bodies were
still slower than sequential in this probe. Raising it to 65536 keeps that medium-size region on
the caller and moves the heavy-body crossover toward 82–98K. This is a conservative, body-agnostic
fallback, not a universal optimum; rerun the probe on a target host before changing it again.

The benchmark's old spawn and per-element-thunk results remain historical evidence in
`docs/open-questions.md`; they are not descriptions of the current generated kernel.
