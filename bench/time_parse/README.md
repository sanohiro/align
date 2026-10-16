# Named timestamp parsing

This C consumer measures the actual non-test release runtime parser entry.
It checks the returned status and complete i64 value on every call. It is a
local measurement, not a timing correctness gate.

```text
bash bench/time_parse/run.sh 0 1
```

Kinds 0–4 are RFC3339, RFC3339-ms, RFC1123, basic ISO and basic date. Groups are
0 January controls, 1 February–December, 2 leap/normalized/endpoint controls,
and 3 malformed input. The first two groups use both 2000 and 2001. All 147
fixed input/expected-result records are independent of either runtime; invalid
results must leave zero output. The endpoint/offset spellings apply only to
formats that admit them. This fixture is a timing sample, not a replacement
for the native grammar and exhaustive calendar owners.

The script requires a writable checkout and forces its release runtime producer
before linking, so a shared Cargo archive cannot silently select another tree.
The [shared native supervisor](../native_probe.md) bounds build, link and probe
work and retires descendants before removing its private scratch directory,
including on timeout, interruption and leader exit. It retains the measured
`-O2 -Wall -Wextra -Werror` link compilation. The C harness
warms the selected group for 50 ms, then records seven samples of approximately
one million calls each (a whole number of passes over every group member).
Observed call/error counts and the unsigned result checksum must match the
independent expected records; every result is also checked individually.

For paired measurements, build this identical C source against exact baseline
and candidate release producers, retain their logs and hashes, then execute
each kind/group in baseline/candidate/candidate/baseline order. Run without
concurrent builds/tests. Preserve every sample, including controls and slower
cases. The native Rust owner separately checks all 213,504 representable dates;
the source owner checks the existing whole/per-unit contract. No application,
network, allocation or portable throughput improvement is promised.

## Local comparison, 2026-10-17

Baseline: `4e2eed525be52f0502aee5ad7aef0bded29c49e2`; candidate: Plan213's
direct Gregorian day conversion. Apple M1 macOS arm64, Rust 1.96.1, release
runtime and Apple clang 21.0.0. Two complete balanced passes retain 28 samples
per arm/cell. The first pass showed small January costs; the second qualified
their size and repeated the ordinary-month gains. Every sample from both
passes contributes to these medians, in nanoseconds per complete native call.

| Format | January | February–December | Leap/normalized/endpoints | Rejected input |
| --- | ---: | ---: | ---: | ---: |
| RFC3339 | 14.579 → 14.560 | 15.928 → 14.909 | 21.069 → 20.135 | 12.413 → 12.343 |
| RFC3339-ms | 18.190 → 18.293 | 19.356 → 18.843 | 20.883 → 19.761 | 11.735 → 11.573 |
| RFC1123 | 20.191 → 20.175 | 47.364 → 46.945 | 44.355 → 43.266 | 25.052 → 24.415 |
| Basic ISO | 12.618 → 12.691 | 13.910 → 12.898 | 14.257 → 13.101 | 8.989 → 9.184 |
| Basic date | 9.002 → 9.018 | 10.317 → 9.154 | 10.656 → 9.446 | 7.864 → 7.820 |

Ordinary-month gains range from 0.9% to 11.3%. January costs are at most
0.104 ns (0.6%); the basic-ISO rejected group adds 0.195 ns (2.2%). This is
not a uniform speedup. The source corpus and input mix differ by format/group;
cross-group absolute times do not isolate calendar cost.

[All 1,120 samples](results-macos.csv) retain observed work and result checksums.
[Producer identities and recomputed medians](results-macos.json) bind the source,
runtime archives and linked executables. Initial baseline qualification and
the checked-in runner reproduction are separate from the paired samples.
An earlier cumulative-month-table candidate was rejected for repeatable January
costs; none of its samples enters this comparison. Linux timing remains unmeasured.

## RFC1123 name dispatch, 2026-10-17

Baseline `4a6d26cc5ee9e3a32c3a664fbde73dcf97b55dab` includes the calendar
arithmetic above. This separate comparison changes only RFC1123 name lookup:
exact byte-pattern dispatch replaces the weekday/month table scans. The same
147 golden records, native calls and compiler flags are retained. Both actual
release producers were recompiled before linking; no builds or tests overlap
the paired runs. Two complete per-cell ABBA passes retain all 1,120 sample rows
(28 per arm and cell), with no discarded or selectively repeated cells.

[rfc1123-dispatch-macos.csv](rfc1123-dispatch-macos.csv) owns every sample;
[rfc1123-dispatch-macos.json](rfc1123-dispatch-macos.json) records producer
identities, host/toolchain, protocol and medians. These results are separate
from the preceding calendar comparison and are not pooled with it.

| Kind | Group | Baseline ns | Candidate ns | Change |
| ---: | ---: | ---: | ---: | ---: |
| 0 | 0 | 14.562 | 14.994 | +2.97% |
| 0 | 1 | 14.899 | 14.892 | -0.05% |
| 0 | 2 | 20.145 | 20.215 | +0.34% |
| 0 | 3 | 12.346 | 12.253 | -0.76% |
| 1 | 0 | 18.297 | 18.315 | +0.10% |
| 1 | 1 | 18.847 | 18.949 | +0.54% |
| 1 | 2 | 19.764 | 20.073 | +1.57% |
| 1 | 3 | 11.579 | 11.512 | -0.58% |
| 2 | 0 | 20.190 | 17.414 | -13.75% |
| 2 | 1 | 47.145 | 17.290 | -63.33% |
| 2 | 2 | 43.277 | 17.901 | -58.64% |
| 2 | 3 | 24.401 | 13.739 | -43.70% |
| 3 | 0 | 12.700 | 12.687 | -0.11% |
| 3 | 1 | 12.909 | 12.886 | -0.18% |
| 3 | 2 | 13.094 | 13.082 | -0.09% |
| 3 | 3 | 9.186 | 8.693 | -5.37% |
| 4 | 0 | 9.021 | 8.971 | -0.55% |
| 4 | 1 | 9.168 | 9.245 | +0.84% |
| 4 | 2 | 9.445 | 9.584 | +1.46% |
| 4 | 3 | 7.819 | 7.838 | +0.23% |

All four RFC1123 groups improve by 13.7–63.3%. Other formats are controls:
the largest observed increase is RFC3339 January, +0.433 ns (3.0%), and no
uniform improvement is claimed. Both complete passes reproduce the January
cost and the RFC1123 improvement. The bounded subnanosecond control cost is
accepted alongside the 2.8–29.9 ns RFC1123 reductions. No Linux or application
throughput conclusion follows.
