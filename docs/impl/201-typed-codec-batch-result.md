# Typed codec batch construction

The nightly run at `9227651f` rejects `core.codec` and its `pkg.frame`
consumer. The same minimal imported `codec.open` wrapper fails per-unit
certification at `ad8cce29`: MIR wraps the input `slice<u8>` directly in
`Result<codec.batch, Error>`. The two descriptors share a physical layout, but
they have different nominal types. The aggregate constructor validator correctly
rejects that mismatch. Whole-program source checking does not reach this gate.

Implement the existing [codec contract](core-design/codec.md) and
[validated-observation boundary](60-validated-codec-observation-plan.md).
Do not weaken aggregate type equality or admit a user-visible conversion.
`codec.open` still validates the complete envelope once, borrows the original
bytes, allocates nothing, and returns the same builtin error on invalid input.
No language, canonical type, interface, runtime ABI, or wire-format change.

## MIR representation

Make `CodecOpen { input: Operand, error_enum: u32 }` one typed MIR operation
returning exactly `Result<CodecBatch, builtin Error>`. Like the existing
`XmlParse` result producer, it owns the validation-to-tagged-value transition.
Its contract returns `Ok` with the unchanged borrowed descriptor only when the
keyed runtime validator returns zero; otherwise it returns `Err(Error.Invalid)`.
The runtime validator and ABI remain unchanged. LLVM lowers this complete MIR
contract with two branch-local ordinary union values and one result phi. There
is no separate status value or conditional batch output slot for later MIR to
read, alias or publish before validation. Input evaluation remains once.

The result has shared input provenance, never ownership or individual cleanup.
The producer equation checks the exact byte input, builtin Error identity and
complete result type, and supports only the complete result and its exact Ok
and Err payload paths. Each selected payload equation records possible absence;
only the complete Result is unconditionally present. Existing
`extraction_guard_matches` then requires the corresponding tag branch on every
path before payload extraction. Do not copy an unconditional payload-present
seed from another native producer. An unconditional producer sweep authenticates even a
discarded operation and checks the readable source. The same sweep demands the
existing producer equation and explicit `extraction_guard_matches` proof for
every `ResultUnwrapOk`/`ResultUnwrapErr` whose
operand is `Result<CodecBatch, _>`, even when the extracted value is discarded,
returned directly or used only by scalar codec accessors. This closes the gap
left by demand-driven protected-owner walks, which do not include CodecBatch.
Call summaries and parameter provenance do not establish which Result arm is
active; the explicit guard requirement therefore also covers local/imported
helper results and Result parameters. The existing guard walk follows every
CFG successor kind; non-Branch edges never refine the allowed tag set. A tag
predicate must have exactly one primary definition, no auxiliary definition,
no duplicate value record, and the selected definition must be the actual
predecessor statement. Duplicate predicates cannot borrow another definition
as evidence. Ordinary
aggregate type checks remain unchanged. Invalid input/value/enum/result IDs
must be rejected before LLVM or interface publication.

Generic native-output equations certify slots without proving that a successful
status branch dominates every use. A zero sentinel cannot represent a valid
codec batch. The atomic Result boundary avoids exposing such a conditional slot
and uses the existing tagged-extraction proof.

## Implementation closure matrix

| Axis | Implementation and acceptance |
| --- | --- |
| Type formation and validation | Keep checked HIR and nominal `CodecBatch` types unchanged. `CodecOpen` producer validation checks exact byte input, builtin Error identity and Result type. `producer::codec_tests` covers malformed input/result/enum identity, unreadable input, duplicated operation/extraction/predicate definitions and discarded operations. Unguarded, reversed, unrelated, bypassed and same-successor Result extraction must fail for both payload arms, including unused extraction, direct batch returns, scalar accessors, helper-call results and Result parameters. The composed tagged/resource gates still reject a raw slice placed directly in a batch Result. |
| Construction and errors | `lower_codec_expr` emits one complete typed Result producer. LLVM constructs only the selected tagged payload after validation, with no conditional output slot. `core_codec` golden, invalid-envelope and alignment owners exercise success and failure. |
| Source lifetime, move-in/out, replacement, Drop | No new owner or cleanup bit. Shared native output facts name the original input. Existing `core_codec` source expiry, validated-byte mutation, reopening and detached-copy controls retain their verdicts; encoder/buffer cleanup remains unchanged. |
| Control flow | Existing `core_codec` owners cover `?`, `else`, `match`, returned Result and generic/imported helpers. MIR structural owners assert there is no separately addressable conditional output or status; LLVM branch/phi inspection and runtime invalid-input owners prove both result arms initialize their active payload. Reuse local observation matrices for if/loop joins and early exits; add a focused typed-construction owner only where existing tests do not assert the corrected MIR relation. `map_err` uses the ordinary Result route and introduces no codec-specific behavior. |
| Whole-program, per-unit, generic and cache | Run complete `core_codec` and `pkg_frame` targets, including their existing interface/generic and downstream join owners. `codec_batch_result_survives_imported_generic_cache_replay` executes valid and invalid input through imported generic/map_err/if/loop forwarding on both cold and hit runs. Public interface records and runtime key are unchanged. |
| IR consumers | Update printing, runtime dispatch, producer validation and all exhaustive matches over the changed `CodecOpen` shape. Remove the candidate-only slot-output/source inventory entries. Existing sema/HIR operation and conservative loop-effect classification remain unchanged. |
| Allocation and resource promises | Reuse existing codec zero-copy and encoder allocation/cleanup owners. No new allocation or performance claim; no benchmark is selected. |

Perform an author-side matrix-to-diff pass, one fresh inspection review, focused
owners, bounded gate and Clippy on the final candidate before publication.
Other nightly classes, including task capture and stale admission expectations,
are independent work and are not bundled into this repair.
