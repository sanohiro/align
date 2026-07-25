# `par_map` — data-parallel map (Align pool vs Rust sequential / rayon)

Measures `s.par_map(work).sum()` against Rust sequential and `rayon` (work-stealing pool), varying N.
The Align runtime uses a persistent worker pool and one generated typed kernel per claimed range.

```sh
bench/par_map/run.sh [baseline|v3|native|threshold]   # headline comparison or threshold probe
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
body elements per timing, alternates pool/caller-only/sequential order, and reports the median of 31
paired `pool/seq` and `pool/caller` ratios plus the `pool/seq` p10..p90 spread. The caller-only control
uses the same materializing `par_map` kernel with the scheduler disabled by the benchmark-only
`par-map-probe` runtime feature. It probes both a cheap vectorizable body and the heavier headline
body around the caller-only/pool boundary. Cold-start behavior remains pinned by
`crates/align_runtime/tests/par_map_cold_start.rs`.

Native Apple Silicon, one representative run after balancing ranges at `PAR_MIN_CHUNK = 65536`:

```
        n      case     median pool/seq  median pool/caller  pool/seq p10..p90
    32768     cheap               1.308                1.000  1.300..1.317
    32768     heavy               1.174                1.000  1.172..1.178
    32769     cheap               1.312                1.000  1.309..1.315
    32769     heavy               1.173                1.000  1.168..1.175
    65535     cheap               1.262                1.000  1.260..1.264
    65535     heavy               1.156                1.000  1.154..1.159
    65536     cheap               1.261                1.000  1.259..1.285
    65536     heavy               1.156                1.000  1.152..1.158
    65537     cheap               1.383                1.091  1.369..1.423
    65537     heavy               1.162                1.012  1.113..1.204
    73728     cheap               1.353                1.074  1.312..1.374
    73728     heavy               1.082                0.939  0.935..1.136
    81920     cheap               1.334                1.060  1.307..1.378
    81920     heavy               1.125                0.982  1.119..1.133
    98304     cheap               1.294                1.038  1.267..1.398
    98304     heavy               1.062                0.927  1.055..1.073
   131072     cheap               1.326                1.068  1.323..1.336
   131072     heavy               0.880                0.772  0.872..0.889
```

The previous 32768 boundary entered the pool immediately at 32769 even though both bodies were
still slower than sequential in this probe. Raising it to 65536 keeps that medium-size region on
the caller. The balanced range plan avoids a one-element helper at 65537; the pool is only about
1.09x the same caller-only materializing kernel for the cheap body, while the fused sequential
control remains faster. The heavy-body crossover against the caller-only materializing kernel is
near 82–98K on this host; the fused sequential comparison is a separate, later crossover. This is
a conservative, body-agnostic fallback, not a universal optimum; rerun the probe on a target host
before changing it again.

The benchmark's old spawn and per-element-thunk results remain historical evidence in
`docs/open-questions.md`; they are not descriptions of the current generated kernel.
