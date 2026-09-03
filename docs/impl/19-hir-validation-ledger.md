# Checked-HIR validation ledger

## Status and authority

This is the exact per-record contract owned by L2b-a2-am-r and consumed by
L2b-a2-am-b1 through am-b4. It is normative for handcrafted checked HIR at the
HIR-to-MIR boundary. `crates/align_sema/src/hir.rs` remains the Rust storage
definition; this ledger fixes which stored combinations are producer-valid.

“Producer-valid” means that the stored combination can be emitted by semantic
checking for some source input. Checked HIR does not retain annotation presence,
original token spelling, or the complete source AST, so this boundary does not
claim to reconstruct the particular source that preceded it. For example, an
`i32` literal plus matching local/consumer records is valid because an explicit
`i32` source context can produce it; the validator does not try to decide
whether an unavailable original annotation was present.

Apart from the exact am-e entry ABI, am-f return completeness, am-w
successful-wait, am-v native output buffer, and am-u extern invocation
corrections, the ledger preserves the checked semantic producer. It accepts no
other new source program and narrows no other source program. On the first
invalid record every one of the four MIR lowering entrypoints returns the
canonical `mir::Program` with all vectors empty, before MIR construction,
native declaration, Align-program/runtime/native/artifact/cache allocation,
ownership transfer, or cache publication. Compiler-owned validation worklists
may allocate and are released before return.

That empty-program contract is for **hand-constructed** HIR only. A caller
holding producer-checked HIR uses the fifth, fallible entrypoint
`lower_program_checked`, which rejects at the same point with the same
pre-construction guarantees but reports `LoweringRejected` instead of an empty
program. See "Producer-delegation closure matrix" below for why an empty
program must never reach a checked consumer.

The complete ledger consists of this file and the callable/native appendix in
`17-library-boundary-prerequisites.md`. An implementation capability may not replace a
row with a broader family assumption. A new HIR discriminator must add one row,
one valid producer fixture, and the safety-relevant malformed stored-field
mutations in the same capability wave. Parameterized owners may close multiple
fields and discriminators; the row does not require its own PR or test binary.

## Validation language

The following notation is closed and exact:

- `E(T)` means an expression whose recursively derived type is exactly `T`.
  Every `Expr` first validates the non-child fields named by its row, then its
  children in the row's written order, then the row's relational/result rule,
  and finally requires stored `Expr.ty == T`.
- `L(id)` means `id` is in range, `locals[id].id == id`, and denotes that exact
  local record. `LT(id)` is its type. `M(id)` additionally requires
  `locals[id].is_mut`.
- `LV(id)` means `L(id)` at a source-lexically visible, definitely initialized
  use site. Function parameters (including lifted capture parameters) enter at
  function entry. A `Let`/`LetTuple` initializer is visited before its binding;
  a successful binding becomes visible for the remainder of its block. Match
  payload bindings are visible only in that arm body. Leaving a block or arm
  removes its bindings, and sibling branches do not share them. Every local
  place root and every direct local/projection/aggregate-base read uses `LV`,
  while a declaration record itself uses `L` and the binding-order rule above.
  Every non-discard local name is unique among the parameters and bindings
  visible at its activation point. Same-scope rebinding and inner shadowing
  reject; disjoint sibling blocks and match arms may reuse a name after their
  prior lexical scope exits. `_` remains a non-binding discard spelling. An
  owned tuple discard is stored with the exact compiler-reserved
  `$tuple_drop<ordinal>` name returned by
  `tuple_drop_local_name(ordinal: usize) -> String`; `$` cannot occur in a
  source identifier. At that exact `LetTuple` ordinal and only when the shared
  `tuple_discard_needs_hidden_local(Ty, structs, enums, tagged_types) -> bool`
  producer predicate succeeds, the source-hidden local does not enter the
  visible-name set. Its id still activates after the initializer, remains
  initialized until block exit, and retains its exact Drop membership/cleanup.
  Source-visible `_drop0`, `$tuple_drop00`, a wrong ordinal, and every other
  near spelling do not satisfy the hidden-record predicate.
- `MV(id)` means `M(id)` and `LV(id)` for an assignment or other mutable place
  root. The structural local table alone is not a definite-initialization or
  lexical-scope proof.
- Every nonparameter local table record has exactly one `Let`, `LetTuple`, or
  match-payload binding. An unused orphan record is malformed even when no
  expression reads it.
- `B(T)` means a block whose statements validate in stored order and whose
  optional tail validates last. A present tail has type `T`; an absent tail has
  type `Unit`. A block known non-fallthrough may carry the context-selected
  result type, but its terminating structure must independently prove that no
  fallthrough edge exists.
- `P(root,path)` walks a non-empty field-index vector from `LT(root)`. Every
  index is in range, every intermediate is `Ty::Struct`, and the result is the
  exact leaf type. `EP(T,path)` is the same walk beginning at struct type `T`.
- `SIG(name)` is the single matching stored, imported, or extern callable
  header after am-n/am-h. It never resolves a runtime key by spelling.
- A `Spawn` validates its callable child with `SPAWN(fallible,ok)` context.
  No other parent supplies this context. In that context the callable must
  name the lambda helper created for that exact spawn: zero explicit
  parameters, `FnOrigin::Lifted`, and stored `FnTy([], ok)`. The lifted
  function returns `ok` when `fallible == false` and
  `Result<ok,builtin Error>` when `fallible == true`. Outside this context a
  function value's stored `FnTy` return equals its target header return.
- `FT(id)` is the in-range `FnTy`; `TAG(id)`, `ENUM(id)`, `STRUCT(id)`, and
  `TUPLE(id)` are the corresponding in-range records already accepted by
  am-g-t/am-p/am-n.
- `FABI(A,B)` means `A` and `B` are in-range `Ty::Fn` records with the same
  parameter count, parameter modes, recursively matching scalar parameter and
  return types, and no comparison of am-b4-owned effect/borrow/region cells.
  A body expression and its local expected type may use different compiler-local
  `FnTy` ordinals when `FABI` holds; sema creates such a fresh local cell so its
  inferred effect can be solved independently.
- Validation carries an exact lexical `unsafe_depth`, initialized zero for
  every stored function and incremented only while validating the child block
  of `Unsafe`. A `SIG(name)` whose declaration is extern may be invoked by
  `Call` or selected as the named callable of a non-escaping Stage/terminal
  only when the owning expression has `unsafe_depth > 0`. Every phrase
  “resolves callable”, “resolves exact ... signature”, or “func resolves” in
  am-b2 includes that predicate before signature correlation. `FnValue` never
  admits extern, even inside Unsafe; lexical permission is not data.
- `I`, `F`, and `N` mean a concrete integer, float, and numeric type.
  `IS(T)`, `FS(T)`, and `NS(T)` mean the corresponding concrete integer,
  float, or numeric scalar. `V(S,n)` and `K(S,n)` mean `Ty::Vec` and
  `Ty::Mask` with the already validated scalar and lane count.
- `same(a,b,...)` requires exact `Ty` equality. `result(T)` means the stored
  expression result is exactly `T`; `unit`, `bool`, `i64`, `raw`, `str`, and
  `string` abbreviate the corresponding concrete `Ty`.
- `owned(T)` is the existing recursive Move/Drop predicate. `view(T)` is a
  non-owning result whose return-borrow/region fact is derived from the named
  children or locals in the row. `copy(T)` is neither moved nor dropped.
- `HTR(id)` is the producer-owned, cycle-safe heap-tree-record predicate from
  `heap_array_builder_record`: `STRUCT(id)` is nonempty, naturally aligned at
  most 8 with no explicit layout/alignment, and every reachable field in source
  order is a Copy scalar, owned `string`, another `HTR`, recursively admitted
  `Option<T>`, or `array<E>` where `E` is a Copy scalar, owned `string`, or an
  `HTR`. Arrays of Options/arrays and every view stay excluded. The checked-HIR
  validator calls that same helper; it does not infer admission merely from a
  nonempty `DropPlan`. `HTRMove(id)` means `HTR(id)` and
  `owned(Struct(id))`; a scalar/Option-of-Copy-only `HTR` is Copy.
- `consume(x)` requires the semantic producer's MoveCheck transfer for `x`;
  `borrow(x)` preserves its owner; `mutate(x)` requires the producer's writable
  place rule. These are ownership facts, not alternate type checks.

Every `Span { file, lo, hi }` is validated after its enclosing record's stored
non-expression fields and before that record's children: `lo <= hi`; all three
widths are the stored `u32`. The validator does not read source text and does
not require `file` or `hi` to be present in a `SourceMap`. This keeps all four
entrypoints' acceptance identical. Located lowering preserves the existing
behavior: an unknown file maps to `(0,0)`, and location calculation uses only
`lo`.
`Fn.span`, every `Expr.span`, and every span key in
`drop_individual_exprs` use this same rule.

### Universal record order and ownership correlation

For a struct-like enum variant, `env[...]` lists every non-expression stored
field in validation order. `child[...]` lists every `Expr`, `Block`,
`MatchArm`, `Stage`, or `TemplatePart` child in producer structural/source
order, which is evaluation order while the path remains reachable. Validation
still visits structurally retained dead children after a terminating sibling;
they cannot contribute a control, ownership, action, or result edge. A vector
is visited by ascending stored index. An optional child is visited only when
present. `post[...]` contains all cross-field, type, result, control, and
ownership relations.

This split is deliberate even when a Rust field declaration interleaves
metadata and children. All parent metadata that can be checked without reading
a child is an envelope. The complete order is:

```text
variant tag
env fields in the row's order
child fields in the row's order, each to completion
post relations in the row's order
stored Expr.ty
body-derived ownership records
```

Therefore a parent envelope error beats every child error, child `i` beats
child `i+1`, and every child error beats a relational or stored-result error.
For a nested child the same rule applies recursively. This is the sole
multi-invalid precedence rule.

The active Request 6 `JsonScan` exception classifies its stored scanner type as
part of the pre-lowering envelope, because MIR consumers must not inspect a
row graph through a mismatched `Expr.ty`. It is one explicit exception to the
universal stored-field-before-`Span` rule. Its complete deterministic order is:

```text
variant tag
Expr.span
Expr.ty == Ty::JsonScanner(struct_id)
existing struct_id
input.ty == Ty::Str
Decode schema
canonical recursive Copy predicate
```

`struct_id` is a typed `u32` in ordinary HIR; its semantic existing-row lookup
is the later step shown above, not a separate raw-representation state.
Reclassifying `Expr.ty` and the logical row-id relation after `Span` is
intentional: a malformed span wins over a wrong stored expression type, an
unknown row id, a wrong input type, a schema error, or a Copy error. Once the
span is valid, those five scanner steps are the sole scanner precedence order.
The active gate retains `hir_program_is_valid(&hir::Program) -> bool`, but its
crate-private owner seam is
`validate_hir::json_scan_validation_reason(&hir::Program) ->
Result<(), JsonScanValidationReason>`. The reason variants are `InvalidSpan`,
`StoredType`, `UnknownRow`, `InputType`, `Schema`, and `Copy`, in the order
above; production lowering consumes `.is_ok()`. This seam is test-only
observability, not a new user-facing diagnostic. The exception belongs to
`align_mir::hir_program_is_valid` and
`validate_hir::json_scan_copy_rows_are_valid`; it is not activation of the
general body-fact replay below.

### Checked-HIR depth bound

`MAX_CHECKED_HIR_DEPTH` is fixed at
`2 * (MAX_EXPR_DEPTH + 1) + 1 == 259`. This is the proved conservative producer
ceiling, not a claim that a source program reaches the minimum possible ceiling.
Depth starts at
one for a function's root `Block` and increments for every traversed `Block`,
`Stmt`, `Expr`, `MatchArm`, `Stage`, or `TemplatePart` record on one child edge.
Leaf metadata and type-definition graphs do not count here and retain their
own iterative graph bounds.

The ceiling derivation is deliberately conservative. `cap_expr_depths` starts a
block-body function `Block` or an expression-body function's expression at
depth one, admits compound records through depth 128, and also admits a leaf
expression at depth 129. The constructor audit charges every principal HIR
record, parent-context `StrBorrow`/`ArrayToSlice`, and structural
`Stmt`/`MatchArm`/`Stage`/`TemplatePart` helper against those parser records and
recursion guards, then adds the one synthetic root `Block` used by an
expression-form function. Non-stacking coercions, constant-path `ConstArray`
leaf expansion, `json.encode(local)` leaf accesses, pipeline flattening,
missing-else blocks, and lifted helpers cannot exceed the resulting 259-record
ceiling. The owner enumerates every creation site and fails when this
classification is incomplete. It does not invent a syntactic tightness witness:
the parser's shared recursion counter and template-hole grammar make several
apparent maximum-depth AST shapes unconstructible. The validator nevertheless
accepts structurally valid handcrafted depth 259 so every producer path retains
headroom and the public boundary stays simple.

Am-d uses explicit enter/exit worklists to measure and reject depth 260 before
any recursive checked-HIR consumer. All four direct HIR-to-MIR entrypoints
return canonical-empty on that rejection. Producer-side HIR finalization,
lint/region/borrow/Move/Escape/effect replay, am-b4 validation replay, and MIR
expression lowering use bounded explicit frames rather than native recursion
for these records, so every structurally valid depth-259 input reaches the
normal result on the 2 MiB test thread. Owners cover depths 258, 259, and 260
for a raw expression chain, a coercion-expanded chain, nested block/statement/
match arms, template parts, and every whole/per-unit entrypoint. The same
in-limit fixtures run through final MIR and LLVM verification; an invalid
depth never starts an ownership action, native registration,
Align-program/runtime/native/artifact/cache allocation, or cache publication.
Only compiler-owned validation worklist allocation may occur, and it is
released on success or rejection.

The body-record bound does not cap the global type domain. Am-g-t accepts every
finite header-mediated nominal, tagged, and function-type DAG regardless of
depth. Am-d factors its explicit enter/exit, stable-order, visit-color traversal
into the common checked-HIR type walker. In the same am-d vertical, every
already-active recursive type consumer reachable from HIR input through final
LLVM verification is inventoried. `drop_plan_rec`, `ty_is_move`/
`struct_is_move`, `ty_may_borrow`, slice/region/escape/ownership predicates,
MIR type/layout conversion, and LLVM struct-body/layout construction either
consume that worklist or have an owner-backed proof that the edge is an
indirection leaf or already non-recursive. Am-p uses the same walker for
placement predicates, am-n for complete source-shape comparison, am-h for
signatures and summaries, am-b1–b4 for body type relations, and am-c for
`CanonicalTy`, `CanonicalFnAbi`, and `GeneratedId` encoding and decoding. Every
current and future owning slice must accept an am-g-t-valid deep acyclic inline
chain and deep header-mediated DAG, then reject a deep malformed reference or
later malformed sibling in the ledger's deterministic order without native
recursion. Owners place those graphs at stored header, parameter, local,
return, aggregate field, Drop, borrow/region, MIR layout, and LLVM struct-body
roots. Canonical semantic-to-byte and byte-to-semantic traversal retains
depth-first first-visit order through explicit work items; no slice may add an
ambient type-depth cap.

The sema boundary entrypoint is
`align_sema::checked_hir_body_facts_are_valid(&hir::Program) -> bool`. It first
rejects a body above `MAX_CHECKED_HIR_DEPTH`, clones the already structurally
validated HIR, and performs all replay/reset work on that compiler-owned clone.
The caller's HIR, its type-table length, local `Ty::Fn` ordinals, and all
compiler/runtime/cache state remain unchanged. Reset clears function return
summaries, function parallel-transfer summaries, both Drop-local vectors, the exact Drop-expression map, every
assignment cleanup `Cell<bool>`, and every concrete `FnTy.effect` cell; imported
return provenance, effect, and parallel-transfer seeds come only from the already validated
imported declaration fields. Direct and concrete function-value transfer summaries translate to
caller roots; an unresolved indirect target selects every compatible argument/capture. A diagnostic, fact mismatch, or panic from a
legacy analysis receiving a direct malformed-HIR call returns `false`.
`ImportedFn.return_provenance_known` preserves whether the producer received
an external provenance record: `false` retains the compatibility API's
all-compatible-input fallback, while an explicit `None` is trusted only when
the record was present. `ImportedFn.parallel_transfer_params` is the decoded interface-v6
strictly increasing unique in-range borrow-capable root set; header validation authenticates it
before replay, and a compatibility omission conservatively selects every borrow-capable parameter.
Both imported validation-only facts are stripped before MIR. This predicate is not the structural HIR validator;
direct callers must supply the checked type, id, header, and body envelope,
and direct malformed metadata that does not trigger a replay diagnostic or
legacy panic is outside this predicate's contract.

Replay reconstruction is an occurrence-frame protocol, not an identity map. The shared depth
walk emits monotone numeric `RecordEnter{id}`/`RecordExit{id}` events for every `Block`, `Stmt`,
`Expr`, `MatchArm`, `Stage`, and `TemplatePart` in producer child order. It does not use `Span`
or native addresses, so repeated nodes and duplicate spans remain distinct. `replay_clone` has an
exhaustive reconstruction inventory for every current `ExprKind`, every `Stmt`, every
`MatchArm`, every `StageKind`, and every `TemplatePart`; child order and all non-child metadata are
explicit, including assignment `Cell<bool>` flags and `FnTy` summaries/effect cells. A completed
replay, and any functions completed before a fail-closed clone rejection, are torn down through
the same heterogeneous worklist, including all owned expression, block, statement, arm, stage,
and template edges. Structural rejection occurs before replay ownership at the shared gate; the
teardown is also used after a completed replay whose analysis panics, so those paths do not rely on
recursive HIR Drop.
The owner tests `clone_frames_distinguish_repeated_same_span_nodes`,
`clone_and_drop_are_iterative_for_a_deep_body`, and
`clone_preserves_fn_type_cells_and_assignment_flags` close the occurrence, deep teardown, and
metadata/cell rows; `finish_children_rejects_missing_and_extra_children` closes the malformed
child-cardinality row; `checked_hir_body_fact_replay_covers_cleanup_and_function_effects` closes
the integration replay and fact-equality row.

The shared am-b4 MIR activation gate runs depth, global type, placement,
nominal/link, and declaration-header validation first. It then runs the active
Request 6 `validate_hir::json_scan_validation_reason` envelope/Copy validator,
the structural body validator, and this replay predicate, in that exact order,
before any MIR construction or downstream identity is published. The scanner
walk rechecks only the scanner envelope and canonical row Copy predicate,
including imported/per-unit reconstructed HIR. A scanner or structural body
failure therefore wins over replay, and all four lowering entrypoints return
the canonical empty program.

Am-b4 independently recomputes the existing producer facts rather than
trusting the stored bits:

1. Re-run the checked-HIR MoveCheck/EscapeCheck ownership state machine in
   statement and child order, including branch/loop joins and early exits.
2. Require `drop_locals` to equal the recomputed ascending set of locals whose
   recursively Move type needs a path-local flag, after excluding every checked-HIR
   projection-only `binding_local` recorded by a borrowed `MatchArm`.
3. Require `drop_individual_locals` to equal the recomputed ascending subset
   whose declaration initializer is individually owned, with the same projection-only
   exclusions. A borrowed arm local never owns the active payload or receives a cleanup bit.
4. Rebuild `drop_individual_exprs` with the producer's source-order
   `HashMap<Span,bool>` insertion semantics and require exact key/value
   equality. A projection-only borrowed binding contributes no entry. If two
   expressions share a span, the later producer insertion is
   authoritative; a conflicting handcrafted map cannot choose another value.
   After this equality check, the `align-production-codegen-v1` encoder emits
   the resulting lookup fact at each expression in the same universal child
   order; it never serializes the map or trusts a separately supplied fact.
5. Recompute every `Cell<bool>` statement flag after its children:
   `Assign.drop_old`, `Assign.drop_new`, and the corresponding indexed/field
   replacement behavior must equal the MoveCheck/EscapeCheck decision.
6. A Copy expression has no individual-ownership entry. A recursively Move
   expression has exactly the entry produced by this traversal. Borrowed views
   never gain ownership merely because their source is individually owned.
7. Re-run the existing source-order effect fixed point. Stored functions begin
   from their bodies, extern declarations are `Impure`, and an imported
   declaration supplies its required am-h-validated `effect` field. Every
   direct, indirect, closure, aggregate projection, assignment join, return
   join, open-world exported callback boundary, and structurally retained dead
   child follows the current producer reachability rules.
8. Require every concrete `FnTy.effect` cell reached through a local,
   aggregate field, tuple element, enum/tagged payload, array element,
   expression result, function return, or capture to equal the replay. Recreate
   the same compiler-owned internal boundary cells while solving; they have no
   persisted producer snapshot, so replay diagnostics and complete parallel
   eligibility are the evidence for their equality. An unused annotation-only
   `FnTy` has `Unknown`; unrelated equal source signatures retain distinct
   cells. `Impure` dominates `Unknown`, which dominates `Pure`.
9. Require every `ArrayParMap` and widened parallel-stage callable to resolve
   to complete replayed `Pure`. `Unknown`, `Impure`, an absent target, and an
   incomplete boundary cell reject before any MIR or generated-kernel identity
   is constructed.
10. Re-run the producer's per-`task_group` successful-wait dominance fact.
    A group starts with no completed task generation. Each group has one
    compiler-only preorder `group: usize`, abstract `current_generation` and
    `proof_epoch` tokens, optional `completed_generation`, `valid_from`, and a
    sparse ordered set of unresolved fallible-wait ids. These are indices in
    function-analysis-owned vectors and have no HIR/interface/wire form.
    The function owns one deterministic interner. A Spawn output is keyed by
    `(spawn syntax site, incoming generation, incoming proof epoch,
    incoming valid_from)`, a Wait id by
    `(wait syntax site, incoming generation, incoming proof epoch)`, an Err
    epoch/invalidation frontier pair by its Wait id and coverage, and a
    differing control join's generation/epoch pair by
    `(active group, syntactic join site)`. First construction appends one
    index; every worklist revisit reuses it. Token creation therefore never
    depends on traversal count or ambient state. A forward `Spawn` advances
    `current_generation` and `proof_epoch` to its interned output, so every
    earlier WaitProof becomes stale and completion no longer covers the current
    generation. If any Wait was unresolved, that Spawn also clears the old
    unresolved set and sets `valid_from` to the new generation, invalidating
    every older TaskProof in O(1); otherwise old TaskProofs remain eligible and
    the next successful Wait can reauthorize old and new handles.
    An infallible group has no unresolved set and its `Wait` sets
    `completed_generation = Some(current_generation)`.
    Each fallible `Wait` registers its interned ordered wait id, fresh for a
    previously unseen key, before producing
    `WaitProof { group: usize, proof_epoch: usize, wait: usize,
    covers_through: usize }`, where `covers_through` is the current task
    generation. Evaluating that Result alone does not establish completion and
    does not revoke completion already established for the same current
    generation. Handling its `Ok` edge
    resolves that exact id idempotently; it establishes completion only when
    every earlier Wait registered in that proof epoch is also proved `Ok` and
    no later Spawn changed the epoch. It then sets
    `completed_generation = Some(covers_through)`. Handling its `Err` edge poisons
    every task covered by that Wait: it advances `proof_epoch`, clears the
    unresolved set, sets `completed_generation = None`, and sets `valid_from`
    past `covers_through`, invalidating
    every affected TaskProof and WaitProof in O(1).
    Therefore a second empty Wait cannot reauthorize task slots while an
    earlier drained Wait Result is unresolved or failed. Conversely, once one
    successful Wait established completion, evaluating a later no-task Wait
    without handling its Result does not make initialized slots unreadable.
    The proof propagates through a bare Result local,
    let/copy/reassignment, a block tail, `ResultMapErr`, and value-producing
    `if`/`match`/`else`/loop only when every reachable result predecessor carries
    that same group/proof-epoch/wait/coverage proof. Overwrite with an unrelated
    Result clears it.
    A call result, return value in another function, closure capture, imported
    value, or aggregate reconstruction has no proof; passing a Copy proof as an
    argument does not clear the caller's original local, but the callee and its
    return do not inherit it. Multiple aliases of the same proof resolve one
    wait id idempotently; a stale proof epoch or wait id has no effect.
    Entering a nested group pushes a distinct false ambient group but preserves
    the enclosing groups' ambient facts and local proofs. Inner Spawn/Wait
    operations change only the inner group. Leaving it removes that group's
    ambient fact and every WaitProof/TaskProof entry naming it, including a
    proof attached to the task-group block result, while preserving entries for
    every still-active enclosing group. A lambda/function instead starts with
    an empty active-group stack and no group proof.
    `Spawn` also produces
    `TaskProof { group: usize, born_generation: usize }` for its Task handle.
    Both fields use the same analysis-vector indices and have no stored/wire
    form. `Task` is Move: a bare Task local, move/reassignment, block tail, or
    value-producing control expression transfers it and preserves its proof
    only when every reachable value predecessor names the same group; overwrite
    clears it. There is no Task-copy path. Calls, returns, captures, imports,
    and aggregate reconstruction do not transport Task proof. Leaving the group
    removes its token from the active-group stack.
    `Try` consumes a proven Result expression only when its group and
    proof epoch remains active and continues only after resolving that wait id's
    true edge. It updates that group, not merely the innermost group. `Match` on a Result
    with a still-active proof starts each exact `Ok` arm with that proof's group
    true and each `Err` arm with it false; a
    wildcard/or-pattern receives completion only when every variant it covers
    resolves the same proof to a completed generation. `ElseUnwrap` on a
    Result with a still-active proof resolves that proof on the unwrapped `Ok`
    edge and poisons it on the `Err` fallback; if that fallback terminates,
    only the successful continuation remains.
    `If`, ordinary `Match`/`ElseUnwrap` facts, and all other control joins start
    alternatives from their incoming group state. For each active group
    independently, if every reachable
    predecessor has byte-identical group tokens, completion, unresolved set,
    and local proof records, retain them verbatim. Otherwise use that syntactic
    join site's one stable `join_generation` and `join_proof_epoch`, assign
    `current_generation = join_generation` and
    `proof_epoch = join_proof_epoch`, clear the joined unresolved set and every
    WaitProof, set `valid_from = join_generation`, and set
    `completed_generation = Some(join_generation)` iff every predecessor had
    `completed_generation == Some(predecessor.current_generation)`; otherwise
    set it to None. For each Task-valued local/result independently, retain a joined
    `TaskProof { group, born_generation: join_generation }` iff every reachable
    predecessor carries a TaskProof for that same active group, its
    `born_generation >= predecessor.valid_from`, and either that predecessor
    has `completed_generation == Some(predecessor.current_generation)` or no
    unresolved Wait on it covers the born generation. The predecessor handles may differ: a
    value-producing branch may select either Task. Failure of any predicate
    clears only that Task proof. This exact remap accepts a completed
    Spawn+Wait/no-Spawn asymmetric join, rejects `get()` after an incomplete
    asymmetric join until a later Wait completes the join generation, and
    prevents a drained unresolved Wait from being hidden by the join.
    A loop computes its header before its exit. Start with the entry state,
    analyze the body, and join the entry state with every reachable body
    fallthrough backedge at the loop header's stable syntactic join site.
    Reanalyze until that header state is byte-identical. Stable interned
    transfer/join tokens and the differing-join proof clearing above make this
    a finite fixed point; no revisit allocates another token. After the header
    first adopts its site tokens, generation, epoch, `valid_from`, unresolved
    set, and WaitProof set are fixed. Completion and each candidate TaskProof
    can then only change from retained to cleared, so a header performs at most
    one canonicalization plus one state change per such fact. Then analyze once
    from the stable header, record the ambient fact and every result/local proof
    independently on each reachable accepted `Break`, and join only those
    break states for the exit. A `Return`, propagated Err, process termination,
    accepted `Break`, or diverging nested construct contributes no backedge; a
    terminating alternative contributes no exit join. A body-only state never
    escapes by traversal order. Thus an unresolved or failed Wait on an earlier
    iteration reaches every later iteration and cannot be hidden by a later
    break plus an empty Wait.
    `TaskGet` requires its operand's TaskProof group to remain active and that
    `born_generation >= valid_from` and
    `completed_generation == Some(current_generation)`. It does not consult
    merely the innermost group: an inner Wait can never authorize an outer
    Task. An outer Task may be read inside a nested group only if its born generation
    is still valid and the group's current generation has completed a successful
    Wait with no unresolved or failed Wait preceding that successful Wait able
    to cover the task. An outer Wait Result may establish this fact while
    handled inside the inner group; a later no-task Wait does not revoke it. A
    failed check chooses its fallible/infallible diagnostic
    from `TaskProof.group`, never from the innermost group. Current Spawn
    results are primitive Copy values, so a successful `TaskGet` reads without
    consuming the Move handle, preserves its TaskProof, and may be repeated.
    Handling an inner Wait Result after that inner group has exited carries no
    proof and cannot change an outer group's fact. Owned task results remain a
    separate future producer extension. Am-w first makes the
    semantic producer use this exact outcome-sensitive state machine, so
    am-b4 neither rejects producer-valid handled-success paths nor trusts
    structurally last branch/loop traversal.
