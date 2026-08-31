# Borrowed string-array indexing

Status: **IMPLEMENTED against the accepted contract.**

This document owns the Align answer to align-llm Request 22. The first blocked consumer is the R7
Qwen2 tokenizer: it materializes `tokenizer.ggml.tokens` and `tokenizer.ggml.merges` as
`array<string>` and must compare their elements repeatedly without moving or cloning them.

The request originally grouped two Move-element shapes. Current Align already supports the record
half without copying an owner: `rows[i].field` is a bounds-checked `ElemField` read, an owned
`string` leaf becomes a `str`, and a complete Move record can be passed directly to an explicit
shared-`borrow` parameter through the checked `BorrowedIndex` call place. The missing
consumer-complete surface is therefore only ordinary indexing of `array<string>`.

The answer is `texts[i] -> str`. It extends the existing non-consuming string-to-`str` rule to the
one remaining array leaf. It does not make `string` Copy, return a whole Move record as a value,
add a reference type, or create another indexing spelling.

## 1. Public-contract ledger

| Surface | Exact input, result, and validation order | Ownership, lifetime, allocation, and cleanup | Compiler and identity owner | Acceptance evidence |
| --- | --- | --- | --- | --- |
| `texts[index]` where `texts: array<string>` | Existing index syntax. The receiver is any expression already admitted as an ordinary dynamic array source; `index` receives the existing `i64` context and is evaluated once after the receiver. Static checking validates the receiver before the index. A reachable non-integer index is the existing type error. Runtime uses the existing signed `0 <= index < len` hard bounds failure. A terminating receiver or index forms no later bounds action, load, or result. The result type is `str`, never `string`. | The array remains the sole owner of every element buffer. The result is one `{ptr,len}` view into the selected element: no allocation, clone, transfer, source null, cleanup bit, independent `Drop`, or element mutation. Its borrow fact and region contain the complete array storage generation and every region root already contained by that array. Moving, dropping, replacing, or mutably borrowing the source invalidates the view under existing rules. An unbound owned receiver uses the existing synthetic temporary owner and is at most frame-lived. | `align_sema` recognizes exactly `DynArray(String)` and emits ordinary `Index` with logical result `Str`; checked HIR recomputes the physical-`String`/logical-`Str` projection and eager lifetime facts. MIR emits the existing receiver/index order, bounds CFG, and `SliceIndex`; LLVM loads the representation-compatible two-word view. No HIR/MIR discriminator, interface field/version, runtime helper/ABI, emitted symbol, or persisted format is added. The changed body participates in ordinary MIR/cache identity. | Direct/local/field/projected/temporary receivers; zero/last/out-of-range index; generic-wrapper monomorphization and generic owned-result rejection; all terminating receiver/index forms; repeated reads and `.clone()`; return and `borrow mut` retention roots; source invalidation; whole/per-unit/cache parity; exact R7 tokenizer owner after pin adoption. |
| `rows[index].field` for an AoS `array<MoveRecord>` | Existing `ElemField` surface remains unchanged. It reads only the selected field; an owned `string` leaf is a `str`, Copy leaves remain Copy, and other Move leaves remain rejected. | Existing array ownership, view provenance, bounds, no-copy, and cleanup rules remain unchanged. | Existing `ElemField` HIR/MIR/LLVM path. | A regression beside the new string-array cases proves that the already-shipped record half still satisfies Request 22 without widening whole-record values. |
| `inspect(rows[index])` for `fn inspect(borrow row: MoveRecord)` | Existing plan-28 `BorrowedIndex` call place remains unchanged. A whole Move element is addressable only for the dynamic extent of an explicit shared-borrow call. | Existing root reservation, later-argument revalidation, call-time pointer, returned-view substitution, and no-copy/no-cleanup rules remain unchanged. | Existing `BorrowedIndex` HIR/MIR validation and borrowed parameter ABI. | Existing `borrowed_params` owner remains green. |

There is no native/wire text boundary, encoding change, artifact/cache format, runtime inspection
surface, process-global state, connection-global state, environment input, or benchmark promise.
Those ledger dimensions are N/A. Source remains UTF-8 under the existing language rule; indexing
selects an already validated owned string and performs no new text validation.

