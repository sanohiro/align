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
     1000       0.001       0.001       0.026      0.74x     26.89x
    10000       0.010       0.007       0.039      0.70x      4.08x
   100000       0.091       0.103       0.120      1.12x      1.32x
  1000000       0.391       0.653       0.267      1.67x      0.68x
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
`par-map-probe` runtime feature; standard headline runs use the default runtime without probe
instrumentation. It probes both a cheap vectorizable body and the heavier headline body around the
caller-only/pool boundary. On a one-worker host it reports that the probe is skipped because the
runtime intentionally stays caller-only. Cold-start behavior remains pinned by
`crates/align_runtime/tests/par_map_cold_start.rs`.

Native Apple Silicon, one representative run after balancing ranges at `PAR_MIN_CHUNK = 65536`:

```
        n      case     median pool/seq  median pool/caller  pool/seq p10..p90
    32768     cheap               1.306                0.999  1.295..1.314
    32768     heavy               1.166                1.000  1.142..1.178
    32769     cheap               1.309                0.999  1.287..1.332
    32769     heavy               1.165                1.002  1.139..1.173
    65535     cheap               1.259                1.000  1.249..1.282
    65535     heavy               1.147                1.000  1.129..1.168
    65536     cheap               1.259                1.000  1.256..1.288
    65536     heavy               1.144                1.000  1.129..1.151
    65537     cheap               1.376                1.087  1.330..1.442
    65537     heavy               1.189                1.022  1.118..1.218
    73728     cheap               1.350                1.073  1.315..1.386
    73728     heavy               1.058                0.918  0.892..1.113
    81920     cheap               1.320                1.056  1.288..1.372
    81920     heavy               1.033                0.906  0.922..1.135
    98304     cheap               1.291                1.034  1.255..1.345
    98304     heavy               0.998                0.869  0.962..1.052
   131072     cheap               1.307                1.054  1.279..1.323
   131072     heavy               0.868                0.760  0.843..0.898
```

The previous 32768 boundary entered the pool immediately at 32769 even though both bodies were
still slower than sequential in this probe. Raising it to 65536 keeps that medium-size region on
the caller. The balanced range plan avoids a one-element helper at 65537; the pool is only about
1.09x the same caller-only materializing kernel for the cheap body, while the fused sequential
control remains faster. The heavy-body pool/caller crossover falls between 65,537 and 73,728 on
this representative run; the fused sequential comparison is separate and later, with pool/seq
falling below 1 at 131,072. This is a conservative, body-agnostic fallback, not a universal
optimum; rerun the probe on a target host before changing it again.

The benchmark's old spawn and per-element-thunk results remain historical evidence in
`docs/open-questions.md`; they are not descriptions of the current generated kernel.
