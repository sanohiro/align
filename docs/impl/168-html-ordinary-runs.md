# Bulk ordinary runs in HTML output

Plan166 accelerates the shared checked length pass. The HTML writer still
copies ordinary bytes individually. After sixteen consecutive ordinary bytes,
when at least 64 input bytes remain, find the next of the five entity bytes and
copy the ordinary run through safe MaybeUninit slice initialization. Search
successive windows of at most 256 bytes; each checks sixteen bytes directly,
then both discriminator groups finish that window before advancing. Short
inputs compile out the run counter. Keep the existing entity writer and exact
destination assertion.

| Boundary | Implementation obligation and owner |
| --- | --- |
| Exact output | Ampersand, less-than, greater-than, double quote and apostrophe retain their exact existing entities. Search stops at the earliest of all five; all other bytes remain unchanged. encoding_writes_tests uses an independent oracle and both initialized sentinels across each entity, repeated runs, NUL/Unicode and run/search/short-specialization boundaries. |
| Bounded search work | Each search receives at most 256 bytes; neither discriminator group may scan the complete later tail independently. The visited windows total at most the returned ordinary prefix plus 256 bytes, each searched at most twice. The shared production window iterator exposes its actual slices to an instrumented owner, which verifies this bound for each entity group alone, absent groups and 4,096 repeated quote/apostrophe runs without timing. The sixteen-byte admission before another search bounds complete-writer rescanning linearly. |
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

The initial native initialized-output/mismatch, HTML/template, isolated allocation
and encoding/template source owners passed (41 tests), as did warning-clean C syntax.
The counter is bounded by 79: reaching sixteen with at least 64 remaining
bytes resets it after search; otherwise fewer than 64 bytes remain. Entity
transitions reset it independently, and the short specialization omits it.

Independent review found that separately searching the entire tail for markup
and then quotes rescanned quote-only inputs quadratically. That candidate and
its 4,896 observations are rejected and retained as such. The local correction
bounds both existing searches within the shared window iterator, preserving
the existing safe slice/allocation/publication strategy. Audit both discriminator
groups independently; add the measured-work owner and both quote-only probes.
Final qualification uses this corrected candidate and expanded corpus.

The corrected candidate passes 42 focused native/source tests, including the
instrumented search-work owner and warning-clean C syntax. The final 7,200
observations cover both quote types at sixteen- and 33-byte ordinary gaps.
Ordinary/sparse/late cases at 4/64 KiB improve 66.8–91.1%. Offset quote runs
cost up to 19.6% (10.308 microseconds at 64 KiB), and the largest short HTML
increase is 8.25 ns. Accept those explicit costs with the later-run gains;
the search-work invariant remains independent of timings. The measurement
record retains all 18,144 observations from the rejected and final experiments.
