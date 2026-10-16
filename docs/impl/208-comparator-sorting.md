# Explicit comparator sorting over Copy elements

Status: implemented; owner verification and preflight review close this capability.

LLM Request27 still contains a real index-array merge sort in
`src/expert_trace.align::sort_spans`. Plans88/134 supplied direct borrowed-text
sorting and owned keys, while explicitly deferring comparator sorting. This
capability supplies that deferred terminal over the existing scalar Copy
element domain. It does not define aggregate ordering or customizable operators,
admit Move elements, or change callable ownership. The other two consumer
registers contain no comparator request. Media has no current sort caller;
voice `assets.align` uses direct text-key and numeric interval sorts, which
remain covered by the existing-sort regression owners. Consumer adoption remains
external.

## Public contract ledger

| Surface | Exact contract |
| --- | --- |
| Terminal | `source.sort_by(cmp) -> array<T>`, exactly one required argument. T is an existing integer, float, bool, char or borrowed str, matching `sort_by_key`'s scalar Copy element domain. The source is an existing fixed array, slice, dynamic array or admitted pipeline with that final element. JSON scanner terminals and aggregate/Move elements remain excluded. There is no optional argument, default, new bound or source syntax. |
| Comparator | An existing named/lifted/imported callable with two by-value T parameters, existing Copy captures, and exact bool result. `cmp(a, b)` means a precedes b. Sequential comparators may be Impure under plan12; inferred effects still propagate through both explicit inputs, captures, imports and enclosing parallel boundaries. No purity annotation, exception or new callable transport is introduced. Generic calls inside existing comparator bodies instantiate normally. Direct generic function operands and generic-body lambdas retain existing restrictions; no new callable resolution is added. |
| Ordering | Stable ordering when the comparator supplies a strict weak order: equivalent elements retain surviving input order. The compiler does not prove the ordering laws. A terminating inconsistent comparator still yields a fully initialized permutation of the surviving input; it cannot cause an out-of-bounds access, lost/duplicated element or compiler/runtime memory unsafety. No sortedness promise applies then. Comparisons follow the explicit stable insertion/merge procedure below, including for Impure callbacks; empty and singleton inputs invoke it zero times. Callback divergence or a hard error has the existing behavior and no normal-result/cleanup promise. |
| Evaluation and effects | Reuse the existing collector's eager source/stage/capture evaluation and reservations. Evaluate each capture once before traversal. Complete surviving-element collection before comparisons. Stage effects retain existing input order and frequency. The comparator is sequential and its observable effects follow the comparison procedure below. Existing capture write/alias restrictions remain; no task, implicit parallelism, global state or hidden synchronization is introduced. |
| Ownership and lifetime | Borrow the source; materialize the existing owned heap/explicit-arena array spine. Copy element values, never clone string bytes or consume source elements. Source/stage element content lifetimes and conservative source-generation loans remain. Comparator-only captures stay live during the terminal; they are not stored in the result. Result construction, move, source nulling, replacement, return and Drop use ordinary array machinery. No per-element Drop is introduced. |
| Allocation and errors | The terminal visibly materializes its result and uses the existing conditional merge scratch. It allocates no key-decoration or key-owner column. Callback allocations, if any, remain those explicitly written in the callable. Invalid metadata/types/effects reject before LLVM; allocation overflow/OOM remains the existing terminal hard-error policy. No fallible wrapper, native text boundary, wire encoding or new error family. |
| Compiler and artifact owners | Sema owns admission, complete call effects and borrows; checked HIR authenticates exact two-input callable/element/capture records and replays ordinary effects; MIR owns stable comparison calls and cleanup. LLVM uses ordinary calls and existing scalar/string array storage. Add one explicit ArraySortCmp HIR variant and update every exhaustive/non-exhaustive semantic consumer. No MIR variant, runtime ABI symbol, package, interface field, cache format or ambient input is added. Existing structural MIR/compiler identity invalidates emitted artifacts. |
| Algorithm boundary | Reuse 32-element insertion runs and stable bottom-up merge. User comparator ordering laws are not producer evidence for adaptive ordered-run shortcuts: disable the total-order precheck and ordered-boundary shortcut for this terminal. For comparator sorting, always delay scratch until merging: N <= 32 allocates no merge scratch; N > 32 allocates the one element scratch spine after the insertion runs. This rule holds even with `ALIGN_SORT_ADAPTIVE=off`. Existing direct/key sorting retains its old off-mode up-front scratch baseline. No speedup or application-throughput claim. |
| Sources of truth | Update draft ordering, condensed spec, design-notes, Settled sorting text, the roadmap capability boundary, checked-HIR ledger19, core array/slice/pipeline English/ja and pipeline guide English/ja and plan12's sequential-effect/comparison contract. Add only a historical scope pointer in plan88; plan134's key ownership is unchanged. HANDOFF records implementation once after owner closure. |

