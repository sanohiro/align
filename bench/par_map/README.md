# `par_map` — data-parallel map/filter (Align pool vs Rust sequential / rayon)

Measures `s.par_map(work).sum()` against Rust sequential and `rayon` (work-stealing pool), varying N;
the `filter` mode measures stable count/prefix/scatter compaction separately. The Align runtime uses
a persistent worker pool and one generated typed kernel per claimed range.

```sh
bench/par_map/run.sh [baseline|v3|native|threshold|width|aggregate|chunks] # headline or threshold probe
bench/par_map/run.sh filter                  # stable filter compaction probe
bench/par_map/run.sh threshold              # threshold probe on the native target
bench/par_map/run.sh width                  # input/output width and stride probe
bench/par_map/run.sh aggregate              # runtime aggregate-like record-stride probe
bench/par_map/run.sh chunks                 # runtime chunk-header allocation probe
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
uses the same fused-reduction `par_map(...).sum()` kernel with the scheduler disabled by the benchmark-only
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

## Width and output-stride probe

`run.sh width` is a focused, benchmark-only follow-up to the threshold probe. It exercises fused
reductions at `i8`, `i32`, and `i64`, widening/narrowing reductions, and materializing maps with
different result strides. Each case checks the parallel result against its sequential Align
control, warms the persistent pool, and samples floor-δ, floor, floor+1, and floor+δ with seven
balanced pool/caller/sequential permutations. The runtime getter supplies the floor; the harness
does not duplicate the cost model. Compiler-generated aggregate shapes are intentionally not
included in this scalar probe.

Native Apple Silicon, 2026-07-26, representative invocation (8 runtime workers):

```
                   case          n         floor     median pool/seq   median pool/caller   pool/seq p10..p90
        reduce i8 -> i8     458752        524288               1.014                0.996  1.002..1.014
        reduce i8 -> i8     524288        524288               1.007                1.000  1.000..1.007
        reduce i8 -> i8     524289        524288               1.094                1.087  1.091..1.114
        reduce i8 -> i8     589824        524288               1.115                1.112  1.099..1.254
      reduce i32 -> i32     114688        131072               1.005                1.001  1.004..1.006
      reduce i32 -> i32     131072        131072               1.006                0.999  1.003..1.008
      reduce i32 -> i32     131073        131072               1.217                1.196  1.194..1.261
      reduce i32 -> i32     147456        131072               1.144                1.124  1.111..1.147
      reduce i64 -> i64      57344         65536               1.005                1.000  1.004..1.005
      reduce i64 -> i64      65536         65536               1.005                1.000  1.003..1.005
      reduce i64 -> i64      65537         65536               1.101                1.090  1.076..1.134
      reduce i64 -> i64      73728         65536               1.074                1.068  1.058..1.087
       reduce i8 -> i64     101946        116509               1.016                1.008  1.016..1.024
       reduce i8 -> i64     116509        116509               1.000                0.994  0.967..1.013
       reduce i8 -> i64     116510        116509               1.297                1.270  1.262..2.090
       reduce i8 -> i64     131072        116509               1.412                1.331  1.258..1.791
       reduce i64 -> i8     101946        116509               1.009                1.002  1.006..1.011
       reduce i64 -> i8     116509        116509               1.006                1.000  1.005..1.008
       reduce i64 -> i8     116510        116509               1.111                1.103  1.082..1.205
       reduce i64 -> i8     131072        116509               1.080                1.068  1.068..1.100
  materialize i8 -> i64     101946        116509               1.001                0.999  0.999..1.001
  materialize i8 -> i64     116509        116509               1.000                1.000  1.000..1.001
  materialize i8 -> i64     116510        116509               1.115                1.117  1.075..1.130
  materialize i8 -> i64     131072        116509               1.015                1.009  0.979..1.019
  materialize i64 -> i8     101946        116509               1.002                1.000  0.999..1.003
  materialize i64 -> i8     116509        116509               0.999                1.000  0.772..1.003
  materialize i64 -> i8     116510        116509               1.025                1.024  1.022..1.055
  materialize i64 -> i8     131072        116509               1.064                1.063  1.041..1.066
