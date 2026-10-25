# Text-builder byte views

Status: implemented. Plan240/#1409 supplies ordinary-call read-only authority; consumer adoption remains external.

LLM Request28 asks for exact comparison against accumulated names. Plan140 supplies initialized lengths and Plan209 supplies independent Copy typed-builder reads. This capability supplies the remaining text prefix. All three consumer registers and relevant text-builder use were reconciled; media and voice add no conflicting ownership requirement. Consumer adoption remains external.

The earlier rejected strategy is retained below as historical evidence. On the current compiler, both of its returned-write witnesses reject in whole-program and per-unit checking; returned reads pass. Plan240 derives read-only access from validated types and parameter modes, including ordinary function results, rather than adding a permission field to the old lifetime summary.

## Public-contract ledger

| Surface | Contract |
| --- | --- |
| Source | `b.bytes() -> slice<u8>` for a bound local `builder`, including shared and explicit mutable borrowed builder parameters; no arguments or default. Pure observation, no mutation or consumption. Follow the existing bound-local receiver admission of builder length. |
| Bytes | Exact initialized UTF-8 byte prefix at evaluation time, length equal to `b.len()`. Zero length is valid. No capacity exposure, allocation, copy, null slice or implicit finalization. |
| Access | Always read-only, through direct use, subslices, reinterpretation, validation, admitted view carriers, view captures and ordinary local/imported/generic/indirect view calls. Plan240 owns call-boundary permission; safe writers never gain authority from `unsafe`. |
| Lifetime | Borrow the builder's current storage and owner generation. Each reached `write`, `write_int`, `write_bool`, `write_char`, `write_float`, including zero/empty writes, invalidates earlier views. `to_string`, move, replacement, Drop and arena exit invalidate them. Scalar copies and explicit independently owned byte copies survive. A fresh observation after mutation is valid. No view-bearing array-builder element widening. Builder aggregate placement, capture and function-value formation remain unadmitted; this operation does not widen them. |
| Ordering | Evaluate a receiver once. A write evaluates receiver then argument, preserves native-handle reservation through argument evaluation, and invalidates views at the completed write action. Divergence before the write performs no mutation. An argument that still aliases the builder at a potentially reallocating write is rejected; no hidden self-append copy. |
| Calls | A reader may return a view of a borrowed builder, with inferred caller lifetime and read-only authority. Writes through explicit `borrow mut builder` must propagate invalidation to earlier caller views, including conditional/early/Err exits, while independent builders retain their views. Unknown effects fail closed. By-value ownership transfer cannot leave a usable old view. |
| ABI | One `BuilderBytes` HIR/MIR/runtime key, native A72 `void align_rt_builder_bytes(ptr builder, ptr out)` where `out` receives an `AlignStr` pointer/i64-length descriptor (exact buffer-bytes convention). No new type tag/interface field/cache option. Existing executable identity invalidates old code. Native aligned live Builder pointer obligation; empty view remains well-formed. Use the existing conservative BufferBytes effect row exactly: IndirectStorage, argmem Unstated, no parameter refinements, escapes [0,1], releases None, returns_fresh false, diverges false. The native operation reads builder storage and writes only the output descriptor; the row does not claim that either pointer is captured. |
| Runtime | Reuse BuilderBuf initialized-prefix observation for heap/arena buffers and compiler stack headers. No layout/allocator/provenance change. Finished/freed native handles are outside FFI safety preconditions. |
| Agreement | draft.md, language-spec, design-notes, Settled, core-design/string.md and its ja/string.md mirror, HIR/runtime ABI ledgers, declaration golden. No performance/resource promise beyond allocation-free observation, owned by allocation counters rather than timing. |

## Implementation closure matrix

| Axis | Proposed implementation / owner |
| --- | --- |
| Types and receiver | New exact unary operation and bound Builder receiver; wrong arity/type/fresh receiver and forged checked-HIR nodes in builder_byte_views. |
| Formation and views | Owner lifetime plus read-only root; all exhaustive walks, clone/replay, region/call summaries, storage and byte-observation paths. Direct/imported readers, typed views, UTF-8 and small carrier negatives. |
| Mutation | Add text-builder writes to existing completed source-visible storage invalidation; keep exact receiver reservation and reject aliased input to write. Cross all five writers with direct/borrow-mut/generic/imported calls, independent-owner positive and real-source negative. |
| Move/cleanup | to_string, replacement, move, lexical/arena Drop and return; heap/arena/stack-header native cases; old view reject and independently copied bytes remain valid. No new cleanup owner. |
| Control | if/match/else/?/map_err, early returns and loop joins preserve only reached mutation and owner lifetime; shared reader returns do not manufacture writable or static backing. |
| ABI and native | Registry/effect/golden and independent native signature/export owners; whole/per-unit/native prefix equality for empty, multibyte, primitive formatting and growth; read operation allocation delta zero. |
| Consumers | R28 remains partially delivered until the provider capability merges; consumer code/pins and original interning adoption stay external. Existing media/voice builder use remains source-compatible. |

