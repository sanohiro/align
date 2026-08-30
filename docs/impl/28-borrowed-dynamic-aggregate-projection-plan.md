# Borrow-safe dynamic aggregate projection

Status: **IMPLEMENTED; Request 17 adopted, Request 21 projection-view repair implemented with
align-llm adoption pending.**

This document extends the borrowed sum-payload projection shipped by
`26-borrowed-sum-projection-plan.md`. The shipped capability intentionally rejects every dynamic
array reachable from a projected payload and accepts shared-borrow call arguments only when they
are named locals or struct fields. The decoded C6 verifier needs both missing pieces: optional
records contain dynamic arrays, and dynamic arrays contain Move records whose nested options must
be inspected without extracting an element owner.

This is one compiler capability, not an application exception. It preserves the existing
`borrow` model, tagged layouts, dynamic-array layouts, Drop plans, and function ABI. The complete
producer-to-consumer path spans semantic classification, move and escape analysis, checked HIR,
MIR, LLVM lowering, generic rechecking, interfaces, and owner tests. Splitting the implementation
between array-bearing sum payloads and indexed shared borrows would leave the first half without a
useful stable consumer and duplicate the same alias, lifetime, and no-cleanup proof. The resulting
implementation may exceed roughly 1,000 changed hand-written lines; the atomic boundary has lower
integration risk because neither dormant half is independently useful or safe to expose.

## 1. Public-contract ledger

| Surface | Exact input and defaults | Result, errors, and validation order | Ownership, lifetime, allocation, and cleanup | Compiler/interface owner | Identity, prerequisite, and acceptance |
| --- | --- | --- | --- | --- | --- |
| Dynamic-array-bearing borrowed sum payload | Existing exhaustive `match place { Variant(binding) => arm, ... }`. `place` satisfies the exact stable borrowed-place rule in plan 26. The payload satisfies `BorrowedDynamicPayload` below. No new syntax, option, type argument, environment input, or default. | The existing tag is read in place. Sema first resolves the complete sum and pattern, then verifies the exact borrowed place, then classifies every bound Move payload in variant/payload order. The first unsupported reachable type keeps the existing borrowed-place diagnostic. Exhaustiveness and ordinary pattern diagnostics retain their current precedence. | The active payload and every reachable dynamic-array header and element remain owned by the original sum. The binding has no independent storage, cleanup bit, source nulling, allocation, mutation, or `Drop`. Read-only tag, length, Copy/view element, string-as-`str`, record-field, and shared-borrow operations are permitted. A whole array, owning element, or enclosing Move payload cannot be moved, replaced, stored, returned, captured, sent, or retained. Derived views carry the original owner generation and underlying region roots. | `align_sema` extends the one cycle-safe payload classifier and records the existing projection metadata. Checked-HIR replay independently recomputes the expanded grammar and cleanup exclusions. MIR retains a pointer/path operand; LLVM reads the array header and element storage through that pointer. No runtime helper, native ABI field, or interface-body field is added. | The extension consumes plan 26 and the existing dynamic-array/AoS representation. Structural reachable types remain part of MIR/cache identity; generic bodies are rechecked from transported source, while non-generic interfaces retain signatures only. Direct and nested `Option`/`Result`/user-sum owners, malformed metadata owners, and the real align-llm result/evidence graph are acceptance evidence. |
| Copy-view read from an admitted dynamic array | Existing `values[index]` and `values[index].field` syntax on an ordinary dynamic `array<str>` or AoS `array<CopyRecordWithViews>`. The base may be a direct borrowed local/parameter, a checked struct field, or an array-bearing borrowed sum projection. The index has existing type, order, and bounds semantics. | Sema resolves the base and exact Copy element/field type, then evaluates the index once. A terminating index (`?`, `return`, `break`, exit/abort, or divergence) produces no bounds action, result, later consumer, or retained fact. A fallthrough index uses the existing hard bounds error. Unsupported/Move value forms keep their existing diagnostics; the separate indexed shared-borrow row handles admitted Move elements. | The Copy result owns nothing. Every admitted region-bearing Copy leaf, including `str` and every `slice<T>`, carries the source array generation and all contained input/arena region roots through direct use, return, control joins, and `BorrowMutRetentionSummary` destination retention, including when nested in a Copy record. The destination must outlive those roots. Moving, dropping, replacing, or mutably borrowing the source invalidates the derived result under existing rules. No allocation, cleanup, nulling, transfer, or independent owner is created. | `align_sema` maps ordinary `Index`/`ElemField` facts from the complete direct/field/projected base through the canonical Copy/borrow classifiers rather than defaulting the projection binding to static provenance or naming one view type by hand. Checked HIR independently replays that mapping and termination. MIR uses the existing bounds-checked Copy `Index`/`ElemField` nodes; LLVM performs their existing pure loads. No new representation or ABI field is added. | Direct/field/projected `array<str>` plus AoS Copy records with direct and nested `str` and `slice<T>` leaves cover return, exact direct and fallback retention, input/arena escape rejection, source invalidation, terminating index families, generic/imported whole/per-unit parity, and no unrelated cache/ABI change. A parameterized type-class owner enumerates every array/record-admitted Copy leaf for which the canonical borrow/region classifiers report provenance, so a future admitted view cannot bypass this row. |
| Indexed shared borrow of an owning dynamic-array element | Existing call syntax `inspect(values[index])`, where the selected direct, imported, or function-value target's parameter mode is exactly `borrow`. `values` is a stable named local, borrowed parameter, projected borrowed binding, or struct-field path of one of those, and has an admitted ordinary dynamic scalar-array or AoS record-array type. `index` has the existing array-index type and semantics. No implicit `borrow`, temporary array materialization, or `borrow mut` element form is added. | Static checking resolves the call target and mode, checks the base place and concrete element type, reserves the complete source root, then checks the index expression and every later call argument in source order. A non-array, unsupported array class, non-stable base, type mismatch, by-value target, or `borrow mut` target is rejected before MIR. Any operation before the call action that might move, drop, replace, or mutably borrow the same or an overlapping source root is rejected; uncertain imported or indirect invalidation is conservatively overlapping. If the index terminates, no bounds guard, descriptor, later argument, pointer, or call is formed. On fallthrough the base and index are each evaluated once: MIR emits the existing bounds-failure CFG at that argument's source position, retains only a guarded place descriptor while later arguments evaluate, revalidates the root on every fallthrough path at the call action, and only then forms the pointer. A terminating later argument forms no pointer and invokes no call; an invalid index keeps the existing hard bounds error before later arguments. | The callee receives a non-null pointer to the existing element for the dynamic extent of the call. The caller and array remain owners; there is no element load, transfer, nulling, cleanup bit, temporary owner, allocation, or independent `Drop`. A view returned through the existing return-borrow summary, or retained in a mutable destination through the existing `BorrowMutRetentionSummary`, is rooted in the source array generation and every contained region root. Exact same-program direct summaries and conservative imported/indirect fallback both substitute the indexed source this way, and the destination must be able to outlive those roots. Moving, dropping, or validly replacing the array invalidates every derived view under existing rules. The element itself cannot be mutably borrowed, replaced, or retained as an owner by this surface. | `align_sema` admits and records the indexed shared-borrow argument, derives exact storage roots, carries the existing eager-operand snapshot from that argument through every later argument to the call action, and applies direct/imported/indirect return and retention summaries. Checked HIR recomputes base stability, mode, type, index fallthrough, target mode, reservation, later-argument flow, and action-boundary revalidation. MIR carries one bounds-guarded dynamic element place descriptor only after a fallthrough index. MIR owns bounds-failure semantics and validation proves that the guard dominates every use and that every intervening fallthrough path preserves the root; LLVM computes the pointer only while lowering the call and passes it through the existing borrowed parameter ABI. | The call changes only implementation/body identity. Function signatures, interface format, runtime ABI, and persisted formats are unchanged. Direct/local/field/projected bases, scalar-string and Move-record arrays, direct/imported/function-local/function-field/joined targets, returned and mutably retained views, source invalidation during the index and in every later argument, terminating index and later arguments, malformed MIR, generic whole/per-unit parity, and cache edit/revert owners are required. |

