# Copy admitted ordinary prefixes in expanded percent output

The whole-output count branch already copies completely unchanged percent
output. Expanded output still writes every leading ordinary byte individually,
even when a lone escape occurs near the end. Evaluate one bounded prefix scan
using the existing exact byte classifier, followed by safe bulk initialization
of those unchanged bytes and the existing scalar writer for the remainder.
This is a complete percent/component and percent/path capability; form and HTML
remain measured controls with their existing transformations.

| Boundary | Implementation obligation and owner |
| --- | --- |
| Prefix authority | Only a zero escaped_byte_count over each complete bounded chunk proves its bytes unchanged. Component excludes slash; path permits it. Stop before the first chunk containing an escape; scalar writing processes that complete chunk and all remaining bytes. No claim about a partially admitted chunk or caller-supplied output length. All256 and mixed reserved-byte owners retain exact uppercase output. |
| Work and short inputs | Consider prefix scanning only on long inputs, with fixed 128-byte chunks. Each scanned source chunk is disjoint and visited once; a failing chunk ends the scan. A dense input pays at most one extra bounded classification. Short inputs use the existing scalar writer without a scan. The existing all-byte count owner qualifies classification; disjoint chunks_exact(128) structurally bounds repeated work. |
| Initialization and extent | Safe slice write_copy_of_slice initializes exactly the proven prefix, then the unchanged scalar writer receives the remaining input/output slices and checks their final extent. Short/long destination owners must still fail without reading uninitialized memory. No new unsafe operations, raw pointers, temporary buffer or allocation. |
| Admission, allocation, ownership and failure | Full checked output sizing precedes the same exact owned allocation. UTF-8-independent arbitrary-byte grammar, null/empty convention, allocation failure, independent output, post-write publication and exactly-once Drop stay unchanged. Existing isolated public allocation oracle and m10_encoding source owners qualify the boundary. |
| Compiler and sibling paths | No syntax/type/IR/ABI/layout/cache/generic/control-flow change. Both component and slash-preserving entry writers share the prefix rule. Whole unchanged-output branches remain. Form cannot use raw copy for spaces and remains outside this capability. K1 and aggregate provenance stay deferred. |
| Measurement | Rebuild baseline/candidate ordinary release archives and use the identical 240-case encoding_writes C harness, including late/sparse/dense entities, quote runs and unchanged form/HTML controls. Independently validate bytes and observed native work; retain all ABBA samples, producer identities and short/expanded/control costs. Adopt only useful local gains with explicit limits; no portable/application/RSS promise or timing gate. |
| Evidence and lifecycle | Explicit LF retained CSV, staged/committed-range whitespace checks, bounded native_probe.run_phase build/link/probe ownership, compact review/measurement retention and post-consumer scratch cleanup. |

The checked-allocation strategy and public contract are unchanged. The author
matrix/extracted-obligation pass and one fresh full-diff review close this
refinement; no separate public design review is required. Prefix threshold and
local adoption are measurement decisions, not language behavior.

## Author measurement revision

The first experiment placed prefix admission and copying inside the scalar
writer. The full 8,640-sample comparison improved long late-escape cases
67.2–72.1%, but a 64-KiB repeated-quote control regressed 29.3% and one short
case cost 12.5 ns. Reject that shape; retain its complete samples and producer
identity. Both artifacts retain the shared scalar-writer symbol, so absent
inlining is not established as the cause. The final implementation isolates
prefix preparation in a wrapper and leaves the scalar loop unchanged; the full comparison was repeated.
This changes neither the public contract nor the allocation/safety strategy.

## Qualification

The final wrapper uses a 512-byte minimum input and admits disjoint complete
128-byte chunks with the existing component/path classifier. A safe split and
copy initializes the prefix; the unchanged scalar writer validates the exact
remaining extent. The private destination owner now exercises that complete
wrapper for both grammars, including short/long refusal. Shared new fixtures
place plus, percent, slash, NUL and non-ASCII bytes before/inside/after chunk and
threshold boundaries on ordinary and slash-filled inputs. The public owner
retains independent byte oracles and isolated exact allocation/free counts.
Four native owners and eighteen m10_encoding source owners pass on macOS ARM64.

The [measurement record](../../bench/encoding_writes/README.md) preserves both
8,640-sample comparisons. The final long late-escape cases improve 67.1–73.1%;
other long percent cases range from −2.04% to +7.52%, and the largest short percent
increase is 4.25 ns. HTML/form controls and the rejected scalar-writer experiment
are explicit. Adopt the final local gains and costs without a portable or
application-throughput claim. No public contract or allocation strategy changes.
