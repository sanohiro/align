# Checked decimal parsing

This direct native probe measures the existing allocation-free
`align_rt_str_parse_i64` entry point. Its 111 fixed goldens contain short positive,
negative and explicit-plus values; i64 endpoints and overflow; long leading
zeros; and short/long invalid text, including embedded NUL and invalid UTF-8.
Every warmup and timed call checks exact status and output. Each sample also
checks call count, invalid count and an unsigned wrapping checksum against
independently accumulated golden values.

Run one group (0 through 6):

```text
bash bench/str_parse_i64/run.sh 0
python3 -B bench/test_native_probe.py
```

The shared supervisor owns the release build, native link and probe under its
existing deadlines, signals and process-group cleanup. It refreshes the actual
runtime producer before linking; it never relabels an old archive as current.
Each process warms for at least 50 ms and records seven samples of approximately
one million calls. Timing includes fixture iteration, status/value checks and
counting; it is not an isolated instruction latency measurement.

## Short-input path, 2026-10-17

The baseline is main `727ead761ae9c2556233ab0f9ef82e7680191722`.
The candidate validates and accumulates at most 18 digits without repeated
overflow checks, using the proven magnitude bound below 10^18. Longer inputs
retain the checked path. Both are actual non-test release runtime builds,
linked with the same C fixture and flags on Apple M1 macOS arm64.

Two complete per-group ABBA passes ran without concurrent builds/tests. All
392 samples are retained in [results-macos.csv](results-macos.csv), with source,
archive and executable hashes, observed build logs, sizes, work counts and
per-pass medians in [results-macos.json](results-macos.json). No sample was
discarded; initial qualification runs are not pooled into these results.

| Group | Baseline ns/call | Candidate ns/call | Change |
| --- | ---: | ---: | ---: |
| 0: short positive | 12.038 | 8.088 | -32.8% |
| 1: short negative | 12.016 | 8.336 | -30.6% |
| 2: short explicit plus | 12.094 | 8.311 | -31.3% |
| 3: endpoints and overflow | 24.890 | 25.239 | +1.4% |
| 4: leading zeros | 83.633 | 82.697 | -1.1% |
| 5: short invalid | 4.379 | 4.226 | -3.5% |
| 6: long invalid | 37.253 | 35.347 | -5.1% |

The predeclared gate requires at least 10% improvement in a short-valid group
and no group regression exceeding both 5% and 0.5 ns. All groups pass; the
endpoint cost is +0.349 ns. These are local primitive observations, with no
Linux, end-to-end application or portable throughput claim. CI checks semantic
and process-lifecycle invariants, not these host-sensitive timings.

## Validated zero prefixes, 2026-10-17

This separate comparison starts from main
`479a112258f4d53dd95c9263240bab37ddb14246`, including the short-input path above.
For inputs longer than 18 digits, Plan218 admits the initial ASCII zero run
without multiply/add arithmetic and then selects the existing scanner by the
remaining length. Original sign/nonempty admission and complete validation of
every remaining byte are preserved. No grammar or native ABI changes.

The identical 95-golden C fixture, compiler flags and actual release producers
ran on Apple M1 macOS arm64 with Rust 1.96.1 and Apple clang 21.0.0. Two complete
seven-group ABBA passes retain all 392 samples, with no concurrent builds/tests.
[Zero-prefix samples](zero-prefix-macos.csv) and
[producer identities, logs and medians](zero-prefix-macos.json) are separate
from the earlier short-input comparison.

| Group | Baseline ns/call | Candidate ns/call | Change |
| --- | ---: | ---: | ---: |
| 0: short positive | 8.106 | 8.269 | +2.0% |
| 1: short negative | 8.325 | 8.384 | +0.7% |
| 2: short explicit plus | 8.305 | 8.343 | +0.4% |
| 3: endpoints and overflow | 25.229 | 25.414 | +0.7% |
| 4: leading zeros | 82.585 | 27.363 | -66.9% |
| 5: short invalid | 4.231 | 4.523 | +6.9% |
| 6: long invalid | 35.374 | 20.233 | -42.8% |

The predeclared gate requires at least 10% improvement in the leading-zero
group and no group regression exceeding both 5% and 0.5 ns. Every group passes.
The short-invalid cost is +0.292 ns and is retained alongside the improvements;
there is no uniform speedup claim. Both passes reproduce the leading-zero and
long-invalid gains. These measurements establish neither Linux nor application
throughput improvement.

## Four-digit accumulation, 2026-10-17

This comparison starts from main
`e8e4b2d3ea7421888cbf78da44f46f1547d8e466`, including the validated zero-prefix
path above. Plan219 groups 8..18 remaining digits into four-byte coefficients,
validates all four bytes before arithmetic, and retains scalar accumulation for
shorter inputs and final remainders. Every accumulated prefix stays below 10^18.
The expanded 111-golden corpus adds 7/8-digit valid and malformed controls before
building either producer; both producers use that identical fixture.

Two complete seven-group ABBA passes retain all 392 samples on Apple M1 macOS
arm64, with no concurrent builds/tests or discarded samples. The first completed
baseline process was retained when the measurement collector's CSV-header
handling was corrected; the remaining processes continued from that checkpoint.
[Samples](digit-blocks-macos.csv) and [identities, logs and medians](digit-blocks-macos.json)
record the actual release producers and remain separate from earlier comparisons.

| Group | Baseline ns/call | Candidate ns/call | Change |
| --- | ---: | ---: | ---: |
| 0: short positive | 7.887 | 6.746 | -14.5% |
| 1: short negative | 8.016 | 6.891 | -14.0% |
| 2: short explicit plus | 7.989 | 6.824 | -14.6% |
| 3: endpoints and overflow | 25.941 | 26.004 | +0.2% |
| 4: leading zeros | 28.087 | 27.170 | -3.3% |
| 5: short invalid | 4.775 | 4.671 | -2.2% |
| 6: long invalid | 22.148 | 21.084 | -4.8% |

The predeclared gate requires at least 10% improvement in one short-valid group
and no group regression exceeding both 5% and 0.5 ns. Every group passes; both
passes reproduce the short-valid gains. The endpoint cost is +0.063 ns. These
are local primitive measurements, with no Linux or application throughput claim
and no host-sensitive timing gate in CI.