There is no native or wire text boundary, persisted scalar/tag encoding, runtime inspection table,
connection-global state, process-global state, or benchmark promise in this capability. Those
ledger dimensions are N/A. All source text and diagnostics retain Align's existing UTF-8 source
rules; no value text crosses a new FFI boundary.

## 2. Admitted grammar

`BorrowedDynamicPayload` is plan 26's `BorrowedSumPayload` with exactly these additions:

```text
BorrowedDynamicPayload :=
  CopyScalarOrView
  | string
  | Struct(BorrowedDynamicPayload fields)
  | Option(BorrowedDynamicPayload)
  | Result(BorrowedDynamicPayload, BorrowedDynamicPayload)
  | UserSum(BorrowedDynamicPayload payloads)
  | DynamicScalarArray(BorrowedDynamicPayload scalar element)
  | DynamicRecordArray(AdmittedRecord element, AoS)
```

The traversal is finite, cycle-safe, concrete after monomorphization, and fail-closed on missing
type-table entries. `DynamicScalarArray` is the ordinary `array<T>` representation carried by
`Ty::DynArray`; its element must already be a representable scalar and satisfy the same payload
grammar. `DynamicRecordArray` is the ordinary AoS `array<Record>` representation carried by
`Ty::DynStructArray(_, Layout::Aos)`; the element record and every reachable field satisfy the
same grammar. This admits the C6 graph's arrays of owned strings, arrays of records, and records
that themselves contain optional records and further ordinary dynamic arrays.

Fixed arrays, dynamic arrays of slices, opaque response arrays, dynamic vector/mask/fixed-aggregate
arrays, SoA, buffers, builders, boxes, resources, resource references, raw owning handles, and every
other specialized collection or opaque Move shape remain excluded. `array<Option<T>>` and
`array<array<T>>` remain outside the language's current dynamic-array element representation rather
than becoming representable through this projection. A future new dynamic-array representation is
not admitted automatically; the closed classifier and its variant sweep must be extended together.

For ordinary Copy `Index`/`ElemField` provenance, “view” is not a `str`-only shorthand. It is the
complete intersection of the existing array/record-admitted Copy grammar with the canonical
borrow/region classifiers. Current concrete owner witnesses include direct and nested `str` and
`slice<T>` leaves. The implementation and checked-HIR owner enumerate that intersection from the
type tables; admitting another region-bearing Copy leaf without adding its projection case and
owner is a variant-sweep failure.

The indexed shared-borrow surface uses the same two ordinary dynamic array classes and concrete
element grammar. Copy element indexing keeps its existing by-value behavior. This plan admits a
Move element only as the argument to an explicit shared-`borrow` parameter. The later Request 22
extension in plan 30 additionally maps ordinary `array<string>[index]` to the canonical `str` view;
every other whole Move element still rejects in a value position, and `borrow mut` remains deferred
because it would require element-local replacement and cleanup state.

## 3. Semantic and lifetime contract

### 3.1 Array-bearing match projections

The borrowed match mode, exact-root predicate, binding metadata, nested borrowed-match exclusion,
and owning-mode parity are unchanged from plan 26. Admitting an array in the reachable graph does
not make an array header an owner of its own. Reading `.len()` projects and loads only the length.
Reading a Copy/view element uses the existing checked index path. Reading a string element uses the
existing non-consuming string-to-`str` behavior. A Move record element is inspected through the
indexed shared-borrow call below; it is never loaded as a value merely to reach one field.

The existing Copy path must not erase provenance merely because the array header is reached through
a borrowed projection. An `array<str>` element and every region-bearing Copy field of an AoS Copy
record, including direct or recursively nested `str` and `slice<T>` leaves, inherit the complete
projection owner generation and contained input/arena roots. Ordinary
return-borrow substitution and `BorrowMutRetentionSummary` destination substitution preserve those
roots for direct, field, and projected bases. A terminating index forms no bounds action or result.