```

The byte-aware floor scales as expected: `524,288` for two one-byte strides, `131,072` for two
four-byte strides, `65,536` for two eight-byte strides, and `116,509` for the mixed one/eight-byte
cases. The first pooled size is consistently more expensive than caller-only control, including
the existing `i64` case; this is evidence for retaining a conservative boundary, not a case for
another common-floor retune. It validates scalar width and output-stride coverage while leaving
compiler-generated aggregate lowering, other hosts, and body-sensitive retuning as separate work.

## Aggregate-like runtime stride probe

`run.sh aggregate` is a focused runtime-only follow-up. It calls the existing `align_rt_par_map`
C ABI with concrete `repr(C)` records of 16, 32, 64, and 128 bytes, transforms every word, and
checks every output record against an untimed sequential oracle, then uses a weighted checksum over
every output word as the measured sink. It compares the warm pool, forced caller-only,
and Rust materializing sequential controls at floor-δ, floor, floor+1, and floor+δ with seven
balanced permutations. The runtime getter supplies each input/output-stride floor; the benchmark
does not duplicate the cost model.

This deliberately measures the scheduler's byte/stride behavior without claiming compiler support
for aggregate `par_map` forms. The current compiler still lowers aggregate, projection, and
aggregate-filter shapes sequentially. It is evidence for the runtime cost model only, and does not
justify a production retune by itself.

Native Apple Silicon, 2026-07-26, representative invocation (8 runtime workers):

```
             record          n         floor     median pool/seq   median pool/caller   pool/seq p10..p90
    record 16 bytes      28672         32768               0.984                1.004  0.979..1.045
    record 16 bytes      32768         32768               1.005                1.000  0.997..1.011
    record 16 bytes      32769         32768               1.078                1.074  1.073..1.104
    record 16 bytes      36864         32768               1.045                1.051  1.043..1.063
    record 32 bytes      14336         16384               0.999                1.001  0.997..1.018
    record 32 bytes      16384         16384               1.000                1.007  0.984..1.008
    record 32 bytes      16385         16384               1.076                1.072  1.072..1.093
    record 32 bytes      18432         16384               1.049                1.054  1.016..1.065
    record 64 bytes       7168          8192               1.005                0.998  0.937..1.010
    record 64 bytes       8192          8192               1.012                1.000  1.009..1.040
    record 64 bytes       8193          8192               1.059                1.041  1.052..1.083
    record 64 bytes       9216          8192               1.056                1.036  1.049..1.069
   record 128 bytes       3584          4096               1.056                0.989  1.050..1.056
   record 128 bytes       4096          4096               1.053                0.986  1.050..1.060
   record 128 bytes       4097          4096               1.155                1.088  1.138..1.208
   record 128 bytes       4608          4096               1.161                1.091  1.159..1.168
```

The floor follows the two-record byte volume: 32,768, 16,384, 8,192, and 4,096 elements for
16/32/64/128-byte records. The first pooled size is slower than caller-only for the narrow and
medium records and is 1.088x at the 128-byte boundary; the 128-byte pool is slightly faster at
the exact floor in this run because the caller control is memory-bound. The boundary jump and
the varied ratios are useful scheduler evidence, not a general aggregate performance claim.

## Chunk header allocation probe

`run.sh chunks` is a runtime-only measure-first probe for the explicit producer allocation retained
by `chunks`. It creates one million `i64` source elements, asks `align_rt_chunks` to allocate/fill/
free the `{ptr,len}` header array, and compares it with an allocation-free control that performs
the same chunk pointer/length cursor work. Both paths produce and validate the same checksum. The
control does not model a shipped no-header parallel implementation; it isolates the producer's
allocation and header-write cost before any production lowering change is considered.
Both timed arms use the same non-inlined pointer/alignment/order/length checksum helper, so ABI
validation does not become a one-sided timing cost. A pre-timing pass checks a fresh materialized
header buffer, and the probe uses an RAII cleanup guard for the runtime-owned buffer so a failed ABI
assertion cannot leak it.

Representative Linux x86_64 run on 2026-07-27 (32 runtime workers, 15 alternating samples, second
of two invocations with symmetric validation in both timed arms):

```
 chunk   headers   materialize ms   cursor ms   materialize/cursor
      1   1000000            2.572       2.041               1.260x
      2    500000            2.530       2.104               1.202x
      8    125000            2.550       2.103               1.212x
     64     15625            2.498       1.956               1.277x
    256      3907            2.511       2.097               1.197x
   1024       977            2.529       1.945               1.300x
```

The producer was consistently slower in the symmetric probe: the two final invocations ranged from
1.183x to 1.304x of the cursor control. This earns an end-to-end no-header chunk-range design
measurement, but not a production allocation-removal change by itself: chunk-body cost, scheduler
cost, consumer layout, and the ownership contract still need to be measured together.

## Stable filter compaction probe

The `filter` mode uses a 50% even predicate and the cheap `i64` body. It compares the generated
two-pass count/prefix/scatter path with a Rust control that materializes the same `i64` result
array before summing. Each row is the median of 15 alternating paired samples on native Apple
Silicon (2026-07-26); the ratio is Rust time divided by Align time.

```
    n   parallel ms   rust seq ms     vs Rust
    16384         0.146         0.145       0.99x
    65536         0.374         0.357       0.95x
    65537         0.151         0.269       1.78x
   131072         0.321         0.590       1.84x
  1000000         0.884         3.501       3.96x
```

The boundary rows show why the existing conservative `65,536` element floor remains useful:
caller-only sizes stay within about 5% of the materializing sequential control, while the first
pooled size amortizes the two passes and range publication. This is a directional scalar
measurement, not a claim that every predicate or selectivity wins; projection, string, chunk, and
aggregate filters remain outside the shipped parallel slice.
