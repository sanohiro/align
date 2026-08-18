# Borrow-safe sum-payload match projection

Status: **PROPOSED DESIGN; implementation pending.**

This document is the design and implementation plan for read-only matching of an owned
`Option<T>`, `Result<T, E>`, or user sum through a stable borrowed place. It closes the general
language gap recorded as align-llm Request 16. The plan is deliberately compiler-wide: an admitted
borrowed payload must remain a borrow through semantic analysis, checked HIR, MIR, LLVM lowering,
generic rechecking, and cache validation.

The capability is not an application workaround, a verifier-specific API, or a second ownership
model. It is accepted only when the public contract and this closure matrix are reviewed and
merged before implementation begins.

## 1. Public-contract ledger

| Surface | Exact input and defaults | Result and errors | Ownership, lifetime, allocation, and cleanup | Compiler/interface owner | Identity and prerequisite acceptance |
| --- | --- | --- | --- | --- | --- |
| Borrowed-place `match` | Existing exhaustive syntax: `match place { Variant(binding) => arm, ... }`. `place` is a stable local or a validated struct-field path whose **complete place** has a direct shared/exclusive borrow fact; a descendant field fact never promotes an owning parent. The selected payload must satisfy the `BorrowedSumPayload` grammar below. No new syntax, option, type argument, environment input, or temporary materialization. | The tag is read in place. Every reachable arm must remain exhaustive; wildcard and or-pattern binding rules do not change. A malformed, non-sum, mixed-provenance, or unsupported-payload place keeps the existing borrowed-place diagnostic. A consuming use of an admitted borrowed payload keeps the ordinary borrowed-place diagnostic. | A non-Copy payload binding is a read-only projection owned by the original borrowed root. It has no independent cleanup bit, `Drop`, source-nulling, allocation, shallow aggregate copy, or ownership transfer. Its static payload type remains available for field and method checking, but ownership-required uses are rejected. Only an owned `string` leaf may use the existing `.clone()` operation to create an owned value; no aggregate/sum clone is introduced. View values derived from the projection use the existing borrow roots, generations, and region escape rules. | `align_sema` classifies the exact place and payload grammar, records the binding projection and borrow fact, and rejects ownership escapes. The checked-HIR validator independently recomputes the exact place, mode, generation, and payload eligibility from the function's parameters and flow state, then rejects any mismatch instead of trusting the record. MIR carries a pointer/path projection and never materializes a Move aggregate for inspection. LLVM reads tags and payload fields through the original storage. No runtime helper or ABI field is added. | Exact owning and borrowed twins are semantic parity owners. The direct `Option<string>` fixture, `Result<string,string>`, a user sum with an admitted Move payload, a borrowed root, depth-1/nested field paths, and a mixed-provenance owning twin must compile or reject as specified while the source remains usable. The capability is a prerequisite for align-llm Request 16 and its `c6-borrowed-option-adoption` gate. |
| Owning-place `match` | Existing syntax over a free-standing or otherwise owning scrutinee, including a sum payload outside `BorrowedSumPayload`. | Existing exhaustive, `else`, `?`, branch, loop, and early-exit behavior. | Unchanged consuming extraction: the selected Move payload is transferred, the source is nulled, the selected binding receives the existing cleanup bit, and inactive/remaining ownership is dropped exactly once. | Existing sema MoveCheck, MIR `lower_match`, DropPlan, and LLVM aggregate lowering remain the owner path. | Existing owning match, nested Move, wildcard, or-pattern, `else`, `?`, and loop owners remain green; no borrowed mode may weaken their source-use diagnostics or cleanup. |

`BorrowedSumPayload` is the complete admissibility rule for the new projection path. It contains
Copy scalars and views, `string`, and finite acyclic structs, `Option`, `Result`, and user sums whose
reachable leaves recursively belong to this same set. It deliberately excludes tuples, fixed and
dynamic arrays, struct arrays, buffers, builders, boxes, resources, and every other opaque or
collection Move shape, even when that shape has an existing `DropPlan`. Those shapes remain valid in
owning matches; this plan does not add a partial-array or handle projection ABI.

The borrowed binding is not a new reference type and does not become an independently storable
value. A payload field may be read repeatedly, a Copy field may be copied, and an owned `string`
field may be passed to an existing `str` consumer without moving the owner. Returning, storing,
capturing, sending, or retaining the whole **non-Copy/Move** payload remains rejected; existing
Copy/view matching retains its current copy/view result behavior. A view produced from a borrowed
payload is checked by the same source-owner generation and inferred-region rules already used for a
direct borrowed field; a caller-owned borrowed parameter may return such a view only when the
existing return-borrow summary permits it.

