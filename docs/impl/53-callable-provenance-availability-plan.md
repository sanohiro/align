# Shared string callback provenance

## Boundary

The source-valid `apply(text: str, action: fn(str) -> str) -> str =
action(text)` rejects an identity callback during per-unit producer
certification. Its concrete callable summary has a parameter root, while the
written formal has `None`: the existing representation uses an empty summary
for both unavailable targets and proved empty target summaries.

This capability accepts shared string callback results without adding a type
refinement, changing nominal identity, or changing any HIR, MIR, interface or
canonical ABI record. Plan 52 continues to own ordinary-call write authority;
R46 array-field assignment remains outside this capability.

## Producer contract

A shared string result is a finite by-value shape composed exclusively of `str`,
primitive scalars, structs, tuples, enums, options and results, with at least one
`str` leaf. Containers, references, raw pointers, function values, owning leaves
and unknown or cyclic shapes are excluded. One iterative fail-closed predicate
owns this domain; an empty protected-leaf list is not evidence of membership.

Only the outer callable value-flow relation may accept a rooted actual into an
empty formal for this domain. Parameter modes, cleanup, canonical parameter
and result types remain exact. Nested callable signatures retain exact identity.
Other result domains retain the existing directional root-subset relation.
Concrete function-address and closure construction, target/capture records,
declarations and copied indirect signatures retain their existing exact checks.

An indirect selected `str` result in this domain is Shared after authenticating
the callee and every argument, type, mode and cleanup record. This rule applies
even to a populated signature: empty-to-rooted joins must not recover stronger
access. A shared result needs no inspectable capture environment from an opaque
formal. Exact closure construction still authenticates capture inventories.
Other result types use the existing producer rules.

Shared value authority is distinct from writable backing and from writable
local-header storage. Plan 52's buffer graph rejects Copy call results as proof
of writable backing, including direct wrappers, retypes, copies and joins.
Canonical borrowed descriptors independently prove local-header replacement.
A lifetime root in an owned callable argument cannot authorize writes through
its returned string. This capability must not weaken either storage proof.

## Source lifetime analysis

The analysis-only callable target set retains an unavailable alternative through
formal inputs, returned callbacks, copies, projections, capture transport and
joins. Joining a known target cannot discard that alternative. At publication
to the existing summary consumers, unavailable sets use the existing unresolved
fallback, retaining all compatible argument and callee lifetimes. Concrete-only
sets preserve target-relative capture precision. No availability bit is written
into a source type or persisted artifact. The region escape checker consumes the same target availability as borrow and
storage inference; `None` with an unknown alternative is not Static. Checked-HIR
replay recomputes the same analysis; generic and imported written inputs use the same rule.

## Implementation closure matrix

| Axis | Implementation / owner |
|---|---|
| Type formation / nominal identity | Existing records and canonical codecs unchanged; returned generic `Holder<fn(str)->str>` positive, numeric sibling, nested-signature negative in codegen and driver owners. |
| Construction / move-in / move-out / return | Exact FnAddr/Closure signatures and captures unchanged; shared-result flow only in callable compatibility and direct argument certification. Driver local/imported identity, captured, fixed, generic and returned-record callbacks. |
| Joins / replacement / early exits | Analysis-only unavailable alternative is monotone; known+unknown lifetime escape negatives, existing `return_provenance` branch/loop/`?`/`map_err` owners. |
| Ownership / Drop / allocation | No runtime or cleanup change; excluded Move/callable result domains retain old rules. `borrowed_params`, `resource_ownership` and codegen malformed cleanup owners. No performance claim or benchmark. |
| Mutable storage / out | Shared call result cannot certify byte writes, directly or through wrappers/joins; codegen forged-write negatives. Mutable local string-header replacement remains accepted. `imported_mutable_retention` owns lifetime retention. |
| Opaque captures | Shared-result invocation authenticates the opaque callable but does not demand a concrete environment; constructor capture mutations remain rejected. Local/imported captured callback controls. |
| Whole / per-unit / generic / interface | Driver `return_provenance` checks and executes local/imported/generic readers in both compilation modes, rejects argument/capture escapes. Existing interface bytes and canonical identities unchanged. |
| Malformed / deep input | Explicit iterative return-domain walk rejects cycles/invalid ids and excluded leaves; producer mutation tests reject wrong modes/types/callee/cleanup/copied signatures and writable laundering. |

The shared-result strategy received independent inspection before implementation.
Author matrix-to-diff verification maps the changed domain and nested identity to
`shared_string_callback_domain_and_nested_identity`, exact invocation producers
to `shared_string_callback_invocations_authenticate_producers`, and direct,
indirect and joined writable-buffer refusal to
`shared_string_callbacks_cannot_publish_writable_backing`. The driver owners
`unavailable_callback_provenance_preserves_readers_across_units`,
`shared_string_callbacks_preserve_structural_results`,
`unavailable_callbacks_do_not_erase_argument_or_capture_lifetimes` and
`unknown_callback_join_retains_argument_and_capture_lifetimes` cover executable
positive and compile-only negative boundaries. The sema owner
`callable_availability_keeps_unknown_alternatives` also rejects stale checked-HIR
summaries by replay. Existing callable-constructor, capture, cleanup and deep
owners remain required. Option shapes have internal domain coverage; this
capability does not widen source function-value formation restrictions.

## Initialization proof closure

The producer review found that a Shared access seed could authenticate its own
uninitialized argument or closure capture through a call/store cycle. Access
classification is not an initialization proof. Before access propagation, a
finite monotone readiness worklist must ground operation inputs independently:
a value-producing operation requires all input-check nodes plus a seed, absence,
or an ordinary source. Storage remains an OR join of entry/ordinary stores and
separately gated action seeds. A native output or complete element-field write
cannot make an unrelated entry/store wait for that action's inputs. Static
absence is a founded alternative; it never supplies READ or WRITE authority.
Cached evidence is reusable only after both readiness and capability validation.

The owner matrix adds initialized/uninitialized twins for indirect argument
values, borrowed-place arguments and closure captures; each is checked at
publication, whole-program emission and ThinLTO validation. Existing direct-call
cycles, native outputs, field assembly, absent payloads, Out buffers and seeded
storage joins remain required owners. This corrects the existing initialization
invariant without adding a source or IR contract.

This capability can exceed 1,000 changed handwritten lines once its producer
closure owners are included. The callback admission, conservative source
lifetime analysis and independent initialization proof must be reviewed and
merged together: publishing only the admission would expose the lost-lifetime
or circular-proof defect, while a dormant intermediate creates no useful
consumer and duplicates the same cross-boundary evidence.