An array-bearing projection may be matched repeatedly, passed to a shared-borrow helper, and read
on branch, loop, and early-return paths. It cannot be the source of `else`, `?`, by-value call,
assignment, return, value-carrying join, task capture, or array pipeline operation that consumes a
Move element. The existing owning forms retain their current extraction, nulling, element Drop,
and partial-cleanup behavior.

### 3.2 Indexed shared-borrow place

The declaration side of the indexed-borrow example is:

```align
Record { value: i64 }
Container { values: array<Record> }
fn inspect(borrow value: Record) -> i64 = value.value
```

Positional call expressions then use the existing call syntax:

```align
fn inspect_two(
  borrow values: array<Record>,
  borrow container: Container,
  i: i64,
) -> i64 {
  first := inspect(values[i])
  second := inspect(container.values[i])
  f := inspect
  third := f(values[i])
  return first + second + third
}
```

where `inspect`'s corresponding parameter is explicitly `borrow`. The same form is valid through an
imported target or a function value whose selected parameter mode is `borrow`, including a local,
function-valued field, or joined target. The base before `[i]` must resolve
to a stable local or struct-field path, including a field rooted in a plan-26/plan-28 projection.
Fresh array constructors, function/control results, pipeline results, nested indexed results, and
arbitrary temporaries are rejected. A field below the indexed element is not a separate borrow
argument in this capability; pass the complete element to a helper and inspect its fields through
that helper's ordinary borrowed parameter.

The checker reserves the complete array root before checking the index expression. The index is
evaluated exactly once after all earlier call arguments and before every later argument, preserving
existing left-to-right call evaluation. The reservation remains live through every later argument
and the call action. A move, Drop, replacement, by-value transfer, or `borrow mut` operation that
might invalidate the same or an overlapping root anywhere in that interval is rejected; an
uncertain imported or indirect effect uses the conservative overlapping fallback. Mutation of an
unrelated root remains valid. This reuses the existing eager-operand snapshot and action-boundary
revalidation mechanism rather than introducing a call-specific lifetime model.

MIR emits the existing signed bounds-failure CFG at the indexed argument's source position, so an
invalid index aborts before any later argument as ordinary left-to-right evaluation requires. The
index expression must first fall through; a `?`, `return`, `break`, exit/abort, or divergent index
forms no guard, descriptor, later argument, pointer, or call. The successful bounds block records
only the base identity, checked index, exact element type, and live root
reservation; it does not form or retain an array pointer or header field. After every later argument
falls through, the call action revalidates the reservation and forms the element pointer. A later
argument that returns, breaks, exits, aborts, or otherwise diverges discards the analysis-only
reservation and forms no pointer or call. A rejected static form creates no HIR/MIR element place.
A runtime bounds failure invokes no callee and creates no owner or cleanup action.

The element pointer itself is valid only for the call, but existing summaries may retain a derived
view. Return-borrow substitution and `BorrowMutRetentionSummary` substitution both map the indexed
parameter to the array place, its owner generation, and every contained region root rather than to
the numeric index or a temporary header. Exact same-program direct summaries retain only the
reported roots; imported, indirect, missing-body, malformed, and unresolved targets use the
existing all-compatible-view fallback. A mutable destination must be able to outlive every
substituted source root. A returned or retained view cannot outlive an arena-backed array or its
borrowed input, and moving, dropping, or replacing the array invalidates it. Different indices are
not inferred disjoint: any simultaneous `borrow mut` peer of the same array conflicts
conservatively, and element `borrow mut` itself is rejected.

## 4. Checked representation and lowering

The existing `BorrowedProjection` record remains authoritative for a tagged payload. Its
`static_ty`, ordered path, exact owner fact, and projection-only cleanup exclusions now validate
against `BorrowedDynamicPayload`. No array index is stored in that record.

Copy `ExprKind::Index` and `ExprKind::ElemField` remain unchanged representations. When their base
resolves through a `BorrowedProjection`, producer and checked-HIR replay map the complete projection
owner generation and contained region roots into the result fact before return or retention
substitution. A non-fallthrough index retains the checked diagnostic subtree but contributes no
MIR bounds action, result, or provenance fact.

Checked HIR represents the call-only element place explicitly rather than weakening ordinary
`ExprKind::Index`:

```text
BorrowedElementBase {
  root_local: LocalId,
  path: Vec<BorrowedPathSegment>,
  array_ty: Ty,
  element_ty: Ty,
  owner_fact: Vec<BorrowedRootFact>,
}

ExprKind::BorrowedIndex {
  base: BorrowedElementBase,
  index: Box<Expr>,
}
```

The expression's type is exactly `element_ty`. The path starts with `RootSlot`, then contains only
the existing typed struct/sum segments. The variant may occur only as an immediate argument whose
selected direct/imported/indirect signature entry is `borrow`; ordinary `ExprKind::Index` continues
to reject recursively Move results. Checked-HIR validation independently replays the base path,
array/element type, canonical owner facts, target mode, source-order flow, and source-root
reservation from `index` through every later argument and the call action. Each fallthrough path
must preserve the recorded generation; a terminating later argument has no action-boundary use.
Missing, duplicate, unsorted, forged, or stale metadata rejects before MIR. The owner-fact check is
field-complete rather than a vector-equality smoke test: it rejects an empty fact vector, removal of
any fact from a multi-root vector, an extra or duplicated fact, non-canonical order, and mutation of
each fact's `kind`, `ordinal`, or typed `path` (missing `RootSlot`, an invalid segment, or a valid but
different place). Replay also rejects a fact made stale by root replacement, move/Drop, or a branch
join and a record whose source generation or contained-root set no longer equals the action point.

That checked HIR argument lowers to one MIR-level place equivalent to:

```text
BorrowedElementPlace {
  base: BorrowedPlace,
  index: Operand,
  element_ty: Ty,
}
```

