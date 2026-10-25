# Indexed fixed-field independent values

Current disposition: this plan owns fixed-field Copy snapshots.
[Plan242](242-indexed-field-views.md) separately supplies explicit retained
read-only indexed-field ranges using Plan240 call authority. Its contract
supersedes the historical view deferrals below; the original operation and
its ownership/evaluation rules are unchanged.

## Evidence and selection

AM-R16 has two different consumers: `inputs[row].shape[index]` returns independent
inline data; `inputs[row].shape[..end]` retains an address into the parent. Both
exact source forms reject in whole/per-unit checking at e314ba41. The media
broadcast planner uses a named Copy-record workaround and has 1,120 consumer-owned
layout/value cases; this is convenience/composition evidence, not a measured speedup.
LLM R94's original fixed-table consumer is retired, while Voice V006 independently
qualifies complete fixed-field Copy reads from heap records. No sibling code changes.

Implement the independent indexing consumer first using the already supported
complete fixed-field Copy value and ordinary local Index. This is one complete
useful expression operation. It does not publish a dormant view producer or depend
on the wider returned-permission repair. The scalar result can be used after the
parent is replaced or retired. Existing stable-place slicing remains unchanged.

The view assessment has a current compile-only witness:

```align
Row { shape: [i64; 2] }
fn prefix(borrow row: Row) -> slice<i64> = row.shape[..]
fn main() {
  row := Row { shape: [7, 12] }
  mut view := prefix(row)
  view[0] = 99
}
```

Both frontends accept it although the shared borrow does not grant write authority.
It was not executed. The corresponding reader also checks. Plan210 identifies the
same missing returned permission: ReturnBorrowSummary carries input indices but
strips ReadOnly. New retained indexed-field views must close permission, lifetime,
helper/import returns, mutable destinations and admitted carriers together; no
return-only flag, temporary view or implicit owning clone is selected here.

## Exact public contract

| Surface | Contract |
| --- | --- |
| Source and type | `rows[row].path.fixed[index]`, where the existing checked indexed-field operation can already read `fixed` by value, its type is `[T; N]`, and the complete field satisfies the existing independent_copy_type graph. T may be an already-formed primitive or independent Copy record; views, pointers, callables and owning elements remain excluded even at N=0. Existing parent/container formation and field-path rules remain unchanged. The result is the ordinary fixed-array Index result T. |
| Evaluation | Evaluate the parent receiver, outer index and existing signed outer bounds/read exactly once. Snapshot the complete fixed-field Copy value before evaluating the inner index. Evaluate that index once, perform the ordinary signed N bound and read the element. Outer failure suppresses the inner expression; inner divergence suppresses its bound/read. The snapshot remains independent if the inner expression replaces the original parent. This is exactly the existing explicit `fixed := rows[row].path.fixed; fixed[index]` composition. |
| Ownership and allocation | The complete inline field is Copy data with no retained backing, owner, Drop, native handle or callable environment. Its value copy adds no heap/arena allocation and no implicit owning clone. Ordinary optimizer scalarization may remove unnecessary copied bytes; no throughput or copy-elision promise is made. Result use after parent move/replacement/retirement is valid. |
| Bounds/errors | Existing outer/index type, field path and Copy-value admission errors remain producer-owned. The inner index is i64 under current index checking; source checking retains its existing index diagnostic order. N=0 always fails a reached inner bounds action, after outer observation and index effects. Negative/high indices abort under the existing bounds contract. |
| Representation | Sema normalizes the new receiver composition into existing Block/Let(ElemField)/Index(Local). Chained record-field projection binds that ordinary independent Index result before an ordinary Field read; both checker entry points share the same fixed-field snapshot normalization. No new HIR/MIR variant, special permission, interface field, runtime export, ABI, native/wire encoding or persisted format. Concrete generic substitution rechecks the complete graph; existing source/compiler/definition identities invalidate caches. Ordinary validators replay every generated node. |
| Exclusions | No indexed fixed-field slice/coercion/pipeline, temporary view, new borrowed payload, whole indexed Move-record borrow, indexed write or Move-field extraction. Arbitrary fixed-array expressions still require a binding. The source-index composition does not broaden formation or the shared borrowed_sum_payload_is_admissible classifier. |
| Effects and defaults | Pure except for the existing receiver/index effects. No new default, ambient option, text validation or process-global state. |
| Consistency | Update the fixed-field access contracts in draft.md, language-spec.md, design-notes.md, Settled fixed-array entry, Plan73 and HIR ledger; synchronize core-design/array-slice-pipeline EN/ja. Plan230/HANDOFF record the partial AM-R16 provider capability and the separate view prerequisite once. V006's direct-index negative is replaced with its still-excluded direct slice twin. |

## Closure matrix

