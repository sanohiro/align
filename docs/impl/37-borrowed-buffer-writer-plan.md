# Borrowed buffer and writer receivers

Status: implemented; align-llm Request 61, issue #981. This extends plans 26/28's
closed borrowed-payload grammar and stable receiver places. It does not change
handle ownership or admit exclusive calls on arbitrary partial Move fields.

## Public contract ledger

| Surface | Exact input and result | Ownership, lifetime, errors and allocation | Owner and acceptance |
|---|---|---|---|
| Stable buffer receiver | `owner.path.data.bytes() -> slice<u8>` and `owner.path.data.len() -> i64`, where the final field is `buffer` and the complete root is a stable local, shared/exclusive parameter, or checked borrowed payload binding. No new arguments/defaults. | Non-consuming load of the existing handle. The view retains the original owner, generation and contained lifetime; no new owner, nulling, independent cleanup or allocation. Existing byte-view mutation rules apply. Temporary receivers remain rejected. | Sema, checked HIR, MIR borrowed load, resource validation and LLVM pure lowering. Direct/nested receiver, borrowed owner, view escape/invalidation and exact-byte execution owners. |
| Stable writer receiver | `owner.path.sink.write(value) -> Result<(), Error>` and `.flush() -> Result<(), Error>`. `value` is the existing `str`, `string`, `slice<u8>` or `builder` argument. Stable roots as above; existing local/std-stream receivers remain. | Existing writer methods take a non-consuming shared handle receiver; I/O remains impure and native errors unchanged. Borrowing does not transfer the writer, its descriptor, buffered bytes or backing connection. Failure/early exit leaves cleanup with the owner. No added allocation or implicit flush. | Same stages; repeated write/flush, failed write, eager-argument owner replacement, one close/free and exact bytes. |
| Borrowed sum payload | Add `buffer` and `writer` leaves to plan 28's closed finite recursive grammar. Existing `match` over a borrowed complete `Option`, `Result`, user-sum or field may bind them or records containing them as checked borrowed projections. | Existing static payload type and projection metadata; active tag/path/authenticated owner only. No independent drop bit, extraction or source nulling. Consumption, storage, return of the handle, capture, exclusive passing and nested borrowed matches remain rejected by existing rules. Ordinary owning match still consumes. | Shared classifier and variant sweep, checked-HIR replay, exact MIR place/type/provenance, negative consumption/escape and owning-match twins. |

A plain `alias := owner.handle` remains an ordinary Move expression; this
capability does not silently turn assignment into borrowing. Use a direct
non-consuming receiver, or the existing borrowed-match binding. Buffer mutation
methods requiring an exclusive local (`put_*`, `append`, `read(out: ...)`) keep
their current place gate. A numeric-stream owner can encode through its existing
byte view while its complete stream is borrowed. No special stream type, method,
network retention behavior, positional buffer operation or application codec is
introduced.

Stable fields include arbitrarily nested validated struct-field paths, but not
indexing a handle array, an unbound temporary or a value-producing branch. The
field type must be exactly the method's existing handle type. Source checking
resolves the receiver and method, then arity and arguments in existing source
order; malformed receivers reject before constructing valid typed operations.
Every eager operand is evaluated once. Earlier receiver validity is checked
through later operands to the operation action; moving or replacing the owner
in a write argument rejects. A diverging argument performs no write. Existing
alias, effect, lifetime and mutable-view rules retain their diagnostic precedence.

The new leaves do not admit other opaque handles, buffers of aggregates, builders,
resources, boxes or specialized arrays. Plan 28's finite/cycle-safe traversal and
existing recursive record/sum/ordinary-array composition remain authoritative.
Connection-backed writer shells retain their existing dependent-owner lifetime;
projection cannot extend it or turn a non-owning descriptor into an owning one.

There is no new HIR variant, interface field, serialized tag, runtime symbol,
wire/native-text boundary, ambient configuration, process-global state or cache
format. Existing checked borrowed-binding records and MIR borrowed-place paths
carry the fact. Source/body implementation identity and the compiler namespace
invalidate prior artifacts normally. Public signatures and nominal type/layout
identity remain unchanged. All added compiler metadata is compiler-owned memory;
no runtime allocation or performance promise is added, so no benchmark is a gate.

## Implementation closure matrix