The mode distinction is determined by the scrutinee place, not by the application type or the
payload spelling. An owning `Option<string>` and a borrowed `Option<string>` have the same type and
runtime representation. `Option<str>` and other Copy/view payloads retain their current behavior;
this plan does not reinterpret them as owned payloads or create a JSON-only exception.

## 2. Semantic contract

### 2.1 Borrowed match mode

Sema selects borrowed mode only when the scrutinee is a stable place and a shared or exclusive
borrow fact is attached to that exact root/path pair. The root may be the borrowed parameter itself
or a checked struct-field path below it, but a flattened fact for a descendant field is not evidence
that an owning local or its parent path is borrowed. A fresh constructor, function result,
control-flow result, unbound temporary, or materialized local remains on the existing
owning/borrowed-expression path and is not silently given an addressable storage slot. A local that
contains both an owned field and a borrowed view therefore remains owning when the match place is
the local or an owning field; this is an explicit closure twin, not a fact join that selects the
borrowed path.

For each single-variant arm that binds a payload, checked HIR records:

- the source sum type and exact variant/payload ordinal;
- the source borrow fact and owner-generation provenance;
- the binding's original static payload type; and
- a `BorrowedProjection { binding_local, ... }` record that forbids Move transfer and independent
  cleanup. The binding local is projection-only: it is visible for type/method checking but has no
  storage owner, cleanup bit, or entry in `drop_locals`, `drop_individual_locals`, or
  `drop_individual_exprs`.

Wildcard and or-pattern arms still bind nothing. Exhaustiveness, variant names, positional binding
order, and the `Option`/`Result` special forms remain unchanged. A nested Move struct is projected
recursively: accessing a Copy scalar reads it, and accessing an owned text leaf uses the existing
non-consuming string-to-`str` path. Only that owned `string` leaf may use the existing `.clone()`;
aggregate and nested-sum clone operations are not part of this capability.

### 2.2 Ownership and escape

The borrowed projection keeps the caller's ownership. An admitted non-Copy/Move payload cannot be
consumed by a by-value call, returned as a Move value, assigned into an owning destination, captured
by a task or escaping closure, or retained after the source generation ends. Copy/view payloads keep
their existing copy/view result behavior. The diagnostic names the borrowed root or field using the
repository's existing borrowed-parameter/borrowed-field wording.

`borrow mut` gives the body exclusive access but does not change the read-only projection contract.
The unchanged source remains valid for the body; a later replacement or mutable-borrow call ends
the old generation and invalidates all derived views through the existing invalidation analysis.
The source remains matchable again after a read-only match. An owning match continues to mark the
selected source moved and rejects a later match or use.

### 2.3 Control flow

The borrowed mode applies through `match` arm joins and must compose with `if`, `else`, `?`, loop
back-edges, replacement, and early exits. A nested `match` whose scrutinee is a borrowed arm
binding is outside this capability and is rejected as a borrowed-place ownership use; it never
falls back to consuming extraction from the outer source. A nested `match` over a separate direct
borrowed stable place retains the ordinary rule. A result produced by an arm is checked
independently of the borrowed payload binding: a scalar result is ordinary, a view result carries
the binding's source roots, and an owned result must be explicitly constructed or use an existing
supported string clone. No arm
may smuggle the borrowed payload through a value-carrying join.

## 3. Representation and lowering boundary

The implementation may extend the existing `BorrowedPlace` representation or introduce one
equivalent checked projection operand. Its complete semantic path must distinguish:

```text
RootSlot
StructField(field)
EnumPayload(variant, payload_ordinal)
OptionSome
ResultOk
ResultErr
```

The path is nonempty after a sum payload is selected, is type-checked at every segment, and retains
the exact payload type. A branch first reads the tag from the root storage, then projects the active
payload pointer in place. Scalar consumers load from that pointer; borrowed string/view consumers
retain the pointer and length; no `extractvalue`/aggregate load may create a shallow Move copy just
to inspect a payload. The source is never nulled in borrowed mode and no binding drop flag is
created.

The checked-HIR validator, MIR validator/fingerprint, generic monomorphization, and cache identity
must all carry and validate the same ordered path and mode. Per-unit compilation rebuilds the record
from the same source; the interface boundary transports only the existing public signatures,
structural type definitions, and generic source bodies described below.
Missing, out-of-range, variant-incompatible, type-inconsistent, or mode-inconsistent metadata
fails closed at the checked boundary. LLVM must reject malformed paths before emitting a GEP/load;
there is no fallback to a copied aggregate or runtime helper.