11. Correlate each stored function root with its body completion. `Return(None)`
    is valid only when that function's exact return is Unit. If the root body
    has no tail and its end is reachable under the same source-order control
    walk, the return is Unit; a non-Unit root instead has a typed tail or no
    reachable fallthrough. Nested blocks may still have an absent Unit tail,
    and a non-fallthrough nested block remains context-polymorphic. The owners
    `body_contract_function_return_none` and
    `body_contract_function_root_completion` mutate the return type, tail
    presence, and every reachable/non-reachable control predecessor.

Owner ids are namespace-qualified and unique:
`body_contract_stmt_<variant>`, `body_contract_expr_<variant>`,
`body_contract_<record>_<variant>`, or `body_contract_record_<name>`.
Thus the standalone Block record and `ExprKind::Block` cannot alias one test.
Each test starts with the semantic producer's valid record, mutates the tag,
each envelope field, each child, each post relation, stored `Expr.ty`, and each
applicable ownership fact one at a time, then checks the universal precedence
with parent-plus-first-child and first-child-plus-later-child pairs.

### Planned `core.test` records (designed 2026-08-30; inactive until implementation)

The accepted `core.test` capability partitions checked output before test bodies may add generated
artifacts. The exact private top-level shape is
`CheckedProgram { production: hir::Program, production_static_descriptors: Vec<StaticDescriptor>,
tests: Option<TestOverlay> }`. `production` is formed to its complete ordinary-source fixed point
first and is then immutable. `TestOverlay` owns an ordered `catalog: Vec<TestMeta>`, test roots and
their generated function closure in `fns: Vec<hir::Fn>`, appended suffixes for each of
`Program::{structs,enums,resources,tagged_types,tuples,fn_types}`, and
`test_static_descriptors: Vec<StaticDescriptor>`. It owns no replacement production entry and no
second extern/import table. An overlay type id addresses the immutable production table as a prefix
and its corresponding overlay table as a suffix; no overlay pass may insert into, reorder, or
rewrite a production prefix.

The exact catalog record is
`TestMeta { function: u32, source_module: String, source_name: String, canonical_id: String,
source_ordinal: u32 }`. Each overlay function has `test_catalog_index: Option<u32>` rather than a
copied identity. A catalog slot points to one unique in-bounds overlay function whose back-reference
equals that slot. That root requires zero parameters and type parameters, exact
`Result<Unit,builtin Error>` return, private `FnOrigin::Source`, Impure effect, no public/interface
record, and a body whose reachable fallthrough is the producer-inserted `Ok(Unit)` tail. Every
production function and every generated overlay `Lifted`/`Monomorph` has `None`; no generated helper
may impersonate a catalog root.

`source_module` is the producer-retained canonical module path: it is valid UTF-8 and every
component repasses the ordinary identifier grammar. For the entry source it is the declared module
path, or `main` only when that source omits a module declaration; `is_entry` never rewrites a
declared path. `source_name` is the independently retained decoded declaration string, valid UTF-8
and 1..=256 bytes with U+0000..U+001F and U+007F..U+009F rejected. `canonical_id` is valid UTF-8,
1..=1,024 bytes, and byte-equals `source_module + "::" + source_name`; the pair and id are unique.
`source_ordinal` is dense from zero in source declaration order within each `source_module`.
Catalog order is the driver-selected dependency-first unit order followed by `source_ordinal`, and
its length is at most 65,535. The overlay owns these three strings independently; none is rebuilt
from the hidden function symbol.

Before catalog construction, the loader rejects an entry that omits its module declaration when
an imported source explicitly declares `module main`. The exact diagnostic is
`default entry module 'main' conflicts with imported module 'main'; declare the entry module
explicitly`. This check precedes canonical-id uniqueness and source-ordinal validation, so the
implicit entry identity can never alias the imported declared identity. An explicitly declared
entry path uses the ordinary duplicate-module rule. Whole/per-unit owners cover the rejected pair,
nearby distinct paths, and diagnostic precedence over duplicate test ids.

Formation is two-phase. Signature and declaration validation still sees every item. Sema then
checks all ordinary source bodies, closes their lifted helpers, generic function/type/resource
monomorphs, interned tuple/function/tagged types, analyses, capability use, and static descriptors,
and freezes that complete production prefix. Only then does it check test bodies against the
read-only prefix. A test reuses an identical production monomorph already in the prefix; otherwise
the test demand and every transitive lifted/monomorph/type/resource product append only to the
overlay. Test analyses may read validated production facts but publish their own mutable facts in
the overlay. Permitted non-database test static descriptors stay in `test_static_descriptors`; a
database Query/command descriptor cannot be source-formed there because its constructor remains the
complete body of an ordinary named top-level descriptor function, and the combined validator
rejects a handcrafted database consumer in the overlay. `db prepare` therefore sees every
source-formable database descriptor in `production_static_descriptors`; `alignc test` reuses the
same offline policy/driver metadata. Native capability collection runs after selecting the
production view or combined test view, never while checking a test body.

The validator proves the partition rather than trusting storage location alone. The production
program validates independently and has no test back-reference. The combined validator resolves
every overlay reference against prefix-plus-suffix bounds and requires each catalog root to be
reachable from its own catalog slot. The complete overlay artifact graph—functions, every type-table
suffix, and test static descriptors—must equal the closure reachable from the catalog-root set
through every checked-HIR reference or generation edge, including direct calls, function values,
callback/destructor descriptors, lifted targets, nominal field/variant/resource types, interned
type members, and transitive function/type/resource monomorph demands. It rejects any
production-prefix edge to an overlay suffix and any database-consumer overlay descriptor. Owner
Before cache lookup, capability collection, or artifact allocation, the selected combined-view
validator walks that same graph from catalog roots in catalog order and dependency-edge then
structural HIR order. A reachable `ExprKind::ProcessCommand` rejects with exactly
`process.command is not available from test code; run the external process in an owner test`.
Direct/imported calls, function-value targets, lifted callbacks/destructors, and concrete generic
monomorphs are all edges; a shared site is diagnosed once at its first root. An unreachable
production function containing `ProcessCommand` remains valid and may stay as inert code in the
frozen-prefix test object with its ordinary runtime selection; no catalog root can execute it. The
ordinary production validator continues to admit it. A handcrafted
combined view cannot bypass the rule by storing the expression in the production prefix when a
catalog root reaches it.

Owner matrices cover direct/nested/imported/function-value/lifted/generic reachable commands,
shared-site and catalog/source-order precedence, an unreachable production control, malformed
prefix/overlay placement, whole/per-unit parity, and inert-prefix retention. They also cover
a test lambda, a test-only generic
monomorph, the same monomorph demanded by production and test, every test-only nominal/interned type
class, capability library use, and static descriptor. Adding or editing only tests must leave the
canonical span-erased semantic/codegen projection of the production Program, semantic descriptor
projection, span-erased MIR codegen graph, object key, link inputs, and executable bytes identical in
whole-program and per-unit paths. The checked Program itself retains current source spans for
diagnostics and located output; those spans, descriptor diagnostic locations, and their
source-keyed/located metadata may change when an earlier test edit shifts byte offsets and never
enter the production semantic/codegen key.

The production key projection is one exhaustive structural encoder, not filtered `Debug` text. It
starts with the versioned domain `align-production-codegen-v1`, visits production Program tables,
functions, statements, and expressions in stored order, encodes every non-`Span` field through the
existing canonical scalar/sequence encoders, and after each expression encodes the exact tri-state
`absent | arena | individual` result MIR obtains by looking up that expression's current span in the
function's `drop_individual_exprs` as one u8 tag: `00`, `01`, or `02` respectively. The validator
rejects a map key that matches no expression in that function; raw `HashMap` iteration and span
bytes are never encoded. This makes an ownership
fact semantic key material without making its diagnostic coordinate part of identity.

The ordered `production_static_descriptors` projection encodes unit, item, descriptor id,
visibility, consumer, driver, the source tag plus `File.path_literal` or `Inline.decoded_sql`,
params/row types, complete reachable contracts, and static options. It omits only the
constructor/common/native-options spans and the source variant's path/literal span. The source tag is
u8 `00` File or `01` Inline; its payload and all other fields use the existing canonical encoders.
Every HIR or descriptor variant must be matched explicitly, so a new field, variant, or semantic
side table cannot silently disappear. Raw Program `Debug`, compiler source-file paths, diagnostics,
located-MIR records, raw map iteration, and `TestOverlay` are forbidden inputs. The lowering memo
key consumes the HIR projection plus its existing visibility/toggle inputs; production
codegen/artifact identity consumes both projections before its existing mode/target/profile and
resolved-artifact inputs.

The HIR expression discriminator is
`TestAssert { condition, kind, line, column }`, where `kind` is the closed
`True | Equal` byte, `condition` is exact Bool, and line/column are positive `u32` coordinates of
the source assertion call. It is valid only in a `Some(TestMeta)` root, outside every nested lambda
function, and as the complete expression directly stored in `Stmt::Expr` of the test body or one
of its ordinary nested blocks. The validator passes an explicit `Statement | Value` placement
context through every expression edge: block values, let/assignment initializers, return/break
payloads, call operands, and all other children are `Value` and reject `TestAssert`, while a nested
block's own `Stmt::Expr` roots are `Statement`. The complete assertion expression is Unit. `Equal`
is emitted only after sema has checked the original two operands with the ordinary `BinOp::Eq`
rule, proved its result is exact Bool rather than vector/mask, and built one left-to-right Eq child;
the validator does not reconstruct the unavailable two source operands from a bool. MIR lowers a
false condition to the bounded diagnostic followed by the exact function Err cleanup edge and a
true condition to Unit fallthrough.

The ordinary parser still represents the last expression before `}` as `Block::tail`, including on
its own terminated source line. Sema assigns `Statement | Value` placement from the AST parent before
checking children. At the root test completion or in a nested block/control expression that is
itself a complete `Stmt::Expr`, sema recognizes an exact imported assertion call in that syntactic
tail, consumes it as the final assertion statement, and leaves Unit fallthrough; checked HIR
therefore still stores only `Stmt::Expr(TestAssert)` and no assertion value. The same call remains a
rejected Value when the enclosing block or control expression is consumed by a binding, argument,
return, break, or other operand, even if that consumer expects Unit. Ordinary functions, lambdas,
non-assertion tails, and parser behavior are unchanged.

Validation order is the complete production prefix, overlay table bounds, catalog length/order,
catalog identity bytes and root/back-reference correlation, overlay reachability closure, overlay
function envelopes and bodies, assertion envelope/placement/kind/location/condition, then ordinary
function completion and ownership records including per-function ownership side-table closure.
Every production lowering entry validates the complete
`CheckedProgram` but lowers only the frozen production prefix; test lowering validates it and lowers
prefix plus overlay. The selected view is an explicit API and cache input, never inferred from
whether a Program happens to contain a test. An assertion in an ordinary or generated function, a
test marked public/Pure, an orphan overlay helper, a prefix-to-suffix edge, a missing synthetic Ok,
duplicate/sparse ordinal, or any future metadata/assertion kind fails before MIR construction.

This section activates atomically with the implementation and its parameterized valid/malformed
owners. Metadata owners mutate each identity string independently and together, declared/default
entry paths, every root/back-reference and prefix/suffix boundary, ordinal, and duplicate relation;
assertion owners place each kind in every `Stmt::Expr` depth, the root and every statement-placement
nested final syntactic tail, plus every rejected value-consuming tail family including expected
Unit. Cache owners mutate only earlier test source width and prove changing spans/located metadata
beside an unchanged span-erased semantic projection and production artifact. They also independently
mutate every ownership tri-state and descriptor semantic field, reject orphan ownership keys, and
vary every descriptor-only span without changing the semantic descriptor projection. The public
grammar, runner, cache, and process protocol remain owned by
`core-design/test.md`.

### Planned `std.log` type records (designed 2026-08-31; inactive until implementation)

The accepted logging capability adds `Ty::Logger` and `Scalar::Logger` as one nominal Move-handle
family. It does not give `log.level` a scalar shortcut: the closed tag-only type remains the ordinary
`Ty::Enum`/`Scalar::Enum` aggregate `{ i32 tag }` through checked HIR and MIR. Each `LogNew`,
`LogEnabled`, `LogLine`, and `LogLineBuilder` level child has that exact enum id and aggregate type;
LLVM native-call lowering alone extracts validated field 0 for the runtime i32 argument.

Canonical type record version 3 already reserves payloaded asymmetric-key tags
`Ty::SignatureKey=63` and `Scalar::SignatureKey=39`. Logging reserves the next append-only leaf tags
`Ty::Logger=64` and `Scalar::Logger=40`; 65 and 41 remain the next unknown Ty and Scalar tags.
`Ty::Logger` encodes exactly as `[3, 0, 0, 0, 0, 64]`, while
`Ty::Option(Scalar::Logger)` encodes exactly as `[3, 0, 0, 0, 0, 4, 40]`. Decoding either vector
must return the identical semantic root. `[3, 0, 0, 0, 0, 65]` and
`[3, 0, 0, 0, 0, 4, 41]` reject as unknown tags; a missing root tag, a missing payload, and a
trailing byte reject before cache publication. Interface format 8 remains unchanged because both
public logging types use the existing nominal named-type/enum grammar.

The logging implementation activates the two type variants, all expression records, canonical
encoder/decoder arms, compiler fingerprint, and exact bidirectional/malformed goldens atomically.
The parameterized type-class and expression-envelope owners in `std-design/log.md` must fail on a
missing variant, wrong enum id, wrong result type, lost writer region, unknown/truncated/trailing
canonical byte, or native extraction before checked validation.

### Implemented `core.codec` records (2026-09-01)

The accepted codec capability adds six nominal builtin type families. `Ty::CodecBatch` and the four
`Ty::CodecI64Column` / `CodecF64Column` / `CodecBoolColumn` / `CodecStrColumn` views are Copy and
region-bearing; their corresponding scalars preserve the exact input region and storage generation
through every admitted carrier.
`Ty::CodecEncoder`/`Scalar::CodecEncoder` is one bare Move owner. `codec.kind` remains the ordinary
unique `Ty::Enum`/`Scalar::Enum` aggregate `{ i32 tag }`, with exact tags 0 through 3; it gets no
scalar shortcut.

Canonical type record version 3 uses the append-only Ty tags
`CodecBatch=65`, `CodecI64Column=66`, `CodecF64Column=67`, `CodecBoolColumn=68`,
`CodecStrColumn=69`, and `CodecEncoder=70`, and the corresponding Scalar tags 41 through 46. Thus
the six Ty roots encode as `[3,0,0,0,0,65]` through `[3,0,0,0,0,70]`, and an Option over each scalar
encodes as `[3,0,0,0,0,4,41]` through `[3,0,0,0,0,4,46]`. Tags 71 and 47 remain the next unknown Ty and
Scalar tags and reject, as do truncation and trailing bytes. Interface format 8 remains unchanged:
the public names and `codec.kind` use the existing named-type/enum grammar.

The expression envelope is closed as follows:

| Record family | Exact checked contract |
|---|---|
| `CodecOpen` | One `slice<u8>` child; exact `Result<codec.batch, builtin Error>` result; batch success provenance equals the child region/storage generation; no output fact on a terminating child. |
| `CodecBatchRows` / `Columns` | One validated batch receiver; exact i64 result; Copy read with no mutation or new fact. Lowering reads the retained envelope at exact byte 16 as alignment-1 little-endian i64, or byte 24 as alignment-1 little-endian u32 zero-extended to i64, with target-required byte swap; the two counts are not extra fields in the batch scalar. |
| `CodecBatchName` / `Kind` / `Find` | Exact batch receiver followed by exact i64 or text child; result respectively `Option<str>`, `Option<codec.kind>`, or `Option<i64>`; only the name result inherits batch provenance. Multi-byte descriptor reads are alignment-1 little-endian with target-required swaps; kind is one byte; no typed descriptor pointer is formed. |
| `CodecBatchI64s` / `F64s` / `Bools` / `Strs` | Batch then i64 ordinal; result is the exact Option over the corresponding i64/f64/bool/str column; every view result inherits the complete batch provenance and storage generation. Kind and multi-byte data/aux descriptor fields use byte or alignment-1 little-endian reads; no typed descriptor or element pointer is formed before ordinal/kind success. |
| `CodecColumnLen` / `At` | Exact one-of-four column receiver; i64 length or exact `Option<i64>`/`Option<f64>`/`Option<bool>`/`Option<str>` result. Only the str result inherits provenance. The kind of receiver and result must agree; numeric/offset access records carry alignment 1 and little-endian semantics. |
| `CodecEncoderNew` | One i64 child; exact `Result<codec.encoder, builtin Error>` result; success creates one fresh complete Move owner and no region. |
| four `CodecEncoderPut*` forms | Bound initialized mutable encoder receiver, text name, and exact `slice<i64>`/`slice<f64>`/`slice<bool>`/`slice<str>` child in source order; exact `Result<Unit,builtin Error>` result; receiver is borrowed and remains the same complete owner on success and recoverable failure; arguments are borrowed only for the call and no region enters the encoder. |
| `CodecEncoderFinish` | Bound initialized encoder receiver; exact `buffer` result; consumes/nulls the complete source exactly once and transfers no encoder identity or region into the buffer. |

Validation independently rejects user construction, casts, raw/extern creation, forbidden
placement/capture/parallel edges, a view result with missing or extra provenance, an encoder child
or result with a region, wrong enum id, cross-kind accessor result, unbound mutable receiver,
incomplete/consumed source, forged Drop/nulling, and any new sibling type/record omitted from the
canonical classifiers. All type/scalar/expression variants, canonical encoders/decoders,
interface/compiler fingerprints, and parameterized valid/malformed owners are active atomically.
`core-design/codec.md` owns the public surface, wire bytes, and full matrix.

### Implemented `pkg.frame` records (2026-09-01)

`pkg.frame.RowPair` and `pkg.frame.JoinError` use the existing declared-record and tag-only-enum
families rather than new `Ty` or `Scalar` variants. The canonical public definition graph fixes
`RowPair` fields as ordered `left: i64`, `right: i64`, and `JoinError` tags as
`InvalidLimit=0`, `LimitExceeded=1`. Interface format 8 and canonical type-record version 3 remain
unchanged. The package definition graph, ordinary recursive Move/Drop plan for
`array<RowPair>`, and enum identity remain part of the existing interface and compiler fingerprints.

The implementation adds exactly two expression discriminators atomically with package recognition,
runtime rows, validation, lowering, and owners. They occur only at the two exact private root-module
bridge calls inside the ordinary public wrappers; a caller's direct or indirect wrapper call remains
the ordinary `Call` / `FnValue` / `CallFnValue` path and converges on that compiled bridge body:

| Record family | Exact checked contract |
|---|---|
| `FrameInnerJoinI64` | Three children in source order: exact `codec.i64_column`, exact `codec.i64_column`, then exact i64. Result is the canonical `Result<array<pkg.frame.RowPair>, pkg.frame.JoinError>`. Each column child carries one live codec input region and storage generation into the call action; neither fact enters success or error. The record stores no package name, hash seed, build-side choice, retained region, or allocation fact. |
| `FrameInnerJoinStr` | The identical envelope with exact `codec.str_column` children. Both input provenance sets remain live through the call action, including distinct and shared-batch roots, and neither enters the ordinal-only result. The discriminator cannot select the i64 ABI row, and the i64 discriminator cannot select this one. |

Sema may emit either record only after ordinary module resolution proves the exact canonical
vendored `pkg.frame` private bridge, its one-call public wrapper body, and public definition graph.
A same-named local function or another module remains an ordinary call. An absent package, a
modified canonical wrapper/bridge body, a wrong column kind, a wider shared helper type, a wrong
RowPair field or JoinError tag, or a result alias rejects package admission before body evaluation;
none is rewritten to these records. Direct, imported,
local/function-field, and control-joined public function values execute the same ordinary wrapper
and therefore reach exactly one corresponding bridge record. Children evaluate exactly once
left-to-right. A terminating earlier child suppresses every later child and native action. The
signed value of `max_pairs` is runtime data, not a malformed-HIR axis; checked HIR validates its
exact i64 type while runtime maps negative and exceeded bounds.

The exact private names are `inner_join_i64_bridge` and `inner_join_str_bridge`. Each has the same
three parameters and result as its corresponding public wrapper and a source placeholder body of
`process.abort()`. The matching record is legal only in that wrapper's complete single-expression
body with the three wrapper parameters passed once in declaration order. A bridge call in any other
body or expression position, a private bridge `FnValue`, a repeated/reordered/wrapped child, or a
wrapper that contains any other expression rejects canonical package admission. The placeholder
abort is unreachable after accepted formation and emits no MIR/native action of its own.

Validation independently recomputes canonical package/type identity, exact discriminator-to-column
kind, child/result types, fallthrough, purity, complete input provenance, and absence of result
provenance. It rejects forged/extra/missing regions, a retained input fact, cross-kind native
selection, an output allocation before all child fallthrough, or any sibling discriminator omitted
from the exhaustive canonical classifiers. Existing array/Result control owners plus the exact
direct/control-selected/whole-per-unit/malformed matrix in `pkg-design/frame.md` must fail for any
missing child, wrong order/type/result, lost generation, retained region, or missing cleanup.

Both records, their canonical encoder/decoder arms, public/private package-body admission,
interface/compiler fingerprint inputs, cache identity, and runtime keys are active as one boundary.

### Reserved `pkg.csv` record (designed 2026-09-03; inactive)

The accepted `pkg.csv` design reserves one expression discriminator, `CsvDecode`. It is not in the
current enum, canonical codec, validator, lowering, capability collector, or inventory. Those
producers activate only atomically with canonical package source and runtime row A123.

`Header`, `LineEnding`, `DecodeOptions`, and `Error` use existing nominal enum/record families; no
new `Ty` or `Scalar` variant or format version is reserved. Their exact definition graph is
`Present=0, Absent=1`; `CrLf=0, Lf=1`; ordered fields `header`, `line_ending`, `max_rows`; and
`Invalid=0, LimitExceeded=1`.

| Reserved record | Exact checked contract |
|---|---|
| `CsvDecode` | Non-child `row: Ty` is template-only `Ty::Param(p)` during the discarded abstract check, with the exact matching existing `SoaParam(p)` result and `SoaPlain` bound, or emitted `Ty::Struct(id)` naming one concrete record in the complete existing `SoaPlain` domain with no CSV-specific field-count cap and unique source names. Explicit AoS layout/alignment does not narrow the bound. Children are exact `str input`, exact destination `region arena`, and exact nominal `pkg.csv.DecodeOptions options`, once in that source order. The concrete result is canonical `Result<soa<id>, pkg.csv.Error>` and effect is Pure. A primitive-only schema puts exactly the arena root in success provenance; a schema containing `str` puts the complete input storage root/generation plus arena root in success provenance, including the existing `Frame`-bounded synthetic owner when `input` auto-borrows an unbound owned `string` temporary. That owner survives fresh and control-wrapped input formation and follows the ordinary later-child cleanup/no-cleanup rule. No fact reaches the error alternative. |

Sema may form the record only for the exact compiler-private call spelling
`pkg.csv.internal.descriptor.decode` in the canonical vendored root `pkg.csv` public generic
wrapper. The exact internal descriptor module contains no import or source item, so the public
generic body references no private declaration omitted from its interface. The retained generic
body and internal module identity are available to importing-unit monomorphization, while the
package `internal` rule prevents application import. The wrapper body is the one positional
internal operation call using its parameters once in declaration order. A same-named application
function/extern, an added internal item, a changed package graph/body, or a noncanonical package
remains ordinary source or rejects package admission. Direct/imported/local/function-field and
control-selected public function values execute the ordinary wrapper and converge on its one
checked record.

The abstract wrapper check forms `row = Ty::Param(p)` only when `p` has the `SoaPlain` bound and
stores the matching `Result<SoaParam(p), pkg.csv.Error>` expression type. It performs no concrete
schema, descriptor, layout, provenance, MIR, or native work. The existing template path discards
that HIR and retains source AST for interface monomorphization. A concrete instantiation rechecks
the body under substitution and forms `row = Ty::Struct(id)` plus `Result<Soa(id), Error>`; generic
forwarding repeats the symbolic check until its outer concrete instantiation. Canonical HIR encoding,
body validation, MIR lowering, capability collection, and artifact publication reject every
`Ty::Param`/`SoaParam` row/result: only the concrete rechecked form may enter emitted `Program`.

Validation checks scalar identity and package/schema facts before the three children, then checks
children in source evaluation order and the relational result/effect/provenance rule. A terminating
child suppresses every later child and native action. MIR projects the already-evaluated options
record once into header tag, line-ending tag, and row bound; it retains the concrete schema,
destination region, result/provenance, and private status map. Status 0 publishes `Ok`, 1 publishes
`Err(Invalid)`, 2 publishes `Err(LimitExceeded)`, and every other status reaches the canonical
package's explicit `ProcessAbort` dependency without publishing output.

For a string-bearing schema, region, escape, storage-generation, and borrow traversals preserve an
auto-borrowed owned input's path-local synthetic owner through direct function/literal results,
blocks, `if`, `match`, `else`, `?`, `map_err`, and value-carrying loops. The result remains
`Frame`-bounded and cannot escape by return. If the already-completed input is followed by a
terminating arena or options child, no operation/result fact is published; cleanup-carrying exits
drop the armed owner exactly once and no-cleanup sinks invent no Drop. Primitive-only schemas do not
publish the input owner in the result fact.

Activation must update every expression match, replay/clone/depth/effect/region/escape/type-
placement/storage-generation traversal, canonical HIR codec, semantic projection, interface and
cache identity, whole/per-unit monomorphization, capability discovery, and the compile-time variant
sweep. The exact malformed-field, control-flow, provenance, allocation, ABI, cache, and package-
admission owners are the closure matrix in `pkg-design/csv.md`. Until that matrix lands, this
section is a reservation and changes no checked-HIR contract.

## Header-adjacent records

| Record | Exact contract |
|---|---|
| `ArithMode::Saturating` | Used only by `IntArith` with `Add`, `Sub`, or `Mul`; result is the common integer operand type. |
| `ArithMode::Checked` | Used only by `IntArith` with `Add`, `Sub`, or `Mul`; result is `Option<IS(T)>` for common integer operand type `T`. |
| `UnOp::Neg` | One operand/result of the same concrete signed integer or float type. Unsigned integers, vectors, generic parameters, and every nonnumeric type reject. |
| `UnOp::Not` | One `Bool` operand and `Bool` result. |
| `UnOp::BitNot` | One operand/result of the same concrete signed or unsigned integer type. Vectors and generic parameters reject. |
| `BinOp::Add` | Two equal concrete numeric scalars, two equal numeric vectors, or one numeric vector plus an exactly matching element scalar in either order; result is that scalar or vector type. |
| `BinOp::Sub` | Same operand/result domain as `Add`; scalar/vector order is preserved and scalar broadcast is permitted in either position. |
| `BinOp::Mul` | Same operand/result domain as `Add`. |
| `BinOp::Div` | Same operand/result domain as `Add`; integer zero aborts, signed `INT_MIN / -1` wraps to `INT_MIN`, and floats follow IEEE 754, lane-wise for vectors. |
| `BinOp::Rem` | Same operand/result domain as `Add`; integer zero aborts, signed `INT_MIN % -1` yields zero, and floats follow IEEE 754, lane-wise for vectors. |
| `BinOp::Eq` | Two equal concrete Int/Float/Bool/Char/Str operands produce `Bool`, or the numeric vector/vector-or-broadcast domain above produces the matching `Mask`. Structural and owned-string equality reject. |
| `BinOp::Ne` | Same operand/result domain as `Eq`. |
| `BinOp::Lt` | Two equal concrete Int/Float/Char/Str operands produce `Bool`, or the numeric vector/vector-or-broadcast domain produces the matching `Mask`. Bool ordering rejects. |
| `BinOp::Le` | Same operand/result domain as `Lt`. |
| `BinOp::Gt` | Same operand/result domain as `Lt`. |
| `BinOp::Ge` | Same operand/result domain as `Lt`. |
| `BinOp::And` | Two `Bool` operands produce `Bool`; the right operand is evaluated only on a true left operand. |
| `BinOp::Or` | Two `Bool` operands produce `Bool`; the right operand is evaluated only on a false left operand. |
| `BinOp::BitAnd` | Two equal concrete signed or unsigned integer operands produce that integer type; vectors and generic parameters reject. |
| `BinOp::BitOr` | Same operand/result domain as `BitAnd`. |
| `BinOp::BitXor` | Same operand/result domain as `BitAnd`. |
| `BinOp::Shl` | Two equal concrete signed or unsigned integer operands produce the left integer type; the shift count deliberately has the same type. Vectors and generic parameters reject. |
| `BinOp::Shr` | Same operand/result domain as `Shl`; signedness selects arithmetic versus logical right shift during lowering. |
| `MathFn::Abs` | One operand. Scalar `N` or vector `V(IS\|FS,n)`; result equals operand type. |
| `MathFn::Min` | Two operands of one exact type. Scalar `N` or vector `V(IS\|FS,n)`; result equals operand type. |
| `MathFn::Max` | Two operands of one exact type. Scalar `N` or vector `V(IS\|FS,n)`; result equals operand type. |
| `MathFn::Sqrt` | One operand. Scalar `F` or vector `V(FS,n)`; result equals operand type. |
| `MathFn::Floor` | One operand. Scalar `F` or vector `V(FS,n)`; result equals operand type. |
| `MathFn::Ceil` | One operand. Scalar `F` or vector `V(FS,n)`; result equals operand type. |
| `MathFn::Round` | One operand. Scalar `F` or vector `V(FS,n)`; result equals operand type. |
| `MathFn::Trunc` | One operand. Scalar `F` or vector `V(FS,n)`; result equals operand type. |
| `MathFn::Pow` | Two operands of one exact scalar `F`; vectors reject; result is that `F`. |
| `MathFn::Fma` | Exactly three operands of one exact scalar `F` or vector `V(FS,n)`; result equals operand type. |
| `MatchArm` | `env[variants,bindings,borrowed_bindings]`: variants are distinct in-range tags of the scrutinee sum in preserved source or-pattern order; empty means wildcard. The sum table is the declared user `Enum`, `Option` as ordered `Some(T),None`, or `Result` as ordered `Ok(T),Err(E)`. Wildcard or multi-tag arms have no bindings or `borrowed_bindings`. A one-tag arm has exactly the selected variant payload count of distinct in-range local ids, whose local types equal payload types and whose locals are bound before the arm body, visible only in that arm, and removed before the next sibling or enclosing tail. In borrowed mode, `borrowed_bindings` has one `BorrowedProjection { binding_local, variant, payload_ordinal, static_ty, path }` per binding; its path starts at `RootSlot`, is type-checked segment by segment, and may contain only the canonical struct/sum segments. Its `binding_local` is projection-only and must not appear in `drop_locals`, `drop_individual_locals`, or `drop_individual_exprs`. Its mode, source owner fact, and exact root/path come from the parent `Match.borrowed_place`; the validator independently replays the producer borrow/move flow and rejects any mismatch, absent/extra, duplicate, unsorted, or forged record before MIR. `child[body]`; `post[a reachable fallthrough body type equals the Match result under the structural body-type relation (including `FABI` for fresh function-value ids); a divergent body is context-polymorphic and contributes no result join]`. |
| `Block` | `env[stmts.len,value presence]`; `child[stmts in stored source order,value if present]`; `post[all retained dead children are structurally valid but contribute no reachable state; each `Let`/`LetTuple` initializer is checked before its binding enters the block scope; an absent reachable tail gives Unit, a present reachable tail gives its type, and an already non-fallthrough block uses its context-selected result type; block exit removes its bindings]`. |