The exact Rust representation may differ, but it must preserve those three fields and may exist
only at an explicit shared-borrow call position after MIR has emitted the existing bounds-failure
CFG at that argument position. `base` retains the root slot and ordered static struct/sum path;
`index` is the once-evaluated checked integer operand; `element_ty` is recomputed from the concrete
dynamic-array class. The record is a guarded descriptor, not a materialized pointer. Its guard
names a unique inert reservation marker emitted before index evaluation and the exact checked
length value; it never stores block or statement ordinals. After all MIR rewrites, validation
locates that marker and reconstructs the unique canonical bounds-success edge from the checked
length and index, so block compaction and statement fusion cannot stale the proof. Direct,
imported, and indirect call lowering preserve and validate the selected function type's parameter
mode. MIR validation rejects a mismatched base type, unsupported array class, wrong element type,
missing slot/path, non-`borrow` target mode, cleanup bit, malformed index operand, a use not
dominated by its exact successful bounds guard, or any possibly invalidating operation on a
fallthrough path from that guard to the call action.

At the call action, after all argument operands exist, LLVM loads the base pointer through the
already validated borrowed place, computes `ptr + index * stride` using the canonical scalar or AoS
record layout, and passes that element pointer to the unchanged borrowed parameter ABI. Bounds
failure is already explicit MIR control flow; LLVM introduces no independent bounds decision. It
does not load the whole
array header as an owning value, extract a Move element, create an alloca for the element, copy
element bytes, emit a cleanup flag, or call a runtime helper. Scalar/record alignment and stride
come from the same producer-owned type/layout tables as ordinary indexing.

Generic source bodies are reparsed and rechecked after concrete substitution. Non-generic
interfaces transport only the unchanged signature and structural type graph; no indexed body place
is serialized. MIR fingerprints include the new place and exact structural array/element types.
Whole-program and per-unit compilation must reconstruct identical acceptance and executable
behavior. No interface-format or runtime-ABI version changes.

## 5. Implementation closure matrix

| Axis | Required closure | Exact owner evidence |
| --- | --- | --- |
| Type formation and classification | Extend the one cycle-safe borrowed-payload classifier through only `DynArray` and AoS `DynStructArray`; recurse through concrete scalar/record elements; preserve all exclusions and malformed-table failure. | `align_sema` wildcard-free type/classifier matrix; direct/nested/imported/generic accepted graphs; each excluded dynamic/fixed/specialized collection twin. |
| Match construction and binding | Produce existing borrowed metadata for direct `Option<array<i64>>`, `Option<array<string>>`, `Option<array<MoveRecord>>`, `Result`, user sum, and an array-bearing record payload; bind no independent owner. | sema metadata owners and driver direct/field/nested-field/`borrow mut` positives; owning twins retain transfer. |
| Copy-view element projection | Preserve the complete direct/field/projected base owner generation and contained regions through ordinary `Index` and `ElemField` on `array<str>` and AoS arrays of Copy records with every region-bearing Copy leaf admitted by the canonical type classifiers, including direct/nested `str` and `slice<T>` fields. Carry exact return and mutable-destination retention, and reject escapes or source invalidation. A terminating index creates no bounds/result/retention fact. | direct/field/projected array-of-`str` plus Copy-record direct/nested `str` and `slice<T>` matrix; a parameterized classifier sweep over every array/record-admitted Copy leaf for which `ty_may_borrow` or `region_bearing` is true; returned view/record positives; exact direct and conservative fallback `BorrowMutRetentionSummary`; input/arena escape and move/drop/replacement/borrow-mut invalidation negatives; `?`/return/break/exit/abort/diverge index twins; generic/imported parity. |
| Indexed call formation | Admit only explicit shared-borrow parameters and stable local/field/projected dynamic-array bases through direct, imported, and indirect/function-value targets; preserve the selected parameter mode through local, function-field, and joined targets; reject by-value, `borrow mut`, temporary, nested-index, unsupported element forms, and incompatible target joins. | sema diagnostic/precedence table; direct/imported/function-local/function-field/joined target matrix; whole/per-unit mode parity. |
| Move-in, move-out, source nulling, replacement, and return | Projection and indexed borrow perform none. Whole projected payload/array/element consumption, storage, return, capture, and mutable element replacement reject. Owning matches and ordinary whole-array moves remain unchanged. | MoveCheck parameterized negative matrix; repeated source use; owning parity; source replacement invalidates returned views. |
| Borrow roots and escape | Indexed element calls substitute the complete array storage roots, generation, and contained region roots into returned views and mutable destinations. Apply exact direct `BorrowMutRetentionSummary` rows and conservative imported/indirect/missing-body fallback; require the destination to outlive every substituted root. | returned and mutably retained string/slice view positives; exact direct versus fallback retention; arena/local/destination escape negatives; move/drop/borrow-mut peer invalidation; no index-as-root artifact. |
| Control flow and eager call evaluation | Match and indexed calls compose with `if`, nested ordinary blocks, loop joins, divergent arms, and early return without manufacturing moves or fallthrough. Reserve the source root before evaluating the index once in argument order and carry the eager snapshot through every later argument to the call action; reject same/overlapping-root move, Drop, replacement, by-value transfer, or `borrow mut`, conservatively reject uncertain imported/indirect invalidation, and permit unrelated-root mutation. A terminating index creates no guard/descriptor/later argument/action; a terminating later argument creates no action-boundary pointer/call. `else`, `?`, and nested match over a projected binding retain their existing exclusions. | direct control-family owner matrix; `?`/return/break/exit/abort/diverge index no-guard/no-descriptor/no-later-argument/no-call twins; same-root invalidation in the index and each earlier/middle/final later-argument position for every operation; uncertain indirect/imported negative; unrelated mutation positive; return/break/exit/abort/diverge later-argument twins; side-effecting index and multi-argument order runtime owner; unchanged plan-26 negatives and owning control owners. |
| MIR and checked HIR | Recompute exact payload eligibility and cleanup exclusions; validate Copy `Index`/`ElemField` projection roots and every borrowed element base/index/type/mode and direct/indirect selected mode; include the descriptor in exhaustive traversal, printing, fingerprinting, and variant tripwires. A terminating index emits no action. On fallthrough MIR emits one inert, uniquely identified reservation marker before index evaluation and the existing bounds-failure CFG at the argument position, carries no pointer through later arguments, and validation locates the marker and reconstructs the unique canonical successful guard after every MIR rewrite. The descriptor stores no block or statement ordinal. The reconstructed success edge must dominate the action, and every intervening fallthrough path must preserve the root. | forged payload/static type/path/cleanup and Copy-view root records; malformed element slot/path/class/index/type/mode/target/guard; parameterized `BorrowedElementBase.owner_fact` mutation owners for empty, each removed fact, extra, duplicate, unsorted, each alternate/wrong `kind`, out-of-range and wrong in-range `ordinal`, missing-root/invalid-segment/valid-other-place `path`, and stale replacement/move/Drop/join generation or contained-root set; forced terminating-index guard/descriptor rejection; intervening invalidation and missing action revalidation; bounds-failure no-later-argument/no-call owner; terminating-later-argument no-pointer/no-call owner; block-compaction and statement-fusion owners proving reservation/guard identity survives all post-lowering rewrites; print/fingerprint goldens and validator rejection. |
| LLVM and layout | Address tags, headers, elements, and record fields in place with canonical type tables after MIR's dominating bounds guard; LLVM performs pure pointer lowering with no second bounds semantic decision, owner aggregate load, element copy, source null, cleanup, allocation, or runtime call. | raw MIR CFG/dominance and LLVM assertions for scalar/string/Move-record arrays, nested field base, alignment/stride, and malformed lowering errors rather than panic. |
| Generic, interface, and cache | Concrete substitution reruns both predicates; whole/per-unit results agree; reachable element-graph edits invalidate exact dependents while unrelated edits remain hits. | generic and imported driver owners; interface hash and isolated cold/hit/edit/revert cache owner. |
| Real consumer | The exact `PromptEvaluationResult`/`PromptEvaluationEvidence` graph matches array-bearing options, passes Move task/row/trace/aggregate/reason/expected-input elements to borrowed helpers, and leaves both decoded roots usable. | align-llm `c6-borrowed-array-adoption`, complete C6c2 owner matrix, whole/per-unit check, and final `make ci`. |

