# Native decoder allocation authority

Plan195 leaves native arena decoder results outside its typed-allocation proof.
At `04676b9d`, a record containing `array<i64>` and `str`, decoded inside an
arena and passed to a whole-record `borrow mut` helper, passes source checking
but fails MIR certification with `owned-leaf argument provenance mismatch`.
The native JSON record/AoS/union decoders allocate every array spine on the
heap, including inside an arena. Only escaped borrowed text uses that arena.
The current Shared producer classification and arena-selected cleanup therefore
also omit individual array frees. Native allocation counters exposed the leak
while closing the original borrowed-call defect. JSON/CSV SoA column buffers,
in contrast, are allocated in their explicit arena.

This completes the existing Plan195 authority separation and Plan193 native
allocation/lifetime distinction for the older decoder family. It repairs cleanup
to match the existing native allocations; it changes no signature, allocation
policy, lifetime rule, IR, interface or runtime ABI. The existing JSON/CSV contracts own their schemas and
errors. Decoding into a mutable binding does not grant writable input text,
individual ownership of actual arena buffers, partial exclusive Move-field calls
or escaping views.
Media AM-R01 and LLM R147 retain this producer limitation in their delivery
records; consumer code, pins and adoption remain external.

## Implementation closure matrix

| Axis | Implementation and owner obligation |
| --- | --- |
| Exact native construction | Reuse the existing native output validator: unique instruction and result, exact output slot/type/selected path, decoder identity and all input/arena/options dependencies. JSON record/AoS/union outputs supply individually Owned array spines, independently of the optional text arena; selected str leaves stay Shared, exactly as for mixed owned JSON. JSON/CSV SoA stays a Shared Copy view with arena-owned columns; its existing calls remain a regression control. `arena_decoder_buffer_authority` covers the decoder family and selected nested buffers. |
| Every writer and join | Join the authenticated individual decoder contribution with parameter seeds and every other writer. A decoder must not upgrade a shared parameter, copied borrowed header, static allocation or unrelated native producer in the same slot. Existing BufferValue/BufferSlot propagation retains forwarding, joins, recursive owners and grounded input requirements without a new arena-column authority rule. `arena_decoder_buffer_malformed` mutates a valid graph and adds competing shared writers. |
| Readable text versus owners | Text leaves retain READ authority and input/arena lifetimes. Only actual collection buffers receive their native ownership/write authority; a slice header, text, native resource, unknown type or unsupported path cannot acquire it. `arena_decoder_borrowed_text_keeps_its_lifetime` and `arena_decoder_buffer_and_payload_are_distinct` distinguish readable text, writable buffers and forbidden Move/free. |
| Mutation and control | Existing source rules govern whole-root and record-field element writes, replacement, direct/imported/generic calls, if/match/else/?/map_err, loop joins and early exits. `borrowed_content_calls::arena_decoded_storage` observes repeated writes, empty/nonempty collections and error cleanup in whole/per-unit execution. No partial exclusive field or matched-payload admission. |
| Ownership and cleanup | Add JSON record/AoS/union to the existing native individual-storage classifier shared by cleanup and generation release. Heap spines receive existing Move/nulling/Drop/replacement handling even inside an arena. JsonDecoded generation content explicitly joins the text arena for str-bearing targets, like mixed owned JSON, so an independent array spine cannot erase shorter content lifetimes. SoA allocations remain arena-owned; borrowed calls transfer/null/free nothing. Native counters must be activated with a positive witness before asserting alloc/free parity and zero requested-live bytes across normal/error/early exits, transfer, replacement and arena exit. Cover nested/optional/array/union owner paths and parse failure after partial construction; do not weaken the owner test to hide missing cleanup. Existing borrowed replacement and mixed-owned JSON owners retain individual-owner controls. |
| Union failure and publication | Resolve the selected tag before writing any payload. On payload failure, clean only that selected arm through the common recursive descriptor cleanup, then zero the complete union storage before an enclosing record/AoS cleanup can inspect it. On later failure, clean the published arm recursively and null every released owner. `selected_union_cleanup_matrix` and `arena_union_error_cleanup` cross variant order, direct/nested/AoS placement, partial record failure, trailing input, later sibling failure and repeated cleanup. This does not widen admitted union payload types. |
| Malformed input and phase | Wrong decoder/input/arena/output/result/type/path, duplicate/missing producer, ungrounded producer and unrelated native writers reject at full and partition producer validation before codegen. Parameterized malformed owners reuse the Plan195 validation entry points. Existing `json_decoded_optional_owner_failure_matrix`, `json_decoded_owner_speculation_transition_matrix`, and mixed JSON owners cover recursive optional and speculative cleanup; CSV parse errors retain their existing owners. |
| Compilation and artifacts | Direct/imported/concrete-generic calls agree across whole/per-unit native emission; existing cache replay and all-four-call-form owners remain applicable because the same common borrowed-call validator consumes the corrected native producer graph. Compiler-byte identity invalidates changed checked artifacts; no serialized proof or format bump. |

The author boundary pass follows Plan195's distinction between readable payload
and exclusive buffer authority, and Plan193's distinction between native
individual owners and borrowed text. The newly exposed cleanup omission makes
this one producer-to-cleanup capability; a borrowed-call-only patch would leave
the leak. The independent boundary review identified recursive union-arm cleanup
and failure-before-tag-publication as necessary parts of this same cleanup boundary;
the selected-arm row records their invariants before implementation. One independent preflight review then covers the
final diff and this matrix. No benchmark is required because no performance promise changes.

## Discriminating evidence

The initial native record owner observed 96 requested bytes surviving scope exit
(7 allocations, 3 frees). Restoring the old union cleanup fails the new native
matrix with a live 16-byte record-arm buffer; restoring the old payload publication
fails the isolated child with a signal on the overlapping variant layout. Removing
the text-arena content fact admits the forbidden array-of-record return. The owner
bundle runs native JSON cleanup, MIR borrowed-call validation, and whole/per-unit
borrowed-content, replacement, mixed-JSON and owned-JSON execution.
