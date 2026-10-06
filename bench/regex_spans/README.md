# Regex span output storage

This C harness compares the real release runtime's `find_all` and `split`
entrypoints. It is a local measurement, not a timing correctness gate.

```text
CARGO_TARGET_DIR=/path/to/shared-target bash bench/regex_spans/run.sh
CARGO_TARGET_DIR=/path/to/shared-target bash bench/regex_spans/run.sh 48
```

The script requires a writable checkout, touches only the runtime source mtime,
then builds its release producer before linking the C `-O2` consumer. This
avoids a shared-target Cargo Fresh result leaving another checkout's top-level
archive in place. The optional case index is `((size_index * 5 + kind) * 2 +
split)`, from 0 through 49. Sizes are 0, 1, 16, 4096, and 65536 units; kinds are
no match, one `x` every 127 bytes, every-byte `x`, `a+` over `aaab` runs, and
empty matches over UTF-8 `πa` units. A UTF-8 unit is three bytes; other units
are one byte. `split` is 0 for find_all, 1 for split.

The corpus constructs every golden span independently in C. Each independently
runnable case warms the engine/allocator for 200ms and checks every output span
before timing. Seven samples each check actual status, count, first/last spans,
observed calls/output bytes, and free every result. Fast cases use at least
2,000 calls (up to 10,000); large cases use at least 32. Compilation and
instrumented allocation owners never run in a timing process.

For comparison, build this identical C harness against both release producers,
retain actual compilation logs and distinct executable hashes, then run each
case in baseline/candidate/candidate/baseline order. Per-case ordering limits
host-load drift across the corpus; medians use 14 samples per arm and case.
Report repeated-call latency, not application throughput or RSS.

## Local comparison, 2026-10-06

Baseline: `83d3f68fbc02276ca74cc39cd19cbfe1659644f3`. Both arms use Rust 1.96.1;
macOS ARM64 uses Apple clang 21.0.0 and Linux ARM64 uses GCC 12.2.0. Linux runs
inside the local VM with `taskset -c 0` inherited by both benchmark processes.
There are no concurrent agent builds/tests/measurements. Ordinary host application
load remains uncontrolled. Full-corpus process ordering showed substantial
between-process variation, so the reported comparison uses per-case ABBA and
200ms warmup. One additional ABBA checks the variable largest Linux split case;
its combined median retains all 28 samples/arm, including the slow process.
Every other row uses 14 samples/arm.

| Case | macOS baseline → candidate | Linux baseline → candidate |
| --- | ---: | ---: |
| Empty text, nonmatching split | 35.65 → 25.50 ns | 55.13 → 25.53 ns |
| One-byte match, find_all | 44.60 → 32.30 ns | 79.28 → 61.02 ns |
| 16-byte dense split | 335.50 → 319.30 ns | 380.31 → 329.38 ns |
| 4 KiB sparse find_all | 980.43 → 934.44 ns | 1490.10 → 1425.07 ns |
| 64 KiB dense find_all | 1066.05 → 1059.47 µs | 1745.27 → 1302.72 µs |
| 64 KiB `a+` find_all | 709.70 → 694.88 µs | 901.24 → 844.03 µs |
| 192 KiB Unicode empty-match find_all | 6129.03 → 6131.81 µs | 14112.97 → 13670.75 µs |
| 192 KiB Unicode empty-match split | 6268.25 → 6213.30 µs | 13997.20 → 13642.68 µs |

The largest short-input increase is Linux nonmatching one-byte find_all,
21.96 → 24.27 ns (+2.31 ns, 10.5%); its 16-byte sibling rises by 1.92 ns.
The largest macOS row increases by 0.05%. Large Linux split process medians
vary despite guest affinity; the additional confirmation is not a claim of
fixed scheduling or a latency guarantee. These results support removing
staging with small short/no-match overhead, not a universal speedup.

The native instrumented owner separately observes zero private Rust allocation
calls/bytes beyond a warmed engine walk on both hosts. Restoring the old Vec
makes it fail (one 64-byte Rust request for the first nonmatching empty split).
The C live-byte owner proves ordinary/unwind release and finish/caller transfer.
Output keeps amortized spare capacity: this removes the Rust staging vector and
final explicit copy, not every growth copy or all allocation. It promises no
exact retained capacity, peak RSS, or application throughput.

Fixture SHA256: `0982aa83ee19a74fa2219b7adf7b67942d98d5505cc88eeb3ac194bdcb93b43d`.
Measured executable identities (baseline / candidate):

```text
macOS e53addb898bd9bfd2ed8f5c3f19438e1f560f331b7db167c949b09bf5c841f31
      a9b418f76a5517a949015791262228c2146277f13a554a1ec866aecb97860da1
Linux 88106581ec153d235bee58a8a711a00cdfee76183f3a58d2d8a378e3b3903a4c
      73f0207b332eed5c97ea0e76f1b113b26bc4841a05b6ac45af5f99c7f6f26d7b
```