## Implementation strategy and exact owners

The storage authority remains the completed receiver identity. `BuilderBytes`
borrows that identity, produces the exact `slice<u8>` type, and adds read-only
access at the descriptor. It follows BufferBytes through escape/region,
MoveCheck storage/lifetime facts, borrowed-parameter summaries and checked replay;
its authority follows StrBytes and the Plan240 ordinary-result policy. No new
permission representation, unknown-origin exemption or lifetime rule is proposed.
Read-only permission and allocation identity are separate: native reads and
UTF-8 validation retain the actual builder backing even though writes reject.

All five `BuilderWrite` operations join the existing completed storage-mutation
action. Extend the native growth receiver classifier used by BufferAppend for
reservation validation and retirement; its semantic name must include text
builders. Preserve selected receiver sources through eager argument completion
using the shared completed-action traversal. Before invalidation, validate the
receiver; retire only its action reservation. Preserve a separate completed
selected-storage fact for the receiver: scope tails forward it, if/match join
only returning value alternatives, and a bound selected builder contributes
its operand-completion storage fact, not its value/retained-content roots.
Use the existing completed-action traversal for source enumeration and keep
`ParamStorage` distinct from `Param`; do not derive storage from an If/Match's
ordinary value roots. Freeze the fact before later eager operands. Fresh owning
results have their existing temporary-owner fact and cannot borrow earlier local
storage. No new source type or interface fact is required. Invalidate this
selected-storage fact through a shared fact-based storage invalidation helper
also used by direct storage actions, then validate all other completed operands
afterward. Thus self-append from
a still-aliasing str fails before code generation, including an empty input,
while a scalar copied from the prefix or an explicit independently owned clone
is valid. A diverging argument reaches no mutation action. Do not use AST
spelling, a source local's post-argument value, or a hidden runtime copy as proof.

The same storage access records `borrow mut` effects, including all returning
exits. Call application uses its existing exact argument and effect mapping;
independent owners must retain their observations. The R49 reduced witness and the new text-builder loop owner expose the same
precision defect: a completed mutable call can install an empty projected-header
fact for a native headerless owner. On loop refinement, `local_storage_roots`
treats the presence of that empty fact as authoritative absence of storage and
omits the stable native owner identity. Byte mutation then becomes unknown and
invalidates observations from unrelated owners.

Preserve the stable owner root when the value owns storage and its validated
static type has no projected header paths, even if an empty fact is present.
Do not apply this fallback to a collection/carrier whose static type requires
headers: an empty selected payload or incomplete header graph must retain its
existing conservative semantics. This restores the same identity already used
when the empty entry is absent; it introduces no unknown-backing exemption.
Cross text and scalar typed builders with source buffer/builder validation,
loops, direct and helper writes, real-source mutation rejection and independent
owner positives. The authenticated original R49 helper and immutable control pass both whole
and per-unit checks in an owned copy; the paired source-mutation negatives remain
rejected. Consumer adoption remains external. This shared precision correction is
required to close the text-view loop contract, rather than a separate producer.


The author-side selected-receiver owner exposed one additional actual-MIR
closure cell. Borrow lowering stores a selected native pointer in both a control
result slot and a conditional cleanup slot while retaining the same SSA for the
write. These are real pointer aliases; Store itself does not free the object.
The existing indexed-builder read proof erases all aliases at every Store and
therefore cannot directly admit this sequence for text builders.

