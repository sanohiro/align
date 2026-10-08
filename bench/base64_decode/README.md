# Base64 decode probe

Run `bash bench/base64_decode/run.sh` on macOS or Linux with a writable checkout
and the normal Rust toolchain. The [shared bounded runner](../native_probe.md)
owns build, link, execution and cleanup. `CARGO_TARGET_DIR` may name the shared
build cache; `CC` selects one C compiler executable. The runner refreshes the
runtime source mtime without changing its contents, builds the ordinary release
archive and links the C consumer with `-O3`. The historical plan125 comparison
below used its original `-O2` harness.

The consumer independently encodes deterministic binary input, verifies complete
decoded bytes before timing, then checks producer status, output length, and
observed call/byte totals in every sample. It warms the allocator/CPU before
sampling. CSV includes seven samples per case: lengths 0, 1, 2, 3, 8, 31, 32,
1024 and 65536 for standard padded and URL-safe unpadded text, plus early/late
invalid inputs and excess trailing padding at the two long sizes. Invalid cases use fixed larger iteration
counts so early admission remains above clock resolution. Late corruption is
four bytes from the end: it lands in the standard padded tail and in the final
full quantum for these URL-safe inputs.

Case tags are 0 = valid, 1 = invalid first symbol, 2 = invalid late symbol and
3 = all but the first encoded symbol replaced by `=`. `input_bytes` identifies
the independently encoded binary fixture size; `encoded_bytes` records the
actual text extent passed to the runtime, including in rejected cases.

For before/after comparisons, retain one executable from each revision with the
same harness and flags, verify both runtime producer builds actually ran, and
alternate baseline/candidate/candidate/baseline without concurrent local builds.
Shared-target top-level archives can otherwise belong to the last worktree that
built them even if Cargo reports the current worktree Fresh. Each arm contributes
14 samples per case. Medians include owned-buffer publication and destruction.
No timing threshold is a correctness or CI gate.

## Local qualification

2026-10-06, Rust 1.96.1, macOS ARM64 (Apple clang 21) and Linux ARM64 container.
Baseline is `183ba069`; candidate is the plan125 quantum decoder. Harness SHA256:
`9d7635b425765d34fbf51c9267e2cfe3a91beaca7f00877bd323cdfd7b7496ea`.

| Decoded payload | macOS baseline/candidate | Linux baseline/candidate |
| --- | --- | --- |
| Empty | 1.62–1.66× | 1.61–1.62× |
| 1–3 bytes | 1.00–1.07× | 0.98–1.02× |
| 8 bytes | 1.21× | 1.09× |
| 31–32 bytes | 1.51–1.53× | 1.26–1.33× |
| 1 KiB | 2.92–2.93× | 2.19–2.22× |
| 64 KiB | 3.07× | 2.06–2.08× |

The largest short-input increase is 0.55 ns/call on Linux (one decoded byte).
Early and late rejection both improve. The standard malformed-tail case now
rejects before allocation/scanning, while URL-safe late full-quantum corruption
still scans its valid prefix; do not compare those two error cases as equal
work. These are local scalar-kernel/ABI measurements, not application throughput
or a universal speed/RSS guarantee. The feature-gated allocation owner separately
proves zero private payload allocations for empty input and one for nonempty
valid input; the public buffer shell remains unchanged.

## Excess-padding admission (plan154)

Baseline: `3e2c2ec13be19e814a20f840afed69c2b75f7e9a`. Candidate: the third-pad
rejection in plan154. Both arms use the extended C harness above with `cc -O3`
and ordinary release runtime archives, with actual producer rebuilds verified.
Apple M1, macOS 27.0.1 and Linux ARM64 Docker on the same host, Rust 1.96.1.
Each host ran baseline/candidate/candidate/baseline without competing builds or
tests. All 1680 observations are retained in `padding-macos-samples.csv` and
`padding-linux-samples.csv`; no outlier is removed.

Medians in nanoseconds per complete decode/status/output/free call. Alphabet
0 is standard and 1 is URL-safe. The 65536-byte fixture becomes 87384 encoded
bytes (standard) or 87382 (URL-safe); case 3 replaces every encoded byte after
the first with `=`. Thus it exercises roughly 87 KiB of excess padding.

| Fixture bytes / alphabet / case | macOS baseline | macOS candidate | Linux baseline | Linux candidate |
| --- | ---: | ---: | ---: | ---: |
| 65536 / 0 / 3 | 28278.63 | 4.80 | 28044.90 | 4.80 |
| 65536 / 1 / 3 | 28310.35 | 4.82 | 27979.82 | 4.82 |
| 1024 / 0 / 3 | 453.80 | 4.77 | 458.49 | 4.94 |
| 0 / 0 / 0 | 17.46 | 19.29 | 15.35 | 14.99 |
| 1 / 0 / 0 | 32.78 | 32.40 | 26.90 | 26.56 |
| 1 / 1 / 0 | 31.77 | 31.95 | 25.85 | 25.57 |
| 32 / 0 / 0 | 48.50 | 48.40 | 42.21 | 41.45 |
| 65536 / 0 / 0 | 29796.88 | 29546.88 | 30423.83 | 30011.08 |
| 65536 / 1 / 0 | 29312.50 | 29500.00 | 29826.17 | 29595.68 |
| 65536 / 0 / 1 | 4.17 | 4.17 | 4.19 | 4.14 |
| 65536 / 1 / 2 | 29818.75 | 29833.85 | 30162.24 | 29726.41 |

Long excess-padding rejection becomes independent of suffix length after the
third pad. Successful large controls stay within 1.4% of baseline on these
runs. The largest short-case increase is macOS empty URL-safe input, +2.17 ns
per call; empty standard input also increases by 1.83 ns. Linux short controls
are flat or improve. This qualifies the bounded malformed-input work only,
not a universal decoding-throughput, application-latency or x86 claim.
