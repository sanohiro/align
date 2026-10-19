# Initialized out-slice read certification

## Existing contract and selected repair

LLM Request157 reproduces at `7d4ccfd5`: whole checking accepts an imported
function that writes `bytes[0]` and appends `bytes[0..1]` to a separate buffer,
but per-unit producer certification rejects BufferAppend. The MIR access graph
seeds every Out parameter as Unreadable, including an ordinary source-formed
slice. This also rejects reads of caller-initialized contents before a write.

The existing out-parameter contract supplies exclusive writable access to an
initialized slice. Safe callers form slices from initialized arrays/buffers;
there is no source uninitialized-slice constructor or definite-initialization
protocol. Unsafe raw slice construction retains its existing validity duties.
Out adds the call's no-alias obligation, not a write-only element type. Reads,
including the original capture collector's written prefix, retain ordinary
bounds and lifetime rules. No new API, type formation, allocation, copy,
ownership transfer, cleanup, ABI or interface field is introduced.

Repair only exact slice-typed Out authority. Invalid Out modes on owners,
scalars, resources and malformed parameter slots remain rejected. Use one typed
mode classifier at the argument, backing-buffer and returned-root seams; an
untyped Out seed must never certify arbitrary storage. ByValue and BorrowMut
slice headers do not independently prove writable backing. Preserve that
distinction and all existing source no-alias, shared-storage and task-capture
checks. Imported calls still need certified signatures and founded arguments.

## Implementation closure matrix

| Axis | Implementation and owner |
| --- | --- |
| Formation and authority | `align_mir::producer` authenticates the complete declared slice type and exact parameter slot before assigning readable exclusive access to Out. `out_slice_reads::producer_authority` crosses Out/Borrow/ByValue/BorrowMut and slice/non-slice forms, malformed mode/type/slot metadata and unfounded storage. |
| Read, construction and control | `out_slice_reads::source_and_imports` reads initialized input before any write, after indexed writes, through a subslice, branch/loop joins, and fallible helper completion. Buffer append observes exactly the selected bytes. Test whole/per-unit/imported generic calls plus repeated reads and empty input. Function values with Out remain outside source formation; this repair does not widen them. |
| Return and backing transport | The same source owner covers a helper returning an ordinary slice rooted in Out and a returned scalar/owned buffer. Original storage stays borrowed for a view; copied/owned outputs retain their existing independence. Existing return-lifetime and source invalidation negatives remain. Do not infer new mutable-return precision. |
| Mutation and exclusion | The source owner rejects immutable literal storage, shared borrowed backing and overlapping arguments through the actual checked-HIR/MIR publication boundary. `view_authority_does_not_expand` rejects writes into text/record-view backing, Move extraction and a returned Out/static alternative used as a native output buffer. Existing `out_params`, `task_callback_access` and `imported_mutable_retention` own neighboring source rules. |
| MIR producer boundaries | The parameterized producer owner verifies BufferAppend and a scalar read contributing to a fresh owned result, malformed native inputs, non-slice Out and uninitialized/cyclic slots. Existing producer permission tests keep their invalid-owner controls. Valid Out slices must not be used as an allegedly unreadable negative. |
| Ownership and allocation | Input remains caller-owned; the append destination follows ordinary buffer growth and Drop. No new runtime path, allocation policy, move-in/out, nulling, replacement or cleanup is added. Reuse existing buffer lifecycle owners. |
| Publication and caches | `out_slice_reads::source_and_imports` exercises separate compilation, imported generic forwarding, cold/hit builds and a helper-body edit. Compiler build identity invalidates old code; serialized signatures and runtime ABI are unchanged. |

The independent inspection-only strategy review confirmed the existing
caller-initialized premise and found no blocking defect. Its explicit boundary
witnesses are returned Out/static alternative views failing native writable-
buffer certification and readable text/view-bearing elements gaining no nested
backing or ownership authority. These belong to the return and mutation rows
above. General scalar indexed writes through returned views already have the
separate interprocedural read-only-summary gap recorded by Plans196/210; this
repair changes neither their source admission nor their lowering. The native
backing owner and the existing LLVM
`producer_out_slices_authenticate_backing_without_reading_forwarded_elements`
owner preserve the stricter protected boundary, including unfounded headers.

The author matrix-to-diff pass maps all three Out seeds to the typed classifier.
Source/native/cache tests close the changed read boundary; malformed modes,
types, slots and absent/cyclic header stores fail full and partition validation.
No ABI, allocation, cleanup, IR variant or source type formation changed.