Independently accepted proof refinement:
track live native allocation identity across Store aliases. A reached Drop,
DropValue or consuming finish/build retires every alias; source nulling or
DropFlagMoveOut clears only that source place. Store links the incoming live
identity to its destination and invalidates an already reserved read snapshot
when it changes the selected owner place, without manufacturing a fresh
identity or erasing live non-owning aliases. Overwriting an old destination
continues to invalidate its prior observation. An exclusive call retires old
SSA and slot aliases, then installs a fresh identity only in the actual caller
place passed by reference, never every slot that happened to alias the old
pointer. A finite optional-boolean fact follows Const Bool, Use, Load and Store
for local cleanup flags only when every possible writer is accounted for.
Parameters, borrowed cleanup slots, every borrowed-place root/cleanup address in
direct/runtime/indirect/raw calls, native output slots and non-Store statement
destinations are ineligible for slot constants. Their Loads remain unknown;
immutable SSA snapshots retain their observed value. The call-family classifier
and statement inventory are exhaustive, and the native output inventory is shared
with producer validation. A known Branch follows only its real successor; an
unknown condition retains both. This preserves selected identity versus conditional
cleanup without a symbolic path solver. A getter must still have an exact fresh
Load shape and an unchanged live reservation whenever reached. Exhausting all
actual paths without reaching a structurally valid dead getter is success.
This boolean write/exposure and semantic-reachability axis was added after the
independent review reproduced stale false facts hiding a Drop, and valid dead
getters being rejected. Owner pairs cover mutable direct/indirect/imported Boolean calls,
dead literal/local conditions, both builder families and mixed borrowed/fresh
cleanup. Do not use source-name exceptions or assume
that a native handle's declared type proves temporal liveness.

Extend the existing parameterized indexed-builder forged-MIR retirement owner
alongside text-byte cases: original/Load/Use SSA, Store aliases, Drop/finish,
replacement, exclusive helper effects and branch/loop joins. All previous
revival/replacement negatives must remain rejected. Add selected borrowed/fresh
write followed by a fresh original-owner read, including mixed bound/fresh branches, and ensure a captured old alias
does not follow a helper's replacement into a new generation. This is one shared
native-identity proof; it does not widen source receiver admission or typed
builder element reads.

The author forged-MIR probe found a pre-existing validation boundary: directly
inserting BytesFill after a read-only getter is accepted by the actual-MIR
validator. BytesSet/BytesFill/BytesCopyFrom do not have a complete writable-backing
contract there. Source checking and independently reconstructed checked-HIR body
facts reject these writes, including builder bytes. Plan195 explicitly separates
mutable Copy-header authority from backing authority, and Plan240 owns read-only
and no-rebind reconstruction at checked HIR. This capability preserves that
boundary; it does not claim arbitrary-MIR writer-authority closure.

Independent strategy inspection rejected a partial upgrade of incoming
BorrowMut slices or BufferBytes owner provenance. A complete separate capability
must connect writable producers, inherited outer-view restrictions, projections,
every direct/imported/indirect mutable call, and post-call descriptor replacement.
It must transport authenticated no-rebind information or independently derive it;
unknown replacement must downgrade authority. The current flow-insensitive MIR
slot graph and mode alone cannot prove that. Existing process-Out proofs must
remain intact. No getter-specific writer exception or permission upgrade is added.

The parameterized checked-HIR readonly-publication owner covers all readonly
origins, adding BuilderBytes, across ordinary assignment and the three native byte
writers, with readable positives. Actual-MIR owners cover exact getter types,
shared producer access and live native identity, not the deferred general writer
proof. The exploratory forged-BytesFill probe is retained with local review
evidence; it is not a passing safety assertion or an executed native program.

At MIR, add the getter to the native operation contract, independent type/effect
validation, typed builder-handle provenance and retirement checks wherever they
own the same native pointer obligation. Forwarding or loading a retired handle
must not revive it. A borrowed result must not be misclassified as an
independent scalar by producer, loop-fact, cleanup or alias consumers. Lowering
uses the existing A72 output-descriptor call and load convention. Existing finish/Drop/layout and allocation paths
are reused unchanged.

The single capability is expected to exceed 1,000 hand-written changed lines:
a new borrowed producer and every invalidating writer need one complete safety
proof across HIR, MIR, ABI and native execution. Splitting the getter from the
invalidation/authority consumers would expose a dormant unsafe chain and repeat
that proof without a useful stable consumer.

