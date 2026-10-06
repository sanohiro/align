# Direct Gregorian date conversion for named time formats

The five existing UTC formatters currently binary-search the year and then walk
months for every value. Their shared `civil_date` helper can instead decompose
the epoch day into a 400-year Gregorian cycle and a March-based month. This is
one private arithmetic refinement; the named-format ledger remains
[`std-design/time.md`](std-design/time.md). No ownership, FFI, IR, effect,
serialization, native dependency, timezone or public surface changes.

Use the arithmetic derivation in Howard Hinnant's
[public-domain date algorithms](https://howardhinnant.github.io/date_algorithms.html#civil_from_days),
with Euclidean division and checked epoch/year arithmetic in Rust. Formatting
still first floors the input at its selected resolution in i128, rejects an
unrepresentable rounded value, then uses the existing fixed stack buffer and
one final owned allocation. Parsing retains its independent year-start and
month-length calculation; it supplies a useful inverse oracle.

| Obligation | Implementation and acceptance owner |
| --- | --- |
| Calendar identity | Replace only `civil_date`. An independent sequential calendar owner checks all 213,504 epoch days reachable from i64 nanoseconds, anchored at 1677-09-21, including Gregorian leap/century and March-era boundaries. Existing independent wire goldens anchor epoch, 2000 leap day and both i64 endpoints. |
| Precision and error identity | Leave format rounding, kind admission, grammar, offsets, parser and native output admission untouched. Existing `time_formats::tests` cover five resolutions, first accepted floors, invalid kind/date/width/range, ABI admission and zero-on-error. |
| Storage and lifetime | No new allocation or pointer operation: fixed Text and final `owned_str_exact` remain. Existing source owner covers returned/replaced/nested/arena-independent Move strings and dropped parser input. |
| Language paths | Existing `align_driver --test time_formats` covers all five names, imports, generic replay, whole/per-unit output, control flow, effects and wrong types/arities. No new fixture or owner target needed. |
| Measured performance | The manual ignored `time_formats::tests::complete_format_entry_probe` measurement of the complete native formatter entry includes final allocation and free, all five kinds, negative/zero/modern/endpoint inputs, nine-sample median/range reporting and an output checksum. Retain compact before/after results for macOS and Linux; no CI timing gate and no universal throughput promise. |

Author review extracts these obligations against the small arithmetic diff and
existing/new owners. The normal fresh independent implementation review and
local owner/gate/Clippy checks apply. The strategy of fixed-stack formatting and
single owned output does not change, so a new cross-cutting plan review is not
required.

Local release measurement (2026-10-06, Rust 1.96.1): macOS ARM64 and Linux
ARM64 in Docker on the same host. Each table entry is the median ns per complete
formatter call, including output allocation/free, over nine samples of 180,000
calls. Both arms use the same nine negative/zero/modern/endpoint inputs and
produce identical checksums. Coarser kinds include the existing lower-endpoint
rejection. Baseline is the parent implementation plus this same ignored probe.

| Format | macOS before | macOS after | Linux before | Linux after |
| --- | ---: | ---: | ---: | ---: |
| `rfc3339` | 267.35 | 147.52 | 205.23 | 165.99 |
| `rfc3339_ms` | 234.93 | 129.67 | 183.36 | 151.72 |
| `rfc1123` | 226.27 | 122.78 | 169.85 | 137.79 |
| `basic_iso` | 208.76 | 103.69 | 157.27 | 123.82 |
| `basic_date` | 167.35 | 59.68 | 109.06 | 75.76 |

The local medians improve for all five formats. These are native formatter
measurements, not an application/network speed guarantee. Each run prints its
minimum/maximum too; wall-clock thresholds never enter CI. Reproduce with:

```text
scripts/cargo.sh test -p align_runtime --release --lib time_formats::tests::complete_format_entry_probe -- --exact --ignored --nocapture --test-threads=1
```
