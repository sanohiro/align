# Consumer-boundary root-cause investigation

## Scope and evidence discipline

Baseline: Align `73251b30` (PR #991), after the consolidated G1 delivery.
The owner requested an Align-only investigate-and-repair batch before resuming
normal performance work. The external request register, related issues and
merged fixes, review finding memory, and existing verification plans are inputs,
not a ceiling on the search. Consumer code and adoption remain out of scope.

A confirmed defect needs a reproducible compiler verdict or independently grounded
source invariant; a suspected gap is not a defect claim. Record surveyed paths,
unconfirmed axes, and explicit deferrals separately. Unsafe witnesses are checked
for rejection and never executed. Safe controls must exercise the same topology.
The default boundary is one coherent repair batch and one PR. Before implementation,
close the cause inventory into an implementation matrix and explain any larger
boundary; do not partition by symptom, test, or line count.

## Shared cause inventory

| ID | Cause class and historical evidence | Current investigation status |
|---|---|---|
| C1 | Missing or over-broad cross-call facts: Requests 42/43/49/62, #983 exact mutable-retention transport, format 10; check versus per-unit disagreement. | Exact interface transport remains unchanged. Confirmed an indirect-call str storage-lifetime false rejection; selected-path producer and cache owners close the related stage boundary. |
| C2 | Conflated owner identity, writable backing and retained lifetime: #982 mixed opaque/header field roots, #991 separate source/destination lattice; G1 06 derived-byte helper still rejects a safe complete-owner use. | Confirmed and repaired: a headerless selected buffer inherited unrelated sibling provenance. Nested owners and str/optional-writer siblings have parameterized controls. |
| C3 | Evaluation-time values conflated with active borrows: Request 48 / G1 11 copied scalar beside mutable owner, Request 46 loop-local mutation. | G1 11 is a settled direct-place restriction, not an implementation defect; widening deferred. Request 46 belongs to the confirmed completed-snapshot cause C4. |
| C4 | Incomplete control-path transport: #991 loop completions and `map_err` Ok forwarding; its review P1 short-circuit bypass, corrected with shared conditional CFG. | Confirmed and repaired completed initializer/store/discriminator snapshots surviving into joins. Existing early-exit/error/eager-order owners remain required. |
| C5 | Partial ownership paths and cleanup: Requests 45/52, #978 nested Move rejection, #980 replaced-value cleanup, #979 borrowed payload authentication. | Historical repairs merged. Survey fields, sibling containers, nesting, move-out/nulling/replacement/Drop; distinguish deferred syntax from unsafe acceptance. |
| C6 | Producer evidence lost or invented across stages: Requests 57/59, #950/#972/#974/#975 plus #984 chunks, #985 Out backing, #986 target-relative callable storage. | Confirmed and repaired borrowed selected element/field/subview read equations rejecting canonical BorrowedPlace operands; general ownership transfer remains closed. |
| C7 | Analysis convergence and validation cost: Request 58 fixed-point termination, #991 pending-block dedup; Request 37 and gate scheduling remain separate performance work. | Survey convergence only where reached by a correctness cause. No unrelated performance promise or budget widening. |
| C8 | Verification topology missed the defect: #960 reduced owner did not reproduce old failure; #982 missing indexed/derived/imported/indirect cases; #991 short-circuit absent from initial retention matrix. | Determine why each confirmed defect escaped existing owners. Prefer parameterized cross-products and structural tripwires over another isolated reproducer. |

## Investigation assignments and closure

The root agent owns this inventory, specification decisions, implementation
integration and PR boundary. Independent investigators report evidence and proposed
matrix cells for (1) borrow invalidation and evaluation-time precision, (2) lifetime,
control flow and ownership siblings, and (3) stage/interface/cache propagation.
They may expand from any confirmed cause, but do not edit production code until
the shared invariant and implementation boundary are settled.

The cross-product includes direct/helper/imported/generic/closure/indirect calls;
branch/short-circuit/loop/early-exit/error/eager-order paths; sibling and nested
fields/containers; construction, move-in/out, nulling, replacement, return and Drop;
whole/per-unit and cache cold/hit/edit/revert. Unchecked cells remain explicitly
unconfirmed rather than implicitly passing. Final owner selection follows
`16-test-policy.md`; broader coverage does not authorize a longer test budget.

## Evidence sources

- External `../align-llm/docs/align-requests.md`, including the numbered requests above.
- Issue #990 and frozen G1 bundle: 21 complete rows on baseline, 06/11 explicit
  precision deferrals, unsafe 08/09 rejected, native golden controls accepted.
- `align-llm-request-audit-2026-09-07.md` (historical selection evidence).
- `17-library-boundary-prerequisites.md`, `19-hir-validation-ledger.md`,
  `xml-producer-investigation.md`, `10-cache-first-optimization.md` and
  `16-test-policy.md` (owning contracts and verification).
- `.claude/skills/align-self-review/FINDINGS.md` and preserved #991 plan/full-diff
  review logs (finding evidence, distinct from author test failures).


## Confirmed baseline results and failure mechanisms

- C2: derived `buffer.bytes()` passed to a mutating slice helper rejects later
  whole-owner use only when the owner also contains an unrelated `str` or
  optional writer. Inline mutation and array/slice field siblings pass. A field
  without its own tracked header currently falls back to every root of its
  aggregate, importing unrelated sibling provenance. Preserve the canonical
  opaque owner root without treating sibling headers as the selected buffer.
- C3: copied i64/bool/f64/u8/Option<i64> fields beside a mutable argument reject
  in direct, generic and named-function-value calls. Block/arithmetic/helper
  copies pass. Initially suspected as false rejection, this is explicitly required
  by `draft.md`'s all-peer direct-place restriction and the Settled borrowing decision
  in `docs/open-questions.md`. It is therefore a language precision limitation,
  not a confirmed implementation defect. Widening follows the friction-ledger
  protocol; this batch preserves direct-overlap rejection and checks the explicit
  independent-copy control. G1 11 remains deferred.
- C1/C2: `retain(values[0], output)` accepts a borrowed string-array source,
  while the same function selected through a function value rejects it. The
  conservative storage edge caps a str at the local declaration frame, although
  str copies a view of bytes rather than borrowing its pointer/length slot.
  Storage retention for str now uses the selected byte lifetime; local owned
  array sources remain rejected. The existing
  `borrowed_params::owned_string_array_index_views_preserve_control_provenance`
  also detects this baseline failure.
- C4: an array-derived guard followed by conditional mutation or replacement
  rejects a later current-owner read; independent-boolean guards and an earlier
  expression-statement read pass. Completed initializer/condition descendants
  remain in `value_headers` after their evaluation has finished. These stale
  snapshots participate in generation joins. Plain generation joining alone is
  not the confirmed cause; retain actual live alias headers and eager reservations.
- C6: borrowed Option/Result arrays of `str`/`string`, including record string
  fields and cloned selected elements, pass sema but fail producer certification.
  Direct arrays and Copy slice carriers pass. `SliceIndex` and `IndexFieldPtr`
  read a canonical `BorrowedPlace`, while the generic producer source helper
  accepts SSA operands only. #979 authenticated selected clone/call inputs but
  did not close these element-read siblings.

The C6 failure already exists in
`borrowed_params::owned_string_array_index_is_a_non_consuming_str_view`; its new
parameterized imported owner also fails before production changes. This target
is outside the bounded gate. The last three nightly runs were cancelled; the
latest full-suite step ended at its 30-minute job bound. Its unavailable full
job log does not prove whether this particular owner ran. Add cheap invariant
owners to the existing sema/codegen library targets so these repaired classes
are mandatory without moving the whole deep driver suite into the gate.

The ownership investigator completed 64 whole/per-unit probe verdicts over
retained nested buffer/string views and returning control forms before its tool
stopped. Admitted negative/control pairs agree; unsupported closure syntax and
slice-result function values are excluded, not counted as accepted evidence.
A pre-loop local-owner view returned through `break` remains a conservative
Frame-lifetime precision limitation. It is not evidence of an unsafe acceptance.
The other investigator's tool also stopped after preserving its probes; the root
continues the unfinished source analysis, rather than claiming a completed review.

## Implementation closure matrix and capability boundary

This batch restores the existing distinction between completed copied values,
selected borrowed storage and live ownership; it adds no source API, type/IR
variant, interface format or runtime ABI. It follows the existing storage and
producer-worklist strategy. One PR keeps the shared positive/negative proof and
whole/per-unit/cache evidence together. If the combined class owners exceed
1,000 handwritten lines, keeping this boundary avoids duplicate retention,
replacement and imported-producer proof and reduces integration risk between the
same analysis consumers; line count is not a reason to separate dormant fixes.

| Axis | Required closure and owner |
|---|---|
| Type formation and Copy argument values | Preserve the settled all-peer direct-place restriction. `consumer_copy_arguments_preserve_settled_place_restrictions` checks scalar/optional direct overlap and explicit independent-copy controls, plus Borrow/BorrowMut and str peers. Generic/indirect baseline probes reject consistently; broadening is explicitly deferred through the friction ledger. |
| Selected opaque fields | A headerless inline-handle projection retains its complete opaque owner without inheriting unrelated sibling headers. `derived_view_mutation_preserves_disjoint_owner_facts` crosses buffer, nested fields, str/optional writer siblings, helper/inline, local/import/indirect and parent replacement/retained-view negatives. |
| Completed action frontiers | Clear only consumed expression snapshots after installation or discriminator evaluation. Preserve assigned value headers, outer eager operands, loop-edge facts, staged owners, real retained aliases and exact borrowed-place reservations. `completed_actions_do_not_retain_operand_headers` crosses let/assignment/tuple, condition/short-circuit, loop, early exit, source-order, safe current-owner reads and invalid old-view reads. |
| Projected read producer | A read-specific adapter authenticates exact borrowed slot/path/type/cleanup, follows every reachable storage producer and supplies no independent ownership seed. Use it for element, record-field and subview read siblings; do not admit a general borrowed Move operand into `Use` or transfer. `borrowed_projection_producer_paths_preserve_readable_elements` plus a parameterized codegen mutation owner cover string/str, Option/Result, record fields, clone/view and malformed mode/slot/path/cleanup/reverse-owner cases. |
| Construction, move-in/out, nulling, Drop, replacement and return | No lowering or runtime lifecycle change. Existing complete `borrowed_params`, `return_provenance`, imported retention, borrowed replacement and resource ownership owners remain required where they touch these corrections. Retaining a view after its real owner ends must continue to reject. |
| Interfaces and cache | Keep format10 exact summaries and source-type identity. Differential owners check whole/per-unit; cache cold/hit/private edit/revert must preserve both new positives and actual retaining/invalidating negatives. Existing malformed interface and producer publication owners remain mandatory. |
| Validation topology | Add discriminating class-level sema/codegen library owners to the bounded gate, retain the full source owner where it already detects the defect, and run selected owners before review. No timeout increase or unrelated nightly/performance redesign. |

No public contract or new safety strategy is proposed: the existing exact
borrowed-read worklist, Copy semantics, selected opaque owner authority and
completed-action snapshot boundary remain the sources of truth. The author-side
matrix-to-diff pass and one fresh full-diff review will check the expanded consumer
boundary. Unsafe source programs are never executed.

## Author-side closure and retained limits

| Cause / obligation extracted from the owning contract | Implementation and discriminating evidence |
|---|---|
| HIR ledger: a completed scalar read retains its resolved value, not a generation observer; mutable-place reservations remain separate | `MoveCheck` clears completed initializer/store subtrees after installation and boolean discriminators before branch splitting, in ordinary and transparent worklists. `consumer_completed_action_frontiers_preserve_current_storage` pairs current-owner reads with invalidated old-view rejection across let, assign, field/index, short-circuit, tuple and loop. A Some(bool) match control already passes baseline; borrowed/Move match storage remains live. |
| Library boundary: structural provenance and complete opaque-owner invalidation | `storage_roots(Field)` excludes unrelated sibling headers for headerless inline handles and preserves `stable_owner_root`. `consumer_opaque_field_roots_exclude_unselected_siblings` crosses nested owners, str/optional writer, direct/indirect helpers and later parent invalidation. Imported source owner and existing `borrowed_replacement`/`resource_ownership` cover caller identity and Drop. |
| Borrowed str denotes selected bytes; an indirect fallback cannot invent a shorter slot lifetime | `EscapeCheck::retained_storage_region` uses str's selected value region. `consumer_indirect_str_retention_uses_selected_byte_lifetime` crosses str/string arrays, direct/indirect, caller/local sources. Both existing borrowed-string-array owners now pass. |
| Producer ledger: every selected edge is typed and grounded; no read may manufacture ownership | `read_source` authenticates slot/path/view type/cleanup and queues the selected slot dependency. SliceIndex/Noalias, IndexFieldPtr and SubSlice use it. `consumer_projected_reads_authenticate_every_source_edge` covers Option/Result, str/string, records, clone, subview and six malformed axes in whole/thin validation. General Use and transfer sources remain unchanged. |
| Cache ledger: private edits miss only the owning unit, restored content can hit, and a rejecting unit is never published | `borrowed_projection_reads_survive_private_edit_and_cache_restore` and existing `retained_buffer_view_rejects_after_cached_helper_edit_and_revert` pass in the complete `unit_cache` owner. Imported differential source checks and safe native whole/per-unit execution also pass. |
| Settled all-peer direct-place restriction | No alias-predicate change. Scalar/optional direct overlap rejects while an independently bound copied value passes. G1 11 requires the friction-ledger protocol, whose five-sites/two-program threshold is not established by synthetic probes. |

The author survey found no new unsafe acceptance in the admitted negative/control
pairs. This is bounded evidence, not a claim that the compiler is free of defects.
The 64 saved investigator verdicts include unsupported lambda captures and
slice-returning function values; those are excluded as correctness evidence.
Existing `borrow_liveness`, `borrowed_params`, `borrowed_replacement`,
`return_provenance`, `imported_mutable_retention`, `resource_ownership` and
`template_ownership` owners cover their existing generic/closure, control, move,
replacement, cleanup and lifetime axes. There is no new runtime/ABI/IR/interface
shape, so allocation parity follows unchanged lowering and the existing lifecycle
owners rather than a new resource-performance claim.

Explicit limits: independent opaque handle fields still share complete-owner
identity; the pre-loop local-owner view returned through break remains a conservative
lifetime limitation; nested borrowed matching and unsupported function-value
shapes remain deferred language surfaces. No native GPU or external consumer
adoption is claimed. Nightly cancellation is an unresolved out-of-gate detection
limit, to be revisited in the paused performance work; repaired cause owners now
run in the existing bounded sema/codegen library targets. Unrelated solver and
runtime paths were not requalified merely because they appear in the initial
cause inventory. These limits are not silently marked as surveyed successes.