## Statement ledger

| Discriminator | Exact envelope, children, and postcondition |
|---|---|
| `Let` | `env[local]`: `L(local)` and the id is the declaration at this statement. `child[init before bind]`. `post[init.ty == LT(local); initialize local once; Move init is consumed into local; recomputed individual flag matches local membership; the new binding is then visible in the enclosing block]`. |
| `LetTuple` | `env[locals,tuple_id]`: `TUPLE(tuple_id)`; vector length equals tuple arity; every present id is distinct and `L(id)` with the matching tuple element type. `child[init before all binds]`. `post[init.ty == Ty::Tuple(tuple_id); each present binding receives its ordinal projection exactly once and becomes visible after init; init is evaluated once]`. |
| `Assign` | `env[local,drop_old,drop_new]`: `MV(local)`. `child[value]`. `post[value.ty == LT(local); replacement Move transfer/nulling is exact; both cells equal the recomputed facts]`. |
| `AssignIndex` | `env[base]`: `MV(base)` is a fixed/dynamic scalar array or writable slice whose exact element satisfies sema-owned `indexed_element_store_ok` (integer, float, bool, char, or borrowed `str`) and the validator-owned stored-width rule. `child[index,value]`. `post[index.ty == i64; value.ty equals the exact element type; base is mutated; index/value are borrowed without consumption; bounds action occurs only after both children]`. |
| `AssignVecLane` | `env[local,lane]`: `MV(local)` has `Ty::Vec(s,n)` and `lane < n`. `child[value]`. `post[value.ty == scalar_to_ty(s); vector replacement is Copy]`. |
| `AssignField` | `env[root,path]`: `MV(root)` and `P(root,path)` succeeds. `child[value]`. `post[value.ty == P(root,path); replacement/drop-old fact for the leaf is recomputed; root remains the owner]`. |
| `AssignElemField` | `env[base,path,struct_id,soa]`: non-empty `path`; `STRUCT(struct_id)`; `MV(base)` agrees exactly with `soa` (`Soa(struct_id)` when true, fixed/dynamic struct array of that id when false); path succeeds from that struct; SoA path length is one and leaf is a permitted SoA scalar. `child[index,value]`. `post[index.ty == i64; value.ty == EP(Struct(struct_id),path); base mutation and fixed/dynamic old-leaf Drop are recomputed]`. |
| `AssignElem` | `env[base,struct_id,soa]`: `STRUCT(struct_id)` is the producer's flat Copy struct; `MV(base)` is exactly `Soa(struct_id)` when true or fixed `StructArray(struct_id,_)` when false. `child[index,value]`. `post[index.ty == i64; value.ty == Struct(struct_id); Copy scatter/store]`. |
| `Return(None)` | `env[presence=false]`; `child[]`; `post[function ret == Unit; terminates current path]`. |
| `Return(Some)` | `env[presence=true]`; `child[value]`; `post[value.ty == function ret; returned Move ownership and return-root/region facts equal the recomputed function boundary; terminates]`. |
| `Break` | `env[value presence,accepted]`: accepted equals the checker-owned loop-target/region decision in both directions: it is true exactly when the innermost target exists and the break remains at that loop's arena/task depth, and false only when no target exists or a nested arena/task region rejects the edge. `child[value if present]`. `post[bare break contributes Unit; an accepted break contributes a loop break when producer control reaches the statement (a nested accepted break in its value may reach the same loop, while a value that returns or enters a diverging nested loop does not); accepted payload type equals the target loop type; rejected break contributes no exit; either form is non-fallthrough]`. |
| `Expr` | `env[]`; `child[expr]`; `post[resolved child type is not Result(_, _); result is discarded after its required Drop; child non-fallthrough propagates]`. |

## Expression ledger: am-b1

The result formula in every row is followed by the universal
`stored Expr.ty == derived result` check.

| Discriminator | Exact envelope, children, and postcondition |
|---|---|
| `Unit` | `env[]; child[]; post[result(Unit),copy]`. |
| `Int` | `env[value]`: value fits the stored concrete `Ty::Int` under two's-complement literal conversion. `child[]; post[result(I),copy]`. |
| `Float` | `env[value]`: finite, infinity, and NaN bit patterns are permitted and conversion to stored `f32`/`f64` follows the producer. `child[]; post[result(F),copy]`. |
| `Char` | `env[value]`: Unicode scalar `0..=0x10ffff` excluding surrogates. `child[]; post[result(Char),copy]`. |
| `Str` | `env[UTF-8 bytes]`; embedded NUL is permitted. `child[]; post[result(Str),view(Static)]`. |
| `Bool` | `env[value]; child[]; post[result(Bool),copy]`. |
| `Local` | `env[id]`: `LV(id)`. `child[]; post[result(LT(id)); Copy reads borrow, Move reads follow the producer's move/borrow use classification]`. |
| `Unary` | `env[op]`; `child[expr]`; `post[Neg: expr/result same signed Int or Float; Not: expr/result Bool; BitNot: expr/result concrete signed or unsigned Int; copy]`. |
| `Cast` | `env[]`; `child[expr]`; `post[source/result pair is Int→Int/Float/Char, Float→Int/Float, or Char→Int/Char; Float↔Char and every generic/composite pair reject; copy]`. |
| `Binary` | `env[op]`; `child[lhs,rhs]`; `post[Add/Sub/Mul/Div/Rem: same numeric scalar result or valid scalar-broadcast/vector pair producing V(s,n); Eq/Ne: same Eq scalar/Str gives Bool or valid vector/broadcast pair gives K(s,n); Lt/Le/Gt/Ge: same Ord scalar/Str gives Bool or vector/broadcast pair gives K(s,n); And/Or: Bool×Bool→Bool with short-circuit control; BitAnd/BitOr/BitXor/Shl/Shr: same concrete I→I; no structural, owned-string, generic-param, vector-bitwise, or mismatched-lane case; copy]`. |
| `IntArith` | `env[op,mode]`: op is Add/Sub/Mul only. `child[lhs,rhs]`: same concrete I. `post[Saturating→I; Checked→Option<IS(I)>; copy]`. |
| `MathOp` | `env[fn_,operands.len]`; `child[operands in order]`; `post[the exact MathFn row above; copy]`. |
| `FnValue` | `env[name]`: non-empty NUL-free exact bytes and one non-extern `SIG(name)`. Outside `SPAWN`, the target is stored `Source`, `Monomorph`, or `Lifted { capture_count: 0 }`, or imported; its exact concrete scalar modes/parameters/return map to the in-range stored `FnTy`. In `SPAWN(fallible,ok)`, the target is stored `Lifted { capture_count: 0 }` and obeys the contextual signature rule above. A local binding may receive a fresh `FnTy` ordinal; its initializer and local type correlate by `FABI`, while effect/borrow/region cells remain am-b4 facts. `child[]; post[result(Fn(id)),copy; callable fact belongs to am-b3 and is consumed by am-c]`. |
| `SqliteCallbackDescriptor` | `env[kind,target,descriptor_id,signature,effect,return_borrow,return_region,return_cleanup,parallel_transfer_params,family_version]`: `kind` is exactly ScalarFunction; `target` is one non-extern stored/imported Source, Monomorph, or `Lifted { capture_count: 0 }` declaration with nonempty NUL-free encoded identity and no open/dynamic target set; `family_version == 1`; every stored signature/effect/return/parallel-transfer fact exactly equals that declaration's complete validated fact. `parallel_transfer_params` is the recomputed canonical sorted unique in-range borrow-capable root set produced through direct/concrete-function-value calls, with every compatible argument/capture selected for an unresolved indirect target; callback parameter 0 is absent. ScalarFunction requires exact `fn(pkg.db.sqlite.function_args) -> Result<pkg.db.value,str>` and effect Pure or Impure. `descriptor_id` is the canonical nominal identity over kind/version/target/signature/effect/return/parallel-transfer facts. `child[]; post[result exact nominal pkg.db.sqlite.scalar_function, copy static descriptor; no FnValue/Closure/environment is formed; callable fact is consumed by generated-callback preflight]`. |
| `Closure` | `env[lifted,captures.len]`: NUL-free `lifted` resolves exactly one stored function with `FnOrigin::Lifted { capture_count }`, `capture_count as usize == captures.len()`, and `capture_count > 0`; it is therefore non-exportable. `child[captures in order]`; `post[capture types equal the lifted trailing parameter types; outside SPAWN, explicit parameter modes/types and return equal result FnTy; in SPAWN, the contextual signature rule above applies; every capture is Copy and borrowed into one non-escaping environment; callable fact belongs to am-b3]`. |
| `CallFnValue` | `env[args.len]`; `child[callee,args in order]`; `post[callee.ty == Fn(id); argument count/modes/types equal FT(id); ByValue Move arguments are consumed, Out is a writable Slice place, and each Borrow/BorrowMut argument follows the enabled stable-place contract; an immediate BorrowedIndex is valid only at a Borrow position, snapshots its root before its index child, emits bounds at that argument position only on index fallthrough, and revalidates the root after every later fallthrough argument at the call action; an overlapping invalidation rejects; a terminating index has no guard/descriptor/later argument/action, and a terminating later argument has no pointer/action; result == FT(id).ret; return and BorrowMut-retention provenance map through exact actuals, with joined/unresolved fallback preserved]`. |
| `TaskGroup` | `env[]`; `child[block]`; `post[result == block/context-selected divergence type; one structured task region; all spawned tasks are joined exactly once on every fallthrough/exit path]`. |
| `EnumValue` | `env[enum_id,variant,payload.len]`: in-range enum/variant and exact payload arity. `child[payload in order]`; `post[each payload type equals its declared ordinal; result Enum(enum_id); active Move payload transfers once]`. |
| `Match` | `env[arms.len,borrowed_place]`: arms are non-empty. `borrowed_place` is absent for the existing owning path, or exactly `{ root_local, root_struct_path, sum_ty, mode, owner_fact }` for a stable place with a direct shared/exclusive borrow fact. The local ordinal and struct-field path identify the complete scrutinee place; a descendant borrow fact never promotes an owning parent. `owner_fact` is a sorted vector of live `BorrowRootFact { kind, ordinal, path }` records for that same place, with only `Local`, `Param`, or `ParamStorage` kinds; ended roots, iteration temporaries, spans, pointers, and process-local hashes reject. The validator independently replays the producer borrow/move flow and compares the complete record, including mode, generation, path, payload eligibility, and cleanup-local exclusion, rather than trusting `borrowed_place`. The sum type is one user `Enum(id)`, `Option(T)`, or `Result(T,E)`, and its reachable payload types satisfy the settled borrowed-projection admissibility grammar when `borrowed_place` is present: Copy scalars/views, `string`, ordinary `DynArray` and AoS `DynStructArray`, and finite acyclic structs/tags recursively. Array elements satisfy the same closed grammar; fixed and specialized arrays, tuples, other collections, resources, and opaque Move shapes reject. A borrowed arm binding cannot be used as a nested borrowed scrutinee; this is an explicit ownership diagnostic, not an owning fallback. `child[scrutinee,arms in order]`; `post[scrutinee uses the exact sum table; each MatchArm uses that table; no tag repeats across arms, at most one wildcard occurs at its preserved source position, and coverage is exhaustive by all tags or that wildcard; all fallthrough arm bodies have one result type under the structural body-type relation (including recursively matching fresh `FnTy` ids), while divergent arms are context-polymorphic; scrutinee evaluated once; owning or borrowed branch ownership joins exactly]`. |
| `ResultMapErr` | `env[]`; `child[result,f]`; `post[result.ty == Result(ok,err); f.ty == Fn(id) with one ByValue err parameter and return err2; FT summary/effect apply; result Result(ok,err2); Ok ownership passes unchanged and Err ownership transfers through f]`. |
| `Spawn` | `env[fallible]`; `child[closure with SPAWN(fallible,ok)]`; `post[inside task_group; ok is one primitive Int/Float/Bool/Char/Unit scalar; closure.ty == Fn(id) with zero explicit parameters and stored return ok; Pure/Impure allowed by current task rule; the exact lifted target returns ok when false or Result(ok,builtin Error) when true; result Task(ok); closure/environment transfers to task storage]`. |
| `TaskGet` | `env[]`; `child[task]`; `post[task.ty == Task(T) for current primitive Copy T; task has recomputed `TaskProof { group, born_generation }`; that group remains active, `born_generation >= valid_from`, and `completed_generation == Some(current_generation)` on this exact path; that completion proves every Wait registered before its establishing success resolved Ok, while a later no-task unresolved Wait is irrelevant; an inner group's Wait cannot discharge it; failed-check diagnostic uses that group's fallibility; result T is copied without consuming the Move task handle, TaskProof remains available, and repeated get is valid; owned task results are not producer-reachable]`. |
| `Wait` | `env[]`; `child[]`; `post[inside task_group; result Unit for infallible group or Result(Unit,builtin Error) for a fallible group exactly as producer records; joins registered tasks once]`. |
| `Call` | `env[func,type_args]`: `print` is a declaration-free core builtin with no type arguments, exactly one printable HIR argument (`Int`, `Float`, `Bool`, `Char`, or `Str`; source `String` is already a `StrBorrow`), and result `Unit`; `hash64` and `hash128` are declaration-free core builtins with no type arguments, exactly one `Str` or `slice<u8>` argument, and result respectively `u64` or `(u64,u64)`. Otherwise non-empty NUL-free func resolves one `SIG`; an extern target requires current lexical `unsafe_depth > 0`. Empty type_args require a non-Monomorph target. Non-empty type_args are concrete graph-valid types; encoding them with the single producer/validator-owned `mangle_mono_suffix(type_args)` (the current `mangle_mono("", type_args)` bytes) yields a non-empty `$...` suffix, func must equal a non-empty base plus that exact suffix, and the stored target must have `FnOrigin::Monomorph`. HIR stores neither the discarded generic template nor its bounds, so this row makes no uncheckable template/bound claim. `child[args in order]`; `post[arity/modes/types match the concrete SIG; an immediate BorrowedIndex is valid only at a Borrow position, snapshots its root before its index child, emits bounds at that argument position only on index fallthrough, and revalidates the root after every later fallthrough argument at the call action; an overlapping invalidation rejects; a terminating index has no guard/descriptor/later argument/action, and a terminating later argument has no pointer/action; Move/Out behavior and return/BorrowMut-retention provenance are exact; result SIG.ret for declaration-backed calls; a source spelling equal to a RuntimeKey still resolves ProgramCall]`. |
| `If` | `env[]`; `child[cond,then,els]`; `post[cond Bool; all fallthrough branches have one result type; missing-source else is represented by empty Unit block; divergent branch is context-polymorphic; state/ownership joins only fallthrough predecessors]`. |
| `StructLit` | `env[struct_id,fields.len]`: in-range struct and exact field count. It is a general value expression and may occur in every consumer that accepts the resulting struct; `Stmt::Let.init` merely has a direct in-place lowering. `child[fields in declaration order]`; `post[field types equal definitions; result Struct(struct_id); each Move field transfers once]`. |
| `Field` | `env[root,path]`: `L(root)` and `P(root,path)`. `child[]`; `post[result P(root,path); Copy/borrowed projection preserves root, Move projection follows the producer's permitted move-out rule]`. |
| `SoaColumn` | `env[base,struct_id,field]`: `LT(base)==Soa(struct_id)`, in-range field, and field is an admitted primitive SoA scalar. `child[]`; `post[result Slice(field scalar); view rooted in base]`. |
| abstract `soa<R>` boundary | Sema admits this template-only form exactly when the corresponding parameter has the closed `SoaPlain` bound. A public template interface preserves canonical `soa<Param(R)>` plus the bound for separate compilation; instantiation substitutes and rechecks the ordinary concrete SoA field rule. `Ty::Param` under SoA and every abstract batch thunk signature are forbidden in emitted HIR; `hir_program_is_valid` accepts only the existing concrete `Ty::Soa(struct_id)` after all nominal ids and resource-rooted return provenance are fixed. |
| `Tuple` | `env[tuple_id,elems.len]`: exact tuple arity. `child[elems in order]`; `post[each type equals its tuple ordinal; result Tuple(tuple_id); ownership transfers element-wise]`. |
| `TupleIndex` | `env[index]`; `child[recv]`; `post[recv.ty == Tuple(id), index in range, result tuple element type; projection ownership exact]`. |
| `IndexField` | `env[base,index,path]`: base local is fixed `StructArray(id,n)`, `index<n`, non-empty valid path. `child[]`; `post[result EP(Struct(id),path); view/Copy projection is rooted in base and any Move read obeys producer restriction]`. |
| `Block` | `env[]`; `child[block]`; `post[result block/context-selected divergence type; ownership/control pass through]`. |
| `OptionSome` | `env[]`; `child[value]`; `post[result Option(payload(value.ty)); active payload ownership transfers]`. |
| `OptionNone` | `env[]`; `child[]`; `post[result Option(payload) for one concrete admitted payload; no active ownership]`. |
| `ElseUnwrap` | `env[]`; `child[opt,fallback]`; `post[opt is Option(T) or Result(T,E); a fallthrough fallback has type T while a divergent fallback is context-polymorphic; result T; Some/Ok moves or borrows the active success payload; None/Err drops its active payload before evaluating fallback; branch ownership joins only reachable continuations]`. |
| `ResultOk` | `env[]`; `child[value]`; `post[result Result(payload(value.ty),E) for one concrete admitted E; active Ok ownership transfers]`. |
| `ResultErr` | `env[]`; `child[value]`; `post[result Result(T,payload(value.ty)) for one concrete admitted T; active Err ownership transfers]`. |
| `Try` | `env[]`; `child[result]`; `post[result child is Result(T,E), enclosing return is Result(U,E) with exact E; expression result T; Ok continues and Err transfers to one implicit return edge]`. |
| `Loop` | `env[diverges,body_locals]`: range is ordered, in bounds, and equals exactly all locals declared lexically anywhere inside this loop body and no lifted-function local. `child[body]`; `post[diverges iff no accepted break reaches this loop; an accepted break must remain at the loop's arena/task region depth; non-diverging accepted breaks agree on result type; body tail is discarded; iteration Drop set equals intersection with drop_locals; loop state reaches the existing finite join]`. |
| `Arena` | `env[]`; `child[block]`; `post[result block/context-selected divergence type; one arena begins before the block and ends exactly once on each exit; no escaping arena-owned value]`. |
| `Unsafe` | `env[]`; `child[block at unsafe_depth+1]`; `post[result block/context-selected divergence type; depth is restored before the next sibling; no runtime region; unsafe permission is lexical and effect is Impure]`. |
| `RawAlloc` | `env[]`; `child[size]`; `post[size i64; result Raw; unsafe lexical owner required; caller manually owns allocation]`. |
| `RawFree` | `env[]`; `child[ptr]`; `post[ptr Raw; result Unit; unsafe; runtime deallocates the referenced manual allocation; Raw is Copy, so its bits are neither nulled nor statically consumed and every later use or second free remains the unsafe programmer's responsibility; no automatic Drop fact]`. |
| `RawLoad` | `env[scalar]`: scalar is Int/Float/Bool/Char or `Struct(id)` whose exact stored definition has `c_repr == true`; unlike extern by-value placement, the producer's raw-storable predicate does not reject an empty C-layout struct. `child[ptr,offset]`; `post[ptr Raw, offset i64, result scalar_to_ty(scalar); unsafe; borrowed memory read]`. |
| `RawStore` | `env[]`; `child[ptr,offset,value]`; `post[ptr Raw, offset i64, value is Int/Float/Bool/Char or Struct(id) with c_repr == true under the same raw-storable predicate; result Unit; unsafe; manual memory write]`. |
| `RawOffset` | `env[]`; `child[ptr,offset]`; `post[ptr Raw, offset i64, result Raw; unsafe; derived pointer has the same manual owner]`. |
| `HeapNew` | `env[]`; `child[value]`; `post[inside arena; payload first satisfies the box type-argument predicate, then additionally rejects Slice; result Box(payload scalar); value is copied into the owned box allocation]`. |
| `BoxGet` | `env[]`; `child[box]`; `post[box.ty == Box(S), S is Copy; result scalar_to_ty(S); box borrowed]`. |
| `BoxClone` | `env[]`; `child[box]`; `post[box.ty == Box(S), S is Copy; result same Box(S); inside arena; source borrowed and destination newly arena-owned]`. |
| `StrClone` | `env[]`; `child[text]`; `post[text.ty is Str or String; result String; source borrowed without transfer; a fresh owned receiver is kept live only through the copy; result individually owned unless current arena captures it]`. |
| `StrPredicate` | `env[kind]`; `child[haystack,needle]`; `post[both Str; Contains/StartsWith/EndsWith/EqIgnoreCase result Bool; Find/Rfind result Option<i64>; both borrowed]`. |
| `StrTrim` | `env[kind]`; `child[recv]`; `post[recv Str; result Str; view inherits recv roots/region]`. |
| `StrBorrow` | `env[]`; `child[string]`; `post[string.ty == String; result Str; owned source is borrowed and remains live]`. |
| `BuilderNew` | `env[capacity presence]`; `child[capacity if present]`; `post[capacity i64; result Builder; one owned builder allocation with current arena/individual fact]`. |
| `BuilderWrite` | `env[kind]`; `child[builder,arg]`; `post[builder Builder and borrowed mutable handle; arg type exactly matches BuilderWriteKind; result Unit; no argument ownership transfer]`. |
| `BuilderToString` | `env[]`; `child[builder]`; `post[builder Builder; result String; consume builder once and transfer its buffer without double Drop]`. |

### am-b1 helper discriminators

| Discriminator | Exact operand contract |
|---|---|
| `BuilderWriteKind::Str` | argument `Str`. |
| `BuilderWriteKind::Int` | argument concrete integer. |
| `BuilderWriteKind::Float` | argument concrete `f32` or `f64`; width is derived from the argument type. |
| `BuilderWriteKind::Bool` | argument `Bool`. |
| `BuilderWriteKind::Char` | argument `Char`. |
| `StrPredKind::Contains` | Both operands are `Str`; result `Bool`. |
| `StrPredKind::StartsWith` | Both operands are `Str`; result `Bool`. |
| `StrPredKind::EndsWith` | Both operands are `Str`; result `Bool`. |
| `StrPredKind::Find` | Both operands are `Str`; result `Option<i64>`. |
| `StrPredKind::Rfind` | Both operands are `Str`; result `Option<i64>`. |
| `StrPredKind::EqIgnoreCase` | Both operands are `Str`; result `Bool`; ASCII-only case folding. |
| `StrTrimKind::Both` | Operand/result are `Str`, share provenance, and trim both ends. |
| `StrTrimKind::Start` | Operand/result are `Str`, share provenance, and trim only the start. |
| `StrTrimKind::End` | Operand/result are `Str`, share provenance, and trim only the end. |

## Expression ledger: am-b2

The am-b2 implementation is closed through contiguous dormant-validator cells inside the
canonical callable capability wave, except for the
narrow Request 6 scanner Copy predicate, which is consumed by the active pre-lowering gate.
Am-b2a owns
`ExprKind::ArrayLit` through `ExprKind::VecLit`; am-b2b1 owns `ExprKind::ArraySum` through
`ExprKind::ElemField` plus all `StageKind`; am-b2b2 owns `ExprKind::Template` through
`ExprKind::ArrayDictEncode` plus all nested `TemplatePart`, `GroupSource`, `GroupAgg1`, and
`GroupOp` records. No cell activates public HIR validation generally; the scanner predicate
is the named Request 6 exception, while am-b4 owns the assembled body activation and body-derived
ownership/effect correlation. The b2b1 checkpoint leaves b2b2 records fail-closed.

For this range, `ERR(T)` means `Result(payload(T), Scalar::Enum(error_enum_id))`
using the already validated builtin Error definition. `PIPE(source,stages)`
means:

1. the source is one exact producer-supported fixed/dynamic array, slice,
   struct-array, SoA, zip, chunks, or JSON scanner source for that terminal;
2. source is validated before all stages;
3. each `Stage` consumes the preceding element type and its stored `out_ty`
   exactly equals the row below;
4. stage captures are evaluated at the stage's written position;
5. the terminal's explicit arguments then terminal captures follow in the
   terminal row's written order; and
6. no action, allocation, capture snapshot, or ownership transfer occurs after
   a non-fallthrough child.

