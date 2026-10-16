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
