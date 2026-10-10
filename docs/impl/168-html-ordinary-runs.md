# Bulk ordinary runs in HTML output

Plan166 accelerates the shared checked length pass. The HTML writer still
copies ordinary bytes individually. After sixteen consecutive ordinary bytes,
when at least 64 input bytes remain, find the next of the five entity bytes and
copy the ordinary run through safe MaybeUninit slice initialization. Short
inputs compile out the run counter. Keep the existing entity writer and exact
destination assertion.

| Boundary | Implementation obligation and owner |
| --- | --- |
| Exact output | Ampersand, less-than, greater-than, double quote and apostrophe retain their exact existing entities. Search stops at the earliest of all five; all other bytes remain unchanged. encoding_writes_tests uses an independent oracle and both initialized sentinels across each entity, repeated runs, NUL/Unicode and run/search/short-specialization boundaries. |
| Initialized destination | Safe write_copy_of_slice initializes exactly the selected run, after a checked slice bound. Offsets advance by the consumed/written extent, and every successful return still asserts the complete output extent. Short destinations panic before any out-of-bounds write; long destinations fail the final equality. The existing parameterized mismatch owner exercises both specializations and bulk runs without reading uninitialized bytes. |
| Admission and allocation | The shared checked count still precedes the same owned string allocation or template reserve. No new unsafe code, extra buffer, allocation, ownership, input retention, UTF-8 validation or alias policy. Existing html_text admission/allocation and template native/source owners retain those boundaries. |
| Publication and control | Private mismatch may initialize a prefix before panic but cannot publish a descriptor or builder length. Existing owned_str_exact/template callers publish only after successful return. Source, IR, generics, interfaces, cache identities and all ownership/control-flow representations remain unchanged. |
| Measurement | Extend encoding_writes with sparse occurrences of every HTML entity byte and a late entity, preserving the independent byte oracle and observed extent/call/work counts. Compare identical C source/flags and ordinary release archives in ABBA order on Linux x86_64, with no overlapping builds/tests. Retain every sample and dense/short/control costs; adopt only with useful ordinary-run gains and acceptable costs. No portable, template/application-throughput or RSS claim, and no timing correctness gate. |
| Lifecycle | Reuse native_probe.run_phase build/link/probe deadlines and complete process-group retirement. Retain compact artifact identities and raw observations; remove temporary archives/executables after their last consumer. |

The author matrix-to-diff pass and one fresh independent implementation review
close this representation-preserving refinement. No new public contract or
safety strategy requires a separate design review. K1 and general aggregate
provenance remain deferred.

## Qualification

The native initialized-output/mismatch, HTML/template, isolated allocation and
encoding/template source owners pass (41 tests), as does warning-clean C syntax.
The counter is bounded by 79: reaching sixteen with at least 64 remaining
bytes resets it after search; otherwise fewer than 64 bytes remain. Entity
transitions reset it independently, and the short specialization omits it.

The [measurement record](../../bench/encoding_writes/README.md) retains all
4,896 observations. Ordinary/sparse/late HTML cases at 4/64 KiB improve
73.1–91.7% on this Linux x86_64 host. The largest short HTML increase is
6.45 ns; unchanged controls range from -17.5% to +4.4% at long sizes. Preserve
that dispersion and the short cost; no portable or application-throughput
promise follows.
