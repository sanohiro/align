# Copy RNG record composition

Request145 asks for the existing `rng` value in ordinary records. Before this capability, source and
checked-HIR field formation rejected it. Standalone explicit
`borrow mut rng` functions already check and lower, while shared mutation
rejects. The semantic layout cache defaulted Rng to 16 bytes despite
LLVM's existing `[4 x i64]` representation. Field admission must include the
exact 32-byte layout proof, not merely remove the two formation refusals.

## Public contract ledger

| Surface | Exact contract, owner and acceptance |
| --- | --- |
| Field formation | An ordinary record field may have the existing concrete type `rng`, also after generic substitution. Nested records retain this leaf. No new type, type argument, syntax, constructor or scalar tag is added. Sema `is_field_ok` and checked-HIR `field_type_ok` agree. Source and handcrafted-HIR owners qualify concrete/generic/nested fields and preserve unrelated placement refusals. |
| Value and lifetime | `rng` is Copy, contains only its four-word state, borrows nothing, owns no heap/native resource and has no Drop. Construction, field read/copy, whole-record copy, replacement, passing and returning copy the exact state without reseeding. A containing record's other fields still determine Move/borrow/Drop behavior. Native owners compare independently advanced copies and a fresh identical seed; Move/borrow-bearing sibling controls keep existing ownership. |
| Layout | The existing internal RNG representation is 32 bytes with natural alignment8 on supported targets. Ordinary field ordering and explicit aggregate alignment retain their existing rules. TypeLayoutCache owns the semantic size; LLVM retains `[4 x i64]`. Independently compare semantic and real LLVM layout for direct/nested/reordered/over-aligned records and enclosing fixed/ordinary dynamic record storage where already admitted. No C layout or raw-state export is granted. |
| Mutation | Existing direct/imported functions such as `fn advance(borrow mut value: rng) -> i64` may receive an admitted mutable local or nested Copy field through the ordinary mutable-place protocol. They advance the selected stored generator; copies and disjoint fields remain independent. Shared/immutable/overlapping borrows reject. RNG intrinsic receivers retain their existing bound mutable-local rule; this capability does not change `RandNext/Range/Shuffle/Sample` place representation or accept `state.random.next()` directly. This is the existing explicit borrowing route, not copy-out/write-back lowering. |
| Existing methods and effects | `rand.seed() -> rng`, `rand.seed_with(s: i64) -> rng`, `r.next() -> i64`, `r.range(lo:i64,hi:i64) -> i64`, `r.shuffle(out xs:slice<T>)` and `r.sample(xs:slice<T>,k:i64) -> array<T>` retain their exact input domains, effects, deterministic algorithm, errors and allocations. Record storage introduces no implicit allocation, clone, seeding or global state. Keep the pinned m10_rand stream/invalid-input/purity owners. A counter-enabled native owner proves no record/copy/advance heap allocation after a positive allocation instrumentation witness. |
| Other placements | Ordinary containing records compose only through existing supported record positions. No `Scalar::Rng`, direct RNG array/slice/tagged payload, SoA-plain field, JSON codec or FFI expansion is added. Baseline-check the concrete refusals before documenting them; preserve native/shared/unsafe placement validators. Test record-containing tagged/fixed/dynamic forms where their existing record grammar admits them, not by bypassing formation. |
| Artifact identity | Public interfaces already encode the `rng` builtin and complete nominal record fields. Keep that schema, nominal identity and existing compiler-bound cache keys; no new wire format, serialization tag or runtime ABI symbol/signature. Imported generic records/forwarding execute whole-program/per-unit and after cold/cache-hit materialization. No artifact/source I/O or runtime reflection is introduced. |
| Scope and prerequisites | Existing Copy/borrow/record/LLVM machinery is the prerequisite. No later milestone, new ownership model, region precision, K1 or parked aggregate-provenance work is consumed. No throughput claim or benchmark gate. Consumer code/pin/adoption remain external; update only Request145's uncommitted register response after Align merge and run the exact batch release build. |

## Implementation closure matrix

