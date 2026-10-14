# Certify independent inline Copy values in owned returns

Request134 reproduces on current main: projecting a borrowed `Capacity` record
of four integers, then placing that Copy value beside a fresh `array<i64>` in
an owned returned record, fails MIR producer certification. Reconstructing the
four fields individually succeeds. This is a Category A implementation gap in
the existing structural Copy contract, not a new projection or ownership rule.
Voice V002 and media AM-R01 need different Move-field capabilities; neither is
admitted by this repair. JSON owned-array roots remain a separate capability.

## Boundary and mechanism

The producer graph already normalizes readable scalar and inline scalar-array
SSA values to independent owned bits. It retains their construction, dependency,
validation, absence and grounding obligations. Extend that same normalization
to closed inline data: supported numeric/bool/char/unit values, numeric vectors
and masks, RNG state, and ordinary records, tuples, fixed arrays, Option, Result
and sum carriers whose complete reachable payload graph contains only those
values. Inspect all variants and even zero-length element types; an empty
container must not erase a Move or view type. Validate every referenced type ID,
scalar width and vector shape. Use an iterative, cycle-rejecting graph walk with
completed-node reuse, not recursive stack growth or a depth-based permissive
fallback.

No `str`, slice, SoA, pointer, callable, arena, owned heap/native resource or
unknown type is plain independent data. A general `!ty_is_move` predicate would
incorrectly include borrowed views and callable environments. Normalize only
whole SSA values (`Value` with empty path), never slots, selected subpaths,
borrowed descriptors, buffer or capture nodes. Each existing producer equation
must first supply its ordinary typed proof; normalization changes only readable
authority, never seeds an ungrounded cycle or supplies missing input checks.
Unreadable Out and mixed/invalid inputs remain rejected. Storage exclusivity,
return-leaf checks and Move transfer requirements remain unchanged.

This changes no source syntax, type formation, region relation, IR variant,
interface format, native ABI, generated allocation or Drop operation. The
existing compiler identity invalidates cached artifacts. Copying inline data
does not retain its source; existing borrowed fields retain their own lifetimes.
There is no new performance/resource promise and no benchmark gate.

## Implementation closure matrix

| Axis | Implementation and owner obligation |
| --- | --- |
| Classification | `producer_plain_copy_type_graph` covers admitted leaves/containers, DAG reuse, missing IDs, cycles, invalid scalar/vector shapes, and nonplain view/pointer/callable/Move families through nested/empty carriers. The classifier is conservative on unknown types. |
| Construction/read | Whole SSA normalization retains exact producer equations. `producer_copy_record_authority_matrix` crosses ByValue/Borrow/BorrowMut/Out, field/whole-load/store forwarding and ungrounded cycles; malformed path, type and dependency twins reject. Reuse `producer_copied_bits_preserves_grounding_and_readability`; the driver owner closes source control joins. |
| Move-in/out and source nulling | Only the Copy sibling becomes independent. Parameterized malformed-MIR owners replace the owning array/string sibling with borrowed storage and still reject; preserve `producer_rejects_borrowed_store_load_before_owned_return`. No source-null or cleanup lowering changes. |
| Return/control/replacement | `copy_record_producer` executes exact values through direct/Result returns, if/match/loop joins, early Err, `?`, `else`, `map_err`, replacement and discard. Counted allocation/free deltas close exactly-once cleanup of the adjacent fresh array on each path. |
| Nested/generic/order | The same source owner exercises imported concrete/generic/nested Copy records, fixed data and reversed declaration order; no name-specific exception. A copied value survives source mutation and source scope expiry. |
| Views and exclusive authority | Borrowed Move store/load and view-bearing/callable classifier negatives retain their proof. Source lifetime and shared-to-exclusive negatives still reject; a whole Copy result cannot make its original borrowed storage writable. |
| Interface/cache | Whole/per-unit object generation and execution plus cold/hit checked-artifact replay agree on imported constructors. No consumer source/pin changes. |
| Existing machinery | Run the focused new driver owner and MIR producer owners; fixed-scalar borrow, owned-fixed-record and RNG composition owners qualify the reused normalization and sibling representations. Required bounded gate/Clippy remain final local verification. |

Author-side review must map each cell to the final diff and owner before the
single independent code review. The initial safety-boundary review qualifies
the classifier and SSA-only placement before implementation; findings are closed
in this matrix first. General aggregate provenance and K1 remain unselected.

## Qualification

The independent initial boundary review found no issues. The author matrix pass
maps classification to `xml_plain_copy_type`, normalization to the existing
`XmlAccessAnalyzer::build` whole-value equation, and all ownership/control
obligations to the two MIR owners and `copy_record_lifecycle_and_replay`.
Existing fixed-point, borrowed-materialization, fixed-array and RNG owners
remain unchanged. The native owner fails on the pre-fix compiler at the exact
imported `model$construct` producer-transfer diagnostic. The corrected compiler
executes both compilation modes and cold/hit replay with exact values, exact
allocation/free deltas and zero retained requested bytes. No consumer adoption
or general aggregate-provenance claim follows from this provider result.