One parameterized owner may close multiple rows when it fails for every listed defect. No benchmark
is required: this capability makes no performance/resource promise. Pointer/no-copy and allocation
absence are correctness assertions.

### Closure matrix reopened: post-lowering MIR identity

The implementation review found that the first candidate stored the reservation block/statement
ordinals and bounds-success block ordinal in `BorrowedElementGuard`. Those coordinates were valid
when lowering created the descriptor but became stale when `simplify_known_drop_flags` compacted
blocks or `fuse_builder_writes` deleted earlier statements. The missed invariant is that a safety
proof consumed after MIR rewrites must use identities preserved by every rewrite, not positions in
the pre-rewrite container.

The redesigned boundary emits one inert reservation statement with a function-local monotonic
token before index evaluation. The descriptor stores that token and the checked length value only.
Validation after all rewrites requires exactly one matching marker for the same root, derives the
unique canonical bounds branch from the descriptor's index and length values, and then performs
dominance and root-preservation checks using the marker's final block/statement location and the
derived final success block. MIR rewrites preserve the marker as a non-movable statement and need
no descriptor-specific coordinate remapping. Dedicated owners force both block deletion/compaction
and statement fusion before validating the resulting indexed-borrow call.

### Closure matrix reopened: validator dominance and complete action boundary

The redesigned-candidate review found four cells that structural type equality alone did not
close. Checked HIR could name a valid sum-payload path outside the arm that activated that payload;
MIR validation did not prove that the reservation marker preceded its SSA index/length evidence
within a shared block; root preservation stopped immediately before the call and therefore missed
an overlapping `borrow mut` argument in that same action; and recursive dynamic-record admission
inherited the pre-extension classifier's `Soa` leaf even though this plan excludes every
specialized collection.

The validator boundary is reopened on one complete-dominance invariant:

- every sum-payload segment in a borrowed element base must have an exact active
  `BorrowedProjection` prefix in the currently traversed match arm, and that fact expires when the
  arm body exits;
- the stable reservation marker must dominate the final index and checked-length definitions and
  precede either definition when they share its block;
- root-preservation inspection includes the call statement itself, so any same-root `borrow mut`
  argument conflicts with the interior shared borrow before pointer formation; and
- the recursive payload classifier treats `Soa` as an excluded leaf at every depth, with a nested
  dynamic-record witness beside the existing top-level specialized-array exclusions.

Owner mutations move a valid projected indexed call outside its activating arm, move the marker
after its length evidence, alias a valid disjoint `borrow mut` call argument to the indexed root,
and place a `Soa` field inside an otherwise admitted AoS dynamic record. Each must fail at its
own checked-HIR, MIR, or classifier boundary while the corresponding valid twin remains accepted.

### Closure matrix reopened: activation root identity

The next redesigned-candidate review found that active-arm validation compared the exact payload
path but not the root that owns that path. Two borrowed sums of the same type therefore had
identical path shapes, and an active arm for one root could incorrectly authorize forged indexed
metadata for the other root.

Payload activation is the pair `(root_local, exact projection-path prefix)`. Every sum segment in a
borrowed element path must match both components from one currently active arm; neither component
can be borrowed from another arm entry. The owner uses two same-shaped borrowed sum parameters,
keeps the first arm active, rewrites the indexed metadata and canonical owner facts to the second
parameter, and requires checked-HIR rejection while the unmodified first-root twin remains valid.

### Closure matrix reopened: call invalidation and dynamic element grammar

