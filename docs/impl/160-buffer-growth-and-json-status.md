# Buffer construction, stable growth and JSON failure status

Align-llm Requests 138–140 expose three provider boundaries at `feca30d0`.
This batch preserves the existing initialized buffer and JSON contracts and
completes stable field receivers for the buffer growth family. Consumer managed
pins, lazy-cache policy, GPU execution and HTTP/first-answer qualification remain
consumer-owned. K1/plan61 and arbitrary partial Move-field borrowing stay deferred.

## Public record

| Surface | Exact contract and implementation boundary |
| --- | --- |
| Fresh zero-filled buffer | Existing `buffer.filled(length: i64, value: u8, alignment: i64 = 1) -> buffer` and `buffer.try_filled(length: i64, value: u8, alignment: i64 = 1) -> Result<buffer, Error>`. Fresh nonempty zero payloads use the global allocator's zeroed acquisition with the exact admitted Layout; only guaranteed initialized bytes become readable. Nonzero fills, empty sentinels, alignment validation and existing terminal/Result policies remain. No new API, implicit retry, allocation or residency/latency guarantee. Plans131/135 own admission and cleanup. |
| Stable growth receiver | Existing `append_filled(i64, u8) -> ()`, `append(slice<u8>/str/string) -> ()` and `put_<scalar>_<le/be>(scalar) -> ()` accept a stable `buffer` field path rooted in a mutable owned/by-value local or an exclusively borrowed complete record. One receiver rule covers the growth family. No defaults change. Shared/immutable roots, borrowed match payloads, indexed owners and temporaries reject. Arbitrary exclusive partial-field calls remain excluded: a helper borrows the complete record. |
| Growth authority and lifetime | Evaluate receiver and arguments once in source order. Retain the exact owner through all eager operands; a later operand must not consume, replace or invalidate it. Existing count/overflow checks precede mutation. Preserve initialized prefix, alignment and existing capacity/growth policy. Growth invalidates previous byte views under existing generation rules, including zero append. Self-append retains the existing runtime overlap contract. No field extraction, nulling, extra cleanup owner or new allocation beyond existing growth. |
| JSON failure | `json.doc(str) -> Result<json.doc, Error>` returns `Error.Invalid` for malformed input. The native parse entry returns common `AL_INVALID`, success remains zero. Failed output is never read and is canonicalized when nonnull. The input remains UTF-8 validated; the tape/views retain existing input/arena lifetimes. Missing-key navigation and ordinary I/O status mapping do not change. |
| Identity and owners | No new HIR/MIR variant, exported symbol, signature, serialized field or object format. Compiler implementation identity invalidates old artifacts. Sema and checked HIR own place/write authority, MIR owns borrowed handle loads and producer validation, LLVM only lowers existing operations, runtime owns payload initialization/status. No new milestone prerequisite. |

This is stable builtin receiver completion within plan37's existing field
ownership machinery, not admission of general partial Move-field aliases.
Source promises must agree in `draft.md`, `docs/language-spec.md`,
`docs/design-notes.md`, `docs/open-questions.md`, plan37 and checked-HIR ledger19.
The append-member library descriptions and their Japanese mirrors must reflect
the same receiver rule. Runtime ledger20 records JSON's common status boundary.

## Implementation closure matrix

