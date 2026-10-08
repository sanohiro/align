# CSV quoted-field discovery probe

`bash bench/csv_quoted_scan/run.sh` measures the ordinary release
`align_rt_csv_decode_soa_v1` entry point, including arena creation, decode,
output validation and arena teardown. The schema matches the summary app's
`active: bool` and `score: i32`; a third, unselected `note` column carries the
text. Every successful row must decode to true and 7. Header-present LF input,
native descriptor tags and the canonical hash entry point use the existing ABI.

The corpus has 31 cases: five body patterns at 0, 8, 16, 64, 4096 and 65536
logical body bytes, each with one row, plus 128 rows of eight-byte quoted bodies.
`plain` is unquoted; `quoted` contains ordinary bytes; `escaped` adds one doubled
quote per 32 logical bytes; `dense_quotes` contains only doubled quotes;
`unterminated` omits the closing quote and must return Invalid with zero output.
Actual input bytes include the header, scalar columns, quoting and newlines.
Below 32 body bytes, `quoted` and `escaped` have byte-identical inputs.

Each case validates every output element before timing, then warms for at least
50 ms. Each of seven trials repeats the warm-up's completed-call count (minimum
16), so trials last approximately 50 ms. Timed calls require exact status,
returned row count and first/last row values; completed calls and producer-returned
rows accumulate independently and must match the expected work. Fixture creation,
descriptor setup and full-column validation are outside timing. No runtime
allocation-count feature or test probe participates in timing.

## Local comparison

Baseline: `a0390bf925fa3a0cf6cc367e7e89b5000163ef5f`. Candidate: plan150 in this
change. Apple M1, macOS 27.0.1 and Linux ARM64 in Docker on the same host, Rust
1.96.1, ordinary release archives and `cc -O3`. Hosts ran sequentially with no
competing builds or tests. Baseline/candidate processes ran in ABBA order;
all 1736 observations are retained in the two sample CSVs (14 observations per
arm/case/platform). No outlier is removed. Earlier short-trial exploration
motivated the uniform-duration protocol; those trials are not mixed into this
comparison.

Medians in microseconds per complete call:

| Pattern / logical body bytes / rows | macOS baseline | macOS candidate | Linux baseline | Linux candidate |
| --- | ---: | ---: | ---: | ---: |
| plain / 65536 / 1 | 147.385 | 147.365 | 151.428 | 148.013 |
| quoted / 8 / 1 | 1.720 | 2.280 | 1.994 | 1.609 |
| escaped / 8 / 1 | 2.105 | 1.716 | 1.928 | 1.536 |
| quoted / 4096 / 1 | 5.180 | 2.752 | 5.379 | 3.143 |
| quoted / 65536 / 1 | 44.954 | 5.358 | 52.464 | 23.291 |
| escaped / 65536 / 1 | 48.195 | 43.949 | 54.325 | 45.011 |
| dense_quotes / 65536 / 1 | 88.467 | 88.492 | 91.558 | 88.471 |
| unterminated / 65536 / 1 | 23.536 | 3.766 | 23.926 | 12.547 |
| quoted / 8 / 128 | 9.255 | 9.521 | 9.539 | 9.335 |

Long ordinary quoted bodies improve on both hosts. Existing quote-at-cursor
handling keeps the dense-quote control close to baseline. Small one-row cases
vary substantially even between byte-identical quoted/escaped inputs, so these
samples do not isolate a short-input effect. In particular, the retained macOS
eight-byte quoted median increases by 0.56 us while the identical escaped
case decreases by 0.39 us. The 128-row short case increases by 2.9% on macOS
and decreases by 2.1% on Linux. This is local evidence for long-run discovery,
not a universal latency, file-I/O throughput, RSS, x86 or consumer-adoption claim.
There is no timing threshold in correctness tests.