## 2. Semantic boundary

### 2.1 Exact admitted shape

Only `Ty::DynArray(Scalar::String)` gains the logical `str` result. Copy scalar/view arrays keep
their existing result types. `array<MoveRecord>` keeps its existing direct-field and explicit
shared-borrow-call forms. Every other recursively Move element, including an entire Move record,
Move sum, resource, nested dynamic array, builder, buffer, box, or opaque handle, remains rejected
in an ordinary value position.

This is not an implicit borrow of every Move value. `string` already has one canonical borrowed
form, `str`, with an identical runtime layout and existing `.len()`, comparison, byte/view,
borrowed-argument, and explicit `.clone()` consumers. A Move record has no corresponding public
view type. Making `row := rows[i]` appear to bind an ordinary record would require hidden
projection storage and a new local lifetime/cleanup state with no blocked consumer. That larger
surface remains deferred.

### 2.2 Evaluation and failure order

Receiver evaluation precedes index evaluation. The existing retained-storage action snapshot
covers the interval from receiver completion through the bounds/load action. If the index
expression moves, drops, replaces, transfers, or mutably borrows the same or a conservatively
overlapping root, the existing invalidation diagnostic rejects the program. Unrelated mutation is
valid. An index that returns, breaks, propagates with `?`, exits, aborts, or diverges produces no
bounds branch or view. A runtime out-of-range index takes the existing hard error and produces no
view or ownership action.

### 2.3 Lifetime and use

The `str` result follows the same storage-root and region rules as an owned `string` field read and
an `array<str>` element read. It may be bound to a `str` local, compared, printed, passed to a
`str`/shared-borrow consumer, returned when the existing return summary permits the source roots,
or explicitly cloned into a new `string`. It may not outlive the source array generation or any
input/arena root reachable through it. The array can be read again and drops every original string
exactly once.

For a temporary owned receiver, existing hidden-owner lowering retains the array for the derived
view and existing escape rules cap the result at `Frame`. No tokenizer path depends on this case,
but treating it like every other borrowed view avoids a second stability-only indexing rule.

## 3. Checked representation and lowering

The producer keeps `ExprKind::Index { recv, index }`. For `recv.ty == DynArray(String)`, the physical
element is `String` while `expression.ty == Str`. That one representation-compatible projection is
the only admitted mismatch. Checked HIR independently requires:

- an exact `DynArray(String)` receiver and exact `Str` result;
- an `i64` index, unless the checked index is non-fallthrough;
- receiver-before-index child order and the existing retained-storage action boundary;
- no result, storage fact, or bounds action after receiver/index termination; and
- the exact source generation and contained roots in return, retention, and escape facts.

Every other `Index` still requires its current exact element/result type relation. The existing
`DynArray(Str) -> Str` relation and the receiver-only `DynResponseArray -> HttpResponse` method
place remain admitted under their current rules. A forged `DynArray(String) -> String`,
`DynArray(Str) -> String`, or any other unsupported source/result pair fails before MIR.

MIR lowers the accepted node through the existing dynamic `SliceIndex` path. The destination value
is typed `Str`; the source buffer element is layout-compatible `String`. Both are the same two-word
`{ptr,len}` representation, but the result carries no cleanup. MIR bounds behavior and source
borrow-owner inheritance are unchanged. LLVM recovers the physical `String` element type for the
GEP/load, whose two-word value is representation-compatible with the logical `Str` destination,
and emits no element Drop, array nulling, allocation, or runtime call. A focused IR assertion
proves one `SliceIndex` and no `StrClone`, source null, or element cleanup.

MIR validation and the codegen preflight independently recover the `SliceIndex` source collection's
physical element type before LLVM pointer construction. They admit existing readable exact
source/result pairs, the existing response method-place pair, and only
`DynArray(String) -> Str` as a physical/logical mismatch. A forged `DynArray(i64) -> Str`, an
exact-but-Move nested-array load, another mismatch, or an untyped source rejects before GEP/load
construction.