| Axis | Implementation and owner checklist |
| --- | --- |
| Construction and validation | Runtime aligned/fallible constructor owners cross zero/nonzero lengths and fills, supported alignment classes, invalid-before-allocation, exact payload layout/count and payload/header refusal. Zero acquisition publishes length only after successful initialization. |
| Field formation and authentication | `buffer_field_growth` source owners cover direct/nested/imported/generic records and mutable locals/exclusive parents. Checked-HIR mutations reject shared/immutable root, invalid path/type and forged projection; MIR mutations reject missing/shared/unfounded producer authority. |
| Move-in/out, replacement, return, Drop | Native source owners move/return/replace containing owners and cross early fallible exits. Runtime allocation/free evidence verifies exactly-once cleanup, prefix bytes, reservation reuse and actual reallocation. Reuse existing aligned/fallible/owned-field cleanup owners. |
| Control and arguments | Whole/per-unit execution covers loops, branch joins, match, else, ?, map_err and early exits through complete-owner helpers. Source negatives reject stale/escaping views, Move consumption, shared parents and eager receiver invalidation. Existing alias, ownership and control owners close unchanged paths. |
| Completion and overlap | A non-fallthrough length/value/data operand performs no outer growth and no outer generation transition. Preserve receiver owner identity through eager operands separately from the permitted self-append source view. Field self-append executes both within reserve and across growth; the existing native self-append owner forces relocation; retained old views reject and fresh views work. |
| Projected invalidation footprint | Opaque buffer fields retain the existing conservative complete-owner observation boundary: growth ends pre-growth buffer views derived from that complete owner, including another buffer field. It does not consume the record, prevent fresh reborrows/repeated growth, or end independently backed slice fields. Mixed-record and two-buffer sibling positive/negative owners pin this boundary. No new field-sensitive K1 analysis is claimed. |
| Generic, interface and cache | Imported complete-owner helpers, generic records and authenticated frontend/interface replay agree. Retain existing body/implementation invalidation; no cache format change. |
| JSON exact error | Native matrix covers truncated containers/strings, invalid tokens/UTF-8, trailing garbage, invalid lengths/arguments, untouched arena on rejection, canonical failure output and successful recovery. Driver `json_doc_error` crosses exact enum match, propagation, imported Result helpers, whole/per-unit execution, empty/nested documents and missing keys; ordinary filesystem NotFound/Invalid remain distinct. Audit all consumers of parse status and common conversion, without changing the common mapper. |
| Performance evidence | A bounded local runtime probe records zero acquisition, first read/write and repeated fill, wall/CPU/faults and peak RSS at real-client sizes, plus nonzero/alignment controls. Retain compact exact-artifact results and inspect emitted runtime. No end-to-end consumer gain is claimed from constructor timing. |

Review the new receiver boundary independently before implementation. The
author-side pass binds every applicable cell to exact implementation/tests before
the single final candidate review. If the combined handwritten diff exceeds
1,000 lines, keep the complete receiver proof, its adversarial tests and native
initialization/error owners together: splitting admission from validation or
cleanup would publish an unusable or unsafe intermediate compiler. The two
runtime fixes introduce no dormant producer/consumer seams.

The independent preimplementation review identified the completion/overlap and
projected-invalidation axes above. Both are now explicit before receiver changes.
MIR mutation operands must retain founded exclusive write authority; observation-
only borrowed-handle admission does not prove that authority.


## Author-side closure and local qualification

- `buffer_field_growth::stable_buffer_fields_grow_without_transferring_ownership`
  executes nested generic/fixed-metadata records, fallible construction, complete-
  owner helpers, repeated zero growth, self-append, scalar put, both terminating
  argument positions, move/return/replacement, Result exits, independent slice
  siblings and exact native live-byte cleanup. Whole/per-unit and cold/replayed
  frontend artifacts agree.
- `stable_buffer_growth_rejects_invalid_authority_and_old_views` crosses all three
  operations with shared/immutable/moved roots and stale own/sibling views. It
  also rejects eager move/replacement in both argument positions, escaping views,
  borrowed match payloads and general exclusive partial-field calls.
- `stable_buffer_growth_hir_authenticates_the_complete_mutable_root` rejects
  forged mode, mutability, root, path and type. The driver's
  `stable_buffer_growth_mir_requires_founded_exclusive_write_authority` rejects
  shared authority, missing SSA, invalid field path and absent slot in full and
  partition validation. Existing aligned/fallible/self-append owners preserve
  unchanged ABI, failure ordering, reserved-window and forced-relocation behavior.
- Runtime `zero_filled_payloads_keep_layout_initialization_and_failure_ownership`
  crosses both constructors, empty/nonempty and all 30 supported alignments,
  with exact payload retirement and injected fallible payload/header refusal.
  Existing native buffer owners retain nonzero growth, invalid admission and
  terminal policies. `bench/buffer_zero` records local measurements and their
  unfavorable read-then-write total as well as reduced constructor work.
- `json_doc_failures_publish_only_invalid_and_recover` and the existing JSON
  native matrix preserve exact common Invalid status, zero output and no arena
  publication on failure. `json_doc_error` executes exact enum matching and
  imported propagation in both compilation modes, successful navigation/recovery
  and filesystem NotFound/Invalid controls. Other JSON decoder failure codes use
  `make_error_code`; the shared common-status mapper is unchanged.

The six self-review gates apply without an IR variant, native symbol/layout,
allocation owner or inference rule change. The changed native operations now
join the existing producer contract validation; new source/mutation owners
supply the corresponding negative evidence. No consumer pin, source, test,
fixture, build machinery or branch has been modified.
