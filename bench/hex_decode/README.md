# Hexadecimal group decoding

This C harness compares the actual release runtime's `hex_decode` entrypoint.
It is a local latency measurement, not a timing correctness gate.

```text
CARGO_TARGET_DIR=/path/to/shared-target bash bench/hex_decode/run.sh
CARGO_TARGET_DIR=/path/to/shared-target bash bench/hex_decode/run.sh 96
```

The script requires a writable checkout. It touches only the runtime source
mtime and rebuilds its release producer before linking the C `-O2` consumer,
so another worktree's shared top-level archive cannot masquerade as this source.
The optional case index is `(size_index * 3 + alphabet) * 4 + mode`, from 0
through 107. Output lengths are 0, 1, 2, 3, 4, 8, 32, 1024 and 65536 bytes;
alphabets are lower-case, upper-case, and alternating case per output byte.
Modes are valid, invalid first symbol, invalid last symbol, and odd length.
For empty invalid-first/last inputs, the fixture supplies `!0`/`0!`.

An independent C encoder generates the golden arbitrary bytes. Every case
warms for 200ms while comparing every returned byte, status and length. Seven
timed samples each check status, Buffer presence/length, actual call and output
byte counts, and free every result. Each sample runs 64 through 2,000,000 calls;
early rejection and odd length always use 2,000,000. Instrumented owners do not
run in timing processes.

Build this identical C source against each non-test release producer, retain
actual compiler logs and distinct executable hashes, then run each case in
baseline/candidate/candidate/baseline order. Report medians of all 14 samples
per arm and case. Run hosts sequentially without agent builds/tests; pin Linux
VM processes to one guest vCPU. Ordinary host applications remain uncontrolled.
This measures repeated-call latency, not application throughput, SIMD or RSS.

## Local comparison, 2026-10-06

Baseline: `b3e3a498b107d3c863507981762eed88bfe37311`. Both arms use Rust 1.96.1;
macOS ARM64 uses Apple clang 21.0.0, Linux ARM64 uses GCC 12.2.0. Linux runs in
a local VM, with `taskset -c 0` inherited by both processes. No agent build/test
runs overlap measurement. The first 10,000-call fixture produced submillisecond
samples with large between-process and within-process variation; the accepted
comparison uses the longer sample counts above and rebuilt producers.

Each of the 108 cases uses per-case ABBA. Two variable macOS cases receive one
additional ABBA: one-byte mixed-case success and 1 KiB mixed-case first-symbol
failure. Their reported medians retain all 28 samples/arm, including the slow
original processes; every other case uses all 14 samples/arm. Guest affinity
does not control the host scheduler, and this is not a latency guarantee.

The following valid rows use the lower-case alphabet and output byte lengths.

| Output | macOS baseline → candidate | Linux baseline → candidate |
| --- | ---: | ---: |
| Empty | 19.10 → 19.82 ns | 13.33 → 13.88 ns |
| 1 byte | 33.36 → 35.22 ns | 24.48 → 24.10 ns |
| 4 bytes | 39.55 → 35.78 ns | 30.56 → 25.12 ns |
| 8 bytes | 49.48 → 34.58 ns | 37.83 → 27.69 ns |
| 32 bytes | 106.64 → 51.30 ns | 78.44 → 41.93 ns |
| 1 KiB | 2538.05 → 670.38 ns | 1854.72 → 641.31 ns |
| 64 KiB | 162.54 → 42.79 µs | 114.28 → 38.30 µs |

Across all cases, the largest accepted macOS increase is the one-byte lower-case
success above (+1.87 ns, 5.6%); Linux's largest is empty success (+0.55 ns, 4.1%).
Odd rejection rises by at most 0.43 ns on Linux. These small costs are accepted
alongside the large-input improvement; no universal speedup is claimed. The
separate instrumented owners preserve one exact-sized private payload request
for nonempty success and zero for empty/odd input. They promise neither RSS nor
reduced allocations for invalid even-length input.

Fixture SHA256: `a712f5a23e9e1ba135e1775b75fcdd8984a702bc771000278a94a87fd85585fc`.
Measured executable identities (baseline / candidate):

```text
macOS 343602c8d0c1c1a122fca108b8cf7e895a8306559a70d5ae2b0b564c96699b05
      91bb5f426de36d175bbe4223506f8826b5e05c73fc58924a82b43f068784f57c
Linux 2663d266fbc78a342b4c426a03f4bcafb3ff2fccee23be36e3c69d557ce89b4e
      4f249baceda1e337ae46ff0e579b26f7deb97e0283d93e6b0eee2e1f9463a929
```
