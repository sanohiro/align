# Owned records with fixed-array values

Request 136's imported `Value { bytes: buffer, counts: [i64; 2] }` constructor
passes source checking but fails MIR publication and native emission. A local
constructor passed to a borrowed helper reaches the same missing proof. Plan 73
already admits this inline Copy field beside an owned buffer; Request 135's
caller-side exclusion correction (plan 152) does not repair value certification.

The producer graph follows fixed-array element stores only at an `Element`
projection. A whole fixed-array load instead requests the empty path. That
misclassifies a complete array literal as invalid when the enclosing Move
record's whole-value proof reaches its Copy field. Constant elements also carry
Shared read authority, although copying scalar inline bytes produces independent
bits. Preserve the existing separation between readable storage and copied
values: authenticate the array load's initialization and element dependencies,
then apply the existing scalar-copy normalization only to plain scalar arrays.
Never normalize storage, borrowed views, callable arrays or owned elements.

This follows plans 53, 55, 73 and 133's founded producer and copied-value rules.
No new syntax, type, IR shape, interface field, ABI, allocation, Drop lowering,
source borrow rule or general K1 strategy is introduced.

## Implementation closure matrix

| Axis | Implementation obligation and owner |
| --- | --- |
| Formation and construction | Exact existing array element/length and typed-slot validation remains required. Whole plain scalar-array loads authenticate literal cardinality, ordering and every element input; copied parameters, root stores, joins and later writes retain their dependencies. `owned_fixed_record` producer matrix includes constant/nonconstant arrays, zero length, primitive siblings and nested Copy metadata. |
| Value versus storage authority | Only a readable, founded whole SSA value of a plain scalar-array type gains independent Copy authority. Source slots and view-bearing arrays keep their original authority. Missing producers, duplicate/out-of-range/late/mistyped element stores, ungrounded cycles and unreadable arguments must reject in full and partition validation. The producer mutation matrix and existing copied-bits/borrowed-producer owners close this axis. |
| Move, return, replacement and Drop | No cleanup changes. Imported/local constructors, transferred whole records, replacement, return and repeated calls preserve buffer contents and fixed fields. Native ownership instrumentation must observe exactly-once destruction after normal and early exits. Existing Move reuse and borrowed-owner consumption negatives remain required. |
| Calls and control flow | Shared and mutable imported calls, generic forwarding, nested fields, if/match/else/?/map_err, loop joins and early returns retain existing checking. New source owners exercise the repaired aggregate through the applicable paths; existing producer and ownership controls close unchanged join machinery. |
| Generic, interface and cache | Whole-program, per-unit publication and authenticated cache replay must agree for the same imported and generic record definitions. No format change; existing compiler identity invalidates old objects. |
| Runtime provenance and allocation | Fixed fields stay inline, with no new allocation or runtime operation. A native owner checks the existing buffer allocation/free lifecycle; no performance/resource improvement is claimed and no benchmark is required. |
| Boundaries | Preserve borrowed-view escape, stale buffer views, use-after-move and overlapping mutable operands. General fixed record-array producer expansion and K1 remain separate unless required by the unchanged plain scalar-array proof. Consumer cache implementation, managed pin adoption, serving and practical measurements remain align-llm-owned. |

One capability closes the producer correction and its discriminating source/MIR
owners. Complete the author matrix-to-diff pass and one fresh independent
preflight review under the existing proof strategy.

## Qualification

`owned_fixed_record_producer_matrix` covers all four plain scalar families,
computed elements, zero length and nested Copy records. The malformed producer
owner rejects missing, duplicate, out-of-range, late and mistyped element stores,
wrong bulk-constant cardinality/type, duplicate SSA definitions, an ungrounded
root-store cycle and unreadable inputs in whole and partition validation.
`imported_owned_fixed_records_execute_and_replay_with_exact_cleanup` executes the
exact Request136 return witness and a three-module four-array record with generic
nested metadata. It exercises shared/mutable calls, copying fixed fields from a
borrowed record, replacement, generic forwarding, branch/loop and fallible early
exit; an enabled runtime probe witnesses a live buffer before checking retained
owners and zero live bytes after scope exit. Cold/replayed frontend artifacts
have identical MIR and execution. Source negatives retain move, borrow, alias
and contained-view rejection in both modes.

The MIR library and existing fixed-array field, Request135 sibling, owned struct
array, Move-return cleanup, owned replacement and borrowed-composition targets
remain the owner set for unchanged control, allocation and lifetime rules.
Standalone canonical borrows of a local fixed array can still reach the existing
whole-storage producer refusal; this request repairs arrays inside owned records
and does not expand that independent storage-descriptor proof.

## Review correction

The independent review found that a zero-length array copied through a local
root store still requested an `Element` dependency from its empty literal.
That path has no element producer. Retain whole-value dependencies for empty
array root stores and parameters, while only a validated fresh empty literal
may seed its own zero bits. `empty_fixed_array_copies_preserve_whole_value_provenance`
executes named copies, record projections, both branch arms, by-value parameters
and borrowed record fields in whole/per-unit modes. Full and partition mutations
retain rejection of an ungrounded zero-bit copy cycle, malformed element writes,
duplicate SSA producers and an unreadable argument isolated from caller checks.
No other review findings or changes to proof strategy were required.
