# Escaped-output entry measurement

Run `bash bench/encoding_writes/run.sh` on an idle macOS or Linux host. The
[shared bounded runner](../native_probe.md) refreshes the ordinary release
runtime, links this C harness against the native entries, prints CSV samples,
and cleans its temporary executable. `CARGO_TARGET_DIR` and `CC` are supported;
no optional native codec/crypto libraries are needed.

Kind 0 is percent/component, 1 percent/path, 2 HTML, and 3 form. Plain and mixed
UTF-8 seeds join unreserved, dense reserved-byte and dense five-entity HTML controls. Requested sizes
are 0, 1, 8, 64, 4096 and 65536 bytes; whole seed repetitions preserve UTF-8,
and CSV records actual lengths. Empty input appears once per encoder, and
repeated rounded lengths are skipped, leaving 96 distinct cases. An independent
oracle checks exact bytes before timing. Each case warms for 100 ms, then emits
nine samples including final output allocation/free. Returned lengths, calls
and output bytes are observed, and empty/nonempty pointer shape is checked.

## HTML narrow-count qualification

Plan166 compares baseline `14f9326b` with the 32-byte/u8 HTML expansion
reduction on Linux x86_64 (WSL2, AMD Ryzen 9 5950X, CPU 0 pinned), Rust
1.96.0 and GCC 14.2. Both ordinary release runtime archives were rebuilt from
their corresponding source before linking the identical expanded C harness
with `-O3`. No local build/test work overlapped timing. The existing
`native_probe.run_phase` bounded each link/probe and retired its process group.

Four runs in ABBA order give 18 observations per arm/case.
[All 3,456 observations](html-count-linux-x86_64-samples.csv) preserve every
case, trial and producer call/output-byte count. The independent byte oracle,
returned extents and counts agree in both arms. Median microseconds per
complete HTML call, including final allocation/free:

| Input | Bytes | Baseline | Candidate | Reduction |
| --- | ---: | ---: | ---: | ---: |
| plain | 65550 | 57.095 | 39.036 | 31.6% |
| mixed | 65550 | 81.904 | 63.214 | 22.8% |
| unreserved | 65536 | 50.121 | 34.077 | 32.0% |
| dense reserved bytes | 65536 | 83.875 | 55.331 | 34.0% |
| dense HTML entities | 65540 | 147.928 | 107.944 | 27.0% |

The corresponding approximately 4-KiB HTML cases improve 22.5–34.6%.
The largest short HTML increase is one unreserved byte, 12.45 → 13.40 ns
(+0.95 ns); empty input rises 5.40 → 6.15 ns (+0.75 ns, 13.9%).
Unchanged encoder controls vary too: their long medians range from -5.2% to
+6.0%, and the largest short increase is dense component encoding at 64 bytes,
152.2 → 161.4 ns (+9.2 ns, 6.0%). These costs are retained and accepted
alongside the HTML reductions; this experiment does not establish that other
encoders have unchanged throughput. Host scheduling and code placement remain
limitations. No ARM/macOS, application-throughput, RSS or portable-speed claim
is made, and timing is not a correctness gate.

A preceding three-usize-counter implementation was rejected: its long
plain/unreserved/mixed HTML cases regressed 34.6%, 46.3% and 23.5%, despite
a 4.4% dense-entity improvement. Its separate ABBA experiment retains
[all 3,456 observations](html-count-three-counters-linux-x86_64-samples.csv).
The narrow bounded reduction avoids those three wide reduction chains.

Reproduce with ordinary release archives from the baseline and candidate,
link each against this same `main.c` and flags, then run the two executables
in baseline/candidate/candidate/baseline order pinned to the same CPU.
Use `native_probe.run_phase` for each bounded build/link/probe, and refresh
the runtime source mtime before each archive build as the normal wrapper does.
The normal `run.sh` measures the current checkout with the same corpus.

## Expansion-count qualification

Plan156 compares main `c6e7773` with the checked expansion-count implementation
on macOS ARM64 (Apple M1, Clang 21) and Linux ARM64 (Docker on the same host,
GCC 12.2, CPU 0 pinned), using Rust 1.96.1 ordinary release archives. This earlier corpus had 76 cases and omitted the dense HTML seed. Both
archives were rebuilt before linking identical harnesses with `-O3`; no agent
build/test work overlapped either measurement. Four process runs in ABBA order
give 18 observations per arm/case. The retained
[count-macos-samples.csv](count-macos-samples.csv) and
[count-linux-samples.csv](count-linux-samples.csv) each contain all 2,736
observations, including returned call/byte counts. Baseline and candidate agree
on case identities, call counts, returned extents and exact oracle bytes.

Median microseconds per call for the longest inputs:

| Input (bytes) | Encoder | macOS baseline → candidate | Linux baseline → candidate |
| --- | --- | --- | --- |
| plain (65550) | Component | 87.01 → 84.77 | 94.96 → 89.07 |
| plain (65550) | Path | 112.85 → 86.86 | 112.77 → 89.05 |
| plain (65550) | Form | 133.85 → 115.98 | 136.78 → 112.75 |
| plain (65550) | HTML control | 115.30 → 115.39 | 124.40 → 123.96 |
| mixed (65550) | Component | 121.55 → 118.05 | 132.26 → 119.19 |
| mixed (65550) | Path | 149.49 → 122.33 | 151.18 → 126.17 |
| mixed (65550) | Form | 163.11 → 137.88 | 164.33 → 140.59 |
| mixed (65550) | HTML control | 141.23 → 140.92 | 141.97 → 142.24 |
| unreserved (65536) | Component | 82.10 → 79.10 | 86.21 → 83.94 |
| unreserved (65536) | Path | 105.16 → 79.41 | 108.22 → 83.97 |
| unreserved (65536) | Form | 125.81 → 101.33 | 126.88 → 102.57 |
| unreserved (65536) | HTML control | 115.05 → 114.92 | 125.62 → 125.52 |
| dense (65536) | Component | 160.65 → 160.20 | 168.56 → 157.87 |
| dense (65536) | Path | 167.08 → 146.63 | 177.83 → 157.35 |
| dense (65536) | Form | 172.72 → 148.65 | 177.49 → 143.07 |
| dense (65536) | HTML control | 126.62 → 126.64 | 128.41 → 128.56 |

Path encoding improves 12.2–24.5% on macOS and 11.5–22.4% on Linux for these
long cases; form improves 13.3–19.5% and 14.4–19.4%, respectively. Component
improvements are smaller. Unchanged HTML provides a control. The largest
macOS short-case increase is dense 64-byte component encoding, 162.1 → 171.7 ns
(+5.9%); Linux's largest increase is unchanged HTML on dense eight-byte input,
29.85 → 31.25 ns (+4.7%). These short costs are retained and accepted alongside
the useful long-input reductions. Host scheduling, allocator behavior and
microbenchmark noise remain limitations; this is neither a timing gate nor an
application-throughput, allocation-count, RSS or portable-speed guarantee.

[Plan123's qualification](../../docs/impl/123-encoding-output-count.md) concerns the
prior destination-initialization refinement and its original smaller corpus;
they are not directly comparable with the expanded corpus here.