| Owner target / invariant | Required cases |
| --- | --- |
| `align_driver --test builder_byte_views` formation and access | bound immutable/mutable/value/shared/exclusive receiver; wrong arguments/types and temporary/control receiver reject; direct and returned read pass, direct/returned/subslice/typed-view/native-writer writes reject, including admitted carriers and view-call function values; whole/per-unit and imported generic readers |
| same target, lifetime and mutation matrix | every write kind, empty write, direct/local/imported/borrow-mut calls, if/match/else/?/map_err and loop/early exit; old view rejects, fresh and independent-builder view passes; self-append rejects, copied scalar/owned clone survives; finish/move/replacement/Drop/arena escape rejects |
| same target, eager order | selected scope/if/match receiver for local and borrowed-parameter builders across later move/replacement/nested write, stale returned/direct views, self-append and divergent operands; exact reservation, no action before operand completion, valid fresh/independent receiver and cleanup |
| same target, native prefix and cache | initialized bytes including NUL, multibyte, all formatted scalar writers, growth, empty and arena/heap/stack header; whole/per-unit execute and imported cache hit/edit/restore |
| `align_mir --lib` checked-HIR and actual-MIR owners | forged receiver/result/arity/effects and checked-HIR readonly-publication evidence; wrong handle type, forwarded retired handle and stale post-call aliases rejected at full, partition and LLVM boundaries; valid transferred/borrowed live handles preserved |
| `align_sema --lib storage_generation_expr_variant_sweep`, `align_mir --lib request11_expr_kind_inventory_tripwire` | exact variant inventory and all recursive analysis consumers; both counts advance with the operation |
| `align_runtime --lib builder_bytes` | exact prefix pointer/length for heap/arena/stack headers and zero allocation/reallocation/free delta for observation |
| runtime key and codegen ABI tests plus independent native export/signature audit | one getter, exact pointer/length ABI, read/return-alias effects, every probe-feature configuration |

Normative agreement is required in `draft.md`, `docs/language-spec.md`,
`docs/design-notes.md`, Settled in `docs/open-questions.md`,
`docs/impl/core-design/string.md` and `ja/string.md`, and the checked-HIR and
runtime ABI ledgers. No new CLI options, configuration, encoding, error model,
artifact format, cache discriminator or allocation provenance is introduced.
Invalid source, malformed checked HIR, and malformed getter types or retired
native handles reject before native execution; the general arbitrary-MIR writer
gap above is separate. Diagnostics follow existing type validation and source
completion order. Runtime callers must
supply an aligned live initialized builder handle; raw FFI misuse does not
acquire a safe-language promise. The author-side obligation pass maps every public ledger row to the owner table;
no selected requirement is delegated to a later producer-only PR. The first
strategy inspection found selected borrowed-receiver storage/value conflation
and an ABI spelling mismatch. The matrix now requires completed selected storage
facts and the exact existing A72 output convention before implementation.
No timing benchmark is required because the
only resource promise is allocation-free observation, measured by counters.

## Historical rejected strategy (before Plan240)


The initial proposal assumed existing immutable-text roots and flat lifetime
summaries could preserve a new byte view's read-only permission through helpers,
carriers and imported returns without new interface facts. Independent review
found that premise false. At merged main `f71c934a5858932150538879e18c54176fe1e471`,
the checker rejects writing through a direct `text.bytes()` result but accepts
both of these compile-only negative witnesses:

```align
fn prefix(text: str) -> slice<u8> = text.bytes()
fn main() {
  mut text := "abc".clone()
  mut view := prefix(text)
  view[0] = 65 as u8
}
```

```align
fn prefix(text: str) -> slice<u8> = text.bytes()
fn main() {
  mut text := "abcdefgh".clone()
  view := prefix(text)
  mut typed: slice<u32> := view.view_le() else { return }
  typed[0] = 0 as u32
}
```

These witnesses were checked only, never executed. A shared returned-view read
also checks successfully. The distinction is permission transport, not type
formation or ownership of a copied scalar.

`summary_from_roots` deliberately removes `BorrowRoot::ReadOnly` when producing
`ReturnBorrowSummary`; that public record carries only parameter/capture indices.
`map_summary_roots`, `selected_move_call_headers` and
`mutable_call_result_completion` do not restore returned read-only authority.
Internal projected header facts are not a public transport contract. Existing
mutable-view effects describe eligible direct Copy-view parameter writes and
rebindings; they do not encode returned permission or selected result paths.

This is the already-recorded prerequisite in Plan61 and Plan196, not a new
builder-specific defect. A new `BuilderBytes` producer or a return-only flag
would leave hidden callee writes, mutable destinations and recursive admitted
carriers without a complete proof. The proposed operation cannot safely be
published by reusing those records unchanged.
