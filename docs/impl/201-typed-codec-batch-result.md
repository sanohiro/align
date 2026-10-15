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

Use the existing native-status plus typed-output-slot convention already used
by `BytesAsStr`. Change MIR `CodecOpen` to `{ input: Operand, out: Slot }`.
Its input is exactly `slice<u8>`, its result is exactly signed i32 status, and
its private output slot is exactly `CodecBatch`. On status zero it publishes
the same pointer and length as the input into that slot; on error it publishes
no batch. The runtime validator and its ABI remain unchanged. LLVM lowering
implements that MIR operation by calling the validator and storing the input
descriptor only on success. The existing MIR status branch loads the typed
slot only on success, then constructs an ordinary correctly typed Result.

The output has shared input provenance, never ownership or individual cleanup.
Register the operation in the shared side-effect-output and source-operand
inventories and `native_owner_mir_contract`. The existing unconditional native
contract sweep checks even unused operations, requires a private output slot
with one writer and no independent root stores, and authenticates readable input.
Validate the complete input/result/output shape before LLVM or
interface publication, including invalid value/slot IDs. The ordinary Result
constructor checks remain exact. This is a representation correction using an
existing validation/output convention, not a new native-memory authority.

## Implementation closure matrix

| Axis | Implementation and acceptance |
| --- | --- |
| Type formation and validation | Keep checked HIR and nominal `CodecBatch` types unchanged. `native_owner_mir_contract` checks exact byte input, i32 status and output slot. `producer::codec_tests` covers input/result/slot identity, unreadable input, parameter outputs, independent stores, duplicate writers and discarded operations. The composed tagged/resource gates still reject a raw slice placed directly in a batch Result. |
| Construction and errors | `lower_codec_expr` supplies a fresh batch slot and the existing status-to-Result control path. LLVM publishes its borrowed descriptor only after successful validation. `core_codec` golden, invalid-envelope and alignment owners exercise success and failure. |
| Source lifetime, move-in/out, replacement, Drop | No new owner or cleanup bit. Shared native output facts name the original input. Existing `core_codec` source expiry, validated-byte mutation, reopening and detached-copy controls retain their verdicts; encoder/buffer cleanup remains unchanged. |
| Control flow | Existing `core_codec` owners cover `?`, `else`, `match`, returned Result and generic/imported helpers. Reuse local observation matrices for if/loop joins and early exits; add a focused typed-construction owner only where existing tests do not assert the corrected MIR relation. `map_err` uses the ordinary Result route and introduces no codec-specific behavior. |
| Whole-program, per-unit, generic and cache | Run complete `core_codec` and `pkg_frame` targets, including their existing interface/generic and downstream join owners. `codec_batch_result_survives_imported_generic_cache_replay` executes valid and invalid input through imported generic/map_err/if/loop forwarding on both cold and hit runs. Public interface records and runtime key are unchanged. |
| IR consumers | Update printing, runtime dispatch, slot-output/source inventories, producer validation and all exhaustive matches over the changed `CodecOpen` shape. Existing sema/HIR operation and loop-effect classification remain unchanged. Audit wildcard consumers for the new slot write. |
| Allocation and resource promises | Reuse existing codec zero-copy and encoder allocation/cleanup owners. No new allocation or performance claim; no benchmark is selected. |

Perform an author-side matrix-to-diff pass, one fresh inspection review, focused
owners, bounded gate and Clippy on the final candidate before publication.
Other nightly classes, including task capture and stale admission expectations,
are independent work and are not bundled into this repair.