## Comparison procedure

After collection, partition the surviving input into consecutive runs of at most
32 elements in input order. Within each run, visit positions from its second
through its last element. Hold the current value and inspect predecessors from
right to left. Call `cmp(current, predecessor)` once per inspected predecessor;
true shifts that predecessor one position right, false ends the scan. Place the
held value after the last remaining predecessor. Exhausting the run's beginning
requires no extra comparison.

Then merge adjacent runs with widths 32, 64, 128 and so on, processing pairs from
left to right at each width. While both sides remain nonempty, call
`cmp(right_head, left_head)` once; true selects the right head, false the left.
Drain the remaining side without comparisons. Finish every pair before doubling
the width, and finish when the width covers the input. Copies between scratch
and output invoke no comparator. No ordered-input scan or ordered-boundary
shortcut is performed, with either setting of the existing adaptive diagnostic
switch. Comparator scratch is always delayed until the merge phase, independently
of that switch; the off-mode up-front allocation baseline applies only to the
older direct/key terminals. This procedure fixes observable invocation order/count without requiring
comparators to be effect-free. Ordinary as-if optimization may still remove or
transform operations only when it preserves all observable behavior, including
hard errors and divergence; Pure alone does not imply total/speculatable.

## Implementation closure matrix

| Axis | Implementation and discriminating owner |
| --- | --- |
| Formation, concrete/generic validation | One source path admits the keyed-sort Copy domain and resolves `(T,T)->bool`. Checked HIR independently matches both parameters, exact result, all modes and captures, source/final/result types and local ordinals. `sort_comparator_domain_and_metadata` pairs valid integer/bool/char/str/float cases with wrong arity/type/result/mode, Move/aggregate/scanner inputs and malformed HIR records; existing generic/callable refusal remains. |
| New-variant sweep | Enumerate every ArraySort/ArraySortBy consumer before editing: child/capture traversal, replay clone/depth, type finalization, effect and callback boundary propagation, liveness/move, storage/escape/header/element facts, task access, HIR envelope/body, MIR collection/donation/drop and callback discovery. The enum sweep tripwire must compile; wildcard consumers get an explicit author checklist. Both comparator parameter origins come from the final pipeline element, not only parameter zero. |
| Effect authority | Extend existing ordinary call and captured-function effect propagation to the comparator, with both explicit parameters receiving final-element origin. Do not add a new Pure-required boundary. `sort_comparator_effects` covers direct/transitive/imported side effects, captured function values, observable comparison traces, ordinary impure stages and rejection when an enclosing parallel callable becomes Impure/Unknown. Reuse `effect_fail_closed` for closed/exported callback laundering. Checked-HIR effect replay must reject a forged Pure claim at an enclosing parallel boundary. |
| Construction and comparisons | Reuse `lower_array_collect` and the one sort implementation with an explicit comparison strategy. Pass `(right,left)` at merge and `(current,previous)` for insertion; false retains the earlier element. Bind captures once and pass them after both explicit operands through ordinary named calls. `comparator_sort_order_and_stability` checks independent expected permutations across empty/one/31/32/33/63/64/65/129 lengths, reverse/duplicate/multi-key/text orders, filtered/mapped sources and both adaptive settings. All-true and cyclic comparators must terminate and preserve every identity exactly once. |
| Ownership, source nulling and Drop | Copy-only elements reuse shallow result/scratch Drop; no key columns or cleanup-bit transfer. `comparator_sort_order_and_stability` covers repeated materialization, array move/return/replacement, early returns and heap/arena scopes. MIR owner `comparator_sort_uses_shared_storage_and_calls` authenticates the two operand/capture order and absence of decoration/owned-key columns; existing sort allocation owners retain baseline policy. |
| Borrow/content/control | Match ArraySortBy's result element facts, source-generation loans and capture-call reservations. `comparator_sort_lifetimes_and_control` covers caller-owned text results, local/captured stage-view escape, owner mutation/move/truncate and arena expiry; comparator-only local text must not become output content. Admit writes to independent captured output only under existing sequential callback authority; reject writes invalidating source text or live overlapping views. Cross if/match/else/?/map_err and branch/loop joins around the terminal, eager source/capture failure, and stage termination. No new proof transport or partial-exclusive borrowing. |
| Imports/cache/serialization | `comparator_sort_order_and_stability` runs imported named/generic producers and lifted captures in whole/per-unit and cold/hit frontend-cache modes. Source-template instantiation and existing interface effects remain authoritative; HIR clone/depth and structural MIR identity retain the new variant/calls. No serialized field or native ABI change. |
| Neighboring behavior | Run the focused new driver/HIR/MIR owners plus existing `sort_strings`, `sort_by_key`, `sort_owned_keys`, `sort_merge`, `sort_adaptive` and `effect_fail_closed` only where they own changed lowering/effect behavior. Workspace build, bounded PR gate and separate-target Clippy precede publication. No benchmark is selected because no new performance improvement is claimed. |

