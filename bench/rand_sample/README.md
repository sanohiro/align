# Sparse random sampling

Measure the actual non-test release runtime's `rng.sample` entry point, with
an independently implemented dense partial-Fisher-Yates oracle. The candidate
keeps exactly the same selected elements, ordering and final RNG state while
using a displaced-slot map when `k <= n / 512`. Other draws use contiguous
permutation scratch. No API, ABI, effect or ownership rule changes.

Run from a writable checkout:

```sh
CARGO_TARGET_DIR=/path/to/shared-target bash bench/rand_sample/run.sh
```

The script forces the runtime producer to rebuild by touching only the source
mtime, then links a C `-O2` consumer. This avoids a shared-target Cargo Fresh
fingerprint leaving another worktree's top-level static archive in place.
It acquires one private temporary directory and removes its executable on exit.
Timing uses no `alloc-count` feature. The native allocation owner separately
observes actual Rust scratch allocation calls and requested bytes.

## Corpus and observations

Twenty-six `(n,k)` cases cover empty/one-element arrays, short/dense/full draws,
both sides of the 512:1 dispatch boundary, and 65,536/1,048,576-element sources.
Elements are eight-byte binary integers. Each case has eight reproducible seeds.
Before timing, compare every output byte and the final RNG state against the
independent oracle. Every timed call verifies the returned length/null state,
first/last selected elements and all four final RNG words, then frees the result.
Printed call/byte totals derive from observed native outputs and are checked
against the requested work. Source construction and oracle work are untimed.

Each process takes seven samples per case. Repeats start at `8,388,608 / n`,
clamped to 32–50,000, with at least 4,096 calls for `k <= 8` and 256 for
`k <= 128`; empty input uses 50,000. The larger minimum prevents the fast sparse
cases from becoming only a few clock ticks. The clock is `CLOCK_MONOTONIC`.
For a paired comparison, build the baseline and candidate producers separately
with this same harness, require actual compile logs and distinct executable
hashes, run each once as warmup, then run baseline/candidate/candidate/baseline.
Do not run builds or other benchmarks concurrently. This yields fourteen
samples per arm/case; the table reports their medians.

## Local qualification, 2026-10-06

Baseline: `86b9b907` (PR #1247). Both hosts are ARM64, Rust 1.96.1.
macOS uses Apple clang 21.0.0; Linux uses GCC 12.2.0 in the local CI-parity
container.
The timing harness SHA-256 is
`9306da47ba0dfc4eadb2afd64b4159ff1f40b12097e304cf7c93decb4931299c`.

| n / k | macOS baseline → candidate | Linux baseline → candidate |
| --- | ---: | ---: |
| 512 / 1 | 82.70 → 60.67 ns | 128.52 → 70.53 ns |
| 65,536 / 8 | 11.05 → 0.242 µs | 10.81 → 0.234 µs |
| 65,536 / 128 | 7.40 → 3.19 µs | 11.18 → 3.05 µs |
| 1,048,576 / 1 | 216.45 → 0.059 µs | 179.20 → 0.071 µs |
| 1,048,576 / 128 | 212.61 → 3.11 µs | 177.71 → 3.05 µs |
| 1,048,576 / 2,048 | 118.38 → 50.17 µs | 190.51 → 49.20 µs |
| 1,048,576 / 2,049 (dense) | 112.38 → 110.06 µs | 196.70 → 187.40 µs |

Empty and short dense cases are effectively unchanged; the largest increase is
1.30 ns (1,024 / 3 on Linux). The entire valid corpus preserves exact outputs
and final state. These are local repeated-call measurements, not an application
throughput or RSS guarantee. Early exploratory samples with insufficient fast-
case repetitions were discarded before this comparison.

For fixed `k=1/8/128`, the actual scratch allocator requests 76/280/4,360 bytes
in one allocation on both hosts, independent of whether `n` is 65,536 or
1,048,576. The previous dense index vector requested `8*n` bytes on these hosts.
Zero draws allocate no scratch. These figures exclude unchanged source/output
storage, allocator metadata and RSS; the test counter is separate from timing.
