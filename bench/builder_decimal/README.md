# Integer builder emission

The direct native consumer checks 81 fixed decimal goldens through the signed,
unsigned and batched str/int/str builder entries. A batch appends 128 copies of
its group into one exactly reserved ordinary builder, freezes it, compares every
output byte against independently fixed text, and frees the result. Timings
include allocation/finish/free, validation and observed call/byte accounting;
they are not isolated division instruction timings.

```text
bash bench/builder_decimal/run.sh 2
python3 -B bench/test_native_probe.py
```

The required group is one digit 0–6. The shared native supervisor refreshes and
builds the actual release runtime, links with `-O2 -Wall -Wextra -Werror`, and
bounds all phases with descendant retirement before scratch removal. Each
process warms for 50 ms and records seven samples of 1,024 complete batches.
Every warmup and measured batch checks complete bytes and lengths. No benchmark
timing threshold is a CI correctness gate.

## Paired emission, 2026-10-17

Baseline: `3e5e0d5cb8736be14c1c9f083a58f08068f8ac1c`. The candidate uses
Plan217's two-digit magnitude helper while retaining existing small signed and
unsigned dispatch. Apple M1 macOS arm64, Rust 1.96.1 and Apple clang 21.0.0.
Both actual release producers use the identical C fixture. Two full per-group
ABBA passes ran without concurrent builds/tests; all 392 samples contribute.

| Group | Baseline ns/write | Candidate ns/write | Change |
| --- | ---: | ---: | ---: |
| 0: small signed | 8.349 | 8.808 | +5.5% |
| 1: medium signed | 10.474 | 10.451 | -0.2% |
| 2: wide signed | 16.594 | 13.255 | -20.1% |
| 3: small unsigned | 7.211 | 7.418 | +2.9% |
| 4: wide unsigned | 14.876 | 12.961 | -12.9% |
| 5: small batched | 18.438 | 18.790 | +1.9% |
| 6: wide batched | 24.557 | 23.401 | -4.7% |

The predeclared admission requires a wide group to improve by at least 10%,
with no group regression exceeding both 5% and 0.5 ns. The small signed cost
is +0.4595 ns; it recurs in both passes and is retained, not dismissed as noise.
The wide signed/unsigned improvements recur too. These local primitive results
do not establish Linux, application or portable throughput improvement.

[Every sample](results-macos.csv) retains measured work;
[identities and medians](results-macos.json) retain source/archive/executable
hashes, build logs, toolchain and per-pass results. Initial qualification used
direct bounded usize conversions; the final checked-in implementation uses
u8-to-usize conversion. Its complete comparison was repeated separately.
