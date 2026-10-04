# Borrowed HTTP stream output

This standalone default-release harness calls the shipped runtime through its C
entrypoints. It covers HTTP/1.0 and HTTP/1.1, plain and SSE sends, empty/one-byte/
multibyte token inputs, 4 KiB and 256 KiB blocks. Each connection measures its
first send and steady p50/p95 sends, first received payload, exact received wire
bytes and total receive duration. Setup and finish allocations are excluded.
Socket buffers and kernel/IPC copies are outside the payload-copy promise.

Build/run timing with `scripts/cargo.sh` from the repository root:

```text
scripts/cargo.sh build --release --manifest-path bench/http_stream/Cargo.toml
bench/http_stream/target/release/http-stream-bench > timing.csv
```

Resource measurement is a separate build and process. The thread-local counting
allocator observes runtime Rust allocations, including Vec growth. Timing does
not link the counting allocator. The native `http_stream_vectored_*` owners
additionally inspect actual payload pointers and every short-write prefix, with
thread-local allocation counters enabled. They count scripted native attempts;
OS syscall tracing, when available, reports real send/sendmsg counts separately.

```text
scripts/cargo.sh build --release --manifest-path bench/http_stream/Cargo.toml --features count-alloc
bench/http_stream/target/release/http-stream-bench > allocations.csv
```

Use the identical harness in baseline and candidate checkouts, preserving
production profile/linker/host/receiver settings. Warm each binary once, then
retain ten order-balanced baseline/baseline pairs to establish repeatability
and ten order-balanced baseline/candidate pairs. Compare first and steady
token rows independently from bulk throughput. An unexplained reproducible
text regression blocks adoption even when allocations or bulk throughput improve.
No local mock result substitutes for later real-consumer token/first-audio timing.

`python3 bench/http_stream/compare.py BASELINE_BINARY CANDIDATE_BINARY NEW_DIRECTORY`
retains the warmups and every balanced pair, hashes the binaries, and reports
paired metric deltas against the maximum absolute baseline-pair delta measured
before candidate comparisons. Zero-nanosecond clock-quantized rows use additive
deltas rather than an undefined ratio. `compare.py --summarize DIRECTORY`
reprocesses completed raw rows without launching a benchmark again.