| Axis | Obligation | Owner |
|---|---|---|
| Formation/validation | Closed leaf classifier, exact stable field path/type, no temporary receiver; malformed owner/path/mode/cleanup metadata rejects. | `struct_handle_fields` receiver negatives; sema classifier sweep; checked-HIR and MIR projection mutations. |
| Construction, move-in/out, source nulling | Owned containing records still construct/move/return; receiver loads never transfer or null handles. Owned match unchanged. | `struct_handle_fields` construction/return/partial-move owners plus borrowed direct/optional twins. |
| Drop/replacement | Exactly one close/free at containing-owner retirement, including optional absence, moved return, write failure and early exit. Derived views reject after replacement/drop. | Runtime counted resource owner or linked test shim with close/free counters; existing owned-tagged cleanup owners. |
| Control flow | `if`, `match`, `else`, `?`, `map_err`, branches, loops and early returns preserve original ownership. Borrowed handle cannot escape through any value-producing join or closure. | Parameterized source negatives; repeated writes and failure propagation execution owners; existing borrowed-sum control-flow coverage. |
| Eager operands and conflicts | Receiver remains live through write data evaluation; reject owner mutation/consumption and actual conflicting view mutation. Divergence performs no action. | Same-root write-argument invalidation, independent argument positive, diverging argument owner. |
| Derived-view mutation and substitution | Mutate header bytes through a field-derived view while borrowing the complete stream; return and retain byte views through direct helpers and imported/function-value fallback, preserving optional-binding source roots and owner-generation invalidation. | Exact writer bytes after view mutation; direct/imported/indirect return and `borrow mut` retention positive/negative twins. |
| Recursive array composition | Arrays of handle-owning AoS records are admitted by the recursive grammar and may be passed by indexed shared borrow to a helper. Direct indexed method receivers remain excluded. | `borrowed_buffer_views_follow_optional_array_and_generic_sources` checks the projected AoS helper and executes its absent case. Nonempty handle-owning array construction remains excluded by the existing heap-builder/pipeline contracts; its execution/cleanup cell is deferred until that separate capability. This change does not admit a new array constructor. |
| Generics/imports/cache | Instantiate classifier after substitution, export only unchanged signatures, replay exact owner/path/type and drop exclusions. | Whole/per-unit generic and imported stream helper; cache edit/revert owner; checked-HIR replay mutations. |
| Runtime provenance/ABI | Borrowed pointer load keeps original allocation/descriptor and backing connection identity; existing runtime row and error mapping. | MIR no-null/no-new-cleanup assertion, resource validator negatives, exact byte and close/free owners including failed write. |

One capability PR contains classifier/receiver admission, both analysis passes,
checked-HIR/MIR proof, lowering and owner coverage. A dormant syntax-only or
producer-only PR is not useful. If this exceeds 1,000 handwritten lines, the
single boundary avoids duplicated ownership proof across admission, validation
and execution and prevents shipping acceptance before cleanup is sound.

Before implementation, independently review this ledger and its boundary.
Before preflight, the author-side matrix pass binds every owner description to
exact regression names or existing tests that fail for the changed defect.
Normative agreement includes `draft.md`, `docs/language-spec.md`,
`docs/design-notes.md`, `docs/open-questions.md`, plans 26/28, and checked-HIR
ledger 19. Update end-user library mirrors only where receiver prose changes.
Align delivery ends at merged compiler evidence and the request-register answer;
align-llm restores its numeric-stream source, adopts the pin and runs
`gpu-numeric-stream`/`gpu-generation-smoke` independently.

Pre-implementation independent review found two P2 coverage gaps: transitive AoS
record-array admission and derived-view mutation/interprocedural retention. Both
are closed explicitly in the matrix above before implementation. Keep receiver
validation specific to writer methods: `io.copy` retains its existing gate.

## Author-side closure

The semantic implementation adds only the two closed leaves and stable field
receiver forms. `slice_is_local` distinguishes borrowed buffer owners from
frame-owned buffers, consistently with `borrowed_storage_cap`. Investigation
also found that a byte-view header could retain parameter fallback roots without
a legacy source-map entry; `BorrowState::invalidate_matching` now observes the
resolved header roots for locals, eager value snapshots and pipeline snapshots.
Borrowed handle match bindings retain an explicit map to their original scrutinee
root; a binding never becomes an independent source owner. The parameterized invalidation owner covers `Option`, `Result` and user
sums as well as returned, retained and indirectly returned views. Neither change
invents ownership, mutates generation identity or skips root exclusions.