| Discriminator | Exact envelope, children, and postcondition |
|---|---|
| `ArrayLit` | `env[elems.len,elem,pooled]`: `elem` is one fixed-literal producer type; when `elems` is empty the producer must have obtained that exact type from an enclosing expected `slice<T>` context. Scalar arrays admit Copy payloads and reject every scalar Move value, including owned `string`, owned `array<T>`, and every owned handle such as File. They also reject every slice-bearing non-struct and Move enum. Fixed struct arrays permit over-aligned structs and recursively Move structs constructed in place. `child[elems]`; `post[all elements exactly elem; result Array(s,n) or StructArray(id,n), n==len and fits u32, including n==0 only for the expected-type form; pooled iff immutable unaligned let-bound nonempty/all-constant primitive scalar literal under the producer's pool predicate; element ownership transfers exactly]`. |
| `ConstArray` | `env[elems.len,elem,len]`: elem is primitive scalar or Str accepted by top-level constants; `len==elems.len` and fits u32. `child[elems]`; `post[each child is the exact folded literal form and type elem; result Slice(elem); static view, no ownership record]`. |
| `ArrayZip` | `env[sources.len,tuple_id]`: at least two sources; tuple arity equals source count and every tuple element is the source element type. `child[sources]`; `post[each source is a Copy-scalar fixed/dynamic array or slice with equal runtime length contract; result Tuple(tuple_id) only as a pipeline source; sources borrowed]`. |
| `Select` | `env[]`; `child[mask,a,b]`; `post[mask K(s,n), a/b same V(s,n); result V(s,n); copy]`. |
| `VecSumWhere` | `env[]`; `child[vec,mask]`; `post[vec V(s,n), mask K(s,n); result scalar_to_ty(s); s numeric; copy]`. |
| `VecDot` | `env[]`; `child[a,b]`; `post[a/b same numeric V(s,n); result scalar_to_ty(s); copy]`. |
| `VecMinMax` | `env[max]`; `child[vec]`; `post[vec numeric V(s,n); result scalar_to_ty(s); max chooses only operation, not type; copy]`. |
| `VecSum` | `env[]`; `child[vec]`; `post[vec numeric V(s,n); result scalar_to_ty(s); copy]`. |
| `VecLoad` | `env[elem,n]`: numeric scalar and lane count exact. `child[src,index]`; `post[src Slice(elem), index i64; result Vec(elem,n); src borrowed; bounds action after both children]`. |
| `VecStore` | `env[elem,n]`: numeric scalar and lane count exact. `child[dst,index,value]`; `post[dst writable Slice(elem), index i64, value Vec(elem,n); result Unit; dst mutated; bounds action after all children]`. |
| `VecLit` | `env[elems.len,elem]`: numeric scalar, supported lane count. `child[elems]`; `post[each exact scalar_to_ty(elem); result Vec(elem,len); copy]`. |
| `ArraySum` | `env[stages]`; `child[source,stage children]`; `post[PIPE; final element numeric scalar T; result T; source/captures borrowed]`. |
| `ArrayCount` | `env[stages]`; `child[source,stage children]`; `post[PIPE; result i64 regardless of final element type; source/captures borrowed]`. |
| `ArrayAnyAll` | `env[stages,func,all,captures.len]`: func NUL-free and resolves exact predicate signature. `child[source,stage children,captures]`; `post[PIPE; predicate parameters are final element then capture types, return Bool; result Bool; all selects fold identity only]`. |
| `ArrayMinMax` | `env[stages,is_max]`; `child[source,stage children]`; `post[PIPE; final element numeric scalar T; result T; is_max selects operation only]`. |
| `ArrayReduce` | `env[stages,func,captures.len]`: func resolves exact reducer signature. `child[source,stage children,init,captures]`; `post[PIPE; init A; reducer params A, final element, captures and return A; result A; accumulator ownership follows producer-supported Copy domain]`. |
| `ArrayScan` | `env[stages,func,captures.len,elem]`: func resolves reducer signature and elem is admitted output element. `child[source,stage children,init,captures]`; `post[PIPE; init/accumulator elem; reducer returns elem; result DynArray(payload(elem)); owned allocation, emitted values copied]`. |
| `ArrayDot` | `env[elem]`: numeric scalar type. `child[a,b]`; `post[a/b fixed arrays of elem with exactly equal static length; result elem; borrowed reads]`. |
| `ArraySort` | `env[stages,elem]`: exact numeric scalar Int or Float. `child[source,stage children]`; `post[PIPE final element elem; result DynArray(payload(elem)); new owned allocation sorted ascending]`. |
| `ArraySortBy` | `env[stages,key_func,key_ty,elem,captures.len]`: NUL-free key_func resolves callable; elem is primitive admitted source scalar; key_ty is concrete orderable scalar. `child[source,stage children,captures]`; `post[PIPE final element elem; key func params elem,captures and return key_ty; result DynArray(payload(elem)); new owned allocation]`. |
| `ArrayToArray` | `env[stages,elem]`: elem is the exact admitted materializable element. `child[source,stage children]`; `post[PIPE final element elem; result DynArray(payload(elem)) or DynStructArray(id) according to elem; new owned allocation and exact element transfers]`. |
| `ArrayToSoa` | `env[struct_id]`: in-range nonempty SoA-admissible flat struct and the expression is lexically inside an active arena. `child[source]`; `post[source fixed/dynamic StructArray(struct_id); result Soa(struct_id); source borrowed; result storage belongs to that exact active arena]`. |
| `ArrayMapInto` | `env[stages,elem]`: elem is admitted Copy scalar and every stage is length-preserving (`Map`/`Project`, no filter). `child[source,stage children,dst]`; `post[PIPE final element elem; dst writable Slice(elem), exact equal-length runtime contract and semantic no-alias proof; result Unit; dst mutated, no allocation]`. |
| `ArrayPartition` | `env[stages,func,elem,captures.len]`: predicate resolves. `child[source,stage children,captures]`; `post[PIPE final element elem; predicate params elem,captures→Bool; result is exact interned tuple (DynArray(elem),DynArray(elem)); two owned allocations]`. |
| `ArrayParMap` | `env[stages,func,elem,captures.len]`: callable resolves and its structural signature is exact; the `complete reachable effect == Pure` requirement is an am-b4 producer fact, not consumed by the dormant am-b2b1 body slice. `child[source,stage children,captures]`; `post[PIPE; callable params final element,captures and return elem; captures Copy; result DynArray(payload(elem)) or supported struct-array result; one owned output; parallel eligibility facts equal producer]`. |
| `ArrayChunks` | `env[elem]`: primitive Copy scalar. `child[source,n]`; `post[source fixed/dynamic array or Slice(elem), n i64; result DynSliceArray(prim(elem)); owned header array whose elements view source; source remains live]`. |
| `ArrayToSlice` | `env[]`; `child[array]`; `post[array is fixed Array(s,n), fixed StructArray(id,n), DynArray(s), or AoS DynStructArray(id); fixed sources are Local or ArrayLit, element identity is exact, and result is matching Slice(s/Struct(id)); view borrows array storage, no allocation]`. |
| `Len` | `env[]`; `child[recv]`; `post[recv is exactly Str, String, Slice(_), DynArray(_), DynStructArray(_,_), DynSliceArray(_), DynResponseArray, or Soa(_); result i64; recv borrowed. Fixed-array lengths are Int literals and never Len records]`. |
| `Index` | `env[]`; `child[recv,index]`; `post[index i64. A Vec(s,n) receiver requires index to be an Int literal in 0..n and returns scalar_to_ty(s). Otherwise recv is Array, Slice, DynArray, DynSliceArray, StructArray, DynStructArray, Soa, or the existing receiver-only DynResponseArray method place; a fixed source is Local or ArrayLit; result is its exact scalar, slice, or struct element and must not be recursively Move, except exactly DynArray(String) returns the representation-compatible non-owning Str view and the pre-existing DynResponseArray method place returns HttpResponse; recv is borrowed and the bounds action is last. A terminating index has no bounds/result fact. For `array<str>`, `array<string>`, and AoS Copy records with any array/record-admitted region-bearing Copy leaf, including direct or nested `str` and `slice<T>`, a direct/field/BorrowedProjection/temporary base maps its complete generation and contained roots into the result, return, and BorrowMut-retention facts through the canonical borrow/region classifier; an unbound owned receiver uses its existing frame-bounded temporary owner]`. Every other recursively Move dynamic scalar/AoS-record element uses only the separate `BorrowedIndex` row below. |
| `BorrowedIndex` | `env[base]`: `base` is canonical `BorrowedElementBase { root_local, path, array_ty, element_ty, owner_fact }`; `path` begins with `RootSlot` and contains only valid typed struct/sum segments; the resolved place is a stable ordinary `DynArray` or AoS `DynStructArray`, `array_ty` and `element_ty` exactly match it, and owner facts are sorted, unique, field-canonical, and exactly equal replayed producer flow. Validation rejects an empty or incomplete multi-root vector, extra/duplicate/unsorted facts, every mismatched `kind`, `ordinal`, or typed `path`, and facts stale after replacement, move/Drop, or a control-flow join; the source generation and complete contained-root set must still equal the action point. `child[index]`; `post[index i64, expression type == element_ty, this expression occurs only as an immediate direct/imported/CallFnValue argument at an exact Borrow parameter position; the complete source root is snapshotted before the child and remains reserved through every later argument and the call action; any possibly overlapping move, Drop, replacement, ByValue transfer, or BorrowMut in that interval rejects while unrelated-root mutation remains valid; an index that does not fall through has no bounds guard, MIR descriptor, later argument, pointer, or call; otherwise every later fallthrough path revalidates the root before pointer formation, while a terminating later argument has no pointer/action; return and BorrowMut-retention substitution use the array generation and contained roots; MIR owns the once-only bounds action at this argument position and carries only a guarded descriptor to the action]`. |
| `SliceRange` | `env[start presence,end presence]`; `child[recv,start if present,end if present]`; `post[start/end i64; recv Str→Str or fixed/dynamic primitive array/Slice(s)→Slice(s); result view inherits recv owner/region; range action last]`. |
| `ElemField` | `env[path,struct_id]`: nonempty valid path in struct_id. `child[recv,index]`; `post[index i64; recv fixed/dynamic StructArray(struct_id) or Soa(struct_id) where producer admits this path; result exact leaf; result view/Copy fact inherits the complete direct/field/BorrowedProjection receiver generation and contained roots; a terminating index has no bounds/result fact; bounds/path action last]`. |
| `Template` | `env[parts.len]`; `child[parts in order]`; `post[nonempty; every part is checked against its exact access type; an exact `Text("{")`/`Text("}")` stack tracks nested optional-field objects, and each `PopComma` requires an optional field since the previous pop in the current object; result Str; hidden builder ownership is registered before holes and cleaned/transferred exactly]`. Part records have no span; only the enclosing expression span participates. |
| `JsonEncodeBounded` | `env[base, parts.len]`; `child[parts in order, max_bytes]`; `post[base is one visible admitted source local; reconstructing its complete reachable schema exactly matches every static token, field ordinal, access root/path, name, descriptor identity, and fixed-array element in parts; nonempty; max_bytes exactly i64; result exactly Result<String,builtin Error>; every part access is borrowed; source accesses precede the limit; success owns the String payload and failure owns no partial builder output]`. |
| `JsonDecode` | `env[struct_id]`: Decode-direction JSON descriptor. `child[input]`; `post[input Str; result ERR(Struct(struct_id)); clean Str fields retain input provenance; escaped selected Str fields require the nullable caller arena and retain input+arena provenance; successful struct ownership exact]`.
| `JsonDecodeArray` | `env[elem]`: JSON scalar-array element is Int/Float/Bool. `child[input]`; `post[input Str; result ERR(DynArray(payload(elem))); new owned array, no input view]`. |
| `JsonDecodeScalar` | `env[scalar]`: scalar is Int/Float/Bool. `child[input]`; `post[input Str; result ERR(scalar); copied result]`. |
| `JsonDecodeStructArray` | `env[struct_id]`: JSON-decodable struct-array descriptor. `child[input]`; `post[input Str; result ERR(DynStructArray(struct_id)); owned AoS buffer; clean Str fields retain input provenance; escaped selected Str fields require the nullable caller arena and retain input+arena provenance]`.
| `JsonDecodeSoa` | `env[struct_id]`: nonempty SoA-admissible struct whose fields are exactly Int/Float/Bool/Char/Str. `child[input]`; `post[input Str; inside arena; result ERR(Soa(struct_id)); arena storage and input provenance exact]`. |
| `JsonDecodeUnion` | `env[enum_id]`: enum variants satisfy the unique JSON shape-class decoder contract. `child[input]`; `post[input Str; result ERR(Enum(enum_id)); clean Str payloads retain input provenance; escaped selected Str payloads require the nullable caller arena and retain input+arena provenance; active payload ownership exact]`.
| `JsonDoc` | `env[]`; `child[input]`; `post[input Str; inside arena; result exactly Result(Scalar::JsonDoc, builtin Error); document view rooted in input and arena]`. |
| `JsonDocKind` | `env[]`; `child[doc]`; `post[doc JsonDoc; result exactly the unique builtin Enum(json.kind) with the seven settled tag-only variants; copy]`. |
| `JsonDocGet` | `env[]`; `child[doc,key]`; `post[doc JsonDoc,key Str; result JsonDoc; view inherits doc provenance]`. |
| `JsonDocAt` | `env[]`; `child[doc,index]`; `post[doc JsonDoc,index i64; result JsonDoc; view inherits doc provenance]`. |
| `JsonDocAsStr` | `env[]`; `child[doc]`; `post[doc JsonDoc; result Option<Str>; view payload inherits doc provenance]`. |
| `JsonDocAsScalar` | `env[scalar]`: exactly i64, f64, or Bool. `child[doc]`; `post[doc JsonDoc; result Option<payload(scalar)>; copy]`. |
| `JsonDocLen` | `env[]`; `child[doc]`; `post[doc JsonDoc; result i64; copy]`. |
| `JsonDocKey` | `env[]`; `child[doc,index]`; `post[doc JsonDoc,index i64; result Option<Str>; view payload inherits doc provenance]`. |
| `JsonDocElems` | `env[]`; `child[doc]`; `post[doc JsonDoc; inside arena; result Slice(JsonDoc); handle slice and elements inherit doc+arena provenance]`. |
| `JsonScan` | `env[struct_id,stored_ty]`: the active Request 6 exception order is enclosing `Expr.span`, exact `stored_ty == JsonScanner(struct_id)`, existing row id, `input.ty == Str`, Decode-direction JSON descriptor, and the complete reachable row graph's canonical recursive Copy/`DropPlan` predicate. A malformed span therefore beats wrong stored type, unknown row id, wrong input type, schema, and Copy errors; `validate_hir::json_scan_validation_reason` returns the exact first reason for the precedence matrix while production lowering keeps the boolean gate. Unresolved `json.scanner<Row<T>>` row arguments and partially substituted composite generic arguments remain outside this row: sema rejects them before constructing a scanner expression and retains the exact producer diagnostics. Generic call checking classifies only the callee's own inference slots, so an enclosing generic parameter carried by a bound slot is valid forwarding; an expected-return seed error stops before any argument is checked. Producer-owned scanner spelling travels through checker-only inference slots, annotated or inferred locals, transparent generic-call results, parameters, and lambda captures; the checker derives it only across producer-owned local/block/borrow/call boundaries, with no HIR field or artifact/source reconstruction. The semantic source producer applies this Request 6 gate before constructing HIR and owns the exact public diagnostic. For imported/per-unit consumers, interface/import reconstruction first materializes checked HIR; the active `align_mir::hir_program_is_valid` pre-lowering gate then rechecks the complete envelope and graph fail-closed before MIR/runtime lowering and never reconstructs source spelling; the structural body validator alone is not sufficient. `child[input]`; `post[input.ty == Str; result JsonScanner(struct_id); pipeline-source-only view rooted in input; escaped declared string tokens are rejected because scanner has no arena; five accepted HIR terminal variants expose seven public methods Sum/Count/Reduce/Any/All/Min/Max, each with exact Result(scalar,builtin Error)]`. |
| `JsonScanGenericCall` | `env[callee_slots,expected]`: expected-return seeding binds generic slots while validating concrete return leaves, so a concrete mismatch stops before any argument; annotated scanner return spelling is retained even when the call has no scanner argument. `child[args]`; `post[callee-local slot classification, source-order argument checking, checker-only spelling through signature/local/block/borrow/call boundaries, no partial HIR or cache publication on failure]`. Owner tests are `m5::json_scan_generic_return_context_expected_concrete_conflict_no_cascade` and `m5::json_scan_generic_argument_source_spelling`. |
| `ArrayGroupAgg` | `env[base,struct_id,key_field,value_field,op,source]`: base/source/struct agree by GroupSource row; key/value ordinals in range; Count iff value_field None, other ops iff Some exact i64 field. `child[]`; `post[result exact tuple: (array<i64>,array<i64>) for SoaI64, otherwise (array<str>,array<i64>); arrays are owned and Str keys borrow base]`. |
| `ArrayGroupAggMulti` | `env[base,struct_id,key_field,aggs,source]`: source is producer-supported AosStr first cut; key is Str; nonempty aggs and each GroupAgg1 row valid. `child[]`; `post[result exact tuple of key array followed by one i64 array per agg; one fused pass; ownership/provenance as single aggregate]`. |
| `ArrayDictEncode` | `env[base,struct_id,key_field]`: base is exactly DynStructArray(struct_id,Aos), key field is Str. `child[]`; `post[result DictEncoded(struct_id,key_field); dense ids owned, dictionary/source slices borrow base]`. |

#### Request 9 owned JSON HIR extension (superseded)

Request 9 introduced the dedicated owned route. Request 13 atomically replaced
its flat V1 plan, template-part variants, and interface format; no V1 producer,
consumer, or compatibility decoder remains.

#### Request 13 recursive owned JSON replacement (implemented)

`docs/impl/25-recursive-owned-json-plan.md` retains the three `JsonOwned*`
discriminants but replaces their flat stored plan and direct owned template parts
with one `OwnedJsonGraphPlanV2 { root, graph }`.

| Discriminator | V2 envelope, children, and postcondition |
|---|---|
| `JsonOwnedDecode` | `env[root,graph]`: root is an existing nonempty natural-layout record; a non-diagnosing scan finds a transitive owned String; iterative root-first validation admits only fixed-width integers, Bool, String, accepted records, non-nested Options, and dynamic arrays of integer/Bool/String/record; graph is acyclic, view-free, constructor depth ≤ 128, exact under recursive `DropPlan`, and byte-equal to reconstructed `OwnedJsonGraphDescV2`. `child[input]`; `post[input Str; exact Result<Struct(root),Error>; every reachable owner free-standing; no input/arena provenance; one complete transfer]`. |
| `JsonOwnedEncode` | `env[root,graph,base]`: the same reconstructed V2 graph and one stable visible root place; no unrolled recursive template parts remain. `child[base borrow]`; `post[exact Str under existing builder lifetime; A80 visits declaration-order graph; source not moved or mutated]`. |
| `JsonOwnedEncodeBounded` | `env[root,graph,base]`: same V2 source plan. `child[base borrow,max_bytes]`; `post[max_bytes exact i64 after source-plan validation; exact Result<String,Error>; bounded A80 bytes equal unbounded on success; no partial owner]`. |

Interface format 8 validates `OwnedJsonInterfaceEnvelopeV2` and the complete V2
graph before cache lookup or HIR construction. The body gate reconstructs it
again from definitions and compares exact bytes. Graph discovery, codec,
validation, Drop comparison, hashing, clone/replay, and table construction use
explicit worklists; an active record is the cycle error, a completed record is a
shared DAG reference, and depth 128/129 is accepted/rejected deterministically.
The Request 9 owned template-part variants are deleted in the same implementation
commit, so no V1 flat producer survives without its V2 consumer.

#### Fixed-array element admission closure

The `ArrayLit` producer and validator close the ownership boundary by element class:

| Element class | Producer and validator disposition | Ownership owner |
|---|---|---|
| Copy scalar, `str`, or function value | Admit; copy each element into the fixed slot. | cumulative array/closure owners |
| owned `string` | Reject before checked HIR; no fixed-array per-element scalar Drop path exists. | `align_mir::tests::fixed_array_move_shapes_match_the_hir_gate` and `align_mir::validate_hir_tests::hir_body_validator_storage_vector_array` |
| recursively Move struct | Admit only when each element is constructed in place; reject copying a pre-existing value. | `owned_structs_arrays` |
| owned `array<T>` or another scalar Move value | Reject before checked HIR; no fixed-array per-element null/drop lowering exists. | `align_mir::tests::fixed_array_move_shapes_match_the_hir_gate` |
| Move enum, resource/ref, or owned native handle | Reject before checked HIR; the fixed collection has no tag/resource-specific per-element transfer path. | `align_mir::validate_hir_tests::hir_body_validator_storage_vector_array` plus cumulative tagged/resource owners |
| slice-bearing non-struct | Reject before checked HIR; per-element view provenance is not representable. | cumulative borrow-liveness owners |

### am-b2 nested records

| Discriminator | Exact envelope, children, and postcondition |
|---|---|
| `StageKind::Map` | `env[func,captures.len]`: exact callable. `child[captures]`; `post[callable params input element,captures; return == Stage.out_ty; capture types exact]`. |
| `StageKind::Where` | `env[func,captures.len]`: exact callable. `child[captures]`; `post[callable params input element,captures→Bool; Stage.out_ty == input type]`. |
| `StageKind::WhereField` | `env[field]`: input is Struct(id), field in range and Bool. `child[]; post[Stage.out_ty == input type]`. |
| `StageKind::WhereStrContains` | `env[]`; `child[needle]`; `post[input Str, needle Str, Stage.out_ty == Str; needle is loop-invariant by producer evidence and snapshot once]`. |
| `StageKind::Project` | `env[field]`: input is Struct(id), field in range. `child[]; post[Stage.out_ty == exact field type]`. |
| `TemplatePart::Text` | `env[UTF-8 bytes]`; embedded NUL permitted; `child[]`; append exact bytes. |
| `TemplatePart::Hole` | `env[]`; `child[expr]`; primitive printable Int/Float/Bool/Char/Str only; append using exact type. |
| `TemplatePart::JsonStr` | `env[]`; `child[expr]`; expr Str; append quoted JSON-escaped bytes. |
| `TemplatePart::OptionField` | `env[name]`: nonempty NUL-free JSON field bytes. `child[access]`; access exactly Option<Int/Float/Bool/Str>; it must be inside an active exact-object state and marks that state as having an optional field. |
| `TemplatePart::OptionStructField` | `env[name,struct_id]`: Encode-direction JSON struct descriptor. `child[access]`; access exactly Option<Struct(struct_id)>; it must be inside an active exact-object state and marks that state as having an optional field. |
| `TemplatePart::PopComma` | `env[]; child[]`; no span; valid only with an active object that has an optional field since its last pop; clears exactly that object's pending-comma state. |
| `TemplatePart::StructArrayField` | `env[struct_id]`: Encode-direction JSON struct descriptor. `child[access]`; access exactly DynStructArray(struct_id,Aos); borrowed encode. |
| `TemplatePart::ScalarArrayField` | `env[elem]`: JSON scalar element Int/Float/Bool/Str. `child[access]`; access exactly DynArray(elem); borrowed encode. |
| `TemplatePart::UnionValue` | `env[enum_id]`: JSON-encodable union. `child[access]`; access Enum(enum_id); borrowed active-payload encode. |
| `GroupSource::SoaI64` | base `Soa(struct_id)`; key field i64; result keys owned i64. |
| `GroupSource::SoaStr` | base `Soa(struct_id)`; key field Str; result key views borrow the source columns. |
| `GroupSource::AosStr` | base dynamic AoS `DynStructArray(struct_id,Aos)`; key field Str; result key views borrow base. |
| `GroupSource::Encoded` | base `DictEncoded(struct_id,key_field)` with exact same ids; dense i64 grouping and dictionary-backed Str result. |
| `GroupAgg1` | `env[op,value_field]`: Count iff None; Sum/Min/Max iff Some in-range i64 field. |
| `GroupOp::Sum` | requires one i64 value field and produces wrapping i64 sum. |
| `GroupOp::Min` | requires one i64 value field and produces i64 minimum. |
| `GroupOp::Max` | requires one i64 value field and produces i64 maximum. |
| `GroupOp::Count` | requires no value field and produces i64 row count. |

## Expression ledger: am-b3

`bytes` is exactly `Slice(u8)`. `byte-view` is the already checked coercion
result `Str` or `bytes`; an owned `String` reaches these rows only through
`StrBorrow`. `argv` is the producer-supported dynamic array or slice of `Str`.
Unless a row explicitly says `consume`, handle operands are borrowed. Bare
handle-type requirements in the rows mean the following exact stable-place
predicate, not type equality alone:

- `LocalHandle(T,e)` is exactly `e.kind == Local(id)` and `LT(id) == T`.
  It is required for borrowed or receiver-mutated `Buffer`, `ArrayBuilder`,
  `Rng`, `Regex`, `Captures`, `CliCommand`, `CliParsed`, `TcpConn`,
  `TcpListener`, `UdpSocket`, `Child`, `Command`, `RunOutput`, `HttpRequest`,
  `HttpClient`, `HttpServer`, `HttpStream`, `ResponseBuilder`, and `File`.
  Request 11 activates `RunBytes` under this same predicate when its reserved
  rows below land; a temporary output handle is never admitted by type equality alone.
- `ReaderPlace(e)` is `LocalHandle(Reader,e)` or the exact zero-argument
  `ReaderStdin` producer. `ReaderRead` and the reader operand of `IoCopy` use
  it. `ReaderReadLine` is narrower: it is a local recorded by the producer's
  buffered-reader fact. `ReaderBuffered` is the sole reader receiver exception:
  it consumes any producer-valid Reader expression and transfers the fd, so a
  temporary does not leak.
- `WriterPlace(e)` is `LocalHandle(Writer,e)` or
  `WriterStd { buffered: false, fd: 1|2 }`. `WriterWrite`, `WriterFlush`, and
  the writer operand of `IoCopy` use it. A buffered `WriterStd` must first be a
  local because unwritten bytes are released only by Flush/Drop.
- `HttpResponsePlace(e)` is `LocalHandle(HttpResponse,e)` or an `Index` whose
  receiver is a local `DynResponseArray`, whose index/type relation satisfies
  the `Index` row, and whose result is `HttpResponse`. `HttpRespStatus`,
  `HttpRespHeader`, and `HttpRespBody` use it.
- `HttpRequestCtxPlace(e)` is `LocalHandle(HttpRequestCtx,e)` or a valid
  `Field` place of that exact type. Every `HttpCtx*`, `HttpRespond`, and
  `HttpRespondStream` receiver uses it. `HttpHeaders` is a Copy view and has no
  stable-handle restriction, preserving `ctx.headers().get(name)`.
- `SourceMutLocal(T,e)` is exactly a bare source local whose checked child is
  `LocalHandle(T,e)` and whose local has `is_mut == true`. It is required
  exactly for `ArrayBuilderPush.builder`, `ArrayBuilderAppend.builder`,
  `RandNext.rng`, `RandRange.rng`, `RandSample.rng`,
  `BufferPut.buffer`, and `BufferAppend.buffer`. `RandShuffle` additionally
  requires its `rng` to satisfy that predicate and its `xs` to be the
  producer's writable `Slice(T)` local: `is_mut == true`, not read-only, and
  admitted primitive element `T`. Am-v adds the same
  `SourceMutLocal(Buffer,...)` predicate to the five native output operands
  `ReaderRead.buffer`, `ReaderReadLine.buffer`, `FilePread.buffer`,
  `UdpRecvFrom.buffer`, and `CryptoRandom.out`. No other native-handle row
  inspects `Local.is_mut`: mutation of a Command, CliCommand, HttpRequest,
  HttpClient, ResponseBuilder, HttpStream, Reader, Writer, File, socket, or
  other runtime object's interior state is not source binding reassignment.
- A row marked `consume-any` transfers the complete temporary and is exempt;
  ordinary `consume` does not erase a row's stated receiver-place gate. The
  complete consume-any set is `ReaderBuffered.reader`,
  `ArrayBuilderBuild.builder`, `HttpClientRequest.req`, `HttpRespond.rb`,
  `HttpRespondStream.rb`, and `HttpStreamReject.rb`. Every other handle
  operand uses the applicable place above.

This predicate is a post relation and is validated only after every row child
has completed, in the universal order above. Owner tests enumerate every
current semantic diagnostic containing “bind ... first”, mutate one accepted
local/exempt producer to an equal-typed temporary, and pair it with an invalid
later child to prove that the child diagnostic wins. Thus a
borrowed read cannot leak an fd, buffer, pid, pending output, or native handle
merely because its `Ty` matches.

