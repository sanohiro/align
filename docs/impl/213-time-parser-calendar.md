# Direct Gregorian conversion for named timestamp parsing

The existing five `std.time` parsers sum the lengths of all preceding months
inside `civil_days`. Replace the year-start calculation and bounded month walk
with Gregorian 400-year/March-based arithmetic, using the inverse of the existing
formatter's [public-domain derivation](https://howardhinnant.github.io/date_algorithms.html#days_from_civil).
Preserve day/month validation, widened nanosecond arithmetic, offset normalization
and representability checks. The public contract remains
[`std-design/time.md`](std-design/time.md); no grammar, API, native ABI, ownership,
allocation, error, compiler or artifact-format change is selected.

| Obligation | Implementation and acceptance owner |
| --- | --- |
| Exact calendar | Change only `civil_days` and remove its unused `year_start` helper. Retain `month_days` admission. Move January/February to the preceding year, use Euclidean division for the 400-year era, and compute the March-relative ordinal without a loop. `civil_date_matches_every_representable_epoch_day` independently advances all 213,504 reachable dates and checks both conversion directions, including non-leap centuries and the 2000 leap era. It is independent of both arithmetic conversions. |
| Invalid and normalized input | Existing `time_formats::tests` retains five grammars, widths, invalid months/days, offsets, both i64 endpoints, pre-epoch values, native span/kind/overlap validation and canonical zero on failure. The parser's four-digit year and two-digit fields bound all calendar intermediates. No input or output reference lifetime changes. |
| Source composition | Existing driver `time_formats` retains exact values and errors, all five names, imports/generic replay, whole/per-unit, control paths, Pure use, owned formatter outputs and independent Copy parser results. No move, cleanup, interface or lowering code changes. |
| Performance admission | Measure actual non-test release native parser calls using one independently checked C fixture for both revisions. Include January controls, other months, leap/century/offset/endpoint inputs and malformed controls for every format. Retain actual status/value checks, call counts, balanced ABBA samples, producer hashes and build evidence. Run without concurrent builds/tests; discard without useful improvement or with a material ordinary-case regression. No application, network or portable throughput promise. |

The author matrix precedes implementation. This is private arithmetic within the
existing checked parser strategy, below 1,000 handwritten changed lines. One
fresh independent preflight review checks the retained implementation and
measurement evidence; no new cross-cutting strategy review is required.

An initial cumulative-month-table candidate improved the basic formats but
reproducibly added 1.8–4.2% to January controls. It is not selected. The retained
candidate must qualify all groups again against merged main; table-candidate
samples do not enter its comparison.

The selected arithmetic passes all nine active native time owners, including
the independent exhaustive calendar and rendering comparisons. Two complete
Apple M1 release comparisons improve the February–December corpus medians by
0.9–11.3% across the five formats. January costs stay below 0.104 ns (0.6%);
the basic-ISO rejected group increases 0.195 ns (2.2%). All 147 fixed native
input/result goldens agree. The [measurement record](../../bench/time_parse/README.md)
retains every sample, observed work, identities and limitations. No uniform
speedup or Linux/application throughput claim follows.