The checked-HIR record is exact and id-free apart from validated declaration-order local ordinals:
`Match` carries `borrowed_place = { root_local, root_struct_path, sum_ty, mode, owner_fact }` when
borrowed mode is selected, and each eligible single-variant `MatchArm` carries one
`BorrowedProjection { binding_local, variant, payload_ordinal, static_ty, path }` per binding. Wildcard and
or-pattern arms carry none. `root_struct_path` is the exact pre-sum place path; `path` starts at
`RootSlot` and includes the selected variant/payload before any nested field or tagged segment.
`owner_fact` is a sorted vector of live `BorrowRootFact { kind, ordinal, path }` records for that
same exact place: `kind` is `Local`, `Param`, or `ParamStorage`, `ordinal` is the declaration-order
local or parameter ordinal, and `path` is the canonical projection path. Ended roots, iteration
temporaries, source spans, pointers, and process-local hashes are not encodings of this fact.
The checked-HIR validator independently replays the producer-owned borrow/move flow over function
parameters, local types, statement/child order, and control-flow joins. It compares the recomputed
exact place, mode, generation, payload eligibility, and `owner_fact` with the record; a handcrafted
or stale record cannot opt an owning place into borrowed mode. It also checks local ordinals against
the function local table, every path segment against the preceding type, the exact variant and
payload ordinal, the static type, and the `BorrowedSumPayload` grammar. It requires every
`binding_local` in `borrowed_bindings` to be absent from the recomputed `drop_locals` and
`drop_individual_locals`, requires no projection-only expression in `drop_individual_exprs`, and
requires the stored drop sets and map to equal the recomputed values after those exclusions. It
rejects absent/extra records, duplicate or unsorted segments, stale local ordinals,
forged borrow facts, and inconsistent metadata before MIR construction. Replay cloning copies these
records as data; generic monomorphization rechecks and rebuilds them after substitution.

The interface artifact does not serialize function bodies or checked-HIR records for non-generic
functions. A generic body's canonical source is already transported in `generic_body`; the importer
reparses and rechecks that source and rebuilds the record. Exported return-borrow/region summaries
carry only the existing boundary roots, not an internal match path. Therefore this capability adds
no `InterfaceSummary` field and does not bump interface format 8; a future body-HIR transport would
need a separate versioned, id-free record rather than silently omitting this one. MIR fingerprints
and cache identity include the canonical record fields and structural reachable types.

No runtime representation, allocation routine, native ABI field, JSON tag, package import, or
special function is introduced. The existing flattened user-sum and builtin `Option`/`Result`
layouts remain the representation. The capability is a frontend/MIR/backend read-only projection
over those layouts.

## 4. Implementation closure matrix

| Axis | Owner boundary | Required implementation and regression evidence |
| --- | --- | --- |
| Formation and distinction | `align_sema::check_match`, exact borrowed-place classification, checked-HIR rederivation | Direct `Option<string>`, `Result<string,string>`, user sum with an admitted Move payload, borrowed root, depth-1 field, nested field, and `borrow mut` positives. Unsupported array/collection/opaque payloads reject in borrowed mode. Forged/stale owner facts, a mixed-provenance owning twin, and owning twins retain Move extraction. Fresh/temporary scrutinees do not acquire hidden storage. |
| Binding mode and type | HIR match records, MoveCheck, exact borrow facts | Binding retains the payload's static type for field/method lookup but is marked read-only. Copy/view payloads retain their existing behavior; admitted Move fields read, owned string leaves borrow as `str` and are the only explicit `.clone()` path, aggregate/sum clone is rejected, and a whole non-Copy/Move payload transfer is rejected. Each projection-only binding local is absent from `drop_locals` and `drop_individual_locals`. Or-patterns remain binding-free. |
| Payload projection | MIR `lower_match`, borrowed-place/path operands, `lower_expr_for_borrow` | Tag branches read the original sum, active admitted payloads bind through a checked pointer/path, no shallow Move aggregate copy occurs, no source nulling occurs, no binding storage is allocated, and no binding Drop/cleanup bit is emitted. Unsupported payloads never fall back to aggregate extraction. |
| Backend lowering | `align_codegen_llvm` borrowed path and type checks | Direct and nested struct/sum paths use in-place GEP/load. Malformed paths, wrong variants, wrong types, missing roots, and invalid cleanup metadata fail as lowering errors rather than panic or fallback copy. |
| Ownership and escape | MoveCheck, `EscapeCheck`, return-borrow and generation summaries | Repeated read-only matches preserve the source; return/store/call/task/closure capture of a whole admitted non-Copy/Move payload is rejected; derived views carry roots; replacement and Drop invalidate old generations. |
| Control flow | match arm joins and all value-producing control forms | `None`/`Some`, `Ok`/`Err`, wildcard/or-pattern, nested match negative for a borrowed arm binding, divergent arms, `else`, `?`, `if`, loop back-edge, early return, and borrowed-field replacement each retain the correct source/borrow state. A nested match over a separate direct borrowed place remains an existing positive. |
| Aggregate and cleanup | recursive `DropPlan`, admitted-payload classifier, field/path classifiers, source nulling | Nested `Option<MoveStruct>` reads, user sums with multiple admitted Move payloads, mixed Copy/Move fields, optional strings, projection-only arm locals excluded from every cleanup set, caller exit Drop, unsupported collection/opaque negatives, and owning-vs-borrowed parity show no double free, leak, or stale cleanup bit. |
| Interfaces and caches | checked-HIR replay, generic-body source transport, MIR fingerprint, per-unit and generic consumers | Replay preserves the checked record; generic source recheck reconstructs it; structural type edits and body edits invalidate exact dependents; malformed checked-HIR records are rejected; no internal projection field is fabricated in the public interface artifact; whole-program and per-unit output agree. |
| Existing surface parity | `Option<str>`/Copy match, owning Move match, `else`, `?`, and current negative owner | Existing view-region tests stay green. `match_cannot_extract_a_move_payload_from_a_borrowed_parameter` becomes positive coverage only for admitted Move payloads with explicit consuming and unsupported-shape negatives, while all owning cleanup tests remain unchanged. |

