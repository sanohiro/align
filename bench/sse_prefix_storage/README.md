# SSE initialized-prefix storage measurement

Baseline: `e9d5e5ee40ad4c6ec7f89bf5a70262266b0a7bd7`, with the same manual probe added. The candidate changes only the private SSE block/ID storage initialization described in [plan 145](../../docs/impl/145-sse-initialized-prefix-storage.md).

Measured on 2026-10-08: Apple M1 macOS and an ARM64 Linux container on the same host, Rust 1.96.1 release builds. Platform runs were sequential, without concurrent builds/tests. This is local parser-storage evidence, not network throughput, physical zeroing, RSS or real-consumer qualification.

The probe calls the actual `HttpSseState::prepare_call` and byte-feed/dispatch implementation. Each operation admits a control-only ID block and publishes a data event, checks every published byte against an independent literal, and records the producer-reported byte total and completed call count. Fresh mode includes work/ID allocation and Drop; reuse retains both allocations. ASCII and malformed-UTF-8 ID inputs exercise ordinary and replacement output. Caller output allocation is outside the timed loop.

Each process warms up for 100 ms, then records seven trials of 10,000 fresh or 50,000 reused operations. Two process-level cycles alternate baseline/candidate order per case and reverse it in the second cycle; the table reports medians of all 14 samples. Raw samples retain the call and byte counts. Baseline fresh allocation timings vary with allocator/page state; the improvement direction holds in both cycles, but the ratios are not portable promises.

| Output capacity | State / ID | macOS baseline → candidate ns/op | Linux baseline → candidate ns/op |
| --- | --- | ---: | ---: |
| 16 | fresh / ASCII | 1904.3 → 308.6 | 3345.4 → 266.2 |
| 16 | fresh / replacement | 1920.6 → 257.1 | 3275.9 → 200.1 |
| 16 | reuse / ASCII | 206.5 → 193.5 | 239.1 → 221.1 |
| 16 | reuse / replacement | 151.3 → 138.3 | 171.3 → 156.8 |
| 1024 | fresh / ASCII | 1923.4 → 307.0 | 3243.7 → 265.1 |
| 1024 | fresh / replacement | 1780.6 → 259.4 | 3305.4 → 203.1 |
| 1024 | reuse / ASCII | 206.2 → 188.7 | 237.1 → 224.9 |
| 1024 | reuse / replacement | 149.6 → 137.2 | 172.1 → 156.5 |
| 65536 | fresh / ASCII | 1162.0 → 312.8 | 4083.4 → 267.6 |
| 65536 | fresh / replacement | 1112.1 → 259.4 | 4048.8 → 201.1 |
| 65536 | reuse / ASCII | 205.2 → 196.7 | 239.2 → 221.4 |
| 65536 | reuse / replacement | 150.6 → 137.4 | 173.0 → 156.4 |

Fresh preparation/dispatch improves 3.7–7.5× on macOS and 12.2–20.1× on Linux in this corpus. Reuse improves 1.04–1.10× and 1.05–1.11× respectively; no measured reuse regression. Every fresh work allocation still requests exactly the caller capacity plus 262,144 bytes. The allocation owner proves zero requested zero-filled bytes and unchanged exact allocation size/count; restoring the former zero-filled acquisition fails it. This distinguishes the mechanism from timing.

Raw observations: [macOS](macos-samples.csv), [Linux](linux-samples.csv).

Reproduce one candidate case (0–11), without other build/test load:

```sh
ALIGN_SSE_PREFIX_CASE=0 scripts/cargo.sh test -p align_runtime --release --lib \
  sse_storage_tests::preparation_and_dispatch_probe -- --exact --ignored --nocapture --test-threads=1
```

Cases are grouped by capacities 16, 1,024 and 65,536. Within each group: fresh ASCII, fresh replacement, reuse ASCII, reuse replacement. A manual probe is ignored by correctness gates; timings are never test thresholds. The existing parser/transport and whole/per-unit driver suites own semantics.
