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
     1000       0.002       0.002       0.018      0.86x      7.55x
    10000       0.028       0.019       0.115      0.69x      4.17x
   100000       0.128       0.139       0.148      1.09x      1.16x
  1000000       0.372       0.661       0.245      1.78x      0.66x
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
    32768     cheap               1.305                1.000  1.303..1.312
    32768     heavy               1.167                1.000  1.166..1.169
    32769     cheap               1.310                1.000  1.304..1.316
    32769     heavy               1.164                1.000  1.163..1.174
    65535     cheap               1.261                1.000  1.238..1.265
    65535     heavy               1.149                1.000  1.146..1.153
    65536     cheap               1.261                1.000  1.252..1.264
    65536     heavy               1.147                1.000  1.137..1.150
    65537     cheap               1.427                1.121  1.398..1.460
    65537     heavy               1.178                1.019  1.140..1.218
    73728     cheap               1.391                1.102  1.365..1.418
    73728     heavy               1.149                0.996  1.053..1.190
    81920     cheap               1.375                1.093  1.299..1.401
    81920     heavy               1.112                0.978  0.878..1.140
    98304     cheap               1.343                1.068  1.266..1.414
    98304     heavy               1.071                0.931  1.062..1.079
   131072     cheap               1.320                1.063  1.315..1.330
   131072     heavy               0.876                0.765  0.852..0.894
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