| Discriminator | Exact envelope, children, and postcondition |
|---|---|
| `FsReadFile` | `env[]; child[path]`; `path Str; result ERR(String); new owned string; Impure`. |
| `ReaderStdin` | `env[]; child[]; result Reader; owned handle with borrowed fd; Pure construction`. |
| `ReaderOpen` | `env[]; child[path]`; `path Str; result ERR(Reader); successful owned fd; Impure`. |
| `WriterStd` | `env[fd,buffered]`: fd exactly 1 or 2. `child[]; result Writer; owned handle with borrowed fd; Pure construction`. |
| `WriterCreate` | `env[]; child[path]`; `path Str; result ERR(Writer); successful owned fd; Impure`. |
| `ReaderRead` | `env[]; child[reader,buffer]`; `ReaderPlace(reader), SourceMutLocal(Buffer,buffer); result ERR(i64); both borrowed, buffer mutated; Impure`. |
| `ReaderBuffered` | `env[]; child[reader]`; `Reader; result Reader; consume source and transfer fd into new owned buffered handle; Pure`. |
| `ReaderReadLine` | `env[]; child[reader,buffer]`; `reader is the producer-recorded buffered Reader local, SourceMutLocal(Buffer,buffer); result ERR(i64); both borrowed, buffer mutated; Impure`. |
| `BytesAsStr` | `env[]; child[bytes]`; `bytes; result ERR(Str); success view inherits bytes provenance; Pure`. |
| `WriterWrite` | `env[builder]`; `child[writer,arg]`; `Writer borrowed; builder=false requires byte-view, true requires Builder; result ERR(Unit); arg borrowed; Impure`. |
| `WriterFlush` | `env[]; child[writer]`; `Writer; result ERR(Unit); borrowed; Impure`. |
| `IoCopy` | `env[]; child[reader,writer]`; `Reader,Writer; result ERR(i64); both borrowed; Impure`. |
| `FileCreateRw` | `env[]; child[path]`; `Str; result ERR(File); new owned fd; Impure`. |
| `FileOpenRw` | `env[]; child[path]`; `Str; result ERR(File); new owned fd; Impure`. |
| `FilePread` | `env[]; child[file,buffer,offset]`; `LocalHandle(File,file), SourceMutLocal(Buffer,buffer), i64; result ERR(i64); handles borrowed, buffer mutated; Impure`. |
| `FilePwrite` | `env[]; child[file,data,offset]`; `File, byte-view, i64; result ERR(i64); borrowed; Impure`. |
| `FileLen` | `env[]; child[file]`; `File; result ERR(i64); borrowed; Impure`. |
| `BufferNew` | `env[]; child[capacity]`; `i64; result Buffer; new owned allocation; Pure`. |
| `BufferBytes` | `env[]; child[buffer]`; `Buffer; result bytes; view inherits buffer provenance; Pure`. |
| `StrBytes` | `env[]; child[inner]`; `Str; result bytes; view inherits string provenance; Pure`. |
| `BufferLen` | `env[]; child[buffer]`; `Buffer; result i64; borrowed; Pure`. |
| `BytesRead` | `env[be]`; `child[bytes,offset]`; `bytes,i64; result exact stored read scalar in {i8/u8/i16/u16/i32/u32/i64/u64/f32/f64}; be must be false for one-byte widths; borrowed bounds-checked read; Pure`. |
| `BufferPut` | `env[be]`; `child[buffer,value]`; `SourceMutLocal(Buffer,buffer); value exact supported binary scalar; be false for one-byte widths; result Unit; buffer mutated; Pure`. |
| `BufferAppend` | `env[]; child[buffer,data]`; `SourceMutLocal(Buffer,buffer),byte-view; result Unit; data borrowed, buffer mutated; Pure`. |
| `ArrayBuilderNew` | `env[elem]`: exact nonrecursive descriptor `Scalar(S)` or `Aggregate(Vec(S,N) | Mask(S,N) | FixedArray(S,N) | FixedStructArray(id,N))`. `child[region?]`; with no region, admit exactly primitive Copy scalars, String, or `Scalar(Struct(id))` with `HTR(id)`; with a region, require the descriptor's concrete type to be recursively `RegionPlain`. Result `ArrayBuilder(elem)`; new owned heap allocation or explicitly region-owned allocation; Pure. An unknown/malformed struct/tagged/array id or closed-predicate failure rejects before MIR allocation. |
| `ArrayBuilderPush` | `env[moves_value]`; `child[builder,value]`; `SourceMutLocal(ArrayBuilder(elem),builder), value exact `elem.ty()`; `moves_value` iff `elem == Scalar(String)` or `elem == Scalar(Struct(id)) && HTRMove(id)`; result Unit. A true bit consumes and nulls the complete source, a false bit copies the producer-valid value with any region provenance, and the builder is mutated. A wrong bit or malformed recursive record graph rejects before MIR ownership transfer. |
| `ArrayBuilderAppend` | `env[]; child[builder,data]`; descriptor must be `Scalar(copy elem)`, `SourceMutLocal(ArrayBuilder(elem),builder), data Slice(elem)`; result Unit; data borrowed, builder mutated; Pure. Aggregate descriptors use `push`. |
| `ArrayBuilderBuild` | `env[]; child[builder]`; `ArrayBuilder(elem)`, consume-any; result `DynStructArray(id,Aos)` for `Scalar(Struct(id))` with the exact same valid id, `DynArray(S)` for every other scalar descriptor, or `DynAggregateArray(elem)` for an aggregate descriptor; transfer the complete producer-valid builder buffer once; Pure. A mismatched/malformed result id rejects before MIR transfer. |
| `FsWriteFile` | `env[builder]`; `child[path,data]`; `path Str; builder=false requires byte-view, true requires Builder; result ERR(Unit); both borrowed; Impure`. |
| `FsExists` | `env[]; child[path]`; `Str; result Bool; borrowed; Impure`. |
| `FsRemove` | `env[]; child[path]`; `Str; result ERR(Unit); borrowed; Impure`. |
| `FsReadDir` | `env[]; child[path]`; `Str; result ERR(DynArray(String)); new deep-owned string array; Impure`. |
| `DnsResolve` | `env[]; child[host]`; `Str; result ERR(DynArray(String)); new deep-owned string array; Impure`. |
| `TcpConnect` | `env[]; child[host,port]`; `Str,i64; result ERR(TcpConn); new owned socket; Impure`. |
| `ConnReader` | `env[]; child[conn]`; `TcpConn; result Reader; new reader handle borrows conn fd and inherits conn provenance; Pure`. |
| `ConnWriter` | `env[]; child[conn]`; `TcpConn; result Writer; new writer handle borrows conn fd and inherits conn provenance; Pure`. |
| `TcpReadTimeout` | `env[]; child[conn,ns]`; `TcpConn,i64; result Unit; conn borrowed and native socket state mutated; Impure`. |
| `TcpWriteTimeout` | `env[]; child[conn,ns]`; `TcpConn,i64; result Unit; conn borrowed and native socket state mutated; Impure`. |
| `TcpListen` | `env[]; child[host,port]`; `Str,i64; result ERR(TcpListener); new owned socket; Impure`. |
| `TcpAccept` | `env[]; child[listener]`; `TcpListener; result ERR(TcpConn); listener borrowed, result owns accepted socket; Impure`. |
| `UdpBind` | `env[]; child[host,port]`; `Str,i64; result ERR(UdpSocket); new owned socket; Impure`. |
| `UdpSendTo` | `env[]; child[sock,data,host,port]`; `UdpSocket,byte-view,Str,i64; result ERR(i64); all borrowed; Impure`. |
| `UdpRecvFrom` | `env[]; child[sock,buffer]`; `LocalHandle(UdpSocket,sock), SourceMutLocal(Buffer,buffer); result ERR(i64); both borrowed, buffer mutated; Impure`. |
| `FsReadFileView` | `env[]; child[path]`; `Str; inside arena; result ERR(Str); view rooted in arena; Impure`. |
| `FsReadBytesView` | `env[]; child[path]`; `Str; inside arena; result ERR(bytes); view rooted in arena; Impure`. |
| `PathJoin` | `env[]; child[a,b]`; `Str,Str; result String; new owned string; Pure`. |
| `PathComponent` | `env[kind]`; `child[path]`; `Str; result Str; view inherits path provenance; Pure`. |
| `PathNormalize` | `env[]; child[path]`; `Str; result String; new owned string; Pure`. |
| `EnvGet` | `env[]; child[name]`; `Str; result Option<String>; Some owns a fresh string; Impure`. |
| `EnvSet` | `env[]; child[name,value]`; `Str,Str; result ERR(Unit); borrowed; Impure`. |
| `TimeNow` | `env[]; child[]; result i64; Impure`. |
| `TimeInstant` | `env[]; child[]; result i64; Impure`. |
| `ProcessCpuCount` | `env[]; child[]; result i64; Impure`. |
| `TimeSleep` | `env[]; child[ns]`; `i64; result Unit; Impure`. |
| `ProcessExit` | `env[]; child[code]`; `i64; result Unit; Impure and non-fallthrough; current-frame cleanup exactly once before exit`. |
| `ProcessAbort` | `env[]; child[]; result Unit; Impure and non-fallthrough; no cleanup`. |
| `ProcessSpawn` | `env[]; child[cmd,args]`; `Str,argv; result ERR(Child); operands borrowed, result owns child pid; Impure`. |
| `ChildWait` | `env[]; child[child]`; `Child; result ERR(i64); borrowed handle marks itself reaped; Impure`. |
| `ChildKill` | `env[]; child[child,sig]`; `Child,i64; result ERR(Unit); borrowed; Impure`. |
| `ProcessExec` | `env[]; child[cmd,args]`; `Str,argv; result ERR(Unit); borrowed; success non-fallthrough without cleanup, error falls through; Impure`. |
| `ProcessCommand` | `env[]; child[cmd,args]`; `Str,argv; result Command; new owned builder; operands copied/borrowed; Impure by producer classification`. |
| `CommandCwd` | `env[]; child[command,dir]`; `LocalHandle(Command,command),Str; result Unit; command native state mutated without source mut, dir borrowed; Impure`. |
| `CommandTimeout` | `env[]; child[command,ns]`; `LocalHandle(Command,command),i64; result Unit; command native state mutated without source mut; Impure`. |
| `CommandEnv` | `env[]; child[command,name,value]`; `LocalHandle(Command,command),Str,Str; result Unit; command native state mutated without source mut, strings borrowed; Impure`. |
| `CommandEnvClear` | `env[]; child[command]`; `LocalHandle(Command,command); result Unit; command native state mutated without source mut; Impure`. |
| `CommandRun` | `env[]; child[command]`; `Command; result ERR(RunOutput); command borrowed, result newly owned; Impure`. |
| `RunOutputCode` | `env[]; child[out]`; `RunOutput; result i64; borrowed; Pure`. |
| `RunOutputStdout` | `env[]; child[out]`; `RunOutput; result Str; view inherits out provenance; Pure`. |
| `RunOutputStderr` | `env[]; child[out]`; `RunOutput; result Str; view inherits out provenance; Pure`. |
| `CommandMaxCapture` *(Request 11)* | `env[]; child[command,limit]`; `LocalHandle(Command,command),i64; result Unit; command native state mutated without source mut; Impure`. A negative runtime value is producer-valid HIR and aborts in the runtime before allocation/fork; malformed stored child/result types reject before lowering. |
| `CommandRunBytes` *(Request 11)* | `env[]; child[command]`; `LocalHandle(Command,command); result ERR(RunBytes); command borrowed, result newly owned; Impure`. |
| `RunBytesCode` *(Request 11)* | `env[]; child[out]`; `LocalHandle(RunBytes,out); result i64; borrowed; Pure`. |
| `RunBytesStdout` *(Request 11)* | `env[]; child[out]`; `LocalHandle(RunBytes,out); result bytes; view inherits out provenance; Pure`. |
| `RunBytesStderr` *(Request 11)* | `env[]; child[out]`; `LocalHandle(RunBytes,out); result bytes; view inherits out provenance; Pure`. |
| `EncodingEncode` | `env[kind]`; `child[data]`; `byte-view; result String; fresh owned output; Pure`. |
| `EncodingDecode` | `env[kind]`; `child[input]`; `Str; result ERR(Buffer); fresh owned output on success; Pure`. |
| `Utf8Valid` | `env[]; child[data]`; `bytes; result Bool; borrowed; Pure`. |
| `Compress` | `env[kind]`; `child[data,level]`; `byte-view,i64; result ERR(Buffer); fresh owned output; Impure`. |
| `Decompress` | `env[kind]`; `child[data]`; `byte-view; result ERR(Buffer); fresh owned output; Impure`. |
| `RandSeed` | `env[]; child[]; result Rng; Copy mutable state; Impure`. |
| `RandSeedWith` | `env[]; child[seed]`; `i64; result Rng; Copy mutable state; Impure`. |
| `RandNext` | `env[]; child[rng]`; `SourceMutLocal(Rng,rng); result i64; rng mutated; Impure`. |
| `RandRange` | `env[]; child[rng,lo,hi]`; `SourceMutLocal(Rng,rng),i64,i64; result i64; rng mutated; Impure`. |
| `RandShuffle` | `env[elem]`: admitted Copy scalar. `child[rng,xs]`; `SourceMutLocal(Rng,rng), producer-writable Slice(elem) local; result Unit; both mutated; Impure`. |
| `RandSample` | `env[elem]`: admitted Copy scalar. `child[rng,xs,k]`; `SourceMutLocal(Rng,rng),Slice(elem),i64; result DynArray(elem); rng mutated, xs borrowed, fresh owned output; Impure`. |
| `RegexCompile` | `env[]; child[pattern]`; `Str; result ERR(Regex); new owned compiled handle; Pure`. |
| `RegexIsMatch` | `env[]; child[regex,text]`; `Regex,Str; result Bool; borrowed; Pure`. |
| `RegexFind` | `env[start presence]`; `child[regex,text,start if present]`; `Regex,Str,start i64; result Option<Struct(regex_match_id)>; borrowed; Pure`. |
| `RegexFindAll` | `env[]; child[regex,text]`; `Regex,Str; result DynStructArray(regex_match_id); fresh owned Copy-struct array; Pure`. |
| `RegexSplit` | `env[]; child[regex,text]`; `Regex,Str; result DynStructArray(regex_match_id); fresh owned Copy-struct array; Pure`. |
| `RegexReplace` | `env[all]`; `child[regex,text,repl]`; `Regex,Str,Str; result String; fresh owned output; Pure`. |
| `RegexCaptures` | `env[]; child[regex,text]`; `Regex,Str; result Option<Captures>; operands are borrowed during the call, while a successful handle owns only copied capture spans and retains no text provenance; Pure`. |
| `RegexGroupCount` | `env[]; child[regex]`; `Regex; result i64; borrowed; Pure`. |
| `RegexGroupIndex` | `env[]; child[regex,name]`; `Regex,Str; result Option<i64>; borrowed; Pure`. |
| `CapturesGroup` | `env[]; child[caps,index]`; `Captures,i64; result Option<Struct(regex_match_id)>; borrowed; Pure`. |
| `CliCommand` | `env[]; child[name]`; `Str; result CliCommand; new owned builder; name borrowed/copied; Pure`. |
| `CliFlag` | `env[kind,default presence]`; `child[cmd,name,default if present]`; `LocalHandle(CliCommand,cmd),Str; Bool requires no default, I64 requires i64 default, Str requires Str default; result Unit; cmd native state mutated without source mut; Pure`. |
| `CliParse` | `env[]; child[cmd,args]`; `CliCommand,argv; result ERR(CliParsed); operands borrowed, result newly owned; Pure`. |
| `CliGetBool` | `env[]; child[parsed,name]`; `CliParsed,Str; result Bool; borrowed; Pure`. |
| `CliGetI64` | `env[]; child[parsed,name]`; `CliParsed,Str; result i64; borrowed; Pure`. |
| `CliGetStr` | `env[]; child[parsed,name]`; `CliParsed,Str; result Str; view inherits parsed provenance; Pure`. |
| `CliUsage` | `env[]; child[cmd]`; `CliCommand; result String; fresh owned output; Pure`. |
| `HttpRequest` | `env[]; child[method,url]`; `Str,Str; result HttpRequest; new owned builder, inputs copied; Pure`. |
| `HttpHeader` | `env[]; child[req,name,value]`; `LocalHandle(HttpRequest,req),Str,Str; result Unit; req native state mutated without source mut, strings borrowed; Pure`. |
| `HttpBody` | `env[]; child[req,data]`; `LocalHandle(HttpRequest,req),byte-view; result Unit; req native state mutated without source mut, bytes copied; Pure`. |
| `HttpRequestTimeout` | `env[]; child[req,ns]`; `LocalHandle(HttpRequest,req),i64; result Unit; req native state mutated without source mut; Pure`. |
| `HttpParse` | `env[]; child[data]`; `byte-view; result ERR(HttpResponse); fresh owned response copy; Pure`. |
| `HttpRespStatus` | `env[]; child[resp]`; `HttpResponse; result i64; borrowed; Pure`. |
| `HttpRespHeader` | `env[]; child[resp,name]`; `HttpResponse,Str; result Option<Str>; view inherits resp provenance; Pure`. |
| `HttpRespBody` | `env[]; child[resp]`; `HttpResponse; result bytes; view inherits resp provenance; Pure`. |
| `HttpClient` | `env[]; child[]; result HttpClient; new owned handle; Pure construction`. |
| `HttpClientTimeout` | `env[]; child[client,ns]`; `LocalHandle(HttpClient,client),i64; result Unit; client native state mutated without source mut; Pure`. |
| `HttpClientGet` | `env[]; child[client,url]`; `HttpClient,Str; result ERR(HttpResponse); borrowed client/url, fresh owned response; Impure`. |
| `HttpClientPost` | `env[]; child[client,url,body]`; `HttpClient,Str,byte-view; result ERR(HttpResponse); operands borrowed, fresh owned response; Impure`. |
| `HttpClientRequest` | `env[]; child[client,req]`; `HttpClient,HttpRequest; result ERR(HttpResponse); client borrowed, req consumed once, fresh response; Impure`. |
| `HttpGetMany` | `env[]; child[client,urls,max_concurrency]`; `HttpClient,Slice(Str),i64; result ERR(DynResponseArray); inputs borrowed, deep-owned response array; Impure`. |
| `HttpServe` | `env[shared]`; `child[host,port]`; `Str,i64; result ERR(HttpServer); shared selects SO_REUSEPORT only; new owned listener; Impure`. |
| `HttpAccept` | `env[]; child[server]`; `HttpServer; result ERR(HttpRequestCtx); server borrowed, result owns accepted fd/request; Impure`. |
| `HttpCtxMethod` | `env[]; child[ctx]`; `HttpRequestCtx; result Str; view inherits ctx provenance; Pure`. |
| `HttpCtxPath` | `env[]; child[ctx]`; `HttpRequestCtx; result Str; view inherits ctx provenance; Pure`. |
| `HttpCtxHeaders` | `env[]; child[ctx]`; `HttpRequestCtx; result HttpHeaders; Copy view is Frame-capped to ctx provenance; Pure`. |
| `HttpCtxHeader` | `env[]; child[headers,name]`; `HttpHeaders,Str; result Option<Str>; view inherits headers provenance without additional Frame cap; Pure`. |
| `HttpCtxBody` | `env[]; child[ctx]`; `HttpRequestCtx; result bytes; view inherits ctx provenance; Pure`. |
| `HttpResponseBuilder` | `env[]; child[status]`; `i64; result ResponseBuilder; new owned builder; Pure`. |
| `HttpRbHeader` | `env[]; child[rb,name,value]`; `LocalHandle(ResponseBuilder,rb),Str,Str; result Unit; rb native state mutated without source mut; Pure`. |
| `HttpRbBody` | `env[]; child[rb,data]`; `LocalHandle(ResponseBuilder,rb),byte-view; result Unit; rb native state mutated without source mut, bytes copied; Pure`. |
| `HttpRespond` | `env[]; child[ctx,rb]`; `HttpRequestCtx,ResponseBuilder; result ERR(Unit); both consumed exactly once on the runtime call; Impure`. |
| `HttpRespondStream` | `env[]; child[ctx,rb]`; `HttpRequestCtx,ResponseBuilder; result ERR(HttpStream); ctx borrowed then marked spent on success, rb consumed; stream owns lifted fd; Impure`. |
| `HttpStreamSend` | `env[event]`; `child[stream,chunk]`; `LocalHandle(HttpStream,stream),byte-view; result ERR(Unit); stream native state mutated without source mut, chunk borrowed; Impure`. |
| `HttpStreamFinish` | `env[]; child[stream]`; `HttpStream; result ERR(Unit); stream consumed exactly once; Impure`. |
| `HttpStreamReject` | `env[]; child[stream,rb]`; `HttpStream,ResponseBuilder; result ERR(Unit); both consumed exactly once; Impure`. |
| `CryptoCtEqual` | `env[]; child[a,b]`; `byte-view,byte-view; result Bool; borrowed; Pure constant-time-content comparison`. |
| `CryptoRandom` | `env[]; child[out]`; `SourceMutLocal(Buffer,out); result Unit; buffer mutated; Impure`. |
| `CryptoHash` | `env[algo]`; `child[data]`; `byte-view; result DynArray(u8); fresh owned output, exact runtime length 32/64 by algo; Impure`. |
| `CryptoHmac` | `env[]; child[key,data]`; `byte-view,byte-view; result DynArray(u8) with exact length 32; fresh owned output; Impure`. |
| `CryptoHkdf` | `env[]; child[salt,ikm,info,len]`; `byte-view,byte-view,byte-view,i64; result ERR(Buffer); all borrowed, fresh owned output; Impure`. |
| `CryptoAead` | `env[cipher,dir]`; `child[key,nonce,input,aad]`; all byte-view; `result ERR(Buffer); all borrowed; cipher fixes 32-byte key/12-byte nonce/16-byte tag contract; dir selects seal/open and all-or-nothing open; fresh owned success output; Impure`. |
| `CryptoArgon2` | `env[]; child[password,salt,params]`; `byte-view,byte-view,Struct(argon2_params_id)` with exact four ordered i64 fields; result ERR(Buffer); all borrowed, fresh owned output; Impure`. |
| `CryptoPrivateKeyFromPem` | `env[algorithm]`; `child[pem]`; `Str; result ERR(SignatureKey(algorithm,Private)); pem borrowed, fresh validated independent Move shell owner on success; shell pins an isolated built-in default provider; Ed25519 validates the derived public point independently; Impure`. |
| `CryptoPublicKeyFromPem` | `env[algorithm]`; `child[pem]`; `Str; result ERR(SignatureKey(algorithm,Public)); pem borrowed, fresh validated independent Move shell owner on success; shell pins an isolated built-in default provider; Ed25519 validates the encoded point independently; Impure`. |
| `CryptoPublicKeyFromJwk` | `env[algorithm]`; `child[first,second?]`; RS256/ES256 require exactly two byte views and Ed25519 exactly one; `result ERR(SignatureKey(algorithm,Public)); inputs borrowed, fresh validated independent Move shell owner with pinned provider; Ed25519 canonical/on-curve/non-small-order validation is wrapper-owned; Impure`. |
| `CryptoSign` | `env[algorithm]`; `child[key,message]`; exact shared-borrow place `SignatureKey(algorithm,Private)`, byte-view; `result ERR(Buffer); key/message borrowed, fresh exact-width signature on success; Impure`. |
| `CryptoVerify` | `env[algorithm]`; `child[key,message,signature]`; exact shared-borrow place `SignatureKey(algorithm,Public)`, byte-view, byte-view; `result ERR(Bool); all borrowed; every signature length/encoding/mathematical mismatch after valid views is Ok(false), malformed ABI/key is Invalid, engine failure is Err; Impure`. |

### am-b3 helper discriminators and classifiers

| Discriminator | Exact contract |
|---|---|
| `AeadCipher::Aes256Gcm` | Selects AES-256-GCM; 32-byte key, 12-byte nonce, 16-byte tag. |
| `AeadCipher::ChaCha20Poly1305` | Selects ChaCha20-Poly1305; the same key/nonce/tag widths. |
| `AeadDir::Seal` | Input is plaintext; output is ciphertext followed by tag. |
| `AeadDir::Open` | Input is ciphertext followed by tag; authentication failure releases no plaintext. |
| `HashAlgo::Sha256` | Exact digest length 32 and runtime key `crypto_sha256`. |
| `HashAlgo::Sha512` | Exact digest length 64 and runtime key `crypto_sha512`. |
| `SignatureAlgorithm::Rs256` | Stable byte 0; RSASSA-PKCS1-v1_5 with SHA-256; private/public key kinds 0/1. |
| `SignatureAlgorithm::Es256` | Stable byte 1; P-256 ECDSA with SHA-256 and raw 64-byte `r || s`; private/public key kinds 2/3. |
| `SignatureAlgorithm::Ed25519` | Stable byte 2; pure Ed25519 with no digest/context/prehash; private/public key kinds 4/5. |
| `signature_key_carrier_class` | One recursive, cycle-safe owner admits local/by-value/return/shared-borrow and struct/sum/Option/Result paths, including AoS Move-struct arrays whose recursive Drop reaches the key. It rejects direct/tagged/sum key collection elements, tuple/box, closure/task/parallel capture, `out`/`borrow mut`, global/constant/native/`layout(C)`, and print/equality/order/hash. Unknown future carrier and kind reject. |
| `CliFlagKind::Bool` | No default child; parsed result Bool. |
| `CliFlagKind::I64` | Exactly one i64 default child; parsed result i64. |
| `CliFlagKind::Str` | Exactly one Str default child; parsed result Str view. |
| `EncodingKind::Base64` | Standard padded base64 encode/decode runtime pair. |
| `EncodingKind::Base64Url` | URL-safe unpadded/padded-accepted pair exactly as the existing runtime contract. |
| `EncodingKind::Hex` | Lowercase hex encode and exact existing case-accepting decode contract. |
| `EncodingKind::Percent` | RFC 3986 component percent encoding; uppercase hex on encode, either case on decode. |
| `EncodingKind::Form` | Form component encoding: space maps to `+`; decode reverses `+` and percent escapes. |
| `EncodingKind::Html` | HTML entity escaping is encode-only; `EncodingDecode { kind: Html }` rejects. |
| `CompressKind::Gzip` | gzip framing/runtime pair; level 0 through 9. |
| `CompressKind::Zstd` | zstd framing/runtime pair; compression level is exactly `0..=22`. |
| `PathComponentKind::Base` | Returns final path component view. |
| `PathComponentKind::Dir` | Returns directory-prefix view. |
| `PathComponentKind::Ext` | Returns extension view. |

### Heap-record array-builder producer/validator closure

Request 8 changes the producer-valid domain of three existing rows, so the same capability extends
this authoritative boundary instead of relying on the older scalar/string validator list.

| Cell | Exact closure | Owner evidence |
|---|---|---|
| Valid construction | Heap `ArrayBuilderNew` accepts Copy scalar, `string`, and every admitted Copy/Move `HTR(id)`; region construction retains the existing `RegionPlain` domain. The stored result uses the exact element descriptor. | Request 8 owners extended by the §7.6 formation and recursive graph matrix; generic/per-unit producer owners pass through `lower_program_checked` |
| Malformed construction | Unknown struct/tagged/array id, empty/cyclic/explicit-layout/over-aligned record, every closed excluded field or composite array element, region/heap predicate swap, invalid descriptor, and wrong stored result reject before MIR allocation in all entrypoints. | delegated `HTR` sweep plus New envelope/result mutations and complete-envelope twins |
| Valid push | For each admitted record, exact builder/value types and mutable source-local receiver are required. `moves_value` is false for Copy `HTR`, true for `HTRMove`, and remains true for top-level `string`; selected Move rvalues consume/null the complete source. | Request 8 source matrix parameterized with None/Some and empty/nonempty recursive arrays |
| Malformed push | Wrong/missing record id, wrong builder/value type, immutable/nonlocal receiver, inverted move bit, and malformed reachable definition reject before MIR ownership transfer. Source-expression construction and selected-arm cleanup remain producer-owned, not reconstructed by the boundary. | one-field mutations in `heap_record_array_builder_rows_match_the_producer`; source-shape owners stay in Sema/MIR |
| Valid and malformed build | A record builder consumes any producer-valid owner and returns `DynStructArray(id,Aos)` with the exact same nominal id. Scalar/aggregate results stay unchanged. Wrong layout kind/id/result or malformed reachable record rejects before transfer. | parameterized Build rows in `heap_record_array_builder_rows_match_the_producer` |
| Exact Request 10 consumer graph | The checked-in field-for-field C6 fixture and projected root from §7.6 pass New/Push/Build unchanged; every field deletion, reorder, type mutation, unknown reachable id, or malformed tagged/array edge rejects before MIR. | `request10_exact_c6_consumer_graph` field-vector assertion plus whole/per-unit malformed graph twins |
| Indirect tagged reachability | Lifting direct `array<string>` record-field formation makes that record reachable through existing Option, Result, and user-sum payload positions. Their tagged ids, payload types, move bits, and active-tag Drop remain exact; malformed/inactive payload state is not reinterpreted by the builder boundary. | parameterized Option/Result/user-sum wrapper owner through source validation and all checked-HIR entrypoints |
| Delegation drift | Sema formation and checked-HIR validation call the same `heap_array_builder_record` classifier and canonical recursive ownership predicate. A newly admitted/excluded field kind or Drop rule therefore changes both domains in one owner. | helper-domain sweep plus whole/per-unit producer HIR passed through `lower_program_checked` |

### Request 11 process-capture activation delta

The five Request 11 expression rows above activated together with `Ty::RunBytes` /
`Scalar::RunBytes`; there is no producer-only intermediate. Its activation brought the `ExprKind`
inventory to 258, and
`variant_sweep_tripwire` must fail at compile time
if any of those five variants is absent from validation or ownership analysis.

| Cell | Exact closure | Owner evidence |
|---|---|---|
| Valid producer envelope | Sema emits each reserved row with exactly the child/result/effect relation written above; `CommandMaxCapture` and `CommandRunBytes` require a bound local `Command`, and all three accessors require a bound local `RunBytes`. | one accepted source fixture per discriminator in `request11_process_rows_match_the_producer`, passed through every checked lowering entrypoint |
| One-field malformed mutations | For each row, mutate every child type, the stored result type, child count/order, receiver locality, and receiver handle kind independently. Validation rejects before MIR allocation, runtime declaration, ownership transfer, or cache publication. | parameterized complete-envelope mutations in `request11_process_rows_match_the_producer` |
| Diagnostic precedence | A malformed child expression is visited before the receiver/result post relation; an equal-typed temporary receiver rejects only after its child validates. Negative `limit` is not a HIR-malformation discriminator and remains the runtime programmer-error abort. | accepted-local/temporary twins plus an invalid later-child twin in the same owner |
| Closed Move classification | `RunBytes` is owned and dropped exactly like `RunOutput`: it is permitted only in its `Result` Ok carrier/local flow, nulls on move, is excluded from aggregates, arrays, task captures, and Copy/region-plain domains, and its byte views inherit only its owner region. | `run_bytes_type_classification_tripwire` sweeps Copy/Move/Drop/region/aggregate/capture/return and every control-flow cleanup owner |
| Canonical/interface parity | Whole-program and imported/per-unit checked HIR resolve source `run_bytes` to the same closed type. Canonical type codec v3 tags are exactly `Ty=60` and `Scalar=36`; current interface format 8 preserves the named-type tag 0/path `run_bytes`/zero args before the recursive owned-JSON graph-list position. | bidirectional golden and unknown/truncated-tag owners named in the process design, plus exact edit/revert unit-cache identity |

## Inventory closure

The implementation must derive an exhaustiveness constant from the Rust enum
definitions and assert that this file has exactly one owner id for every
`Stmt`, all 266 `ExprKind` variants, `ArithMode`, `MathFn`, every
`BuilderWriteKind`, `StrPredKind`, `StrTrimKind`, `TemplatePart`, `StageKind`,
`GroupSource`, `GroupAgg1`, `GroupOp`, `CliFlagKind`, `EncodingKind`, `CompressKind`,
`PathComponentKind`, `AeadCipher`, `AeadDir`, and `HashAlgo`. The test fails on
an added, removed, duplicated, or unowned discriminator.

Request 11 changed the asserted `ExprKind` total from 253 to 258 in the same integrated tree that
already contains `JsonEncodeBounded`, and activated its five rows.

### Request 5 bounded-HTTP activation delta

Request 5 activates both setters with the bounded decoder; neither discriminator may ship as a
dormant producer. The implementation adds exactly these two rows and changes the asserted current
`ExprKind` total from 258 to 260 in that integrated tree:

