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

Native Apple Silicon, one representative run after balancing ranges at `PAR_MIN_CHUNK = 65536`
and adding the conservative body/byte-aware floor (2026-07-26):

```
        n      case     median pool/seq  median pool/caller  pool/seq p10..p90         floor
    16384     cheap               1.033                1.000  1.032..1.039         65536
    16384     heavy               1.007                1.000  0.980..1.019         65536
    32768     cheap               1.011                1.002  0.994..1.023         65536
    32768     heavy               1.003                1.000  1.003..1.008         65536
    32769     cheap               1.008                1.000  1.006..1.013         65536
    32769     heavy               1.002                1.000  1.001..1.006         65536
    49152     cheap               1.004                1.000  1.002..1.007         65536
    49152     heavy               1.001                1.000  1.001..1.004         65536
    65535     cheap               1.003                1.000  0.997..1.003         65536
    65535     heavy               1.001                1.000  0.997..1.003         65536
    65536     cheap               1.003                1.000  1.001..1.004         65536
    65536     heavy               1.001                1.000  0.991..1.001         65536
    65537     cheap               1.097                1.091  1.010..1.177         65536
    65537     heavy               1.067                1.065  1.055..1.082         65536
    73728     cheap               1.064                1.061  1.038..1.090         65536
    73728     heavy               1.061                1.060  0.620..1.088         65536
    81920     cheap               0.974                0.971  0.923..1.000         65536
    81920     heavy               1.052                1.051  1.042..1.064         65536
    98304     cheap               0.982                0.977  0.876..1.026         65536
    98304     heavy               0.986                0.987  0.977..0.994         65536
   131072     cheap               0.943                0.944  0.875..1.059         65536
   131072     heavy               0.820                0.822  0.816..0.832         65536
   196608     cheap               1.075                1.065  0.975..1.122         65536
   196608     heavy               0.580                0.578  0.456..0.622         65536
   262144     cheap               0.721                0.722  0.677..0.790         65536
   262144     heavy               0.445                0.445  0.401..0.452         65536
```

The previous 32768 boundary entered the pool immediately at 32769 even though both bodies were
still slower than sequential in this probe. Raising it to 65536 keeps that medium-size region on
the caller. The balanced range plan avoids a one-element helper at 65537; the current pool/caller
ratios there are 1.091x for the cheap body and 1.065x for the heavy body. The heavy-body crossover
falls between 73,728 and 98,304 in this representative run, while the fused sequential comparison
falls below 1 at 81,920 for cheap and 98,304 for heavy. The current `i64` cheap and heavy floors are
both 65,536: the body hint is carried through MIR and the runtime, but the measured aggressive
reduction at this boundary created short ranges and regressed pool/caller by about 7%. Width-sensitive
math is pinned by runtime unit tests; a broader width/aggregate performance sweep is still required
before changing the common floor again.

The benchmark's old spawn and per-element-thunk results remain historical evidence in
`docs/open-questions.md`; they are not descriptions of the current generated kernel.