One capability includes source admission, complete ordinary effect propagation, HIR/MIR
transport and executable consumers. The diff may exceed roughly 1,000 handwritten
lines because one HIR variant must cross every analysis and needs independent
ordering/lifetime/effect owners. Splitting those producers and consumers would
leave an unusable or unauthenticated callback path and duplicate the same proof;
the complete terminal is the useful merge boundary.

## Author ledger pass

The ledger specifies all public arguments/results, callback order laws and exact
invocation procedure, ordinary effect propagation, source/capture/result
lifetimes, materialization/scratch and failure behavior. There are no option
states, wire bytes, native state changes or new artifact records. Plan12's
settled sequential-Impure rule applies; the comparator does not introduce a
Pure-only exception. Both explicit callback arguments need the same final-element
provenance in every analysis, and comparator/capture effects must reach the
existing closed/open-world parallel checks. Existing strict-weak-order assumptions cannot
justify adaptive shortcuts for arbitrary comparator bodies. Each promise maps
to an owner above; source examples will be syntax/behavior checked with the
implementation before the authoritative prose is updated.

## Author matrix-to-diff closure

- `check_array_sort_by` and shared `resolve_fn` enforce the source domain/signature.
  `sort_comparator_domain` rejects both borrowed-parameter positions, scanner,
  Move/aggregate elements and bad arity/result/type. Existing direct generic
  callable operands are not a new supported form; imported ordinary wrappers
  invoke generic comparison bodies in the execution owner.
- `pipeline_callable_ok` takes an explicit type sequence. The metadata mutation
  owner checks each operand and all producer fields; full replay separately owns
  forged parallel effects. The imported effect owner covers absent/Unknown/
  Impure/Pure authority. A captured callable is bound as a local inside the lambda
  before invocation, following existing capture lookup.
- The variant sweep pairs every existing ArraySortBy analysis arm with the new
  terminal, except key type/cleanup, which do not exist for comparisons. Escape
  content initialization and borrow-result folding explicitly exclude comparator
  captures; eager child/capture walks and access reservations still include them.
  Both compared-parameter origins use the final pipeline element. Result headers,
  source loans, arena allocation, Move/Drop and termination use shared owners.
- `comparator_sort_order_and_stability` checks independent stable permutations,
  all-true/cyclic comparisons, actual Request27 span/index captures, scalar domains,
  stages, repeat materialization and imported generic bodies. It executes whole,
  per-unit and frontend-cache-hit MIR; cold/hit identities are asserted.
  `sort_comparator_effects` pins the exact 33-element stage/comparison trace and
  rejects enclosing parallel effects, including through a captured callable.
- `comparator_sort_lifetimes_and_control` covers caller-owned views, comparator-only
  local captures, local/stage view escapes, replace/move/truncate, arena and loop
  expiry, if/match/else/?/map_err, return, replacement and early source exit.
  Returning a newly materialized view array directly from a loop retains the
  existing conservative loop-escape restriction; this capability does not widen it.
- `comparator_capture_write_authority` distinguishes an independent initialized
  captured output view from a source-byte invalidating write. Existing out-parameter
  unread-content restrictions remain; no new output/capture ownership is added.
- `comparator_sort_uses_shared_storage_and_calls` checks ordinary two-input/capture
  calls, exactly two comparator call sites and result/scratch-only spines. Shared
  donation and terminating-capture owners include `sort_by`. The executable
  `comparator_sort_scratch_and_drop` measures exact allocations/frees at lengths
  1/32/33/65 under both adaptive settings; no throughput claim or timing benchmark.

The six executable owners run within the existing bounded fixture/process-group
harness. No runtime ABI, native boundary, persisted format or package changes are
needed. The plan review resolved its scratch-option completeness finding before
implementation; there is no change to the reviewed ownership strategy.

The author mutation pass exposed a shared pipeline-capture gap: mutable-access
summaries recorded caller/task effects but did not end byte-validation observations
for writes through copied capture headers. `record_pipeline_access` now projects
that same summary onto completed capture backing, invalidates the existing byte
proofs, and validates repeated capture uses before the terminal source snapshot
is released. The parameterized write owner covers comparator, keyed and map
callbacks, source views and sibling captured text, with independent initialized
numeric output as the positive control. This uses existing backing/validation
identity and error policy; it does not introduce a new alias or ownership rule.