The following redesigned-candidate review found two remaining enumeration gaps. The complete call
action classified a same-root `borrow mut` peer as invalidating but not a by-value Move argument
loaded from the same root, and the recursive payload classifier reused itself for `DynArray`'s
element even though the language representation excludes nested dynamic arrays.

Call-action validation uses one mode-complete invalidation classifier. A `borrow` peer is read-only;
a same-root `borrow mut` place invalidates the reservation; and a by-value argument invalidates it
when its `Arg`/`Load`/`Use` provenance reaches the reserved root. Direct, imported, indirect, and
cleanup-returning calls all use that classifier. The malformed owner redirects a valid disjoint
by-value peer's load to the reserved root and requires rejection at the same pre-pointer boundary
as the existing mutable-peer owner.

`DynArray` is a grammar boundary rather than another recursive edge. Its element is one of the
ordinary primitive, `string`, or `str` scalar representations; nested dynamic arrays and every
other aggregate/specialized scalar tag fail closed. AoS `DynStructArray` remains the sole recursive
dynamic-record edge, and its record fields continue through the closed cycle-safe classifier. A
direct nested-`DynArray` classifier witness sits beside the existing specialized-array exclusions.

### Closure matrix reopened: action-bounded function validation index

The next redesigned-candidate review found three related failures in the post-rewrite MIR proof.
Root-preservation reachability crossed the current call action and followed a loop back-edge into a
later iteration; a forged `RawCall` could carry a borrowed-element descriptor even though raw calls
are not an admitted target; and each descriptor independently rescanned definitions and recomputed
dominators, making validation scale roughly cubically on functions with many indexed borrows.

LLVM preflight now builds one validation index per MIR function containing a reservation marker.
The index records CFG adjacency, entry reachability, dominators, unique SSA-definition positions,
and reservation positions once. Each descriptor performs only indexed lookups plus an
action-bounded path query: forward traversal stops at the current action before intersecting the
blocks that can reach that action, so no later loop iteration enters the preservation interval.
Functions without reservation markers take a constant-shape early return and do not compute
dominators. Raw calls reject every borrowed-element operand during callable preflight, independently
of forged signature modes; the shared call-invalidation classifier also handles raw calls
defensively for malformed MIR that reaches preservation analysis.

The owner matrix adds a loop whose source mutation occurs only after the valid call action and a
forged typed raw call carrying the otherwise valid descriptor. The first must lower successfully;
the second must fail in callable preflight before pointer construction. Existing multi-argument and
multi-call owners exercise repeated descriptors through the one function-scoped index. This closes
the missed action boundary, target-kind exhaustiveness, and validation-cost axes without adding a
performance promise or benchmark.

### Closure matrix reopened: root-derived ownership termination

The subsequent full review found that slot-targeted invalidation and by-value call consumption did
not cover `DropValue` applied to an SSA value loaded from the reserved array root. It also found
that the `Arg`/`Load`/`Use` provenance walk still found each definition by rescanning all blocks,
undermining the function-scoped validation index.

One indexed provenance query now follows `Arg`, `Load`, and `Use` through unique definitions from
the function index. It is shared by by-value call invalidation and every statement-level ownership
terminator whose operand can name derived storage: `DropValue`, arena/raw/task-group termination,
and generated column-batch finish/drop. A cycle, duplicate definition, absent definition, or other
rvalue fails to prove derivation; the surrounding callable/type preflight remains responsible for
rejecting malformed operand kinds. The dedicated malformed-MIR owner loads the reserved array root,
drops that SSA value before the call, and requires rejection before pointer formation. Existing
by-value direct-call and `Use`-chain owners exercise the same indexed provenance authority.

### Closure matrix reopened: use-site root validity

The next full review showed that type-table membership was being mistaken for a live HIR place and
that MIR derivation omitted direct borrowed-place operands. A forged descriptor could therefore
name a same-typed local declared later, an exited sibling-scope local, or a projection-only binding
with no independent storage; an operand-based terminator could also free the root through a direct
`BorrowedPlace` while the reservation remained apparently valid.

Checked HIR now performs one source-ordered, scope-aware root-use pass over the existing stack-safe
body event topology. Parameters start live; a `let` or tuple binding becomes live only after its
initializer completes; block and match-arm scopes remove their locals on exit; and borrowed
projection bindings are never accepted as storage roots. Every `BorrowedIndex` must name one of
those live roots at its exact expression-enter event before path typing, active-payload proof, and
owner-fact equality are considered. MIR's shared indexed derivation authority recognizes `Arg`,
`Load`, `Use`, direct `BorrowedPlace`, and nested `BorrowedElementPlace` roots, so all call and
ownership-termination consumers see the same place closure.

Mutation owners retarget a valid descriptor to a later local, an exited nested-block local, and an
active projection-only binding; each must fail checked HIR before MIR. A separate malformed-MIR
owner drops a direct borrowed place during the reservation and must fail before pointer formation.
Together these owners close formation order, lexical lifetime, projection storage, and direct-place
derivation as one use-site validity axis.

### Closure matrix reopened: borrowed projection view retype

The Request 21 real-client owner exposed one final cross-stage type-identity gap. Sema correctly
formed `ArrayToSlice` when a dynamic array field below a borrowed sum projection was passed to a
`slice<T>` parameter. MIR retained the borrowed field path and changed only its declared result
type to the layout-identical slice view. LLVM's malformed-place guard nevertheless required that
view type to equal the owning array type reached by the field path, so `check` succeeded and
`build` failed before code generation.

The field path and the view type have distinct exact roles. Path replay must first recover the
owning source type. A retyped borrowed place is then valid only for the existing layout-identical
`string` to `str`/byte-slice conversions, `DynArray<S>` to `slice<S>`, or AoS
`DynStructArray<R>` to `slice<R>`, with canonical structural element identity. No other array
class, layout, or element change is admitted. Pointer lowering continues to address the original
owner in place and loads only the two-word view; it creates no owner, allocation, clone, source
nulling, or cleanup.