The following exact owners close the matrix:

- `struct_handle_fields::borrowed_handle_receivers_preserve_nested_and_optional_owners`:
  direct/nested fields, optional writer, complete-owner borrowing, byte mutation,
  returned view, repeated exact output, owned record return and final cleanup.
- `struct_handle_fields::borrowed_buffer_views_follow_optional_array_and_generic_sources`:
  borrowed optional buffer, generic record, returned and retained byte views,
  imported helpers, indirect text-view return, and typed projected-array helper.
  Its nonempty array cell is explicitly deferred above; no array construction
  restriction is relaxed.
- `struct_handle_fields::borrowed_handle_projections_reject_consumption_and_escape`:
  return, by-value call, owning field alias, value join, storage, capture,
  whole-owner replacement, later eager-argument replacement, local view escape
  and exclusive partial-field refusal on whole/per-unit paths.
- `struct_handle_fields::borrowed_handle_io_failure_retires_the_containing_owner`:
  ordinary runtime I/O and error mapping, four exact-byte writes, `if`/loop/
  `match`/`?`/`map_err`/`else` paths, no action for a diverging write argument, and
  success/error cleanup. Test-only LLVM symbol redirection counts non-null
  buffer/writer frees while delegating to the production runtime. Each owner is
  freed once, and the real descriptor is open until retirement then closed.
- `struct_handle_fields::borrowed_handle_receiver_cache_replays_and_invalidates_body_edits`:
  cold, unchanged frontend/object hits, edited output, and reverted frontend/
  object hits.
- Sema `borrowed_payload_classifier_admits_only_ordinary_dynamic_array_graphs`
  and `borrowed_sum_match_metadata_rejects_forged_records`, MIR
  `borrowed_handle_receiver_hir_rejects_forged_places`, and LLVM
  `borrowed_handle_receivers_reject_forged_mir_projections`: leaf closure,
  independently replayed source owner/type/cleanup facts, and malformed
  receiver/slot/path/type rejection before LLVM use.

The existing `struct_handle_fields`, `borrowed_params`, `borrowed_replacement`
and `resource_ownership` owners retain owning-mode transfer, partial nulling,
non-owning match lowering, borrowed return substitution, arena cleanup and
resource provenance coverage. The pre-change release compiler separately
reproduces `borrowed_params::owned_string_array_index_is_a_non_consuming_str_view`'s
`projected` producer-certification failure; this unrelated existing failure is
not claimed fixed by Request 61.

## Reopened axis: mixed header and opaque-handle ownership

The preflight review found that a sibling slice header could hide the complete
owner of a buffer/writer field. Header identity and opaque-handle ownership are
orthogonal: an absent field header cannot inherit only its siblings' headers.
Preserve an explicit checked match-binding-to-source-root map in both semantic
analyses. A headerless Move receiver uses its original stable owner in addition
to existing contained provenance; projected storage headers retain their existing
path-specific generation tracking. Inline buffer/writer fields are identified
through a cycle-safe struct/tuple/sum traversal that stops at collection headers.
Returned-view candidate matching in both analyses retains opaque owner fallback
roots: a compatible sibling slice is not a proof of the handle's identity.
This changes no HIR/interface/runtime record.

| Mixed-owner cell | Implementation and owner |
|---|---|
| Direct and nested buffer plus static slice sibling | Stable opaque field owner root; returned view rejects after containing-owner replacement. |
| Borrowed match buffer/record with slice sibling | Checked projection root map, never a match-local owner; Option/Result/user-sum positive and invalidation twins. |
| Writer plus slice sibling, eager mutation | Same stable receiver root survives later argument evaluation; owner replacement rejects before the I/O action. |
| Dynamic-header sibling precision | Existing field-specific headers keep their own generations; the fallback applies only when the selected Move field has no header. |

`mixed_handle_fields_keep_the_complete_owner` and the parameterized
`borrowed_buffer_views_follow_optional_array_and_generic_sources` /
`borrowed_handle_projections_reject_consumption_and_escape` owners cover these mixed layouts on
whole-program and per-unit paths. The review's P1 is repaired as one ownership
root class; one fresh review of this redesigned fallback axis is required.
