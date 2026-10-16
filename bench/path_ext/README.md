# Path extension search

This direct native fixture contains 56 fixed, byte-counted paths with independent
result offsets and lengths for `path.base`, `path.dir` and `path.ext`. Every call
checks the exact returned pointer and length. Every timed sample also checks
the call count and an unsigned checksum against golden totals. No returned
view is allocated or freed by the fixture.

Run one operation and group:

```text
bash bench/path_ext/run.sh 2 1
python3 -B bench/test_native_probe.py
```

Operations 0/1/2 are base/dir/ext. Groups 0 through 4 are short ordinary paths,
long names without extensions, long extensions, deep prefixes with short tails,
and root/dot/Unicode/trailing-slash controls. Lengths include embedded NUL;
the fixture does not truncate paths with `strlen`.

The shared native supervisor owns build, link, run, deadlines, signals and
process-group retirement. It refreshes the actual runtime producer before
linking. Each process warms for at least 50 ms, then records seven samples
of approximately one million calls. Timing includes fixture iteration,
pointer/length checks and counting, rather than isolated native instruction
latency. All selected pointers and inputs are prepared outside each sample.

## One reverse extension search, 2026-10-17

Baseline main `8f80e3836877fdff190740d0bd890b0952dbfd1e` finds the last component
and scans it again for its last dot. The candidate searches for the last
dot-or-slash once through the existing `memchr` dependency. Empty, root and
dotfile behavior and input-bounded views remain unchanged. Base and dir remain
unchanged operation controls.

Both arms use actual non-test release runtime builds with the same C bytes and
flags on Apple M1 macOS arm64. Qualification compiled the fixture from scratch;
the checked-in C file is byte-identical to that copy. Two complete per-cell ABBA
passes ran without concurrent builds/tests. All 840 samples are retained in
[results-macos.csv](results-macos.csv); [results-macos.json](results-macos.json)
records producer identities, observed builds, executable sizes, work counts,
all 15 cell medians and per-pass results. No sample was discarded, and initial
qualification samples are excluded from the reported comparison.

| Extension group | Baseline ns/call | Candidate ns/call | Change |
| --- | ---: | ---: | ---: |
| Short ordinary | 9.3435 | 7.5430 | -19.3% |
| Long name, no extension | 264.2530 | 11.5880 | -95.6% |
| Long extension | 273.5485 | 19.1555 | -93.0% |
| Deep prefix, short tail | 8.0445 | 6.9535 | -13.6% |
| Root/dot/Unicode/trailing slash | 7.7490 | 6.7875 | -12.4% |

The largest positive control change is +0.083 ns (0.35%) for dir with a deep
prefix. Every cell meets the predeclared gate: at least 20% improvement in a
long-tail extension group, and no cell regression exceeding both 5% and 0.5 ns.
These local primitive measurements make no Linux, application-throughput or
portable-speed claim. CI checks semantic and process-lifecycle invariants,
not host-sensitive timings.
