# Selected CSV text normalization probe

Run `bash bench/csv_normalization/run.sh`. This selects the `note: str` column
in the existing CSV field-discovery corpus, alongside `active: bool` and
`score: i32`. It uses the same 31 cases, fixed 50 ms warm-up, seven trials,
returned status/count checks and shared bounded process runner documented in
[the discovery probe](../csv_quoted_scan/README.md) and
[the native runner](../native_probe.md).

The selected variant verifies every decoded text byte before timing. Timed
calls check the producer-returned text length and first/last bytes for the
first/last row as well as both scalar values. Each complete call includes arena
creation, UTF-8/CSV admission, conversion, normalization, output checks and arena
teardown. Fixture and expected-byte construction are outside timing; no
allocation probe runs during measurements. The normal discovery variant keeps
its original two selected columns and does not construct expected text.

## Comparison (plan153)

Baseline: `381f7794cbdd3cf48fb76dea726309cfb0998141`. Candidate: plan153's
reuse of `next_quote` in decoded-length and chunk discovery. Both arms use the
same selected-text C harness, ordinary release runtime archives and `cc -O3`.
Apple M1, macOS 27.0.1 and Linux ARM64 Docker on the same host, Rust 1.96.1. Baseline/candidate/candidate/baseline
processes run sequentially with no competing builds or tests. All 1736 final
observations are retained in `macos-samples.csv` and `linux-samples.csv`
(14 per arm/case/platform), without
outlier removal. An initial harness layout error failed its output check before
timing; those failed observations are not performance samples.

Medians in microseconds per complete call:

| Pattern / body bytes / rows | macOS baseline | macOS candidate | Linux baseline | Linux candidate |
| --- | ---: | ---: | ---: | ---: |
| escaped / 4096 / 1 | 12.572 | 9.000 | 12.132 | 8.280 |
| escaped / 65536 / 1 | 168.975 | 110.696 | 171.759 | 108.011 |
| dense_quotes / 65536 / 1 | 425.293 | 383.263 | 476.847 | 433.085 |
| quoted / 65536 / 1 | 5.527 | 5.587 | 23.767 | 23.806 |
| plain / 65536 / 1 | 10.932 | 11.025 | 57.925 | 57.926 |
| unterminated / 65536 / 1 | 3.831 | 3.853 | 12.999 | 13.021 |
| quoted / 8 / 1 | 2.000 | 2.114 | 1.585 | 1.599 |
| escaped / 8 / 1 | 1.969 | 1.977 | 1.582 | 1.684 |
| quoted / 8 / 128 | 10.804 | 11.145 | 10.337 | 11.352 |

The long escaped case improves by 34.5% on macOS and 37.1% on Linux; the
short 128-row control increases 3.2% and 9.8%, respectively. The byte-identical
eight-byte quoted/escaped single-row controls also vary, including a 6.4% Linux
increase for the escaped-labelled case. These observations qualify local normalization of long selected text;
they do not establish a universal latency, I/O, RSS or x86 improvement. There
is no timing threshold in correctness tests.
