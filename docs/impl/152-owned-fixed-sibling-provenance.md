# Preserve owned fixed-array siblings across mutable view calls

Request 135 is blocking CUDA consumer adoption. A borrowed record containing a
buffer and `[i64; 5]` loses its caller provenance after a mutable helper receives
only the buffer's byte view. Both checking modes reproduce on current main.
The dynamic-array twin succeeds. `exclusive_header_observations` recognizes
independent heap storage but omits the existing `InlineFixed` storage identity.

Apply the existing exact caller-field storage contract to fixed inline storage.
Do not change heap release ownership: `owns_storage` still means a separately
owned allocation. Only exclusion's owner-observation and backing-independence predicates admit
the inline kind. Unknown/view headers, contained borrows, missing records and actual
shared generations retain conservative exclusion. No syntax, public type,
interface encoding, IR, ABI, allocation, Drop or general K1 strategy changes.
Requests 130 and 134 remain separate.

## Implementation closure matrix

| Axis | Closure and owner |
| --- | --- |
| Formation and validation | Existing typed paths and caller generation formation authenticate fixed scalar and fixed record arrays, nested/imported records and field order. Parameterized source owners exercise these shapes and the dynamic-array control. |
| Exclusive observations | Distinct known inline/heap backing identities may be disjoint; matching identities, borrowed content, view descriptors and unknown facts remain observable. A direct BorrowState kind/content matrix pins the distinction without altering release ownership. |
| Calls and control paths | Whole/per-unit source owners cross shared reads, mutable helpers, repeated calls, nested fields, branch/loop use and early exit. Existing disjoint-field, borrow-liveness and array-truncate owners retain eager argument, rebind, same-field and contained-view negatives. |
| Move, replacement, return and cleanup | No formation, transfer, source nulling, replacement, return or Drop implementation changes. Existing storage-generation and ownership owners remain authoritative; execute the imported positive program in both compilation modes and reuse existing stale/replaced owner rejection. |
| Generic, interface and cache | The same semantic checker and ordinary imported summaries own both modes; no format change. Include a generic forwarding path and replay the exact source through the existing per-unit cache, retaining negative borrowed-view twins. |
| Runtime and allocation | Unchanged compiler output contract and runtime ownership. No performance/resource claim or benchmark. Bound new compile/link/run owners with the existing parent-owned fixture. |

This corrects one omitted existing storage class under the reviewed generation
strategy. Complete the author matrix pass and one fresh independent preflight
review; do not broaden caller-root erasure or resume deferred K1 work.

## Qualification boundary

The provider owner compiles the exact empty-main request, its dynamic control,
and nested fixed scalar/record sibling forms in whole/per-unit modes. A separate
imported constructor transfers owned inputs into the four-array session, as the
real consumer does; repeated shared/mutable calls execute in both modes and
through authenticated cache replay. Overlapping call operands, stale buffer
views, fixed borrowed contents and escaped local views remain rejected.

Native construction from local aggregate temporaries can still reach the
pre-existing MIR producer refusal described in the 2026-09-26 self-audit. That
independent aggregate-construction boundary is not relaxed or repaired here;
Requests 131/134 and the parked experiment remain separate. The test constructor
uses the already-supported transferred-input shape and does not claim general
aggregate producer closure.

The unmodified align-llm source at `71c05e9bf75f8cbc40af68d8fb132cd1f5ea0aad`
passes `check-per-unit` for all 33 units and `build --profile release` through
native linking on macOS ARM64. The unchanged C loader is compiled locally only
for this link check; no consumer repository file is changed. This proves the
compiler/native entry path, not NVIDIA execution or paired request timings.