Generic bodies that declare a concrete `array<string>` receiver retain the projection after
monomorphization. A generic `array<T> -> T` index instantiated with `T=string` is rejected: its
declared owned `string` result cannot disguise the logical borrowed `str` result. Non-generic
interfaces carry only the unchanged signatures and reachable structural types. Whole-program and
per-unit compilation must accept the same source and produce the same output. No interface
version or ABI fingerprint changes; ordinary body/MIR fingerprints change when the expression
changes.

## 4. Implementation closure matrix

| Axis | Required closure | Exact owner evidence |
| --- | --- | --- |
| Type formation and result | Admit exactly dynamic `array<string>` as physical `String` to logical `Str`; preserve all Copy results, the existing receiver-only response method place, and every other Move rejection. | Wildcard-free type matrix covering String, Str, primitive, Move record/sum, nested array, response, resource, box, builder, buffer, and opaque twins; positive `DynArray(String) -> Str`, `DynArray(Str) -> Str`, and receiver-only `DynResponseArray -> HttpResponse` records plus forged receiver/result mutations. |
| Construction and source ownership | Build arrays through `array_builder<string>`, index zero/last repeatedly, retain the array, and deep-drop every original element once. | `borrowed_params` executable owner plus existing `m12_array_builder` deep-drop owner. |
| Receiver/index control | Preserve receiver then index order, one evaluation, existing bounds failure, and no action after every terminating receiver/index family. | Side-effect order case; negative/len index subprocesses; `?`, return, break, exit, abort, and divergence MIR/runtime twins. |
| Borrow-transparent receiver lowering | A receiver delivered by a plain block, `unsafe`, anonymous arena, named arena, or `task_group` is excluded from eager nonborrow lowering and is evaluated once through `lower_borrowed_owned`; statements and arena/task framing occur exactly once. | Parameterize `owned_temporaries::borrowed_scope_temporaries_lower_their_scope_exactly_once` over `Index` beside the existing string-borrow consumer and retain all five structural cells. |
| Move-in, move-out, nulling, replacement, Drop | Indexing performs none; explicit `.clone()` is the only owned result. Source move/drop/replacement/`borrow mut` invalidates a retained view, while unrelated mutation remains valid. | MoveCheck invalidation matrix, repeated source use, clone/drop counter or repeated-cycle control, unrelated-root positive. |
| Borrow roots and escape | Direct, field, borrowed-projection, and temporary receivers carry complete storage generations and contained input/arena roots through local binding, return, `if`, `match`, `else`, Result `?`, `map_err`, loop joins, and exact/fallback `borrow mut` retention. | `borrowed_params::owned_string_array_index_views_preserve_control_provenance` covers every named wrapper for direct/field/projection/temporary inputs, return, exact named and imported/indirect fallback retention, and frame/input/arena/source-invalidation negatives. |
| Checked HIR | Recompute the one physical/logical mismatch, child order, termination, retained-storage snapshot, and provenance. No new variant or metadata can bypass exhaustive validation. | Producer-valid HIR plus wrong receiver/result/index, forced fallthrough/action after termination, and stale/missing/extra source-fact mutations; existing variant-sweep tripwire unchanged. |
| MIR and LLVM | Reuse bounds-checked `SliceIndex`; validation recovers the physical source element and admits readable exact pairs, the response method-place pair, and only `DynArray(String) -> Str` as a mismatch; load only the two-word physical element as logical `Str`; inherit borrow owners; emit no clone, owner copy, cleanup, null, allocation, or runtime helper. | MIR print/shape assertion, LLVM type/one-load assertion, malformed checked-HIR rejection, and forged `DynArray(i64) -> Str`, `DynArray(String) -> String`, and nested-Move MIR mutations rejected before LLVM GEP/load construction. |
| Generic, interface, cache | A generic wrapper with a concrete `array<string>` receiver preserves the view; generic `array<T> -> T` rejects `T=string` rather than disguising `str` as owned `string`; whole/per-unit behavior agrees; exact body/type edits invalidate dependents and revert restores hits. | Generic/imported return and retention owner, owned-result negative, whole/per-unit executable parity, focused cache edit/revert owner. |
| Existing Move-record surfaces | `rows[i].string_field` and explicit `borrow` call remain the only non-consuming record-element reads; `row := rows[i]` and by-value calls still reject. | Existing `ElemField` and `BorrowedIndex` owners plus explicit whole-record ordinary-value negative. |
| Existing tensor-render migration | `src/gguf.align::render_tensors` replaces its NUL-separated prefix stream and parallel offset arrays with an indexed `array<TensorRow>` or equivalent, using the already-shipped Move-record field view. | The first registered Request 22 target and `make gguf-smoke` after pin adoption. |
| Existing producer migrations | `GgufTable`, `BlockPlan`, and `model_forward.StepColumns` migrate their stream-plus-column internals together to indexable record arrays without changing their accessor signatures. | The second registered Request 22 target and its R1-QWEN-MODEL-IR, R1B-GPTOSS-MOE-IR, and R6-STEP-N focused owners after pin adoption. |
| R7 tokenizer | R7 reads GGUF token and merge `array<string>` values through ordinary indexes without a copied vocabulary or compatibility layout. | The third registered Request 22 target: the hosted synthetic owner plus named real-Qwen2 tokenizer parity qualification after the merged pin. |

