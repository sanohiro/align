# Disjoint field origins through shared match bindings

## Scope and existing contract

Request130 supplies a real nonblocking caller: a shared payload from
`state.optional` is read beside a writable byte view from `state.scratch`.
The provider baseline 40f2c45 rejects both argument orders as aliased in whole
and per-unit checking. The reduction and exact diagnostics are retained with
the request assessment. No consumer source or compiler pin is changed.

Plans26/28/37 own read-only sum-payload matching; Plan65 owns the existing
stable disjoint-field/type-graph proof. Compose those existing capabilities.
Plan80's existing exact mutable-view effect proves whether a call can rebind
its direct Copy-view parameter; preserve a local origin only when that checked
effect says no rebind. Unknown or possibly rebinding effects discard it.
Retain sole parent ownership, ordinary generation invalidation and the complete
borrowed payload grammar. Do not admit exclusive matched payloads (R142),
partial exclusive Move fields (R132), arbitrary returned view authority or K1.
No new syntax, allocation, clone, runtime helper, ABI or interface record.

## Implementation closure matrix

| Axis | Implementation obligation and discriminating owner |
| --- | --- |
| Origin representation | Reuse the existing private StoragePath/BorrowProjection vocabulary for local origin places, preserving StructField versus TupleElement and selected Option/Result/enum payloads. Do not represent a payload field by appending an untyped ordinal to its containing Option field. Existing ordinary field/tuple/view origins remain exact. |
| Borrowed construction | At each checked shared-match arm, seed only authenticated borrowed bindings from the scrutinee origin plus its exact active payload projection. Owning match results and unrelated locals do not acquire a borrowed-place certificate. Source and checked-HIR replay rebuild the same facts from the existing metadata. |
| Pair proof | Preserve direct/prefix overlap and every non-parent root. Traverse common typed segments with the existing projection type resolver. Exclude common parent roots only at a divergent pair of actual struct fields or tuple elements satisfying the existing nonborrowing/resource-only type-graph condition. Distinct enum arms, dynamic indices, missing paths and contained view aliases are not disjoint-storage proofs. |
| Assignment, move, replacement and Drop | Convert existing struct invalidation paths to typed segments. Assignment, owner replacement, branch intersection and loop joins clear or replace origins as before. A returning mutable call preserves an eligible local Copy-view origin only with an available exact no-rebind effect, captured after eager argument validation; unknown/possibly rebinding calls still discard it. Existing call alias/reservation checks exclude overlapping owner replacement. Native repeated calls plus unconditional/conditional/indirect rebind negatives close this effect consumer. Parent ownership and generation changes still invalidate selected borrowed payloads/views. No source nulling, cleanup or ownership authority changes. |
| Control and call order | Some/None, Ok/Err, admitted user sums, nested record paths, direct/reversed/generic/indirect/imported calls, repeated calls, if/match/else/?/map_err, early exits and loops. A later argument replacing an earlier selected owner must reject. |
| Negative controls | Same field/backing, containing owner, two aliased Copy views, a borrowed-view payload, a rebound scratch view, expired/replaced selected payload, exclusive payload and partial Move-field calls remain rejected. Existing malformed borrowed-match metadata controls authenticate origin producers before replay. |
| Native and replay | Exact bytes and independent sibling values in whole/per-unit execution; cache cold/hit/provider edit/restoration with existing source identity. Counted allocation/cleanup through ordinary/early/error paths, proving no allocation from matching/borrowing and one destruction per owner. |
| Owners | `shared_match_field_origins::source_matrix`, `rejects_aliases_and_expiry`, `possibly_rebinding_calls_discard_origins` and `native_lifecycle_and_replay` own the new path, control, refusal, imported/cache and native obligations. Reuse `disjoint_field_borrows` for field/view rebind and overlap, `borrowed_params` for read-only payload restrictions; bounded-gate `align_sema::borrowed_sum_match_metadata_rejects_forged_records` authenticates the existing match record, and `resource_ownership` for the existing type-graph/borrow-leaf proof. `return_provenance::mutable_copy_view_*`, `imported_mutable_retention::mutable_copy_view_effect_transport` and bounded-gate `align_sema::mutable_view_effect_replay_rejects_stale_facts` own the unchanged effect transport/authority boundary. No performance promise or benchmark. |

## Author boundary assessment

The safety rule remains Plan65's reviewed type-graph proof at a stable divergent
field, using Plan26's already-authenticated read-only payload projection. The
missing implementation fact is the origin path, not permission to weaken an
alias test. This is private semantic analysis and its owner coverage; no HIR/MIR
shape or serialized field is proposed. The named owner matrix covers every changed path above. Those paths and owners
must be checked against the implementation before its one full-diff preflight
review. If inspection reveals a new authority/lifetime strategy, stop and
review that strategy before implementing it.

## Author implementation closure

`local_origin_places` uses existing typed StoragePath segments; ordinary field
and tuple origins, assignment invalidation and exact branch intersection remain
one mechanism. Borrowed arm bindings append the authenticated payload projection
to the evaluated scrutinee origin. `is_disjoint_sibling_fields` traverses those
types but permits divergence only at struct fields or tuple positions, retaining
the existing no-borrow/resource-only leaf proof and all non-parent conflicts.

An eligible local Copy view keeps its captured origin after a returning call
only when its existing checked effect excludes header rebinding. This does not
restore borrow generations, readable/writable authority, byte validation or
arbitrary observer facts. Eager reservation and call alias checks still reject
a concurrent source-owner replacement. Possibly rebinding and unavailable
effects retain conservative origin loss. No effect producer or record changes.

The source matrix covers Some/Ok/Err/user payloads, direct/reversed/indirect calls
and nested records through producer and LLVM validation. Native imported generic
forwarding and repeated direct/indirect calls execute across loop/error/normal
returns and cache cold/hit/provider edit/restoration. Allocation/free counters
remain fixed across borrowing and requested-live bytes return to baseline after
normal/error/None paths. Negative owners retain overlap, containing-owner calls,
view-bearing siblings, rebound views, expired payloads and consuming/exclusive
payload refusals. Existing lifecycle/type-graph and malformed metadata owners
close the unchanged source ownership and checked replay paths.
