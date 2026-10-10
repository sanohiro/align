# Escaped-output entry measurement

Run `bash bench/encoding_writes/run.sh` on an idle macOS or Linux host. The
[shared bounded runner](../native_probe.md) refreshes the ordinary release
runtime, links this C harness against the native entries, prints CSV samples,
and cleans its temporary executable. `CARGO_TARGET_DIR` and `CC` are supported;
no optional native codec/crypto libraries are needed.

Kind 0 is percent/component, 1 percent/path, 2 HTML, and 3 form. Plain and mixed
UTF-8 seeds join unreserved, dense reserved-byte, dense five-entity HTML,
sparse-entity, late-entity and repeated quote/apostrophe controls. Requested sizes are 0, 1, 8, 64, 4096
and 65536 bytes; whole seed repetitions preserve UTF-8, and CSV records actual
lengths. Empty input appears once per encoder, and repeated rounded lengths
are skipped, leaving 200 distinct cases. An independent oracle checks exact
bytes before timing. Each case warms for 100 ms, then emits nine samples
including final output allocation/free. Returned lengths, calls and output
bytes are observed, and empty/nonempty pointer shape is checked.

## HTML ordinary-run copying, 2026-10-10

Baseline: `b0aa0fec8619b6e86f61bb62901ead20017d86b6`. Plan168 keeps the
shared checked count and copies ordinary output runs after sixteen ordinary
bytes, with at least 64 input bytes remaining. Both discriminator searches are
bounded to successive windows of at most 256 bytes. Each window checks its
first sixteen bytes directly before using bulk search, avoiding setup for nearby
entities. Inputs below 80 bytes compile out the run counter.

Linux x86_64 WSL2, AMD Ryzen 9 5950X, pinned to CPU 0; Rust 1.96.0 and
Debian GCC 14.2.0. Both actual producers rebuilt ordinary release archives;
identical C source used `-O3 -Wall -Wextra -Werror`. No build/test overlapped
any ABBA run. The final 200 cases contribute nine samples per process, hence
18 per arm/case and 7,200 observations, with none removed.

Final harness SHA256:
`7dcda15d4272f82bc5a6bd500a38985d6ddbd2bc48913dc7baf60b61b7365c54`.
Baseline/candidate archive SHA256:
`9034c54ba9da19e1dae2cf0a094c1e2c6d48552e8ccbfc5743aca034462c7292` /
`a7e403738023822129b7eb551af71865d9a0b948e68d3a0a9d78f4abf4fa2df1`.

The corpus adds `html_sparse`, ordinary ASCII with an entity byte every 256
bytes cycling through all five entities, and `html_late`, ordinary ASCII with
a final apostrophe. `double_quote_runs` and `apostrophe_runs` repeat sixteen
ordinary bytes and one quote; their `offset_runs` counterparts repeat 33
ordinary bytes and one quote. The independent oracle verifies complete bytes
before timing; every sample checks extent, pointer convention and exact observed
call/output counts. All four encoders consume identical fixtures, with the other
three as controls. Medians below are microseconds per complete HTML encode/free
call at approximately 64 KiB.

| Input | Baseline | Candidate | Change |
| --- | ---: | ---: | ---: |
| plain | 51.461 | 11.868 | -76.9% |
| mixed | 77.287 | 61.875 | -19.9% |
| unreserved | 48.160 | 11.565 | -76.0% |
| dense reserved bytes | 135.622 | 12.037 | -91.1% |
| dense HTML entities | 133.532 | 109.628 | -17.9% |
| sparse HTML entities | 51.182 | 16.229 | -68.3% |
| late HTML entity | 48.425 | 11.870 | -75.5% |
| double-quote runs | 52.336 | 52.569 | +0.4% |
| apostrophe runs | 52.176 | 51.162 | -1.9% |
| offset double-quote runs | 52.639 | 62.947 | +19.6% |
| offset apostrophe runs | 52.154 | 60.636 | +16.3% |

The reserved-byte seed is space/slash/plus/percent, so it needs no HTML entities.
At 4 KiB, ordinary/sparse/late cases improve 66.8–75.9%; offset quote cases
cost up to 18.3%. The largest long cost is 10.308 microseconds at 64 KiB
(+19.6%, offset double quotes). The largest short HTML increase is 8.25 ns
(64-byte reserved input, 60.80 → 69.05 ns, +13.6%). Unchanged long controls
range from -3.0% to +3.7%, with a maximum short increase of 4.65 ns.
The offset-quote and short costs are accepted alongside the substantial
ordinary-run gains; no general encoder, portable, template/application-throughput
or RSS claim follows. A separate instrumented owner verifies linear search work
for each discriminator group independently, without a timing threshold.

Two superseded experiments are retained. The reviewed unbounded-tail candidate
`3bb11b99` had 4,896 observations over 136 cases but omitted quote-only runs;
review exposed quadratic rescanning on that input, so its reported improvements
do not qualify it for adoption. Its harness SHA256 was
`fadce8f16cd5a4a99f6b8b8aaa2909f6f8a967c018a306ba2fdf822ceee58ec9`.
The first bounded-window experiment added both quote-only cases (168 cases,
6,048 observations) but still paid full search setup for nearby entities:
64-KiB double-quote/apostrophe runs rose 118.5%/105.4%. It was rejected in favor
of the direct short prefix, then the offset cases were added to qualify that
choice beyond the original witness.

All 18,144 observations remain: [final comparison](html-runs-linux-x86_64-samples.csv),
[rejected unbounded search](html-runs-unbounded-linux-x86_64-samples.csv), and
[rejected window-only search](html-runs-window-only-linux-x86_64-samples.csv).
Timing is not a correctness or CI gate.

## HTML narrow-count qualification

Plan166 compares baseline `14f9326b` with the 32-byte/u8 HTML expansion
reduction on Linux x86_64 (WSL2, AMD Ryzen 9 5950X, CPU 0 pinned), Rust
1.96.0 and GCC 14.2. Both ordinary release runtime archives were rebuilt from
their corresponding source before linking the identical expanded C harness
with `-O3`. No local build/test work overlapped timing. The existing
`native_probe.run_phase` bounded each link/probe and retired its process group.

The then-current 96-case corpus has four runs in ABBA order, giving 18
observations per arm/case.
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
