# Reuse exact expansion counts for unchanged output

The percent/component, slash-preserving percent/path and HTML entries already
compute exact output length before allocation. Every transformed byte in those
three encoders strictly expands, so equal input/output lengths prove that no
byte changes. Evaluate copying the complete input directly on that branch,
retaining the existing writers for expanded output. Apply the same proof at the
HTML template append's exact-count call site. Form encoding is excluded: a
space becomes plus without changing length.

| Boundary | Implementation obligation and owner |
| --- | --- |
| Count authority | Only the existing checked percent/component, percent/path and HTML length functions supply this proof. Do not infer unchanged bytes from a caller-supplied destination length in the private writers. Their short/long-destination refusal owners remain unchanged. All 256 bytes, slash versus component, and every HTML entity have an independent exact-output oracle. |
| Initialized output | Equal-count branches copy the complete source into the exact MaybeUninit slice using safe write_copy_of_slice. Expanded branches use existing writers. Public native owners check independent output, empty/null shape, NUL/Unicode, long ordinary and expanded controls; source owners check encoding and template behavior. |
| Admission, allocation and publication | Existing UTF-8/native validation and checked expansion precede allocation/reserve. Keep the same owned_str_exact allocation, template reserve, failure policy and post-write publication. No new buffer, allocation, unsafe operation, retention or alias rule. Native allocation owners and template lifecycle/malformed owners retain exact boundaries. |
| Forms and specialized grammar | Form is unchanged; same-length space-to-plus controls must retain transformed output. Path preserves slash; component expands it. HTML template escaping cannot become raw-write grammar. Existing failure/error order and owned storage remain. |
| Compiler and control | No syntax/API/type/IR/ABI/interface/cache/generic/ownership change. Source callers keep existing Result/control/Drop behavior; K1 and aggregate provenance stay deferred. |
| Measurement | Reuse identical encoding_writes C source, independent oracle and observed call/output extents. Rebuild baseline/candidate ordinary release archives and compare ABBA on idle macOS ARM64, no concurrent builds/tests. Retain all samples, artifact identities and expanded/short/form controls; adopt only with useful gains and explicit costs. No portable or application-throughput promise and no timing correctness gate. |
| Lifecycle | Use native_probe.run_phase process-group deadlines for build/link/probe. Retain compact CSV/evidence and remove temporary archives/executables after their last consumer. |

This preserves the reviewed allocation/safety strategy and public contract.
Perform the author-side matrix/extracted-obligation pass and one fresh full-diff
inspection review; no separate design review is required.

## Qualification

The three standalone exact-allocation callbacks and the HTML template append
reuse their existing checked count; the private writer contracts and form path
are unchanged. `public_encoding_entries_preserve_exact_owned_output` checks
all byte values for the byte codecs, valid HTML text, slash/space/Unicode/NUL,
long ordinary/expanded controls and independent output storage. Its isolated
allocation mode starts with a positive witness and verifies exactly one payload
allocation/free per nonempty output and none for empty output. Existing private
mismatch, HTML invalid-input, template lifecycle/allocation and whole/per-unit
source owners retain the surrounding failure and publication boundaries.
The template byte owner now combines ordinary prefixes/suffixes with entity and
raw appends; its allocation owner exercises an ordinary append inside capacity.
Nine focused native owners and 23 source owners pass on macOS ARM64.

The [measurement record](../../bench/encoding_writes/README.md) retains all
7,200 ABBA observations and exact producer identities. At 4/64 KiB the unchanged
component/path cases improve 83.4–85.6%, and HTML 10.5–12.6%. Long expanded cases
cost at most 2.0%; the largest short increase is 2.45 ns. Form controls remain
explicit. Adopt these local gains and costs without a portable or template/
application-throughput promise. No public contract or allocation strategy changes.
