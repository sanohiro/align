# Borrow-safe sum-payload match projection

Status: **PROPOSED DESIGN; implementation pending.**

This document is the design and implementation plan for read-only matching of an owned
`Option<T>`, `Result<T, E>`, or user sum through a stable borrowed place. It closes the general
language gap recorded as align-llm Request 16. The plan is deliberately compiler-wide: a
borrowed payload must remain a borrow through semantic analysis, checked HIR, MIR, LLVM lowering,
interfaces, and cache validation.

The capability is not an application workaround, a verifier-specific API, or a second ownership
model. It is accepted only when the public contract and this closure matrix are reviewed and
merged before implementation begins.

## 1. Public-contract ledger

| Surface | Exact input and defaults | Result and errors | Ownership, lifetime, allocation, and cleanup | Compiler/interface owner | Identity and prerequisite acceptance |
| --- | --- | --- | --- | --- | --- |
| Borrowed-place `match` | Existing exhaustive syntax: `match place { Variant(binding) => arm, ... }`. `place` is a stable local or a validated struct-field path whose root is reachable through `borrow` or `borrow mut`; no new syntax, option, type argument, environment input, or temporary materialization. | The tag is read in place. Every reachable arm must remain exhaustive; wildcard and or-pattern binding rules do not change. A malformed or non-sum place keeps the existing diagnostic. A consuming use of a borrowed payload keeps the ordinary borrowed-place diagnostic. | A non-Copy payload binding is a read-only projection owned by the original borrowed root. It has no independent cleanup bit, `Drop`, source-nulling, allocation, shallow aggregate copy, or ownership transfer. Its static payload type remains available for field and method checking, but ownership-required uses are rejected. An explicit `.clone()` creates the existing owned value. View values derived from the projection use the existing borrow roots, generations, and region escape rules. | `align_sema` classifies the match mode, records the binding projection and borrow fact, and rejects ownership escapes. Checked HIR validates the projection metadata. MIR carries a pointer/path projection and never materializes a Move aggregate for inspection. LLVM reads tags and payload fields through the original storage. No runtime helper or ABI field is added. | Exact owning and borrowed twins are semantic parity owners. The direct `Option<string>` fixture, `Result<string,string>`, a user sum with a Move struct payload, a borrowed root, and depth-1/nested field paths must compile and run while the source remains usable. The capability is a prerequisite for align-llm Request 16 and its `c6-borrowed-option-adoption` gate. |
| Owning-place `match` | Existing syntax over a free-standing or otherwise owning scrutinee. | Existing exhaustive, `else`, `?`, branch, loop, and early-exit behavior. | Unchanged consuming extraction: the selected Move payload is transferred, the source is nulled, the selected binding receives the existing cleanup bit, and inactive/remaining ownership is dropped exactly once. | Existing sema MoveCheck, MIR `lower_match`, DropPlan, and LLVM aggregate lowering remain the owner path. | Existing owning match, nested Move, wildcard, or-pattern, `else`, `?`, and loop owners remain green; no borrowed mode may weaken their source-use diagnostics or cleanup. |

The borrowed binding is not a new reference type and does not become an independently storable
value. A payload field may be read repeatedly, a Copy field may be copied, and an owned `string`
field may be passed to an existing `str` consumer without moving the owner. Returning, storing,
capturing, sending, or retaining the whole Move payload remains rejected. A view produced from a
borrowed payload is checked by the same source-owner generation and inferred-region rules already
used for a direct borrowed field; a caller-owned borrowed parameter may return such a view only when
the existing return-borrow summary permits it.

The mode distinction is determined by the scrutinee place, not by the application type or the
payload spelling. An owning `Option<string>` and a borrowed `Option<string>` have the same type and
runtime representation. `Option<str>` and other Copy/view payloads retain their current behavior;
this plan does not reinterpret them as owned payloads or create a JSON-only exception.

## 2. Semantic contract

### 2.1 Borrowed match mode

Sema selects borrowed mode only when the scrutinee is a stable place and its borrow fact reaches a
shared or exclusive borrow root. The root may be the borrowed parameter itself or any checked
struct-field path below it. A fresh constructor, function result, control-flow result, unbound
temporary, or materialized local remains on the existing owning/borrowed-expression path and is not
silently given an addressable storage slot.

For each single-variant arm that binds a payload, checked HIR records:

- the source sum type and exact variant/payload ordinal;
- the source borrow fact and owner-generation provenance;
- the binding's original static payload type; and
- a `BorrowedProjection` mode that forbids Move transfer and independent cleanup.

Wildcard and or-pattern arms still bind nothing. Exhaustiveness, variant names, positional binding
order, and the `Option`/`Result` special forms remain unchanged. A nested Move struct is projected
recursively: accessing a Copy scalar reads it, accessing an owned text leaf uses the existing
non-consuming string-to-`str` path, and an explicit `.clone()` is the visible owned copy.

### 2.2 Ownership and escape

The borrowed projection keeps the caller's ownership. It cannot be consumed by a by-value call,
returned as a Move value, assigned into an owning destination, captured by a task or escaping
closure, or retained after the source generation ends. The diagnostic names the borrowed root or
field using the repository's existing borrowed-parameter/borrowed-field wording.

`borrow mut` gives the body exclusive access but does not change the read-only projection contract.
The unchanged source remains valid for the body; a later replacement or mutable-borrow call ends
the old generation and invalidates all derived views through the existing invalidation analysis.
The source remains matchable again after a read-only match. An owning match continues to mark the
selected source moved and rejects a later match or use.