The focused owner covers scalar and AoS record arrays under `Some`, the `None` arm, sibling fields
before and after the match, repeated use and calls, whole/per-unit execution, and source-owner
survival. A codegen mutation redirects a valid `slice<str>` descriptor to an `array<i64>` sibling
while retaining the forged view type and requires rejection before LLVM GEP/load construction.
The existing checked-HIR and MIR representation, public contract, interface, cache, and runtime ABI
are unchanged; no benchmark or target-local qualification is required.

## 6. PR boundaries and gates

The design lands first. Its independent adversarial review must resolve the public grammar,
indexed-place lifetime, validation order, checked representation, and capability boundary before
implementation begins. The implementation is one capability PR because payload admission without
element inspection leaves the C6 graph unusable, while indexed Move-element borrowing without the
array-bearing payload grammar cannot reach that consumer.

Before implementation review, the author performs one matrix-to-diff pass. Every applicable row
above points to production code and a regression owner or is explicitly shown N/A. Rust changes
under `crates/` use the repository's compiler self-review, one fresh full-diff review, the narrow
owner target, and the exact final-SHA preflight. The real-client adoption runs only after the Align
implementation merge is pinned.

## 7. Non-goals

- No new syntax, reference type, lifetime annotation, trait, macro, implicit clone, or implicit
  borrow.
- No by-value Move-element indexing, partial element move, element replacement, element
  `borrow mut`, disjoint-index proof, mutable iterator, or Move-element pipeline callback.
- No fixed-array, SoA, array-of-slice, response-array, vector/mask/fixed-aggregate-array, resource,
  box, builder, buffer, or opaque-handle projection.
- No nested borrowed `match` over an outer arm binding; pass a complete indexed element to an
  explicit borrowed helper when its own options need inspection.
- No runtime helper, allocation, JSON exception, application sentinel, alternate verifier
  signature, interface-format field, or native ABI change.

## 8. Author-side consistency pass

The author-side ledger-to-prose pass is complete:

- `draft.md`, `docs/language-spec.md`, `docs/design-notes.md`, `docs/open-questions.md`, plans 26
  and 28, checked-HIR and MIR documentation, and the L2 prerequisite table use the same two ordinary
  dynamic-array classes, Copy-view projection roots, shared-only indexed element rule, complete
  index/later-argument termination lifecycle, lifetime roots, and closed exclusions;
- every public input, result, error/precedence rule, ownership/lifetime/allocation rule,
  compiler/interface/cache owner, prerequisite, and acceptance owner appears in the ledger and
  closure matrix;
- every static multi-invalid form fails in type/call/base/index order, while runtime index and
  bounds evaluation remain at the argument position, root revalidation follows every later
  fallthrough argument, and pointer formation occurs only at the call action;
- the three example call forms are declarations-independent positional calls and will be
  syntax-checked by the implementation owner;
- no native/wire text, persisted format, runtime inspection, global-state, benchmark, later mutable
  element, or new array representation promise is implied;
- Request 16 remains the shipped base contract; this extension changes its payload grammar only
  through the named plan-28 capability and does not rewrite its historical acceptance; and
- the prerequisite consumes no later evaluator/provider capability. The align-llm verifier remains
  paused until the merged implementation is pinned and its exact real-record adoption owner passes.

## 9. Design-review finding-to-fix ledger