| Axis | Completion owner |
| --- | --- |
| Formation and representation | Both field gates plus exact TypeLayoutCache Rng case; layout-parity test against LLVM with nontrivial siblings/padding and generic/nested record definitions. Malformed C/unsupported placements fail before lowering. No change to LLVM ABI producer. |
| Construction/copy/return/replacement | Native deterministic state comparisons cover local literal, imported constructor/return, generic identity/wrapper, nested projection, whole/field replacement and source independence after copy. Copy introduces no source nulling/Drop. Mixed Move siblings retain their existing source/Drop authority. |
| Mutable action | Use existing BorrowedPlace field path and mutable-call source-order/exclusion protocol, with direct/imported helpers invoking each applicable native method on its ordinary local parameter. Same-field overlap/shared-root negatives and disjoint/sibling positives qualify the admitted mutable boundary. No mutable indexed-field or intrinsic receiver widening. |
| Control and cleanup | Records flow through existing if/match/else/?/loop/return paths; reuse established Copy/record control owners where invariant-equivalent. New native selected/repeated seeded-state owners prove no hidden reset/copyback. A Move-sibling counted owner proves cleanup parity on normal and early exits. |
| Containers and malformed input | Exercise recursive containing-record layout/Copy classification at fixed and dynamic record storage and tagged payloads where admitted; direct Rng element/payload and C/JSON/SoA restrictions remain. Handcrafted-HIR placement/ABI owners fail closed. |
| Generic/interface/cache | Imported concrete and generic record definitions plus explicit mutable helper, whole/per-unit execution and cache hit replay. Exact bytes/stream results and independent-copy state comparisons are the observation, not acceptance alone. |

## Source-of-truth pass before implementation

Update draft.md and docs/language-spec.md std.rand paragraphs, concise rationale
in docs/design-notes.md and the Settled entry in docs/open-questions.md, relevant
plan19 field-placement record, and the English/Japanese std.rand guide together
for the new record promise. Keep native receiver-local restrictions explicit in
the recipe. HANDOFF records the capability once. The runtime ABI inventory needs
no change because no runtime symbol, signature or representation changes.

Every ledger row above maps to the matrix. No new fallible operation, encoding,
input-validation order, persisted format, cache identity, process-global native
state or inspection API is introduced. Seed/range/sample error ordering belongs
to unchanged existing method owners. Before implementation, obtain one fresh
independent review of this field-admission/ABI closure and its useful boundary.
If the existing explicit mutable-field borrowing or recursive record machinery
cannot represent this contract correctly, revise the matrix before widening
receivers or inventing an alternate copy-back mechanism.

## Independent plan qualification

The fresh independent review qualified this field-admission and semantic-layout
boundary without findings. Existing pointer-based borrowed places own mutation.
RegionPlain and heap-tree-builder leaf grammars remain unchanged alongside the
C/JSON/SoA/direct-scalar restrictions; a containing record does not bypass them.
The code owner must qualify each admitted recursive record position and retain
these specialized refusal controls.

## Implemented closure

| Matrix cell | Implementation and owner |
| --- | --- |
| Formation, generic and Copy state | `is_field_ok` and `PlacementValidator::field_type_ok` admit Rng; existing recursive Copy/Drop and interface classifiers remain authoritative. `rng_records_execute_and_replay` constructs local/imported/mixed/nested and `Wrap<rng>` fields, copies/extracts/returns/replaces them and compares independently advanced pinned streams. |
| Representation and recursive storage | `TypeLayoutCache::layout` supplies `(32,8)`. `sema_and_codegen_struct_layout_agree` compares real target-data layout for direct, reordered, nested, over-aligned, Option/Result, fixed-length 0/1/2/4/37 and dynamic-header records. The new direct case failed on the baseline with semantic `(16,8)` versus LLVM `(32,8)`. Native owners exercise fixed/dynamic elements, Option/Result and user-enum payloads. |
| Mutation and native actions | Existing borrowed-place field paths supply the real pointer. The native owner compares next/range/shuffle/sample actions against pinned outputs and a direct local generator, including independent siblings and repeated field replacement. `rng_record_placement_and_authority_refusals` pins immutable/shared, same-place/ancestor overlap and direct intrinsic-field refusals. |
| Control, allocation and cleanup | The native owner exercises both if/match/else selections, `?` success/failure, loop/break and early returns. An allocation/free witness precedes a zero-allocation record/copy/advance interval. Borrowed-owner plus Move-string siblings retain their storage and free exactly once on normal/early/error exits; final live bytes are zero and allocation/free counts agree. |
| Specialized placements and malformed HIR | The refusal owner pins direct Rng array/slice/Option, C/FFI, SoA, JSON, RegionPlain and heap-builder diagnostics. `rng_record_fields_preserve_placement_boundaries` checks both checked entrypoints and all four fail-closed infallible entrypoints after C/SoA/invalid-array/RunBytes mutations. No specialized shape predicate is widened. |
| Interface, cache and prose | The native owner checks and executes whole/per-unit imported concrete/generic records, materializes both cold/hit frontend results, and executes the hit. `rng_record_guide_recipes_check_and_lower` reads both live language guides and validates their exact recipes. Existing `m10_rand` pins algorithm, invalid inputs, element admission and purity. |

The author-side ledger extraction and matrix-to-diff pass cover all cells above.
There is no new IR variant, allocation path, runtime ABI, interface version or
performance claim. General indexed mutable-field expansion, direct RNG scalar
placements, native receiver widening and K1 remain outside this capability.
