# UTF-8 replacement decoding probe

`bash bench/utf8_lossy/run.sh` measures the ordinary release runtime's
`align_rt_utf8_decode_lossy` entry point, including output allocation, writing
and free. Fixed independent input/output pairs cover ASCII/NUL, valid multibyte
text and literal U+FFFD, early/late malformed prefixes, mixed text and dense
invalid bytes. Whole seeds are repeated, so actual lengths are recorded instead
of claiming the requested approximate size. Empty input is measured once.

Each case checks every output byte against its oracle before measurement, warms
for 100 ms, then records seven trials. Timed calls check the actual returned
length and accumulate completed calls and output bytes. Repetitions are
`max(64, 2097152 / (input_bytes + 64))`; fixture construction and oracle comparison
are outside timing. There is no runtime allocation-count feature or test probe.

## Local comparison

Baseline: `c8b709e25cba05d39e162df386e7dbb6ff4979ea`. Candidate: plan147 in this
change. Apple M1 macOS and Linux ARM64 in Docker on the same host, Rust 1.96.1,
ordinary release archives and `cc -O3`; hosts ran sequentially without competing
builds or tests. Baseline/candidate processes ran in ABBA order. Linux retains
four processes (14 observations per arm/case). A variable macOS 4 KiB control
prompted a second complete ABBA comparison; all eight processes are retained
(28 observations per arm/case), including the slow block. Every observation is
in `macos-samples.csv` or `linux-samples.csv`.

Approximately 64 KiB input medians, microseconds per call:

| Input | macOS baseline | macOS candidate | Linux baseline | Linux candidate |
| --- | ---: | ---: | ---: | ---: |
| ASCII/NUL | 5.281 | 3.039 | 17.718 | 15.570 |
| Valid multibyte | 84.727 | 42.328 | 96.998 | 55.543 |
| Early malformed prefix | 5.547 | 5.516 | 17.983 | 17.978 |
| Late malformed prefix | 5.125 | 2.953 | 17.777 | 15.646 |
| Mixed | 171.367 | 174.445 | 174.111 | 169.137 |
| Dense invalid | 390.000 | 389.797 | 349.644 | 350.146 |

Valid inputs improve on both hosts, including short nonempty cases. Malformed
controls mostly stay close to baseline or improve. The macOS 4,112-byte early
invalid case changes from 350.60 to 360.56 ns in the combined median; its four
candidate-process medians were 633.47, 368.53, 360.56 and 354.58 ns, while the four
baseline-process medians were 350.60 ns. This unexplained variation remains part
of the evidence. Empty-call medians increase from 3.75 to 5.00 ns on macOS and
3.43 to 5.00 ns on Linux. These measurements establish a local valid-input
benefit, not universal latency improvement, application/network throughput,
RSS, x86 performance or consumer acceptance. No timing threshold gates tests.