| `ExprKind` | Exact producer and checked-HIR envelope |
|---|---|
| `HttpRequestMaxResponseBodyBytes` | `env[]; child[req,limit]`; `LocalHandle(HttpRequest,req),i64; result Unit; req native state mutated without source mut; Pure`. |
| `HttpClientMaxResponseBodyBytes` | `env[]; child[client,limit]`; `LocalHandle(HttpClient,client),i64; result Unit; client native state mutated without source mut; Pure`. |

Each row requires exact child order, handle kind, bound-local receiver, `i64` limit, Unit result,
and Pure effect. Complete-envelope mutation owners reject a temporary or wrong handle, wrong limit
or result type, reordered/missing children, and malformed nested children before MIR allocation or
runtime declaration. Negative and above-global values are producer-valid HIR and remain runtime
programmer-error aborts before storage or network work. Whole-program and imported/per-unit lowering
must emit the same row and interface/cache spelling.

## Producer-delegation closure matrix

This matrix closes the **silent-empty-MIR** class: the body validator answering
a question the producer already owns, with a rule that is not equivalent. Before
`#774`, each occurrence rejected the whole checked program into the canonical
empty program and an undefined `_main`; the fallible production boundary now
reports the refusing validator and function instead. Seven earlier occurrences
shipped through `#742`, `#744`, `#749`, `#737`, and `#774`; the owned-`string`
clone row below is the eighth. Every one was "the validator re-derived a producer
fact", never malformed HIR.

One reachability argument is banned outright when judging these gates: "a Move
element collection cannot be constructed." A `slice<Move>` **type** is declarable
and passable even though no expression builds such a **value**, and the body
validator validates every function body. See "Axis A (continued) — the reachable
`slice<Move>` parameter domain" for the six gates that argument had wrongly
cleared.

### Split model

A nominal id is not a stable name for a source type. `check_program` caches
monomorph instances by *mangled name* (`struct_mono`, `enum_mono`,
`resource_mono`), and `intern_fn_type` compares the whole `FnTy` including its
mutable `effect` cell, so one source type can own several ids:

```text
struct[4] = Wrap$F0_vi64_i64_bn_rn  source_name Wrap$F_vi64_i64_bn_rn  callback: Fn(0)
struct[6] = Wrap$F3_vi64_i64_bn_rn  source_name Wrap$F_vi64_i64_bn_rn  callback: Fn(3)
```

Sema treats those as one type (`source_scalar_matches`). The validator's
equivalent is `body_ty_matches` / `body_scalar_matches`, which compare through
`source_shape_equal`. A **raw** `==`/`!=` on two nominal ids is therefore only
correct when the two operands cannot be independent derivations of one source
type. The duplicate ids themselves are a separate (non-blocking) producer
inefficiency; see "fn_types interning" below.

### Axis A — validator-owned admission gates spelled `scalar_to_prim`

Eight occurrences; one diverged.

| Gate | Producer authority | Verdict |
|---|---|---|
| `ArrayScan` output element | `check_array_scan` | **Divergent.** Sema admits any non-struct `ty_to_scalar` accumulator; `scalar_to_prim` rejected `()` and every sum-type accumulator. Fixed by delegating to `align_sema::scan_accumulator_scalar`, now called by both. |
| `rng_elem_ok` (`shuffle`/`sample`) | `check_rand_method` | **Divergent.** The `scalar_to_prim` half is identical, but `rng_elem_ok` *also* asks `scalar_copy_ok` and sema did not. See the Move-value gate below. |
| `ArraySortBy` element | `check_array_sort_by_key` | Identical rule. No change. |
| `ArrayPartition` element | `check_array_partition` | Identical rule. No change. |
| `ArrayChunks` element | `check_array_chunks` | Identical `scalar_to_prim` rule, but the validator *also* asks `scalar_copy_ok`. See the Move-value gate below. |
| `array_builder_elem_ok` | `check_array_builder_new` / `heap_array_builder_record` | **Delegated exact rule.** Heap scalar/string and `HTR(id)` admission plus region-backed `RegionPlain` admission use the producer-owned classifiers; the validator must not retain an older wider/narrower list. Requests 8 and 10 own parameterized valid/malformed New/Push/Build rows. |
| `ResourceViewFromRaw` view scalar (2 sites) | none | Structural: `Scalar::Slice` is *constructed from* a `PrimScalar`, so this is a representation conversion, not an admission rule. No change. |

Two further admission gates diverged without being spelled `scalar_to_prim`:

| Gate | Producer authority | Verdict |
|---|---|---|
| `orderable_body_ty` (`sort_by_key` key) | `Bound::Ord` | **Divergent.** Sema's bound accepts owned `string`; the validator re-listed the arms and stopped at `str`. Both now call `align_sema::ord_body_ty`. |
| `StrClone` receiver | `check_box_clone` | **Divergent.** Sema lowers both borrowed `str` and owned `string` receivers to `StrClone`; the validator admitted only `str`. Both now call `align_sema::str_clone_body_ty`. |

The owned-receiver repair also closes the lowering boundary exposed by admitting the producer's
complete contract:

| Cell | Required behavior | Owner |
|---|---|---|
| Construction and malformed HIR | Sema and the body validator share the exact `Str`/`String` receiver gate; every other receiver remains rejected before MIR. | `align_driver::m5::owned_string_clone_duplicates_locals_and_fields`; existing malformed-HIR validator owners |
| Bound local and field | Clone borrows the place, leaves its ownership bit and storage unchanged, and creates one independently owned result. | `align_driver::m5::owned_string_clone_duplicates_locals_and_fields` |
| Fresh receiver and cleanup | A fresh `string` moves into a hidden owner, stays live through `Rvalue::StrClone`, and is dropped immediately after the copy, including on each loop iteration. | `align_driver::m5::owned_string_clone_duplicates_locals_and_fields` (MIR owner plus executable result) |
| Value-carrying control flow | `if`/`match`/`else` and transparent scopes lower once in borrow mode; the selected bound arm is not moved or nulled, while a fresh selected arm retains its path-local cleanup bit. | `align_driver::m5::owned_string_clone_duplicates_locals_and_fields`; shared `lower_expr_for_borrow` owners |
| Early exit | Receiver evaluation that terminates does not emit a clone; the pre-registered synthetic owner participates in existing exit cleanup. | shared `lower_borrowed_owned` early-exit owners |
| Whole/per-unit and backend | The unchanged `StrClone` MIR/codegen shape accepts either layout-identical text receiver after checked-HIR validation. | `align_driver::m5::owned_string_clone_duplicates_locals_and_fields`; real align-llm per-unit check |

### Axis A (continued) — the reachable `slice<Move>` parameter domain

**The axis.** An owned `array<string>` can be built and, under plan 30, indexed only as the
non-consuming `str` view; it still cannot be sliced or borrowed as a `slice<string>`, and `string`
cannot be a fixed-array element. Separately, a `slice<Move>` **type** is fully
declarable and passable — `fn take(xs: slice<string>) -> i64 = xs.len()` checks,
and so does forwarding it to another such parameter. `align_driver::struct_index::
a_move_element_slice_type_is_still_declarable_and_passable` pins that on purpose:
over-rejecting the type would remove the whole `slice<E>` surface.

The body validator validates **every** function body, not the reachable ones, so
a never-called function taking a `slice<string>` is enough to reach any gate that
function's body touches. This asymmetry — the type is formable, a value is not —
invalidates the argument "a Move element collection cannot be constructed, so
this gate is unreachable" in every place it might be used. That argument had been
applied to `rng_elem_ok` and `map_into`; both were live internal errors.

**The rule.** The validator refuses a Move value in a *copy position* — one it
buffers, collects, views, rearranges, draws, or writes — through one of two
spellings:

- `ty_copy_ok(ty)`: `body_ty_ok(ty)` **and** not `ty_capture_is_move(ty)`;
- `scalar_copy_ok(scalar)`: not `scalar_is_move(scalar)`.

These are different functions. They agree on a plain scalar, because
`ty_capture_is_move` on a scalar `Ty` reduces to `ty_is_move`, which reduces to
`scalar_is_move`; they differ on tuples and fixed arrays, which cannot occupy any
of these positions. Sema's `reject_move_copy_position` implements the **Move
half** that both share — the `body_ty_ok` half of `ty_copy_ok` is a separate
well-formedness question the producer already answers elsewhere.

