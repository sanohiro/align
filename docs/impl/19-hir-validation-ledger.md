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
| `AssignIndex` | `env[base]`: `MV(base)` is fixed/dynamic scalar array or writable slice. `child[index,value]`. `post[index.ty == i64; value.ty equals the exact element type; base is mutated, index/value are borrowed or consumed according to value type; bounds action occurs only after both children]`. |
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
| `Index` | `env[]`; `child[recv,index]`; `post[index i64. A Vec(s,n) receiver requires index to be an Int literal in 0..n and returns scalar_to_ty(s). Otherwise recv is Array, Slice, DynArray, DynSliceArray, StructArray, DynStructArray, or Soa; a fixed source is Local or ArrayLit; result is its exact scalar, slice, or struct element and must not be recursively Move; recv is borrowed and the bounds action is last. A terminating index has no bounds/result fact. For `array<str>` and AoS Copy records with any array/record-admitted region-bearing Copy leaf, including direct or nested `str` and `slice<T>`, a direct/field/BorrowedProjection base maps its complete generation and contained roots into the result, return, and BorrowMut-retention facts through the canonical borrow/region classifier]`. A recursively Move dynamic scalar/AoS-record element uses only the separate `BorrowedIndex` row below. |
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

### am-b3 helper discriminators

| Discriminator | Exact contract |
|---|---|
| `AeadCipher::Aes256Gcm` | Selects AES-256-GCM; 32-byte key, 12-byte nonce, 16-byte tag. |
| `AeadCipher::ChaCha20Poly1305` | Selects ChaCha20-Poly1305; the same key/nonce/tag widths. |
| `AeadDir::Seal` | Input is plaintext; output is ciphertext followed by tag. |
| `AeadDir::Open` | Input is ciphertext followed by tag; authentication failure releases no plaintext. |
| `HashAlgo::Sha256` | Exact digest length 32 and runtime key `crypto_sha256`. |
| `HashAlgo::Sha512` | Exact digest length 64 and runtime key `crypto_sha512`. |
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

**The axis.** A Move element *collection* cannot be built by any expression: an
owned `array<string>` cannot be indexed, sliced, or borrowed as a view, and
`string` cannot be a fixed-array element. But a `slice<Move>` **type** is fully
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
| Index / slice-range of a Move collection | Symmetric | `indexing an array of the Move type string` / `slicing a collection of the Move type string`. |

**Still deferred: actually admitting these.** Lowering a Move key or element
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