One parameterized owner may close multiple cells when it would fail for each listed defect. No
benchmark is required because this capability makes no performance or resource claim. The
no-allocation/no-copy statements are ownership correctness requirements.

## 5. Capability and PR boundary

The design is reviewed before Rust implementation because it changes a public ownership surface.
The implementation is one consumer-complete compiler PR: sema admission without checked-HIR,
provenance, MIR/LLVM, and owner closure would expose an unsafe view; splitting any of those dormant
layers would leave no usable intermediate consumer. The expected hand-written diff is below the
repository 1,000-line explanation threshold.

Before implementation review, the author maps every applicable matrix row to the final diff and a
passing owner. Rust changes run the compiler self-review, one fresh full-diff review, the focused
`borrowed_params` owner, and the exact final-SHA preflight. The external align-llm pin/adoption runs
only after the Align implementation merges.

## 6. Non-goals

- No general reference type, lifetime syntax, `borrow` local declaration, `.at()` alias, implicit
  clone, or new collection type.
- No ordinary whole-value read, local binding, match, return, capture, or storage of a Move record
  element.
- No partial Move-element transfer, replacement, `borrow mut`, disjoint-index proof, mutable
  iterator, Move-element pipeline callback, or range slice of `array<string>`.
- No widening to fixed arrays, nested arrays, sums, resources, builders, buffers, boxes, response
  arrays, SoA, or opaque handles.
- No runtime, allocator, ABI, interface-format, wire-format, persisted-format, or package-specific
  change.

## 7. Author-side consistency result

The 2026-08-31 author pass verified:

1. This ledger, `draft.md`, `docs/language-spec.md`, `docs/design-notes.md`, the Settled decision,
   plan 28, the English/Japanese array guide, the HIR/MIR guides, and the L2 prerequisite table all
   name exactly the String-to-Str array projection and retain the existing Move-record boundaries.
2. Receiver/index/termination/bounds precedence is identical in every source.
3. Every ownership, region, cleanup, identity, prerequisite, and acceptance field is present or
   explicitly N/A.
4. Every closure-matrix row has an implementation owner and exact regression target before coding,
   including every value-carrying control wrapper, all five borrow-transparent receiver scopes, the
   malformed-MIR physical/result mismatch, and all three registered consumer targets.
5. No example implies a whole Move record value, hidden clone, or new reference/storage state.
6. The design consumes only shipped plan-28 mechanisms and introduces no later evaluator,
   tokenizer, runtime, or package capability.
7. The implementation closure maps the source surface to `align_sema`, checked HIR, existing MIR
   `SliceIndex`, and LLVM physical-element preflight; the focused ownership, control-flow,
   malformed-HIR/MIR, whole/per-unit, cache, and borrow-transparent-scope owners pass.
