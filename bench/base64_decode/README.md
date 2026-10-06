# Base64 decode probe

Run `bash bench/base64_decode/run.sh` on macOS or Linux with a writable checkout
and the normal Rust toolchain. `CARGO_TARGET_DIR` may name the shared build cache;
`CC` selects the C compiler. The script touches the runtime source mtime to force
its static-archive producer to run, then builds the non-test release runtime and
links an `-O2` C consumer in an exclusively owned temporary directory. No source
contents change. The directory and executable are removed on exit.

The consumer independently encodes deterministic binary input, verifies complete
decoded bytes before timing, then checks producer status, output length, and
observed call/byte totals in every sample. It warms the allocator/CPU before
sampling. CSV includes seven samples per case: lengths 0, 1, 2, 3, 8, 31, 32,
1024 and 65536 for standard padded and URL-safe unpadded text, plus early/late
invalid inputs at the two long sizes. Invalid cases use fixed larger iteration
counts so early admission remains above clock resolution. Late corruption is
four bytes from the end: it lands in the standard padded tail and in the final
full quantum for these URL-safe inputs.

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