## 5. Acceptance owners

The implementation PR must add the narrowest stable owners rather than a broad aggregate solely for
reachability:

- `align_sema` unit coverage for the mode distinction, borrow-root propagation, explicit consuming
  diagnostics, and malformed checked-HIR metadata;
- a focused `align_driver` owner for direct and nested `Option<string>`, `Result<string,string>`,
  and user-sum admitted Move payloads, including repeated matching, source use after the match,
  unsupported collection/opaque-shape negatives, nested borrowed-match rejection, and the
  projection-only local's no-Drop behavior;
- MIR/codegen assertions that the borrowed path is pointer-based and does not emit aggregate
  extraction or source nulling for the projection;
- existing `enum_match`, `owned_tagged_payloads`, `structured_error`, `m11_http`, interface,
  generic, per-unit, and cache owners rerun only where their changed boundary owns the cell; and
- the align-llm `c6-borrowed-option-adoption` fixture plus a representative
  `PromptEvaluationResult`/`PromptVerifierTrust` verifier read-only fixture after the merged Align
  commit is pinned.

No benchmark is required. The contract promises no new allocation or runtime helper, so allocation
parity and pointer-vs-copy assertions are correctness evidence, not a performance claim.

## 6. Non-goals and deferrals

- No new pattern syntax, destructuring syntax, reference type, lifetime annotation, trait, macro,
  or implicit clone.
- No recursive/boxed sum types, borrowed projection through tuples, fixed/dynamic arrays, struct arrays,
  buffers, builders, boxes, resources, opaque handles, or other collection Move shapes. Owning matches
  still use the existing supported recursive Drop graph; this extension does not narrow that path.
- No nested borrowed `match` whose scrutinee is an outer borrowed arm binding, and no aggregate or
  nested-sum `.clone()` operation. A separate direct borrowed stable place may still be matched by
  the existing rule.
- No change to by-value match, `else`, `?`, `map_err`, or ownership transfer semantics.
- No permission to return or retain an owned payload through a borrowed match; only ordinary derived
  views may follow the existing borrow summary and region rules.
- No application sentinel, wrapper record, alternate verifier signature, compiler-known package
  type, or JSON-specific match exception.

## 7. Author-side consistency pass

Before implementation begins, verify that:

- the Match paragraphs in `draft.md` and `docs/language-spec.md`, the rationale in
  `docs/design-notes.md`, this ledger, and the Settled entry in `docs/open-questions.md` use the
  same exact-place predicate, `BorrowedSumPayload` grammar, binding mode, escape rule, and
  owning-mode parity;
- every public promise in §1 appears in the exact matrix row and in at least one named owner;
- no row adds a runtime/ABI/JSON/interface field that §3 forbids, and the generic-body source
  transport is the only cross-unit body transport;
- `docs/impl/17-library-boundary-prerequisites.md` owns the tagged projection milestone and
  `docs/impl/04-mir.md` names both the existing owning extraction and this pointer-based borrowed
  lowering; no later borrow milestone is consumed;
- examples distinguish declarations from positional calls and syntax-check in the implementation
  owner; and
- the plan does not consume evaluator/provider work or any later C6 milestone.

The capability is not implementation-ready until this pass and one independent design review are
complete. Implementation follows from the reviewed merge and reopens this matrix only if the
strategy or public contract changes.