| Axis | Implementation and owner |
| --- | --- |
| Formation and graph | check_index and check_index_field use one shared normalization for an existing ElemField with a fixed independent Copy type, creating a synthetic local plus ordinary Index. A chained direct/nested record-field projection uses that same complete-array snapshot before selecting the final field; final-field independence alone never qualifies a view-bearing array. Keep the existing independent graph classifier unchanged. A bounded driver owner parameterizes primitive and nested Copy-record elements, nested fixed-field paths, N=0/1/3/37, fixed/dynamic/slice parent classes, admitted Move parents with a Copy field, and excluded view/owner/callable twins. Pair every newly accepted form with its explicit-local existing control. |
| Source effects and bounds | Native whole/per-unit owner checks receiver/outer/inner exactly-once order, outer negative/high/empty failure before inner effects, inner negative/high/zero-width failure, if/match/else/?/map_err/block/loop/early-return inner expressions. Parent invalidation during the outer index preserves existing complete-field refusal; replacement during inner evaluation is safe after the snapshot and observes its original value. Cross these effects with a whole independent record result and direct/nested field projection, so the check_index_field entry cannot skip the snapshot or repeat either index. |
| Lifetime, moves and allocation | Independent primitive/record results survive parent replacement, move, return and arena retirement; positive allocation observers distinguish the parent allocation from a zero-allocation read. No new Drop or source nulling: existing record/fixed-array lifecycle remains authoritative. |
| Replay and imports | Whole/per-unit imported concrete/generic helpers plus cold/hit/element-definition-change/exact-revert cache owner. Native lowering validates the canonical ordinary Local/Index/ElemField nodes in both compilation modes; existing forged-HIR field/index/place owners continue to reject malformed publication. No producer permissions are synthesized. |
| Neighbor boundaries | New direct slices, coercions, pipelines, arbitrary temporary arrays, writes and shared fixed-field payloads retain their previous restrictions. Existing fixed_array_fields and indexed_array_field_copy_reads suites plus V006 owner are the affected neighbors. |

The author ledger-to-prose pass fixes the key semantic choice: this is an eager
independent value snapshot, not a retained field address. It makes the existing
explicit Copy binding composable in one expression and leaves no new permission
transport. Independent boundary review must confirm that distinction before code.

## Independent boundary review closure

The fresh inspection accepted the eager independent-value strategy and found one
P2: chained record fields enter check_index_field instead of check_index. Both
entrypoints now belong to the one planned normalization and effect owner; the
complete fixed array must qualify before a final scalar field is selected. This
changes no lifetime or permission strategy. The explicit-binding record result
control crosses an explicit-region builder, arena retirement and returned Point;
whole/per-unit checking and native execution pass at e314ba41. Retain that control
beside the new source spelling because primitive results bypass record-region tracking.

## Outer receiver completion audit

The new negative owner exposed a pre-existing local ElemField gap: `rows[take(rows)].values`
checks even though MIR loads the header before the consuming index and then reads
through that old pointer. The same source with a scalar field also checks; an
ordinary Index and a field-rooted receiver correctly reject. These are compile-only
witnesses, never executed. MoveCheck's local ElemField arm checked named liveness
but omitted receiver evaluation, so the existing eager completion machinery never
registered the loaded header. Walk dynamic/slice/SoA receivers before their index
just as the ordinary Index and nonlocal ElemField paths already do. Fixed locals
instead supply a stable inline slot; retain their selected-field move check so
an independently moved sibling does not invalidate the whole local. This reuses the existing
generation/alias/invalidated-completion proof and introduces no new reservation,
IR shape or safety strategy. The matrix now covers local, field and returned-view
receivers, outer consume/replacement/exclusive mutation, both frontends, and the
valid independent snapshot followed by parent Drop during the inner index.

## Author matrix-to-diff closure

- `check_index` and `check_index_field` share `index_with_fixed_field_snapshot`;
  `independent_fixed_field_value` reuses the unchanged complete independent graph.
  `values_imports_and_cache` covers concrete generic fields, chained/nested records,
  fixed/dynamic/slice parents, explicit Copy controls and record return across arena
  retirement. Its cold/hit/definition-change/exact-revert checks pair interface/MIR
  identity with native execution. Existing checked-HIR validators replay the ordinary
  nodes; neither their record contract nor any IR variant changes.
- `snapshot_effects_and_control` owns exactly-once receiver/outer/inner effects,
  both checker entrypoints, nested record projection, replacement, and successful or
  terminating `if`, `match`, `else`, `?`, `map_err`, loop and return paths.
  `bounds_and_termination` retains reached bounds even for discarded results, checks
  signed outer/inner limits and N=0, and proves divergence suppresses later effects.
- `primitive_fields_and_owned_parent` enumerates all primitive families and N=1/37,
  distinguishes positive parent allocation from allocation-free observations, and
  consumes the parent during the inner index after its independent snapshot.
  `receiver_completion` owns the local ElemField receiver walk: direct and explicit
  reads, scalar siblings, replacement, exclusive helper mutation, aliases, returned
  views, field roots and joins reject invalidated backing in both frontends. Its
  controls retain early termination and valid inline storage replacement.
  `pkg_template::resource_fields_in_fixed_move_record_arrays_borrow_finish_and_drop_once`
  retains independent sibling extraction, replacement, nulling and exact Drop after
  partial moves; it rejects treating a fixed receiver as a whole-value use.
- `excluded_neighbors` keeps complete-graph view/pointer/callable/owner refusals,
  including zero-length twins, direct slices/pipelines/writes and arbitrary temporary
  arrays. `fixed_array_fields`, `heap_fixed_scalar_fields`,
  `indexed_array_field_copy_reads` and `owned_structs_arrays` retain formation,
  replacement, view, source-nulling and Drop coverage. No owner, runtime, ABI,
  allocation strategy or source-null operation is introduced by the normalization.
- The normative fixed-field paragraphs agree across the specification, digest,
  rationale, Settled entry, HIR ledger, Plan73 and core-design EN/ja. Plan230 and
  HANDOFF distinguish the implemented independent-value capability from retained
  views. No performance claim needs a benchmark.