| Finding | Ledger decision and closure |
| --- | --- |
| P1: a helper can retain a view from an indexed element into a `borrow mut` destination, but the first design covered only returned views | Apply the existing `BorrowMutRetentionSummary` to indexed arguments. Exact same-program direct rows and conservative imported/indirect fallback both substitute the array generation and contained region roots, and the destination lifetime is checked against all of them. The public ledger, semantic contract, closure matrix, design/specification summaries, and owner evidence now include return and mutable-destination retention. |
| P1: the index expression can invalidate its own array after base capture and before bounds or pointer formation | Reserve the complete source root before index evaluation and reject any same/overlapping-root move, Drop, replacement, or `borrow mut`; uncertain imported/indirect invalidation is overlapping, while unrelated mutation remains valid. No pointer/header fact crosses that evaluation. The validation order, lifetime contract, closure matrix, specifications, and L2 prerequisite row now own this axis. |
| P2: indirect indexed-borrow calls were absent | Admit direct, imported, and function-value targets under the same selected `borrow` parameter mode, including local, function-field, and joined targets. Target-mode validation and parity owners are explicit in the ledger, semantic contract, representation, matrix, and summary documents. |
| P2: assigning bounds semantics to LLVM contradicted the locked backend-agnostic MIR rule | MIR emits the existing bounds-failure CFG and only its successful block creates the guarded place descriptor; checked validation proves the guard dominates action-time use. LLVM forms the pointer during pure call lowering only. The ledger, MIR contract, matrix, and implementation documentation now agree. |
| P1: a later eager argument could invalidate the array after the indexed argument's bounds guard but before the call | Extend the existing eager-operand snapshot from the indexed argument through every later argument to the action boundary. Bounds stay at the original argument position, but no pointer/header fact crosses later evaluation; every fallthrough path revalidates the root before call-time pointer formation, and a terminating later argument forms no pointer or call. The public ledger, HIR/MIR contract, closure matrix, specifications, and owner evidence now cover earlier/middle/final invalidation and terminating twins. |
| P1: a terminating index expression could still be followed by bounds/descriptor lowering | Make index fallthrough an explicit formation gate. `?`, return, break, process exit/abort, and divergent index owners require no bounds guard, descriptor, later argument, pointer, or call; malformed checked HIR/MIR cannot manufacture the action. The public ledger, semantic/lowering contract, and control matrix now cover the complete formation lifecycle. |
| P1: ordinary Copy-view indexing from an array-bearing borrowed projection lacked region owners | Add a dedicated public ledger and closure row for direct/field/projected `array<str>` and AoS Copy-record-with-view indexing. Existing `Index`/`ElemField` facts must inherit the projection generation and contained input/arena roots through return, exact/fallback mutable retention, control, and escape checks. The specifications, HIR contract, prerequisite row, and real owner matrix now include both region-tracked sibling shapes. |
| P1: slice-bearing Copy-record projections had no exact acceptance owner | Define Copy projection provenance over the complete array/record-admitted Copy and canonical borrow/region-classifier intersection, not a `str` special case. Add direct and nested `slice<T>` witnesses beside `str` plus a parameterized type-class sweep that fails whenever any admitted region-bearing Copy leaf lacks `Index`/`ElemField` provenance and escape owners. |
| P2: `BorrowedElementBase.owner_fact` rejection had no field-complete mutation owner | Make the validation evidence exhaustive over vector shape, canonical ordering, every record field, typed path structure, and stale flow state. The representation contract and closure matrix now name empty/removal/extra/duplicate/order, all `kind`/`ordinal`/`path` mutations, and replacement/move/Drop/join staleness for multi-root facts. |
| P1 implementation review: borrowed-element guards stored block and statement ordinals that post-lowering MIR rewrites did not maintain | Reopen the closure matrix on post-lowering identity. Replace every stored coordinate with one inert function-local reservation token, derive the unique canonical bounds-success edge from stable value identities after rewrites, and locate the marker's final position before checking dominance and root preservation. Block-compaction and statement-fusion owners close both rewrite classes without transform-specific descriptor remapping. |
| P1 redesigned-candidate review: a typed sum-payload path could be used outside the match arm that activated it | Track exact active `BorrowedProjection` path prefixes while traversing each arm and expire them at the arm body's exit. A forged projected indexed call moved outside its arm must fail checked-HIR validation. |
| P1 redesigned-candidate review: block dominance did not prove that the reservation marker preceded same-block bounds evidence | Locate the final index and length SSA definitions after rewrites, require marker-block dominance, and require marker-before-definition ordering when either definition shares the marker block. A marker moved after the length owner fails before pointer lowering. |
| P1 redesigned-candidate review: preservation scanning excluded the call action and missed an overlapping `borrow mut` peer argument | Include the current call statement in the preservation interval. A disjoint peer remains valid, while mutating its borrowed-place slot to the indexed root fails before either pointer can be used. |
| P2 redesigned-candidate review: recursive AoS admission inherited the explicitly excluded `Soa` leaf | Remove `Soa` from the closed recursive payload grammar and add a nested-field classifier owner so no ordinary dynamic record can smuggle in a specialized collection. |
| P1 activation-identity review: an active payload path on one borrowed sum authorized the same path shape on another root | Reopen activation as the exact `(root_local, projection path)` pair. A two-parameter mutation owner redirects valid indexed metadata and its canonical owner facts to the inactive same-shaped root and requires checked-HIR rejection. |
| P1 call-invalidation review: a by-value Move peer could consume the indexed root at the same call action | Replace the mutable-only call check with a mode-complete invalidation classifier. Trace by-value `Arg`/`Load`/`Use` provenance to the reserved root and add a malformed direct-call owner that redirects a valid disjoint peer load. |
| P2 dynamic-element review: recursive `DynArray` admission accepted the explicitly excluded `array<array<T>>` shape | Make `DynArray` a non-recursive grammar boundary over ordinary primitive, `string`, and `str` elements. Add a nested dynamic-array classifier witness beside the closed specialized-array set. |
| P2 action-boundary review: root-preservation scanning followed a loop back-edge beyond the current call action | Bound forward reachability at the current action before intersecting paths that can reach it. Add a valid loop owner whose same-root mutation is reachable only after the action and on the next iteration. |
| P1 raw-call review: forged MIR could pass a borrowed-element descriptor through `RawCall`, outside the admitted target set | Reject every borrowed-element operand in raw-call callable preflight regardless of forged signature mode, and retain defensive raw-call invalidation classification. Add a typed forged-raw-call owner that fails before pointer construction. |
| P2 validation-cost review: each descriptor repeatedly scanned all definitions and recomputed CFG dominance | Build one marker-gated validation index per function with adjacency, reachability, dominators, SSA definitions, and reservations. Descriptor checks use indexed lookups and one action-bounded path traversal; existing repeated-call owners cover reuse. |
| P1 ownership-termination review: `DropValue` could free a root-derived array SSA value during a live reservation | Route every operand-based ownership terminator through the same root-derivation query used by by-value calls. Add a malformed-MIR owner that loads and drops the reserved root before the action and must fail before pointer construction. |
| P2 provenance-cost review: `Arg`/`Load`/`Use` tracing rescanned every block for each value | Resolve every value through the function-scoped unique-definition index and retain cycle detection. The by-value and value-drop owners share this indexed authority. |
| P1 use-site-root review: checked HIR accepted a same-typed local that was declared later, out of scope, or projection-only | Add one stack-safe source-order/scope pass. Require the exact root to be live at the `BorrowedIndex` event and independently storage-owning before path and owner-fact validation. Add later-local, exited-scope, and projection-binding mutations. |
| P1 direct-place review: operand-based termination could free the root through `BorrowedPlace` | Extend the shared derivation query to direct and nested borrowed places, and add a malformed direct-place `DropValue` owner that fails before pointer construction. |
| P2 final review: index checking dropped the i64 result context needed to infer generic index-producing calls | Route ordinary and borrowed-element indices through one index-specific contextual checker. It supplies i64 to producer inference, skips final value reconciliation only when the checked index cannot fall through, and retains the existing integer rejection on reachable values. One owner covers generic result inference in both index paths; the termination-family owners preserve return, break, exit, abort, and divergence behavior. |
| Request 21 real-client finding: `ArrayToSlice` below a borrowed sum projection checked successfully but LLVM validation compared its slice view type directly with the owning array field-path type | Replay the owning path first, then admit only exact canonical scalar/AoS array-to-slice layout retypes beside the existing string view retypes. Whole/per-unit Some/None execution and a mismatched-element malformed-MIR mutation close the source/view distinction. |