### 2.3 Control flow

The borrowed mode applies through `match` arm joins and must compose with `if`, nested `match`,
`else`, `?`, loop back-edges, replacement, and early exits. A result produced by an arm is checked
independently of the borrowed payload binding: a scalar result is ordinary, a view result carries
the binding's source roots, and an owned result must be explicitly constructed or cloned. No arm
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

The checked-HIR validator, MIR validator/fingerprint, per-unit interface serializer, generic
monomorphization, and cache identity must all carry and validate the same ordered path and mode.
Missing, out-of-range, variant-incompatible, type-inconsistent, or mode-inconsistent metadata
fails closed at the checked boundary. LLVM must reject malformed paths before emitting a GEP/load;
there is no fallback to a copied aggregate or runtime helper.

No runtime representation, allocation routine, native ABI field, JSON tag, package import, or
special function is introduced. The existing flattened user-sum and builtin `Option`/`Result`
layouts remain the representation. The capability is a frontend/MIR/backend read-only projection
over those layouts.

## 4. Implementation closure matrix

| Axis | Owner boundary | Required implementation and regression evidence |
| --- | --- | --- |
| Formation and distinction | `align_sema::check_match`, borrowed-place classification, checked HIR | Direct `Option<string>`, `Result<string,string>`, user sum with a Move payload, borrowed root, depth-1 field, nested field, and `borrow mut` positives. Owning twins retain Move extraction. Fresh/temporary scrutinees do not acquire hidden storage. |
| Binding mode and type | HIR match arm records, MoveCheck, borrow facts | Binding retains the payload's static type for field/method lookup but is marked read-only. Copy fields read, owned string fields borrow as `str`, explicit `.clone()` is accepted, and a whole-payload transfer is rejected. Or-patterns remain binding-free. |
| Payload projection | MIR `lower_match`, borrowed-place/path operands, `lower_expr_for_borrow` | Tag branches read the original sum, active payload binds through a checked pointer/path, no shallow Move aggregate copy occurs, no source nulling occurs, and no binding Drop/cleanup bit is emitted. |
| Backend lowering | `align_codegen_llvm` borrowed path and type checks | Direct and nested struct/sum paths use in-place GEP/load. Malformed paths, wrong variants, wrong types, missing roots, and invalid cleanup metadata fail as lowering errors rather than panic or fallback copy. |
| Ownership and escape | MoveCheck, `EscapeCheck`, return-borrow and generation summaries | Repeated read-only matches preserve the source; return/store/call/task/closure capture of a whole borrowed payload is rejected; derived views carry roots; replacement and Drop invalidate old generations. |
| Control flow | match arm joins and all value-producing control forms | `None`/`Some`, `Ok`/`Err`, wildcard/or-pattern, nested match, divergent arms, `else`, `?`, `if`, loop back-edge, early return, and borrowed-field replacement each retain the correct source/borrow state. |
| Aggregate and cleanup | recursive `DropPlan`, field/path classifiers, source nulling | Nested `Option<MoveStruct>`, user sums with multiple Move payloads, mixed Copy/Move fields, optional strings, caller exit Drop, and owning-vs-borrowed parity show no double free, leak, or stale cleanup bit. |
| Interfaces and caches | checked-HIR replay, interface serialization, MIR fingerprint, per-unit and generic consumers | Imported direct and generic consumers preserve the mode/path; definition edit/revert invalidates exact dependents; corrupted projection metadata is rejected; whole-program and per-unit output agree. |
| Existing surface parity | `Option<str>`/Copy match, owning Move match, `else`, `?`, and current negative owner | Existing view-region tests stay green. `match_cannot_extract_a_move_payload_from_a_borrowed_parameter` becomes positive read-only coverage with explicit consuming negatives, while all owning cleanup tests remain unchanged. |

## 5. Acceptance owners

The implementation PR must add the narrowest stable owners rather than a broad aggregate solely for
reachability:

- `align_sema` unit coverage for the mode distinction, borrow-root propagation, explicit consuming
  diagnostics, and malformed checked-HIR metadata;
- a focused `align_driver` owner for direct and nested `Option<string>`, `Result<string,string>`,
  and user-sum Move payloads, including repeated matching and source use after the match;
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
- No recursive/boxed sum types, arrays of Move elements, or changes to the existing supported
  recursive Drop graph.
- No change to by-value match, `else`, `?`, `map_err`, or ownership transfer semantics.
- No permission to return or retain an owned payload through a borrowed match; only ordinary derived
  views may follow the existing borrow summary and region rules.
- No application sentinel, wrapper record, alternate verifier signature, compiler-known package
  type, or JSON-specific match exception.

## 7. Author-side consistency pass

Before implementation begins, verify that:

- the Match paragraphs in `draft.md` and `docs/language-spec.md`, the rationale in
  `docs/design-notes.md`, this ledger, and the Settled entry in `docs/open-questions.md` use the
  same borrowed-place predicate, binding mode, escape rule, and owning-mode parity;
- every public promise in §1 appears in the exact matrix row and in at least one named owner;
- no row adds a runtime/ABI/JSON/cache identity that §3 forbids;
- examples distinguish declarations from positional calls and syntax-check in the implementation
  owner; and
- the plan does not consume evaluator/provider work or any later C6 milestone.

The capability is not implementation-ready until this pass and one independent design review are
complete. Implementation follows from the reviewed merge and reopens this matrix only if the
strategy or public contract changes.
