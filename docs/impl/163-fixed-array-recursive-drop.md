# Recursive ownership of fixed record arrays

Plan73 already requires `[T; N]` to be Copy exactly when T is Copy, including
zero length. A record containing `[User; 2]`, where User owns a string, currently
passes two consuming calls on main `402060fa`. The root fixed-array Move check
and the LLVM struct-field cleanup special case do not compose through the
canonical DropPlan used to classify the containing record. A successful native
exit alone cannot detect the missing cleanup.

Add the existing fixed-record-array edge to the canonical recursive DropPlan.
Its element plan participates in iterative construction, validity, classification
and destruction; length retains the array identity but never changes Move into
Copy. Reuse the existing ascending element cleanup and whole-aggregate source
nulling. Remove the compensating root/field classification special cases.
This implements the reviewed plan73 ownership strategy. It introduces no source
type, admitted element domain, layout rule, runtime ABI, interface field, allocation
or general aggregate-provenance rule. Independently owned scalar fixed arrays
and K1 remain deferred.

The wrapper owner also exposed a sibling omission in `TypeLayoutCache`: both
fixed-array kinds defaulted to the 16-byte dynamic header estimate. The LLVM
layout already stores inline elements, so an enum carrying the array owner is
rejected by semantic/LLVM layout parity. Include the same element edge in the
semantic layout walk, with checked/saturating size arithmetic that never wraps.
This restores the existing inline ABI promise rather than choosing a new layout.
Ownership and its reachable wrapper layout ship together so the native wrapper
owners exercise the corrected classification through actual cleanup.

## Implementation closure matrix

| Axis | Implementation and exact owner obligation |
| --- | --- |
| Formation and validation | `drop_plan` must traverse fixed-record elements through enclosing structs, Option/Result, tagged types and enums. Copy and Move elements, lengths 0/1/2, missing IDs and cycles use a parameterized sema `drop_plan_fixed_array` owner. Existing inline graph validation remains the source/layout gate. |
| Construction and move-in | Existing fresh aggregate and element stores transfer initialized owners. The driver `fixed_array_ownership` owner constructs local/imported generic records, nested record arrays and arrays beside another owned field. It observes positive allocation/live-byte witnesses before cleanup assertions. |
| Move-out and source nulling | Whole records and returned values transfer exactly once. The same driver owner exercises generic forwarding and owning sum extraction; a parameterized negative owner rejects repeated consumption, consumption through a borrow and extracting a nested Move array field in whole/per-unit modes. |
| Drop and replacement | `index_drop_plan` indexes the array child and all field cleanup consults the canonical plan. Existing ascending fixed-element cleanup remains unchanged. Native free counts and live-byte probes must show no leaked or duplicated leaves after normal return, replacement and early exit. Copy-array controls stay Copy. |
| Zero-size payloads | A zero-length Move array keeps its Move classification. A containing zero-size record may occupy an omitted physical union payload; cleanup must skip that proven storage-free payload while retaining semantic ownership. Driver owners cover Option, Result and enum wrappers; malformed layout/Drop validation must still reject invalid graphs before emission. |
| Control flow and partial construction | Driver owners exercise if/match/else/?/map_err, branch and loop joins, early return and an exit during array/record initialization. Existing `owned_structs_arrays`, `owned_field_replacement` and `move_return_cleanup` owners retain the element replacement and destination-cleanup boundary. No general join algorithm changes. |
| Generic, interface and cache | Imported generic definitions and forwarding must agree in whole-program, per-unit and cold/hit checked-artifact replay, with identical MIR on replay. The driver owner executes both compilation modes. Compiler identity invalidates prior artifacts; no format change. |
| Malformed and deep graphs | Sema owners cover missing/cyclic fixed-array elements, including zero length; the existing deep DropPlan owner gains fixed-array edges so iterative validation and destruction remain stack bounded. Existing codegen malformed-drop and shared-helper owners cover pre-layout refusal and emission. |
| Layout parity | The semantic layout cache must descend through both scalar and record fixed arrays and multiply element stride by exact length; zero length retains element alignment. Its owner pins primitive/record arrays, enclosing tagged layouts, zero length and overflow saturation. Native Option/Result/enum owners check agreement with LLVM; no runtime signature or interface change. |
| Runtime provenance and allocation | Existing string payload allocation and free ABI remain unchanged. Native owners isolate the probe in their generated process and pin equal allocations/frees plus zero live bytes, including a positive live witness. No additional allocation or performance promise is made; no benchmark is required. |
| Every DropPlan consumer | Exhaustive compiler matches and the `pkg_kv_timeout_ownership` cleanup-graph oracle must accept the array node. The oracle uses the canonical array plan instead of its separate root-array wrapper. The existing type-variant tripwire remains unchanged because Ty/Scalar gain no variant. |

One capability closes recursive classification, dependent cleanup and the owners.
The author matrix-to-diff pass and the fresh preflight review check these
boundaries together under the existing ownership strategy. Source specifications
and language mirrors need no changes because their normative promise is intact.

## Existing producer boundary

Main `402060fa` already refuses a fresh fixed-record-array constructor containing
a fallible owned-string element when the completed array is exported through a
Result or passed to a borrowed whole-record helper: its dynamically cleaned
element cannot satisfy the current producer certificate. The same refusal was
reproduced with the unchanged optimized baseline compiler. This capability
retains that fail-closed boundary and does not resume the deferred general
aggregate-provenance experiment. Partial-initialization cleanup is exercised by
local construction, direct field observation and scalar return; ordinary
nonfallible aggregate return and generic transfer retain their native owners.

## Qualification

The negative ownership matrix rejects repeated consumption, borrowed consumption
and nested Move-array extraction in both compilation modes. The native lifecycle
owner observes 24 payload allocations, 24 frees and zero live bytes after actual
function exits, with positive live-byte witnesses before cleanup. It covers
replacement, return, generic forwarding, nested fixed-record arrays, a sibling
owned field, both fallible branches, partial initialization and active/empty
sum wrappers. Whole-program, per-unit and authenticated cache replay execute the
same source; cold/hit MIR is identical. The unchanged baseline compiler's
standalone two-string record witness reaches eight live bytes, then exits with
the explicit leak verdict after its owning function returns. Thus the runtime
probe discriminates the missing Drop independently of layout rejection.

The author extraction pass binds every matrix obligation to the named owners:
`drop_plan_fixed_array_composes_through_every_wrapper` and the existing deep
DropPlan owner cover classification/invalid graphs; `fixed_array_layout_composes_through_records_and_tags`
covers stride, alignment and nonwrapping overflow; `fixed_array_ownership` owns
native lifecycle and source refusals; the exhaustive `pkg_kv_timeout_ownership`
oracle covers every DropPlan variant. Existing Drop-emission, fixed-array-field,
Move-array, field-replacement and Move-return targets retain the unchanged
lowering paths. The producer boundary above is the explicit deferred cell.