Sema already owned the **input** half of the ownership rule
(`reject_move_pipeline_call_arg`, plus `map`'s "cannot produce a Move element"),
which is why `map` / `where` / `any` / `all` / `partition` / `par_map` /
`reduce` / `scan` and the `sort_by_key` *element* all diagnosed correctly. The
copy positions had no producer gate at all:

| Cell | Validator spelling | Symptom before the gate |
|---|---|---|
| `sort_by_key` key | `pipeline_callable_ok`'s `ty_copy_ok(output)` | `Ord` admits owned `string`; a `string` key passed `check` and hit `body_only_metadata_is_valid`. |
| `to_array` element | `ty_copy_ok(final_elem)` | The Move-*struct* arm had a message; the scalar arm admitted any `ty_to_scalar`. |
| `chunks` element | `scalar_copy_ok(source_scalar)` | `scalar_to_prim` admits `String`; the chunk views would alias elements the source still owns. |
| `shuffle` element | `rng_elem_ok`'s `scalar_copy_ok` | Fisher-Yates swaps raw `{ptr,len}` headers through the slice. |
| `sample` element | `rng_elem_ok`'s `scalar_copy_ok` | Worse: the drawn values are copied into a fresh owned `array<T>`, so each `string` is freed twice. |
| `map_into` element | `ArrayMapInto`'s `scalar_copy_ok` | Writing into `dst` overwrites owned headers without dropping them and copies the source's. |

All six now call one shared helper, `Checker::reject_move_copy_position`, which
asks `align_sema::ty_capture_is_move` — the same predicate `ty_copy_ok` asks — so
producer and validator cannot drift again. Only `sort_by_key` names a workaround,
because the key function's return type is the user's to choose; the element rows
deliberately say the capability is deferred rather than invent one.

**Re-check of every other copy gate under this axis.** Each row was retested with
a `slice<string>` parameter, not merely reasoned about:

| Gate | Verdict | Evidence |
|---|---|---|
| `array_literal_element_ok` | Unreachable | The gate itself excludes Move (`!scalar.is_move()`), and sema rejects a Move fixed-array element (`string cannot be an element of a fixed array yet`). |
| `array_builder_elem_ok` | Delegated exact rule | Heap scalar/string/`HTR(id)` and explicit-region `RegionPlain` use the producer-owned classifiers; the Request 8/10 valid/malformed row owners prevent drift. |
| `ArrayBuilderAppend` | Unreachable | The validator refuses `Scalar::String` outright, and sema refuses the method: `'.append()' is not available` for a `string` builder. |
| `ArrayZip` element / tuple source | Symmetric | Sema stops first with `'zip' v1 supports only Copy …`, for `count`, `map_into`, and `to_array` terminals alike. |
| `BoxNew` / `BoxGet` / `BoxClone` | Unreachable | Sema: `a box payload must be a primitive` — `heap.new("a".clone())` never forms a `box<string>`. |
| Struct-array element field path (`ty_copy_ok(leaf)`) | Symmetric | Sema rejects both spellings: `reading a Move-type field` and `'arr[i].xs' needs a …`. An owned `string` field is demoted to `str` on both sides. |
| `array_to_array_result` struct arm | Symmetric | `to_array`'s own Move-struct arm rejects it first. |
| Lifted / fn-value capture (`ty_copy_ok(flow.ty)`) | Symmetric | Sema asks the identical `ty_capture_is_move` when lifting a lambda; a `slice<string>` capture is Copy on both sides and still builds. |
| `sort` / `sum` / `min` / `max` element | Symmetric | Rejected earlier as non-numeric. |
| Index / slice-range of a Move collection | Exact exception plus symmetric rejection | `DynArray(String) -> Str` is the plan-30 exception. Slice range still reports `slicing a collection of the Move type string`; every other ordinary Move-element index reports its existing unsupported-Move diagnostic. |

### `AssignIndex` store closure

`AssignIndex` is the write-side sibling of the element-read rows above. The source checker used
`scalar_to_prim(...).is_some()` as its admission rule, which includes both borrowed `str` and owned
`string`; the body validator independently accepted only integers, floats, bool, and char. The
split rejected the settled `str` replacement path at the MIR boundary, while merely adding `str`
to the validator would leave the source checker able to form an owned-`string` overwrite whose raw
store has no drop-old or source-nulling operation.

| Closure axis | Exact rule | Owner evidence |
|---|---|---|
| Formation and validation | Fixed scalar arrays, dynamic scalar arrays, and writable slices admit integer, float, bool, char, and borrowed `str` element stores through one sema-owned `indexed_element_store_ok` predicate. Owned `string` and every other Move/non-scalar element reject before HIR formation. The body validator delegates the same class rule and separately checks stored integer/float widths, mutability, index type, and exact value type. | fixed/dynamic/slice frontend-to-MIR positives; delegated constructible-scalar/malformed-width sweep; source and forged-HIR Move-`string` negatives |
| Construction and replacement | Index and RHS evaluate in source order. A terminating child emits no bounds check or store. Fixed arrays use the existing checked `StoreIndex`; dynamic arrays and writable slices use the existing checked pointer store. A `str` replacement copies only its view header, and EscapeCheck requires the incoming view to outlive the destination storage. | existing continuation twins and arena-to-outer-element rejection; `return_provenance::{borrowed_str_element_stores_run_for_fixed_dynamic_and_slice_bases,product_projection_summaries_select_exact_parameter_roots}` |
| Ownership lifecycle | Every admitted value is Copy: there is no move-in, move-out, source nulling, per-element Drop, replacement Drop, or return cleanup bit. A Move value is rejected on both the source and malformed-HIR paths, so the raw store can never overwrite an owned header. Whole-struct and element-field replacement remain in `AssignElem`/`AssignElemField` and are unchanged. | Move-`string` producer/boundary negative plus Copy-`str` whole/per-unit positive |
| Public/interface boundary | The source surface and ABI do not widen. The already-settled `str` write publishes the exact selected parameter root through whole-program and per-unit summary formation; no projection trie or store classifier is serialized. | exact summary assertions and whole/per-unit verdict parity |

#### Reopened axis: backing storage and `out` retention (2026-08-23)

The first implementation review found that the table above had closed the
element classifier but not the complete storage transition. Two independent
symptoms have one root cause: element stores compared against the previous
*contents* of the base instead of the backing storage lifetime, and mutable
retention considered only `borrow mut` destinations. Merely admitting `out` in
that filter is fail-open. A writable slice may be a local alias or an inline
`ArrayToSlice`/`SliceRange`, so both the lifetime check and the installed borrow
fact must resolve through the same backing-storage state.

This is the reopened implementation closure matrix. It supersedes the earlier
row's shorthand that `state.region[base]` or a destination binding's lexical
depth is the element-store target.

This closure intentionally stays in one capability PR despite exceeding the
roughly 1,000-line review threshold. EscapeCheck's backing lifetime and
MoveCheck's backing owner identity are the two proof halves of the same store:
landing either half alone would leave the admitted `str` write unsound, while
landing a dormant producer first would duplicate the alias/join machinery and
its tests. The one boundary also lets the same direct, aliased, and `out`
owners prove both halves against one candidate SHA.

| Closure axis | Exact rule | Owner evidence |
|---|---|---|
| Destination formation | EscapeCheck and MoveCheck each carry one path-sensitive backing-storage fact for writable fixed arrays, owned dynamic arrays, region-backed dynamic arrays, slice locals, and `borrow mut`/`out` slice parameters. A fixed or definitely individually owned array is rooted at the binding that will release it, including Move into a shorter scope and owned tuple/match bindings; a pure region allocation keeps its exact arena/caller allocation region; a slice copies the roots and storage region of the array/slice expression it views. A fresh SoA allocation owns a new local backing identity, while a Copy SoA alias or proven forwarding call retains the source identity; its materializing pipeline retains element owners but strips the source collection/header generation. An unresolved SoA producer stays unknown but retains every possible source root and never mints the receiving local as a false fresh allocation. `ArrayToSlice`, `SliceRange`, nested ranges, local aliases, straight-line reassignment, reachable branch/match arms, and loop joins all use that same fact; diverging arms contribute no destination. A join unions possible roots and selects the longest-lived possible destination region, because the incoming value must be safe for every reaching backing store. | `indexed_str_store_accepts_frame_and_same_arena_storage`; `indexed_str_store_alias_uses_backing_storage_region`; `indexed_str_store_reassigned_and_joined_aliases_use_reaching_backings`; `indexed_str_store_mixed_region_and_heap_join_keeps_the_longer_backing`; `moved_and_destructured_owned_arrays_use_binding_storage_lifetime`; `indexed_str_store_ignores_diverging_backing_arms`; `materialized_primitive_soa_survives_source_array_reassign`; `soa_alias_reassignment_does_not_cross_publish_contents`; `unknown_soa_call_results_preserve_source_backing_roots` |
| Unknown or non-retaining backing | A region-bearing element/field write through a slice whose backing storage cannot be resolved is fail-closed, including an owner-free static value: the compiler cannot prove which storage or caller transition the write changes. A by-value or shared-borrow slice/soa parameter is likewise caller-backed but has no caller transition: copying it into a `mut` local must not bypass the signature, so every direct or forwarded region-bearing element write through that alias rejects unless the root parameter is `out` or `borrow mut`. `out` and indexed `borrow mut` calls validate the same backing fact as a direct store only when the destination's writable content shape may carry region provenance; scalar-only slices and primitive SoA columns have no retention transition to prove and keep accepting unresolved headers. An unknown or non-retaining region-bearing fact cannot be laundered through a helper. | `indexed_str_store_unknown_backing_fails_closed_for_all_region_bearing_values`; `indexed_str_store_requires_a_retaining_parameter_mode_for_caller_backing`; `forwarded_mutable_str_stores_require_a_retaining_parameter_mode`; `non_borrowing_mutable_calls_do_not_require_region_backing`; existing `out_params` unknown-root/no-alias owners |
| Storage lifetime check | `AssignIndex`, `AssignElemField`, and `AssignElem` compare `retained_contained_region(value)` with the resolved backing storage region, never with the previous content region. Caller storage is `Caller(position)`, frame/arena inline storage uses the owner's declaration region, individually freed dynamic storage uses its owner binding, and pure region storage uses its allocation region. Same-storage-frame and same-arena writes pass; frame/inner-arena values into caller, outer-frame, or outer-arena storage reject. | `indexed_str_store_accepts_frame_and_same_arena_storage`; `indexed_str_store_alias_uses_backing_storage_region`; `out_str_store_rejects_callee_local_and_inner_arena_views`; `out_str_store_rejects_inner_arena_sources_at_the_call_site` |
| Content and observer transition | After every accepted direct element write, the base, every possible backing root, and every already-live alias whose known backing or storage-borrow fact can observe one of those roots retain the incoming contained region and borrow fact. Observers include collection headers nested in each source-reachable aggregate form (struct, tuple, nested `Option`/`Result`, and user sum) rather than only locals whose top-level type is a mutable collection; the tuple owner covers a moved dynamic collection observed through its pre-move slice alias. A scalar element view such as `str` is not itself a collection-header observer and an unrelated slot write cannot retarget it. The region state joins the old and incoming contents. A direct fixed-array store at an exact constant index replaces that element fact; a computed fixed index joins every possible element. Dynamic arrays, slices, reverse aliases, aggregate aliases, and backing roots reached through a range have no shared offset map and conservatively join. Overwriting one exact element therefore need not clear an obsolete owner dependency from another observer until a common index abstraction is designed. | `indexed_str_store_distinguishes_exact_and_conservative_overwrites`; `indexed_and_out_str_stores_keep_source_owners_live_through_backing_roots`; `indexed_str_stores_update_preexisting_and_unresolved_alias_observers`; `indexed_str_store_updates_aggregate_alias_observers`; `scalar_element_views_are_not_collection_alias_observers`; `indexed_str_store_updates_the_contained_region_before_return`; `soa_str_field_store_keeps_the_installed_owner_live` |
| Callee summary | Mutable-retention summaries cover both `borrow mut` and `out` destinations at every normal, explicit `return`, `?`, branch, match, loop-join, and forwarding edge. An `out` element store records the destination's previous compatible content plus the exact stored source roots. Recursive direct-call dependencies converge through the existing finite fixed point. The fact remains analysis-local and is not serialized. | `forwarded_and_branching_out_str_stores_retain_every_possible_source`; `recursive_and_try_exit_out_str_summaries_retain_the_installed_source`; `two_out_str_destinations_snapshot_regions_before_updates` |
| Argument completion snapshots | Each eager argument snapshots four distinct facts when it finishes, before a later argument can rebind the same local: element contents, whole-value retained provenance, reachable storage, and mutable backing. An indirect callee is a fifth frozen operand: its target-relative captures complete before argument zero. Storage/backing snapshots are recorded for every storage-bearing collection even when its element type carries no borrow, and every stable `borrow mut` local/field actual also reserves that exact source place until the call action. Whole-value provenance includes the backing lifetime/owner when a slice header itself is copied into a `borrow mut` whole place or field; an `out` destination's self-source instead denotes its previous elements and may keep its completed header/backing value even when a later argument rebinds the syntactic local. A call-valued argument forms its result from that inner call's completed child facts rather than re-reading the child syntax from the later live state. Before advancing any destination, the call action rejects an earlier `borrow mut` reservation when a later eager argument retargets that place, advances a tracked storage generation, or writes an observable collection backing; it then consumes only the frozen facts and computes all destination updates before applying any of them. Thus a later argument may invalidate an earlier owner and reject the call, but it cannot silently retarget an already-evaluated callee, place, slice header, composite call result, storage root, or selected source. | `mutable_str_retention_uses_argument_completion_snapshots`; `nested_mutable_call_results_keep_completion_backing`; `borrow_mut_places_and_numeric_storage_use_argument_completion_snapshots`; `indirect_call_snapshots_the_callee_before_later_arguments`; `two_out_str_destinations_snapshot_regions_before_updates`; existing `pkg_db_q6::borrow_mut_shaper_retention` fixed/dynamic forwarding owners |
| Parallel-transfer consumer | A direct one-argument transparent call, an eager multi-argument direct call, and an indirect function-value call each translate the callee's worker-transfer summary through completed pre-call argument facts before a mutable action advances or ends any destination generation. The callee body has already reduced its own source order to exact selected parameter/capture roots; the caller must not re-read those selected facts after `borrow mut` invalidation, because ended parameter/storage markers are intentionally absent from the published summary. | `mutable_calls_publish_parallel_transfer_before_advancing_destinations` parameterizes transparent direct, eager direct, and indirect calls |
| Post-call result completion | A mutable call publishes one expression-local post-call completion after all destination transitions. `borrow mut` strongly replaces the selected argument's normalized value, storage, and backing generation; `out` joins installed contents while preserving the pre-call header/backing. The direct or function-value return summary selects those post-call facts. Result value provenance and result allocation storage remain separate: fixed and every owned dynamic collection variant materialize their own result buffer and never inherit a selected argument's allocation fact, while a compatible slice/SoA view forwards selected storage roots. Return-summary backing identity is always unknown because dependency does not prove exact writable-header identity, but every compatible selected backing root remains available to observer propagation. The pending override exists only for mutable calls whose result needs a completion snapshot and is consumed exactly once by that call expression. | `mutable_call_results_use_post_call_completion`; `nested_mutable_call_results_snapshot_post_call_owners`; `mutable_call_owned_dynamic_results_materialize_their_own_storage`; `mutable_call_result_backing_reaches_the_replacement_source`; `unknown_soa_call_results_preserve_source_backing_roots`; `indexed_result_storage_materialization_inventory_is_closed`; existing direct/indirect selected-result owners |
| Dynamic whole-place replacement | Replacing a whole dynamic array through `borrow mut` updates the destination's must-individual and may-individual allocation facts together with its content, storage, backing, and local-storage marker. A heap replacement strongly re-marks the destination as locally owned; a pure caller-region replacement clears obsolete local ownership and selects the caller storage. Fixed-array destinations never retarget inline storage, and projected dynamic owners remain fail-closed until a projection-aware ownership transfer exists. | `borrow_mut_dynamic_array_heap_replacement_re_marks_storage`; `borrow_mut_dynamic_array_region_replacement_clears_local_storage`; existing dynamic-storage locality owners |
| Source-visible mutation actions | One exhaustive `SourceVisibleMutationAction` descriptor classifies every built-in that mutates a source binding, growable buffer/builder, RNG receiver, or caller-visible collection backing. Eager-operand classification, exact-destination self-exemption, and the post-operand action all consume that descriptor rather than maintaining parallel variant lists. The action invalidates any earlier overlapping `borrow mut` place reservation, including writes through a backing alias, but exempts its own exact destination snapshot from self-invalidation. A caller-owned `borrow mut` buffer ends its exact `ParamStorage` generation while preserving provenance contained by the parameter. `http_sse_stream.next` is the one compound storage-and-source action: it advances both the output buffer generation and the stream's committed-ID view, then publishes its nested event completion against the fresh output generation. A builder reallocation advances only its linear header/allocation generation: borrowed owners already copied into its elements remain content provenance, and a region builder's mutable storage stays its constructor-selected region rather than being capped by the header local's lexical frame. Distinct receivers/backings remain valid. Opaque runtime-handle interior changes are not source-place mutations unless their API publishes a view whose validity that mutation changes; raw/native calls cannot carry checked `borrow mut`/`out` modes and remain outside this transition. | storage growth owners `buffer_growth_invalidates_an_existing_view` and `read_line_growth_invalidates_an_existing_buffer_view`; SSE owners `sse_output_views_cannot_survive_buffer_reuse_or_stream_state_change` and the compiled event projection; `rand_receiver_mutation_invalidates_an_earlier_borrow_mut_place`; `mutating_a_distinct_rng_keeps_an_earlier_borrow_mut_place_valid`; `shuffle_through_a_backing_alias_invalidates_an_earlier_borrow_mut_place`; `shuffling_a_distinct_backing_keeps_an_earlier_borrow_mut_place_valid`; `map_into_through_a_backing_alias_invalidates_an_earlier_borrow_mut_place`; `region_builder_mutable_calls_keep_constructor_storage_separate_from_elements`; existing `pkg_db_q6::borrow_mut_shaper_retention` owners |
| Caller transition | `borrow mut` keeps its existing exclusive-generation invalidation and strong header/place replacement semantics. A whole-header replacement updates both the destination's contained provenance and its backing-storage fact from the completed selected source, including unknown/non-retaining state; stale known backing may not survive and launder a later write. Because the analysis-local summary intentionally has no replacement-versus-element-mutation discriminator, an indexed `borrow mut` call also validates the resolved backing lifetime and conservatively joins possible element contents into its backing observers. `out` is element mutation only: it does not replace the slice header, backing fact, or backing allocation generation; it joins returned contents into the actual slice, every resolved backing root, and every existing observer. Inline array coercions, subslices, and local aliases therefore update the original collection rather than only the temporary slice value. | `indexed_and_out_str_stores_keep_source_owners_live_through_backing_roots`; `indexed_str_stores_update_preexisting_and_unresolved_alias_observers`; `borrow_mut_str_store_checks_backing_and_preserves_same_region_control`; `borrow_mut_slice_replacement_updates_backing_storage`; existing whole-place `borrow mut` replacement owners |
| Whole/per-unit fallback | A same-program body selects its exact stored roots. An unavailable imported body conservatively exposes contained and storage provenance from every compatible argument for each mutable destination; the resulting over-rejection is intentional and owned. Both paths still reject a concrete shorter-lived source before it reaches longer-lived caller storage. | `out_str_retention_matches_whole_and_per_unit_checking`; existing imported mutable-retention fallback owners |
| Ownership and cleanup | Borrowed `str` remains Copy: no move-in, source nulling, replacement Drop, or allocation occurs. The added facts only keep its source owner live and reject later owner replacement/Drop while the installed view is reachable. `out` cannot be represented by a function value, so there is no indirect-call cell; `out_param_fn_cannot_be_a_fn_value` remains the compile-time boundary. | `indexed_and_out_str_stores_keep_source_owners_live_through_backing_roots`; `soa_str_field_store_keeps_the_installed_owner_live`; existing function-value rejection |

The implementation uses one backing representation/resolution path in each
analysis and one shared mutable-retention source-selection path for these
cells. It must not add an `out`-only summary, an
element-store-only lifetime list, or a second interface fact. If a future
writable place shape cannot participate in both the storage-region and borrow-
fact halves, its region-bearing store stays rejected until the corresponding
representation in both analyses is extended.

#### Reopened axis: stable storage generations and projected observers (2026-08-24)

The second implementation review found that the first reopened matrix still
identified a backing by its current top-level local. That representation cannot
express two required transitions at once: moving an owned header changes the
binding that releases the allocation without changing the buffer observed by an
already-created slice, while putting that header inside an aggregate or closure
adds a projection rather than a new buffer. The top-level map consequently
masked the move hole with an invalidated-alias diagnostic, skipped aggregate and
closure observers, and lost exact allocation backing at tuple and match payload
bindings.

This matrix supersedes the destination-formation, content-observer, and caller-
transition storage-generation shorthand above; their diagnostics and existing
public retention-summary contracts remain. It reopens one implementation
boundary rather than adding five local exceptions: EscapeCheck and MoveCheck
must carry the same projection topology over their distinct lifetime and borrow
facts. The topology is analysis-local; no public type, ABI, interface summary,
or serialized cache record changes.

The boundary remains one capability PR even though the cumulative diff exceeds
the review-size guideline. A storage generation without projected observers
would leave the closure-return UAF open, while projected observers keyed by the
current owner would either invalidate valid aliases on Move or cross-publish
after source rebinding. Tuple/match projection is the producer half consumed by
the same observer transition, so splitting it would land a dormant and
unprovable fact.

| Closure axis | Exact rule | Owner evidence |
|---|---|---|
| Compile-time classifier closure | One common classifier exhaustively matches every `ExprKind` without a wildcard and returns its storage role (`HeaderProducer`, `HeaderForwarder`, `CarrierProducer`, or `NoStorage`), exact result paths/ordinals, and the required content-initializer family. EscapeCheck and MoveCheck consume that result rather than maintaining sibling inventories. Adding an HIR variant is therefore a compile failure until its storage role is explicit. The runtime unknown/missing-initializer fallback remains only a defensive response to malformed checked HIR or unavailable imported facts; it is not the future-variant mechanism. | `align_sema::tests::storage_generation_expr_variant_sweep` compile-time inventory owner plus malformed-classification controls |
| Non-writable carrier inventory | The non-index-writable carrier inventory is explicit. Direct `ArrayChunks` and every direct materialized, builder-produced, or call-produced `DynSliceArray` own their outer carrier storage under the existing rules but store each contained Slice's stable source-generation dependency in ordinary projected content; direct construction seeds it from the completed source/element fact and calls from the selected summary union. `ArrayDictEncode` records the base `DynStructArray` generation for its dictionary/source slices while its dense-id storage remains independent. View-bearing `DynFixedArray` and `DynFixedStructArray` results seed exact fixed element/field generation dependencies from the completed element fact; primitive-only leaves are empty. `DynVecArray`, `DynMaskArray`, and `DynResponseArray` currently carry no admitted view-bearing element and therefore seed no generation dependency. Moving a carrier or moving a source allocation's release owner preserves these contained dependencies; ending/replacing the source generation invalidates them. No carrier becomes a writable header merely by carrying this fact, and any future writable index surface must first move its type into the header domain. | `return_provenance::storage_generation_nonwritable_carrier_matrix`: chunks direct/materialized/builder/call, dict-encoded source slices, view-bearing fixed-array/fixed-struct-array, primitive vector/mask/response negatives, carrier/source Move, source end, and whole/per-unit call parity |
| Header domain and producer classification | The projected writable-header domain is exactly fixed arrays/AoS (`Array`, `StructArray`), owned dynamic arrays/AoS (`DynArray`, `DynStructArray`), writable views (`Slice`, `Soa`, `SoaParam`), and those leaves reached through struct, tuple, `Option`, `Result`, user-sum, or first-class callable captures. Buffers/builders and specialized non-index-writable `DynSliceArray`/vector/mask/fixed-array/response collections retain their existing release and mutation rules; adding an indexed-write surface to one must first add it to this domain. A contained view in a non-writable carrier still records its source as a stable generation dependency under the exhaustive carrier rule above, so moving the source allocation does not invalidate the carrier merely because its release place changed. `FsReadBytesView` is a deliberate type-level exception: although its Ok payload is `Slice<u8>`, the mmap view and every forwarded alias/range are read-only, so it retains only the existing arena non-storage lifetime and any forged writable use rejects before generation state changes. Local/field/tuple/fixed-index/range/unwrap/block/control expressions forward or project; `ArrayToSlice` forwards the exact fixed/dynamic source generation and `SliceRange` keeps that same generation. Aggregate constructors prefix child facts. An unbound fixed array gets a hidden producer-site inline-temporary origin and rebases only when copied/moved into a canonical place. Any other expression with an owned in-domain leaf materializes a fresh producer-site/result-path origin (including build, materializing pipelines, sampling, JSON decode, group aggregation, and owned direct/indirect/raw call results). A call-produced view leaf is always unknown but retains every candidate generation/fallback root selected by the existing flattened return summary; `ArrayToSoa`, `JsonDecodeSoa`, direct `CloneIn`, and `JsonDocElems` are the fresh known view-storage exceptions. `ArrayPartition` uses tuple paths 0/1, `ArrayGroupAgg` paths 0/1, and `ArrayGroupAggMulti` key path 0 plus aggregate paths 1 through K; aggregate call results use every reachable owned leaf path. The exhaustive classifier makes a future HIR variant a compile failure; the owned-default/view-unknown fallback is limited to malformed checked HIR or unavailable imported facts. | type-domain sweep including FsReadBytesView read-only pre-state rejection; ArrayToSlice/range forwarding owners; hidden fixed temporary; fresh build/pipeline/sample/JSON/call owners; direct CloneIn/JsonDocElems/SoA fresh-view twins; partition/group K+1/result-leaf cardinality; carrier inventory above; compile-time variant sweep; malformed/unavailable fail-closed controls |
| Generation-content initialization | Header identity and stored content are classified separately, but the same fallthrough producer event must initialize both exactly once; a fresh generation without its semantic content fact is invalid. Fixed literals initialize exact element/field paths, while a dynamic collection uses its one direct element wildcard. A nested Copy view in a fixed literal carries only its completed child's owner roots and generation dependencies: constructing the inline aggregate creates no staging, arena, or lexical release owner for the non-owning view header itself. Build, materializing pipeline, sort, parallel-map, sample, and partition results seed that wildcard from the completed builder or pipeline-element fact; partition paths 0 and 1 each receive that same element fact in distinct generation-content entries. Direct `ArrayChunks` remains outside the header domain and seeds its one direct element dependency from the completed source header generations. `ArrayToSoa` and `JsonDecodeSoa` seed each admitted column under its exact `StructField`, and array/struct-array JSON decode uses the decoded schema's projected view-bearing element fact. Direct `CloneIn` materializes every reachable `Slice` leaf as a distinct `(CloneIn expression key, result path)` region-owned generation in the explicit destination region: primitive bytes seed empty content, recursive RegionPlain fields use exact projected content, and no cloned leaf forwards the source storage generation. `JsonDocElems` similarly creates an arena-owned fresh Slice generation at its expression/result path; its one direct element wildcard is seeded from the completed doc's generation dependencies and projected non-storage tape/input roots, while its new handle buffer never forwards the doc's storage identity. For `ArrayGroupAgg`, a string-key source (`AosStr`, `Encoded`, or `SoaStr`) seeds only generation path 0 from the exact selected base-key view dependency; `SoaI64` path 0 and numeric aggregate path 1 are empty. `ArrayGroupAggMulti` requires nonempty K and its current `AosStr` source, seeds only string key path 0, and leaves numeric paths 1 through K empty; any forged alternate source rejects before state mutation. Every fixed or owned direct/imported/raw/function-value call-result leaf materializes caller storage before content seeding: a fixed `Array`/`StructArray` uses the call-site hidden inline generation and later rebases to a bound destination place, while a dynamic owned leaf gets its fresh allocated caller generation. Each then seeds every borrow-bearing exact element/field or dynamic wildcard content path from the completed parameter/capture union selected by the existing flattened return summary; because the summary has no result projection, the same compatible union is conservatively normalized into each such path. An unavailable or malformed summary uses every compatible completed input/capture. A call-produced `DynSliceArray` carrier likewise remains outside the header domain but seeds its direct element generation dependency from that selected union; a call that happens to invoke a clone helper remains an ordinary unknown view result because that implementation detail is absent from the public summary. Any producer/result path for which the exhaustive initializer has no rule becomes unknown and rejects provenance-bearing use rather than defaulting to empty. | fixed exact/dynamic wildcard literals, including repeated use of static and caller-backed Copy-view elements after construction; builder/pipeline/sort/par-map/sample; ArrayToSlice alias move/rebind/store; direct chunks before source move; direct CloneIn bytes/recursive same-typed slice fields/source-end independence/region exit; JsonDocElems arena-versus-doc lifetimes, aggregate/closure paths, indices/stores, doc/source end, arena exit, and loop Current/Prior; partition str/numeric paths 0/1; group single AosStr/Encoded/SoaStr/SoaI64 and Multi nonempty AosStr-only K=1/K>1 key/aggregate/whole/hole, including forged-source rejection; SoA/JSON projected fields; direct/imported/raw/function-value fixed `array<str,N>`/fixed view-bearing AoS plus owned `array<str>`/view-bearing `DynStructArray`/`DynSliceArray` results; N>1 same-type slots; selected/unselected inputs and captures; materializer/identity and heap/arena/whole/per-unit parity; missing-initializer fail-closed sweep |
| Identity formation, finiteness, and malformed input | An allocated origin uses the existing immutable-HIR `expr_key(&Expr)` completion identity plus its canonical `BorrowProjection` result path, never a span, traversal counter, or process-external hash; an inline fixed-array origin uses its canonical local/projection place, caller storage its parameter position plus reachable aggregate projection path, and an owned-dynamic post-call replacement origin the call expression key plus destination-parameter ordinal and exact place path. Two distinct immutable Expr nodes deliberately carrying the same Span remain distinct origins in both analyses and under either traversal order. Path order is source/type order: struct field, tuple element (including holes), fixed element, Option Some, Result Ok then Err, user-sum variant then payload ordinal, and callable target then capture ordinal. Origins are analysis-local and never serialized or compared between compiler invocations. A distinct source occurrence, result path, or mutable destination remains distinct. Binding or moving an allocated header never advances or renames it, while copying or moving an inline value materializes the destination-place origin. Replacing an already-live fixed inline destination preserves that addressable generation and strongly replaces its content; consuming the separate inline source still ends the source generation. Unresolved imported/view producers are unknown. Checked-HIR validation rejects an impossible field, tuple ordinal, fixed index, variant, payload, target, or capture before either analysis mutates state; a defensive analysis mismatch produces an unknown leaf and no invented identity. | producer-site/path, owned mutable-destination, and inline-place unit tests; same-Span distinct-node/result-path content/directory identities in EscapeCheck and MoveCheck under reversed traversal; fixed bind/copy versus replacement alias controls; aggregate caller-parameter paths; partition/group/call cardinality; two simultaneous distinct-producer buffers; malformed `BorrowProjection` sweeps; existing checked-HIR projection validation |
| Parameter entry and caller translation | Function entry enumerates every reachable header leaf by parameter position and aggregate path before the body runs. A ByValue fixed `Array`/`StructArray` is copied into the callee's inline parameter place; a ByValue owned dynamic leaf has a stable parameter-value generation released by that callee parameter local (arena-owned dynamic values are already forbidden from crossing ByValue). A ByValue `Slice`/`Soa`/`SoaParam`, or any header leaf reached through `borrow`, `borrow mut`, or `out`, instead gets a non-owning symbolic caller-storage generation for `(parameter position, path)` and no callee release owner. Each borrow-bearing content leaf is conservatively seeded with the existing symbolic contained `Param(position)` root, while storage identity uses the distinct `ParamStorage(position)` translation; because public summaries have no input projection, normalization may place that root in every compatible content path. An incoming function value has no serialized capture trie: each target-summary-selected compatible capture leaf is unknown and falls back to the callable parameter root until a concrete same-program target/actual supplies candidates. At a call site, these parameter/capture symbols translate only through the already-completed actual value/storage facts; malformed or unavailable mapping selects all compatible actual paths. Parameter origins do not participate in Current/Prior advancement. | ByValue fixed/owned/view and borrow/borrow-mut/out mode matrix; nested mixed aggregate parameter paths; Param versus ParamStorage controls; callable-parameter concrete/unavailable target twins; whole/per-unit actual translation; malformed position/path fail-closed owner |
| Recency transition and directory join | Exactly one fallthrough completion event owns freshness for each materializing producer; resolver queries never advance state. That event atomically demotes every `Current(origin)` for all of the producer's result paths in locals, aggregate/callable leaves, expression/eager-argument/indirect-callee/pipeline completions, control and loop values, break frames, pending mutable-call facts, the release directory, and the generation-keyed content table to one sticky `Prior(origin)` summary before publishing every fresh Current result leaf and freshly initialized content entry. Current content merges into any existing Prior content; it is never left under the reused Current key. A terminating producer publishes and advances none. Existing Prior facts join, so a loop converges while the new value stays distinct from ended old aliases; stores through summarized prior aliases may conservatively cross-update prior instances. Caller origins never advance. Each directory entry is a may-set of live release places plus an optional ended reason: joins union owner alternatives, retain any ended path, and choose the strict storage region; an exact move removes/adds its one release place atomically, while an end records the reason without reviving at a later join. Ending reason affects diagnostics only; if the same generation is Consumed on one alternative and Dropped on another, the joined sticky ending deterministically reports Consumed, matching the existing `BorrowEnd` precedence. | current-versus-prior loop aliases and distinct old/new content; new-current acceptance after an ended prior; two-live-prior conservative control; owner-alternative branch join; consumed-versus-dropped deterministic ending join; eager/callee/pipeline/loop-break snapshot renaming; multi-result content atomicity and terminating producer; finite fixed-point owner |
| Storage generation versus release owner | Each non-inline writable allocation has one stable, analysis-local storage-generation identity distinct from the place that may release it. Copying a slice/SoA header copies that identity. Moving an individually released owned dynamic header transfers its release owner and lexical lifetime to the destination without ending the generation; moving a region-owned header preserves its arena/caller allocation lifetime and creates no individual owner. Every already-live view of either buffer stays valid and observes later stores. The directory retains every generation-qualified exact projected release place previously attached to a generation: terminal by-value consumption, replacement, reallocation, and scope Drop end scalar snapshots created under any earlier owner, while ending one aggregate projection never ends a live sibling projection. Current-to-Prior recency transitions rename the generation embedded in every root, snapshot, invalidation record, content fact, and directory history atomically with the header tables. Rebinding a non-owning Copy header only detaches that header. Binding a fresh or unrelated right-hand value into a moved-from source installs that right-hand value's distinct generation, so ending the old generation cannot poison a view of the fresh rebound value, an old alias follows the moved allocation at its current destination, and neither follows the unrelated rebound value. A round-trip Move `a -> b -> a` instead transfers the same allocation generation back to release owner `a`, and every pre-first-move alias still follows it. Fixed arrays instead use their inline place as identity: a copy/move creates destination inline storage, and a consuming move ends the source generation rather than transferring it. Unknown identities remain sticky and fail closed. | `moved_storage_aliases_follow_the_new_release_owner`; `moved_storage_aliases_do_not_follow_a_fresh_rebound_source`; `round_trip_move_restores_the_same_release_owner`; terminal consumption, replacement, and scope Drop after a prior owner transfer; fresh rebound before old-owner terminal consumption; projected terminal-consumption, scalar-owner partial-move, and supported field-replacement sibling controls; Copy-header rebind snapshot; heap/arena move twins; fixed-copy source/destination distinction; existing reassignment and source-visible reallocation owners |
| Exact self-assignment | After validating both sides and their projection, exact same-place `x = x` is a MIR no-op for every fixed, dynamic-owned, and Copy-view header leaf. An exact `root.field = root.field` is likewise a no-op only when that field place is already admitted by the ordinary assignment surface; this rule does not admit projected owned-array replacement that the producer currently rejects. Accepted self-assignment does not end a displaced generation, advance Current/Prior, remove or add a release owner, replace content, detach a view header, or null the source; pre-existing aliases continue to name the same generation. A transparent but non-exact right-hand expression and every different place retain the ordinary consuming/replacement rules; this exemption is place identity, never a same-generation alias guess. | whole-local fixed/dynamic/view self-assignment plus supported Copy-view field self-assignment with pre-existing aliases; rejected projected-owned control; exact versus transparent-wrapper/different-place controls; no Current/Prior or directory delta |
| Projection topology | Both analyses carry a finite backing/header trie keyed by the existing `BorrowProjection` vocabulary: struct field, tuple element, fixed-array element, `Option`, `Result`, user-sum payload, callable target, and callable capture. A top-level collection occupies the empty path, but that empty-path generation never implicitly flows into a nested header subtree the way `BorrowFact.direct` content does. Aggregate and closure construction prefix child facts; field/index/unwrap/destructure/match binding project them; assignment strongly replaces only the selected path; control-flow joins union identities and use the conservative backing lifetime. Projecting or binding a payload whose concrete type has neither a storage header nor any borrow-bearing leaf clears a conservative direct outer fact: a scalar sum alternative cannot inherit an arena/frame lifetime merely because a view-bearing sibling was possible before selection. `SoaColumn` retains the base generation and selects an exact `StructField` suffix in that generation's content; `SliceRange` retains the same generation and fallback roots, and because offsets are not represented any store through it conservatively updates the selected generation content. Indexing fixed `Array`/`StructArray` storage with an in-range literal selects exactly one `ArrayElement(i)` subtree, while a nonliteral selects the union of all N matching element subtrees; `ElemField` then applies its exact field suffix. Indexing dynamic `DynArray`/`DynStructArray` storage uses the one direct element wildcard for both literal and runtime indices, again followed by an exact field suffix. A scalar element read resolves the selected generation content into the result's ordinary non-storage fact at that read; a later store does not retarget that completed scalar value. A read whose selected element is itself an in-domain collection instead copies its header generation and remains a live observer. `ArrayChunks` carries one direct stable dependency on the source generation rather than one identity per runtime chunk: either literal or runtime `chunks[i]` selects that source dependency, moving the carrier or the source release owner preserves it, replacing/dropping the source ends it, a store through a bound chunk updates the source and sibling observers, and a `par_map` element carries the same dependency. `ClosureTarget` is the validated common callable-target ordinal already consumed by callable provenance; both analyses share that target-to-capture-path classifier, and an unavailable target becomes an unknown union of compatible capture paths. A scalar element view has no header entry even when it borrows the same owner as a collection. Malformed or unprojectable shapes become unknown rather than flattened into a false identity. | parameterized struct/tuple/nested-`Option`/`Result`/user-sum owners, including a scalar payload selected from an arena-backed runtime-indexed sum; SoA-column exact-field and offset-free range owners; same-typed sibling fixed literal/runtime and dynamic literal/runtime index/ElemField owners; scalar-read-before-same-slot-store snapshot and nested-header live-observer twins; empty-path noninheritance; chunks literal/runtime index, carrier/source moves, source end, bound-store sibling update, and par-map propagation; closure selected/unselected-capture owners; scalar-view and same-content-distinct-buffer negatives; malformed projection unit tests |
| Owned aggregate move | Moving one or more owned dynamic collection headers into a struct, tuple, `Option`/`Result`, or user sum preserves each allocated generation at its exact destination projection and transfers its releasing place atomically with source nulling. A pre-move slice alias can mutate that moved buffer; returning or later reading the aggregate observes the installed element owner. Slice/SoA view headers preserve their referenced generation without acquiring a release owner. Each inline fixed-array leaf instead materializes the destination projection's generation; a consuming move ends its source-place generation, while a Copy leaves the source generation live. A whole aggregate move applies these rules leaf by leaf rather than blindly renaming or preserving the trie. A supported partial payload move transfers only its projected allocated generations and rematerializes projected inline leaves; unsupported deeper Move projections retain their existing source diagnostic. | `indexed_store_updates_moved_aggregate_observers` parameterized by aggregate form, with no invalidated dynamic-alias diagnostic; fixed Copy/Move source-destination controls; positive static-store twins; partial/whole move controls |
| Staged construction and ordinary-call action | Aggregate parts and ordinary direct/imported/raw/function-value callees and arguments evaluate in source order, but completion is not ownership transfer. A completed direct bound Move source remains owned by its source local while later siblings/arguments run. A completed fresh or wrapped Move temporary is moved immediately only into an analysis-visible hidden staging owner so a later cleanup-carrying terminating edge can clean it. If a later operand does not fall through, no aggregate/call action occurs and no parent or result generation is published. A `return`, `?`, cleanup-carrying else/match edge, or `process.exit` runs MIR lexical cleanup, drops every staging owner exactly once, and leaves each bound source to that same scope cleanup. Process abort, hard trap, successful `process.exec`, and permanent loop divergence perform no synthetic Drop and have no successor that can observe either owner. Only after every operand falls through does one atomic action install every completed aggregate projection or pass every ByValue argument to the callee, transfer each allocated generation directory release place, null each bound source, and clear each staging owner before publishing the parent/call result. Callable captures use the same aggregate action; a function-value callee completes before argument zero. Mutable calls reuse this operand boundary before their additional destination transition. | bound-first/fresh-first times later return/`?`/else/match/`process.exit` cleanup and process-abort/hard-trap/exec-success/infinite-loop no-cleanup divergence; nested struct/tuple/Option/Result/user-sum/closure; direct/imported/raw/function-value call argument matrix; success exact transfer/source null/staging clear; absent-action cleanup/no-cleanup twins with no parent or result Current; exactly-once cleanup owners |
| Raw-call mode boundary | `RawCall` completes its optional guard first. A non-fallthrough guard evaluates no callee or argument; a false guard aborts before loading the callee and contributes no call action/result state; only the true edge completes the callee and arguments before the ordinary ByValue transfer action. Its current producer-owned closed signatures admit only ByValue and shared Borrow parameters. A forged `RawCall` carrying `BorrowMut` or `Out` rejects during checked-HIR validation before EscapeCheck or MoveCheck changes any generation, directory, content, cleanup, or source state. Adding a mutable raw ABI later requires an explicit closed signature and the same frozen-destination mutable-call transition; merely populating `param_modes` is insufficient. | guard false/nonfallthrough/true source-order controls; checked-HIR raw-signature mode sweep for every current descriptor/batch-plan offset; forged BorrowMut/Out pre-state rejection; ByValue/borrow return-summary controls |
| Short-circuit control | `&&` and `||` first complete the left Boolean, then fork from that exact post-left state. `&&` evaluates the right expression only on the true edge and `||` only on the false edge; the opposite edge skips every right-side mutation, replacement, Drop, freshness transition, and cleanup. A non-fallthrough right edge (`return`, `?`, abort, or divergence) contributes no Boolean result or post-expression state. When both skip and evaluated edges fall through, their complete directory, projected-header, generation-content, non-storage-content, cleanup, and Current/Prior states join: an old generation live on the skip edge remains a live alternative, while an old generation ended and a fresh generation installed on the evaluated edge remain sticky distinct alternatives. Nested short circuits and loop revisits apply this rule recursively and converge under the same finite recency summary. | `condition && mutating_call(...)` and `condition || mutating_call(...)`; old-alias use after owned dynamic, fixed inline, and Copy-view destinations; direct store/replacement/Drop twins; right-side return/`?`/abort/divergence; nested and loop-repeated fresh producer owners |
| Multi-result and payload binding | Every materializing output gets its exact allocation fact before binding. `ArrayPartition` produces distinct producer-site ordinals zero and one; region allocation fixes both regions at the current arena, while individual allocation transfers each ordinal's release lifetime to its receiving lexical owner. `LetTuple` always advances the source ordinal rather than reusing the whole tuple. A `None` HIR binding is possible only when `tuple_discard_needs_hidden_local` is false: that Copy/non-droppable hole installs no header. An ignored owned element is represented instead by `Some($tuple_dropN)`, receives the exact projected generation and release place like any other owned binding, and its ordinary MIR `Drop` ends that generation exactly once. Both scrutinee-first and largest-arm-first match traversals use one selector that projects `OptionSome`, `ResultOk`, `ResultErr`, or the exact user-sum variant plus payload ordinal before owned-binding lifetime adjustment. Empty, wildcard-only, malformed, or type-incompatible selection is unknown rather than flattened. Borrowed projections remain aliases and never acquire an owned release fact. | `partition_outputs_keep_exact_arena_backing`; `match_payloads_keep_projected_backing` for `Option`, both `Result` variants, and each user-sum variant/payload ordinal; Copy-view `None` hole and owned hidden-drop twins; both-match-traversal structural owners; heap and borrowed-projection twins |
| Mutation-to-observer update | Each generation owns one projected content fact: EscapeCheck stores direct regions plus generation dependencies, and MoveCheck stores ordinary borrow roots plus generation dependencies. A direct store, `out` effect, indexed `borrow mut` effect, or existing source-visible collection mutation snapshots the completed destination header before any update and updates the content entry for every selected generation exactly once; locals, aggregates, callables, and completed snapshots observe that change by resolving their unchanged header facts at use, not by copying the update into parallel observer maps. Ending a real named owner or iteration temporary rewrites that root to one ended marker in every reachable generation content fact and unknown-leaf fallback before legacy borrower invalidation. A resource receiver's exclusive action likewise ends the pre-call generation content while preserving the receiver local for its post-call generation; merely detaching or replacing a Copy mutable-view header does not end the backing generation's content roots. A destination with an unknown bit also selects candidate generations through overlapping fallback storage roots; a rootless unknown operation that could install borrow provenance rejects before mutation, while a provenance-free scalar update need not invent observers. Exact fixed indices may strongly replace one slot; dynamic indices, ranges, offset-free aliases, and dynamic storage conservatively join, with SoA field suffixes remaining exact. MoveCheck separately invalidates matching completed mutable-place reservations. No generation is selected by shared element-content owner roots alone. | aggregate-return rejection after a pre-move alias store; closure and eager/callee/loop-snapshot rejection after direct and `out` stores; generation-content owner/iteration/resource-action ending versus Copy-view detach; owner-reassignment invalidation; known/unknown fallback twins; rootless-unknown provenance rejection; distinct-buffer/same-old-owner negative |
| Mutable-call action | Callee, arguments, destination places, headers, and contents complete in source order; only one successful call action, after every eager operand falls through, computes every destination transition from those frozen headers before applying any transition. A later non-fallthrough operand creates no call-result generation, ends no by-value input at the absent call action, and applies no mutable effect. On success, by-value owned inputs transfer to the callee/end in the caller before fresh owned result leaves are published. Direct/imported `out` preserves its completed destination header/generation and joins only content; `out` is not representable by a function value, and forged checked HIR rejects before any state transition. Every accepted `borrow mut` call first applies its possible indexed content join, then classifies each whole destination leaf. An exact same-program self-only retention summary suppresses whole-place replacement but not the exclusive action: the destination is re-established and remains usable, while a value completed from its pre-call generation is invalid for any later use. A live Drop-bearing dependent resource blocks every exclusive action because its eventual Drop still needs the parent; a Copy reference rooted only in a borrowed parameter may instead be invalidated lazily and diagnoses at its next use. An owned dynamic destination ends only its displaced pre-call generations and advances the exact `CallMutation { call-site, destination-parameter-ordinal, path }` origin once to a fresh Current generation with the destination release owner and content seeded from the frozen incoming facts. A fixed inline destination preserves its addressable destination generation and strongly replaces its content. A Copy `Slice`/`Soa`/`SoaParam` destination, including one projected inside an aggregate, only detaches and strongly replaces that destination header with an unknown union of every compatible completed old/source generation plus fallback roots: it neither changes those generations' directory/end state nor mints a generation or release owner. This also covers a callee-installed fresh `clone_in(out)` view, whose unrepresented allocation remains region-rooted unknown at the caller. Multiple mutable destinations use distinct parameter ordinals and transition simultaneously. `borrow mut` uses this rule for direct, imported, and function-value calls. Stable producer-admitted field destinations use the exact path; any still-rejected projected owned destination remains rejected rather than receiving a synthetic owner. | direct/imported `out`-preserves and forged function-value-Out rejection; direct/imported/function-value `borrow mut` owned/fixed/view twins; no-replacement resource reuse plus pre-call-dependent later-use rejection and Drop-child preguard; old slice alias and original fixed/dynamic backing remain live after view rebind; aggregate/closure old-alias and selected-source candidates; clone-in region fallback; owned dynamic displaced-generation ending; indexed content plus whole-header effect; two-destination ordinal and simultaneous-snapshot owner; repeated owned-call Current/Prior convergence; later-argument non-fallthrough; Copy field destination; rejected projected-owned control |
| `ResultMapErr` split call | The receiver completes before the mapper expression. The `Ok` edge forwards only the receiver's exact `ResultOk` projected headers, content, release owners, and cleanup bit into the result's `ResultOk` path. The `Err` edge applies one ordinary indirect-call action to the exact `ResultErr` payload: a by-value owned payload ends at that successful action, every owned mapper-result leaf gets a fresh `ResultMapErr` expression-site/`ResultErr`-prefixed result-path generation, and every view result is unknown with the compatible completed Err-input and selected mapper-capture generation union. The resulting content and cleanup are installed only under `ResultErr`; no identity or owner is borrowed from the Ok sibling. If the receiver does not fall through, the mapper is not evaluated. If the mapper expression does not fall through, no Err call action or result generation occurs. A cleanup-carrying mapper exit drops the already-completed receiver's hidden owner exactly once; abort, hard trap, successful `process.exec`, and permanent-loop sinks keep that owner live but unobservable and contribute no successor, under the common staged-action rule. Whole-program and per-unit checking consume the same existing flattened function-value summary and therefore produce the same conservative Err view fact. | Ok-forward versus owned-Err-fresh twins; owned Err input consumption; view Err result/capture unknown-union whole/per-unit parity; receiver nonfallthrough plus mapper cleanup/no-cleanup twins; ResultOk/ResultErr projection isolation |
| Callable capture snapshot | Closure construction copies the header identities present in each capture at that instant and prefixes them by callable target and capture ordinal. Later rebinding the source capture local cannot retarget the closure. A call result consumes only capture ordinals named by the existing flattened return summary. An entirely unselected capture ordinal cannot taint the return, but once an aggregate capture ordinal is selected, every type-compatible header subpath under that ordinal conservatively unions into every compatible result view because the public summary has no capture-subpath discriminator. A different-typed sibling is filtered by compatibility. Storing the callable inside another aggregate prefixes the same fact without flattening it. | `closure_observers_follow_captured_backing`; capture-rebind old/new backing twins; same-type selected-capture siblings and different-type filter; entirely unselected capture; multi-target whole/per-unit parity; closure-in-struct projection |
| Discard-before-body cleanup | Control selection applies the same source nulling and Drop boundary as MIR before the selected body begins. On a `Result` else-unwrap Err edge, every owned `ResultErr` generation discarded by the language ends as Dropped before fallback evaluation, even when the fallback diverges; an `Option` None edge has no payload generation to end. Match cleanup is conditioned on MIR's fresh/materialized-scrutinee hidden owner, never on the payload type alone. After selecting the exact active variant, a Move binding transfers its exact owned payload generation from that owner or consuming bound source and nulls the source; a borrowed projection remains an alias and never ends the owner. When a fresh/materialized consumed scrutinee has a hidden owner and the selected single, wildcard, or or-pattern transfers no owned payload, the active owned payload generations are dropped and ended before the arm body, even when it diverges. A direct bound scrutinee inspected by a non-Move single/wildcard/or arm keeps its owner and generations live for later uses and its ordinary scope Drop. Inactive variants contribute nothing, and a post-arm join retains every actual ended alternative without inventing one for read-only inspection. | Result-Err else discard before live/diverging fallback; Option-None control; fresh constructor/materializer wildcard/or pre-body Drop; bound-local wildcard/or later-use and exactly-once Drop; Move-binding transfer/source nulling; borrowed-projection control |
| Move/drop/return closure | Transfer, source nulling, destination installation, and generation-owner update are one successful action, never an eager-child side effect. Dropping, replacing, or reallocating the current individual owner ends every generation it owns; leaving the owning arena ends its region-owned generations; moving either header does not end its allocation. A consumed inline source generation does end as its destination is rematerialized. A value whose complete storage-header set has exact local generation identities may be assigned into a lexically longer local: the owning release action invalidates that installed observer before any later out-of-region use. Unknown headers and caller destinations retain the eager destination-lifetime proof. An accepted `break` transfers owned leaves to the loop-result place. Explicit or tail return transfers individually released owned leaves to a symbolic Returned place before caller-side reidentification and leaves caller-region allocations in that caller region; a local-arena allocation, returned view of a local generation, or returned generation content that still depends on a local owner rejects. `if`, `match`, else-unwrap, `?`, `ResultMapErr`, short-circuit `&&`/`||`, loop entry/back-edge/break, explicit and tail return, and malformed non-fallthrough joins retain every possible generation/owner pair without reviving an ended generation. `Try` and else-unwrap transfer only the selected projected payload and its cleanup bit; `ResultMapErr` uses the split-call rule above; short circuit uses the dedicated skip/evaluate rule above. On an absent construction/call action, return/`?`/cleanup-carrying selection/`process.exit` drops every hidden staging temporary or still-owned aggregate leaf exactly once; abort/trap/exec-success/permanent-loop sinks perform no invented Drop and contribute no successor state. The current Copy-only `TaskGet` transfers no generation. | moved top-level and aggregate owned-return positives; exact-generation local assignment with inner/outer arena release and post-release invalidation; caller-region return positive; local-arena, local-view, and local-content return negatives; owner/arena-drop, replacement, reallocation, and staged aggregate/call cleanup versus no-cleanup sink diagnostics; parameterized branch/match/else/try/`map_err`/short-circuit/loop transfer owners; existing cleanup-bit, `TaskGet`, and return-summary tests |
| Completion, monomorphization, and interprocedural boundary | Eager argument and indirect-callee completion snapshots carry the projected header plus projected non-storage content in the one analysis-specific value fact; no independent top-level writable-backing snapshot remains. A later argument may end a captured generation but cannot retarget the completed trie. Each concrete generic monomorph runs the same analysis over its substituted aggregate shape; no template-only `Ty::Param` path enters generation formation. Same-program whole-program and per-unit bodies still publish only the existing flattened parameter/capture borrow roots, which have neither a result-projection discriminator nor a self-relative owned-leaf relation: every compatible view leaf of any call result is therefore unknown and receives the same candidate union translated through completed selected argument/capture headers; an unavailable/malformed summary selects every compatible input. A result containing an owned leaf plus a view of that same leaf also remains unknown rather than being guessed to point at its fresh caller generation. This deliberate cross-leaf over-approximation is identical in whole/per-unit checking. Every fixed call-result leaf materializes its exact call-site/result-path hidden inline generation and every owned dynamic leaf its exact allocated generation; a consumed owned argument ends independently even if the callee happens to return the same runtime header. Precisely preserving either relationship would require a new ownership-transfer/result-projection summary and remains deferred, so no projected trie or new fact is serialized. Runtime allocation provenance remains the existing heap/arena/inline mode carried by the release owner, and both analyses must agree on result-path allocation parity. | generic struct/sum/closure monomorph owners; existing completion/parallel-transfer matrix plus closure-callee capture mutation; aggregate-view cross-leaf over-approximation and whole/per-unit parity; fixed/owned result materialization and self-relative owner+view rejection; fresh-materializer versus conservative owned-identity-call controls; heap/arena/inline allocation twins |

#### Reopened axis: pre-generation lifetime consumer closure (2026-08-29)

The nightly detector found that the stable-generation migration had replaced
the authority used by its new owner matrix without retaining several existing
lifetime consumers. The generation topology and public summaries remain
unchanged. This repair closes the missing consumers as one root-cause class:
each must read the selected completed generation fact, while no consumer may
manufacture a lifetime solely from the lexical wrapper around an independently
owned result.

| Closure axis | Exact rule | Owner evidence |
|---|---|---|
| Projected scalar and record reads | Reading a `str` field through a fixed array element, a SoA column element, or a whole SoA row resolves every nested header dependency at the selected element/field path. Returning that observation or assigning it beyond the source generation's frame/arena rejects. A primitive field remains independent. | bounded `align_sema::tests::pre_generation_lifetime_consumer_closure_matrix`; `owned_structs_arrays::string_field_view_of_element_cannot_escape_the_array`; `soa::{str_column_view_cannot_escape_the_arena,str_struct_gather_cannot_escape_the_arena}` |
| Projected stores | A field store, whole-row store, and struct-literal row store resolve the incoming selected content before updating the destination generation. A view from an inner arena cannot be retained by an outer-arena SoA. | bounded `align_sema::tests::pre_generation_lifetime_consumer_closure_matrix`; `soa::{str_column_field_write_cannot_store_shorter_lived,str_column_whole_elem_write_cannot_store_shorter_lived,str_column_whole_elem_write_via_literal_cannot_store_shorter_lived}` |
| Independently owned and Copy results | A Copy-only gathered row and an `i64`-key grouped result carry no source-view lifetime and may leave their construction arena. The selected result content, not the producer's lexical wrapper, decides the exit. | bounded `align_sema::tests::pre_generation_lifetime_consumer_closure_matrix`; `soa::{a_gathered_struct_is_a_free_copy_returnable_from_its_arena,soa_str_key_group_by_result_cannot_escape_the_arena}` |
| Lexical exit and iteration cleanup | A task-group tail transfers its independently owned result exactly once. Loop iteration cleanup ends only temporaries actually retained by the completed value; direct `chunks` and shuffle consumers do not inherit a synthetic temporary owner from their enclosing operation. | bounded `align_sema::tests::pre_generation_lifetime_consumer_closure_matrix`; `task_group::owned_tail_moves_out_exactly_once`; `chunks::chunks_each_chunk_len`; `m10_rand::shuffle_is_a_deterministic_permutation` |

The row-level evidence labels above are closed by these exact parameterized
owner targets; one target may cover many Cartesian cells, and an existing test
is reused when it already fails for the changed defect:

| Owner group | Exact target |
|---|---|
| formation, path cardinality, same-Span identity, and malformed input | `align_sema::tests::storage_generation_formation_and_malformed_matrix`; `align_sema::tests::storage_generation_expr_variant_sweep` |
| producers, content initialization, projections, and non-writable carriers | `return_provenance::storage_generation_producer_content_matrix`; `return_provenance::storage_generation_nonwritable_carrier_matrix` |
| move/replacement, self-assignment, cleanup, staged action, and control joins | `borrow_liveness::storage_generation_move_replacement_cleanup_control_matrix`; `return_provenance::storage_generation_move_replacement_escape_matrix`; `align_mir::tests::staged_storage_generation_actions_follow_cleanup_boundaries` |
| direct/imported/raw/function-value calls, summaries, monomorphization, and heap/arena/inline parity | `return_provenance::storage_generation_interprocedural_allocation_parity_matrix` |
| resource-action generation ending, mixed header/non-header products, and header-free mutable clears | `pkg_db_q4b::streamed_views_cannot_cross_generation_or_escape`; `pkg_db_q6::borrow_mut_shaper_retention::unary_exact_clear`; `resource_ownership::raw_views_cannot_escape_their_resource_generation` |

The implementation topology is closed before code changes:

Both analyses use one common `StorageOrigin`/`StorageGeneration` key and one
exhaustive producer/ordinal classifier. A generation is a stable parameter or
caller form, or the `Current`/summarized-`Prior` recency of a finite producer,
inline-place, or mutable-replacement origin. Their flow state
separates (1) a projected header
fact that says which generations a value can address, (2) a generation
directory that says where each allocation currently dies, (3) one projected
content entry owned by each generation, and (4) projected non-storage content
copied directly by ordinary values. Content leaves may depend on a generation when the
contained value is itself a view (for example a chunk slice); MoveCheck resolves
that stable dependency through the directory instead of storing or retargeting
its current release local, and EscapeCheck resolves its current region at use.
A move changes the directory entry, not every alias
leaf. A store resolves the completed destination header and changes each selected
generation's content entry once; every current local and frozen snapshot that
still names that generation observes the same content. EscapeCheck's directory maps a generation to its strict storage region,
allocation mode, and possible release places; MoveCheck's maps it to possible
live release places plus a sticky optional ending. A state join unions possible
places and retains any ended path, rather than choosing one owner or reviving a
maybe-ended generation. Unknown header leaves keep conservative fallback storage
roots but never mint a generation. Public summaries flatten current parameter
or capture roots from the directory only at their existing publication point.
EscapeCheck uses one `EscapeValueFact` (projected headers, projected
non-storage regions, storage-locality, and individual/may-individual mode) for
locals and every completion. Its current scalar `region`, top-level
`backing_storage`, and separate callable-region maps become derived reads or are
removed; they may not remain independent authorities. MoveCheck's corresponding
value completion carries projected headers plus non-storage `BorrowFact`.

| Operation | EscapeCheck owner | MoveCheck owner |
|---|---|---|
| Fact formation | one projected header fact whose leaves carry generation sets, known bit, and fallback caller roots; one generation directory carries storage region/allocation mode/release place; each generation owns projected content regions/dependencies; ordinary values retain only projected non-storage content | the same projected header fact and producer keys; one generation directory carries live/ended release roots; each generation owns a projected `BorrowFact`, while ordinary `BorrowFact` values carry non-storage content and stable generation dependencies, never the mutable header's current release local |
| Parameter entry | form exact inline, owned parameter-value, or symbolic caller-storage headers per mode/path; seed projected content with symbolic contained parameter roots and leave callable capture leaves unknown when no concrete target is available | form the identical header paths and distinct Param/ParamStorage translations, with local release ownership only for ByValue owned dynamic leaves; concrete call sites translate symbols through frozen actual facts |
| Fresh construction | consume the type-directed owned-default/view-unknown classifier, hidden inline-temporary rule, SoA exceptions, and exact producer-result paths; advance Current to Prior across every state/snapshot fact before installing fresh Current headers and the path-specific content initializer above | consume the same identity classifier, content initializer, recency transition, and ordinals, with no element-content roots mixed into generation identity |
| Aggregate/callable construction | snapshot each child under a staging owner, then after all children fall through atomically prefix allocated/view headers through struct/tuple/sum paths and rebase inline fixed arrays to the destination place; publish nothing on failure | use the identical action boundary, then prefix every header capture by target and capture ordinal so later call selection stays exact; bound-source nulling and staging-owner clearing happen with installation |
| Ordinary call action | complete a function-value callee and all arguments before transferring any ByValue owner; on success, end/transfer inputs and publish classified results atomically; an absent action leaves bound sources owned, drops staged temporaries only on a MIR cleanup-carrying edge, and publishes no result, while a terminal no-cleanup sink invents neither Drop nor successor state | use the same frozen completions and action boundary for direct/imported/raw/function-value calls; no input ending or result Current exists on an absent action |
| Read/projection | local, field, tuple index, fixed index, `Try`, else-unwrap, and exact match variant/payload ordinal project the trie; malformed paths return unknown | the same projection vocabulary and fail-closed fallback; `BorrowFact` projection type-check remains the authority |
| Binding | `Let`, `LetTuple` (source ordinal, including holes), and both match traversal routes install the selected trie before owned-binding lifetime adjustment; allocated generations transfer, views copy, and inline leaves rebase | the same binding forms consume one shared selector, apply the same leaf-class rule, install projected headers, and update allocated generation-directory release places after the consuming move; `BorrowRoot` content facts are not retargeted |
| Replacement | whole local and supported field assignment strongly replace the selected trie; an owned dynamic allocation mode/release place transfers with its generation, a newly bound inline destination materializes its place generation, and an existing inline destination preserves that generation while replacing content | replace the selected header paths, transfer allocated release places without rewriting content roots, preserve an existing fixed destination's storage generation, and end only displaced allocated owners or consumed separate inline sources |
| Mutation | direct store and mutable-call actions resolve completed destination generations, then update each selected generation-content entry once; collection-header and callable observers resolve it lazily, while a completed scalar read retains its already-resolved non-storage fact | update the same selected generation-content entries at the exact/conservative element path and invalidate matching mutable-place snapshots; scalar reads already copied out do not become generation observers |
| Mutable call | after all operands fall through, direct/imported `out` preserves each frozen destination generation and joins content; `borrow mut` simultaneously applies per-leaf owned-dynamic replacement, fixed-inline content replacement, or Copy-view detach/unknown-union before result publication | use the same source-order action boundary and per-leaf transition; by-value input ending, destination changes, and fresh result installation occur once from the frozen pre-call facts; function-value Out rejects before the action |
| `ResultMapErr` | forward the completed Ok projection unchanged; on Err, apply the indirect-call result classifier to the completed Err payload and mapper captures, with no action after a non-fallthrough mapper | transfer the same Ok owner/cleanup; on Err, end a consumed owned input and install fresh owned or unknown view output under only `ResultErr`; a non-fallthrough mapper cleans the completed receiver only on a cleanup-carrying exit and publishes no successor on a no-cleanup sink |
| Control flow | EscapeState join unions every projected leaf and picks the strict destination region; divergence contributes no leaf across `if`, match, else-unwrap, `?`, `map_err`, short-circuit `&&`/`||`, loop, break, and return; short circuit forks after the left completion and joins the unmodified skip state only with a falling-through right state | BorrowState joins projected generations/content/ended roots across the same edges, retaining the skip edge's old live generation and the evaluated edge's sticky end plus fresh generation without executing the right side on the skip edge |
| Selection discard | project only the active Result/Option/sum edge; a discarded owned Err or a fresh/materialized hidden match owner with no Move transfer ends before fallback/arm-body evaluation, while a direct read-only bound scrutinee and borrowed projections remain live | apply the identical hidden-owner predicate and pre-body Drop/source-nulling transition; transfer a Move binding exactly, keep real endings sticky through live/diverging cleanup and joins, and invent no ending for read-only inspection |
| Completion | expression, eager-argument, indirect-callee, pipeline, control, loop, and break snapshots freeze projected headers plus non-storage content; later mutation of the same generation remains visible through the shared content entry, while rebinding the source cannot retarget the frozen header | the same snapshots carry headers and non-storage `BorrowFact`; mutable-place reservations separately reject an illegal later overlapping mutation |
| Exit and cleanup | return/break read post-store generation content; owned allocated leaves transfer to Returned/loop-result places, views still validate their directory lifetime, arena-backed moves preserve allocation lifetime, and inline moves rematerialize | return summaries publish only translated content/view roots while owned leaves transfer cleanup to Returned; source nulling transfers allocated ownership, and replacement, reallocation, Drop, arena exit, loop cleanup, and consumed inline places end only their applicable generations |
| Interprocedural fallback | concrete monomorph/direct view summaries install the flattened selected completed parameter generations into every compatible result leaf and mark it unknown; imported or malformed views retain every compatible argument identity with the same unknown bit; fixed leaves use hidden caller inline origins and owned dynamic leaves fresh allocated caller origins | direct and imported view summaries obey the same unknown cross-leaf rule without serializing projected or self-relative state; fixed/owned leaves use those same fresh caller forms and never infer an unrecorded transfer |

The fresh independent plan review is scoped to these matrices and the single
capability boundary. The final full-diff review uses the reopened axis
`storage-generation-projections`.

**Still deferred: admitting Move keys or elements.** Lowering either
needs per-element Drop in the owning path — the same shape as #739's deferred
fixed-array Move elements, not a validator edit. That remains a separate
capability. Do not "fix" a Copy gate without the MIR support: accepting a Move
value the path cannot drop trades a rejected program for a leak or a double free.
Until then the reachable witness for the `ord-key` row stays a `str` key.

Owners:
`align_mir::validate_hir_tests::move_copy_positions_are_refused_by_the_producer_not_the_boundary`
is the load-bearing one — it runs the frontend **and then the MIR boundary** and
asserts the rejection is the producer's diagnostic and never
`report this program shape`, so deleting any one gate reproduces that cell's
internal error. (A `check`-only assertion that the diagnostics do not mention
`failed HIR validation` is vacuously true, because `check` never runs the
boundary; do not add one.) The surface-local wording owners are
`align_sema::move_copy_positions_are_rejected`,
`align_driver::sort_by_key::sort_by_key_move_key_rejected`,
`align_driver::array_materialize::to_array_of_a_move_scalar_element_is_diagnosed`,
`align_driver::chunks::chunks_over_a_move_element_array_is_diagnosed`,
`align_driver::m10_rand::move_element_slices_are_rejected_by_shuffle_and_sample`,
and `align_driver::map_into::map_into_move_element_rejected`.

### Axis B — nominal-identity comparisons

`body_core` contains 538 comparison lines; 210 compare a `Ty` or `Scalar`.
Classification is by **operand provenance**, because that is what decides
whether a split is reachable:

| Class | Count | Verdict |
|---|---|---|
| Delegated: already asks `body_ty_matches` / `body_scalar_matches` | 48 call sites | Correct by construction. |
| Split-free: at least one operand is a primitive, a native singleton handle (`Ty::Raw`, `Ty::Reader`, `Ty::HttpClient`, …), or a scalar a sibling predicate has already narrowed to a primitive (`const_array_scalar_ok`, `valid_vector_scalar`, `numeric_body_ty`, `array_zip_scalar_ok`, `scalar_to_prim`) | 197 | Raw comparison is exact. No change. |
| Same-derivation: a stored discriminator id compared against the child type sema derived it *from* — resource operations (`Ty::Resource(*resource)`), SoA / struct-array store bases (`Ty::Soa(*struct_id)`), JSON decode targets (`Ty::Enum(*enum_id)`), pipeline terminals (`final_elem != *elem`), pooled-array locals | 9 families | One derivation cannot split against itself. Raw comparison retained; converting them would only widen. |
| **Cross-derivation: a callee or lifted signature compared against a call-site type.** This is the only class where two independent derivations of one source type meet. | 17 | 13 already delegated; **4 were raw and are fixed** (below). |

Fixed cross-derivation cells:

| Cell | Symptom |
|---|---|
| `ResultMapErr` mapper parameter vs receiver error scalar | **Proven.** `fail_with(Wrap { callback: quiet }).map_err(keep_error)` gave `Struct(6)` from the inferred instantiation and `Struct(4)` from the declaration header. Whole program rejected. |
| `Spawn` closure `FnTy::ret` vs task payload | Latent: payload is primitive today, but the comparison is cross-derivation. |
| `Spawn` lifted-signature return vs constructed `Result` expectation | Latent, and additionally sensitive to the `Ty::Result` / `Ty::Tagged` spelling that only `body_ty_matches` normalizes. |
| Lifted-signature spawn check (`resolve_lifted_signature` consumer) | Same family as the previous row. |

### Axis B (continued) — the other seven gates

Axis B above measures the body gate, which owns the expression-level rules. The
other seven gates were swept with the same criterion. They are small: **40**
comparison lines in total, **zero** `scalar_to_prim` admission gates, and
**seven** comparisons whose operands could carry a nominal id:

| Gate | Nominal comparisons | Verdict |
|---|---:|---|
| `json_scan_validation_reason` | 2 | `expression.ty != Ty::JsonScanner(*struct_id)` is same-derivation (the node's stored row id and its own type come from one resolution); `input.ty != Ty::Str` is split-free. |
| `declaration_header_metadata_is_valid` | 4 | All split-free: `Ty::Unit` (entry ABI return), `Ty::DynArray(Scalar::Str)` (argv), and two `Ty::ArenaHandle` mode checks. |
| `nominal_link_metadata_is_valid` | 1 | Split-free (`Ty::Str`). |
| `global_type_metadata_is_valid`, `type_placement_metadata_is_valid` | 0 | They validate the id domain itself, so they compare ids to table bounds, not types to types. |
| `checked_hir_body_depth_is_valid` | 0 | Structural depth only. |
| `checked_hir_body_facts_are_valid` | n/a | Compares a program against a **clone of itself** (`replay_clone::clone_program`), whose type tables are copied unchanged. Both sides carry identical ids by construction, so a nominal split cannot arise. |

Audited clean; nothing deferred from this sweep.

### Axis C — the vanished-checked-program rule

The rule "a checked program with functions must not lower to the empty
program" had three independent copies in `align_driver` and no copy at the
lowering boundary, so the whole-program test surface — the one the two
regressions above were observed through — got the empty program back with no
error and failed at link with an undefined `_main`.

`align_mir::lower_program_checked(program, per_unit, source_map)` is now the
single fallible boundary and owns the rule:

- returns `Err(LoweringRejected)` when validation rejects a program that has
  functions or structs, **and** when lowering itself publishes neither;
- returns `Ok(empty)` for a genuinely empty input;
- the four infallible entry points keep their fail-closed empty-program
  contract for hand-constructed HIR (`assert_rejected` is unchanged), and are
  documented as unusable for checked input.

`align_driver` exposes `try_lower_to_mir{,_per_unit,_located,_per_unit_located}`,
and **every** production path uses them: the CLI walk, the interface-summary
producer, the whole-program static-descriptor surface, the unit-cache
rehydration path, and database metadata preparation. Each of those first proves
the input checked without errors, which is what makes the rule meaningful.

The infallible `lower_to_mir*` names stay fail-closed rather than panicking.
They are the inspection surface, and owner tests legitimately reach them with
HIR no producer emits — HIR from a program that failed checking (`m5`'s scanner
inference cases assert the rejected MIR is empty) and HIR whose analysis facts
a test overrode by hand (`owned_tagged_payloads`'s arena provenance case). For
those inputs the empty program is the correct answer, so "a checked program did
not vanish" is not a property the infallible surface can assert.

### Axis D — machinery added by a capability must itself be verified

Reopened twice. The first version of this axis was itself wrong, which is the
most useful thing in it.

**What actually happened.** This capability added a `raw-ty-compare` row to
`scripts/lint-ratchet.sh` — a pinned count of raw `Ty`/`Scalar` comparisons in
`validate_hir.rs` — and it failed CI twice. The first diagnosis blamed tool
identity: this shell aliases `grep` to ugrep, the pin was baselined locally, CI
runs GNU grep, so the tools must disagree. That was false, and asserting it
without measuring was the real error. Measured afterwards, on the same tree, all
three tools agree exactly:

```text
branch 6335438a:  ugrep 301   perl 301
merged with main: ugrep 326   perl 326   (CI's GNU grep: 305 and 326 in turn)
```

Both failures had one cause: **CI evaluates the merge with `main`, and the pin
was taken on the branch.** `main` advanced three PRs that grew the counted file,
so the pinned number described a tree that is never tested. The second failure
was the same defect surviving a fix aimed at the wrong cause.

**Why the mechanism was removed, not repaired.** Even pinned correctly, the row
could not do its job:

- *It is orthogonal to the class.* Six occurrences of silent-empty-MIR are on
  record. A count over every comparison identifies none of them: it cannot say
  which site is wrong, and the four comparison-shaped occurrences do not change
  the total in a recognisable way. A gate that would not have caught a single
  instance of the class it names is not a gate for that class.
- *It collides with unrelated work by construction.* The count moves whenever
  any PR touches the file, so every merge with `main` re-opens it. That is not
  a property to tune; it follows from counting a whole file.

**What replaced it.** A source-analysis owner in `validate_hir_tests.rs`
(`raw_nominal_comparisons_stay_enumerated`), following this repository's existing
precedent of recomputing a set from the repository rather than pinning a number
— `variant_sweep_tripwire`, and `scripts/test-pr-workflow.sh` recomputing the
gate's target list. It extracts only the comparisons whose two operands can each
be an *independent derivation* of one source type, matches them against a named
allowlist, and fails with the site and the instruction to ask `body_ty_matches`.
A comparison against a fixed constructor is excluded, so unrelated work adding
`flow.ty != Ty::Raw` does not touch it.

**The cells.** A capability that adds a lint, ratchet, tripwire, or gate closes
all of these before pushing:

| Cell | Rule | How this capability failed it |
|---|---|---|
| **Detection** | Does the mechanism fire on the class it exists for? Enumerate the recorded instances and check each against it. | Missed. A count caught 0 of 6; the replacement detector pins all 6 spellings as a test. |
| **Friction** | Does it stay silent on changes that are not the class? An unrelated PR must not have to think about it. | Missed. A whole-file count moves with every edit; the replacement pins that split-free comparisons do not fire. |
| **Evaluated tree** | CI evaluates the **merge with `main`**, not the branch. Any pinned number, golden, or snapshot taken on the branch is pinned to a tree that is never tested. | The single cause of both CI failures. |
| **Direction** | Prove it fails when it should and passes when it should, on the real artifact, not only in principle. | Verified for the replacement: reintroducing `*parameter != err` into the real file reports `validate_hir.rs:5605`. |
| **Diagnosis** | Do not assert a root cause you have not measured. Two competing explanations cost a round each. | Missed: tool identity was asserted, never measured, and was wrong. |
| **Pin semantics** | State whether a pin is a *current* count or an *audited* set. | The allowlist is an audited set: every entry is classified in this matrix as same-derivation or split-free. |

The rule this axis adds: **verification machinery is production code for the gate
it guards.** It gets the same locally-before-push discipline as the compiler
change it accompanies — including the question a count can never answer, *would
this have caught the bug I just fixed?*

### Owners

| Invariant | Owner |
|---|---|
| Every source shape sema accepts survives every delegated gate | `align_mir` `checked_source_shapes_survive_every_delegated_gate` (scan `()`, scan enum, reduce `()`, map_err independent monomorph) |
| A vanished checked program is an error at the one boundary, and the infallible entry points still fail closed | `align_mir` `lower_program_checked_reports_a_vanished_checked_program` |
| Delegating a gate does not switch it off | `align_mir` `delegated_gates_still_refuse_a_genuinely_different_type` (struct accumulator, unordered key, mismatched map_err mapper, and two distinct source shapes under the shape matcher) |
| A sum-type scan accumulator compiles and runs | `align_driver` `unit_values::sum_type_scan_accumulator_compiles_and_runs` |
| Borrowed and owned text clones both survive checked HIR and execute | `align_driver` `m5::str_clone_escapes_arena_as_owned_string`, `m5::owned_string_clone_duplicates_locals_and_fields` |
| A new raw `Ty`/`Scalar` comparison in the body validator cannot land silently | `align_mir` `raw_nominal_comparisons_stay_enumerated` — recomputes the sites from `validate_hir.rs` and matches them against an audited allowlist, naming any new site and what to do about it |
| The detector recognises the class and ignores everything else | `align_mir` `the_raw_comparison_detector_recognises_the_class` — all six recorded spellings fire; six split-free spellings do not |
| End-to-end acceptance | `align_driver` `mir_continuation`, `unit_values` |

**Review-ledger bookkeeping.** These occurrences were found by an internal
investigation, not by an independent review, so `align-self-review`'s counting
rules keep them out of `FINDINGS.md`'s root-cause table. The class is tracked
here instead, and the two source-analysis owners above are its automated owner —
the sixth occurrence is what promoted it from prose to machinery.

### fn_types interning (follow-up, not in this class)

`intern_fn_type` deduplicates on the whole `FnTy`, including the `effect` cell
that later inference mutates, so two structurally identical `fn(i64) -> i64`
entries survive and multiply into duplicate struct/enum monomorphs. This costs
duplicated monomorph records and mangled names; it is not a correctness defect
now that the cross-derivation comparisons delegate. Track separately.
