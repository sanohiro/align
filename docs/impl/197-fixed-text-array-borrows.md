# Borrowed local fixed text arrays

At Plan196's provider baseline, local `[str; N]` literals pass source checking
but fail whole-array shared/exclusive calls in MIR producer certification.
Returning the same array through an ordinary helper works; wrapping it in a
whole borrowed record works. Empty local arrays fail too. This is the Copy-view
sibling of Plan170's fixed scalar construction defect, exposed while qualifying
Plans195/196 and requests AM-R01/R147/R137.

The existing fixed-array, Copy-view and explicit-borrow contracts apply unchanged.
Use Plan170's reviewed separation between anonymous literal construction and
named mutable storage, together with Plan195's separation between readable
contents and exclusive storage. A mutable borrow may replace a str header; it
does not gain authority to mutate or free the referenced text. No new syntax,
type, allocation, ownership model, interface, ABI or native helper is required.
Move-element arrays, inline record arrays and native arena decoder outputs retain
their distinct construction/cleanup proofs and remain outside this repair.

## Implementation closure matrix

| Axis | Implementation and owner obligation |
| --- | --- |
| Formation and construction | Extend the existing plain-Copy literal normalization to str: materialize a complete anonymous array, then Store its Copy value into the named local. Numeric pooled constants retain their sealed constant-only path; str literals remain unpooled under the existing source rule. Exact type/cardinality, N=0/1/many, computed and borrowed text, natural inline layout, and a terminating element are covered by `fixed_scalar_borrows::fixed_text_borrow_producer_matrix`. Explicit binding alignment retains its numeric-only source rule. No new formation rule. |
| Initialization and mutation | Authenticate a complete typed whole Store on every path before a root call or Load. Retain strict anonymous construction, all typed input producers and later named-root writers. SSA forwarding must keep an independent whole-value construction query beside selected payload queries. Missing, late, duplicate, mistyped, non-dominating, dynamic or out-of-range anonymous writes reject. Pooled storage cannot mix with root/indexed/native writers. Reuse Plan170's malformed owner with str twins where its input class changes. |
| Authority and lifetime | Require readable text independently from writable/exclusive header storage. Cover shared/exclusive forwarding, local/static/owned/arena input, replacement, retained views, overlap and expired owners. Copies retain text origins; no copied-bits normalization may mint writable text or ownership. Out remains unreadable until initialized. Existing source lifetime rules remain authoritative. |
| Control and optimization | Observe exact values through if/match/else/?/map_err, repeated loops, early exits, branch publication and whole Copy replacement. Computed text-header stores retain the existing unmodelled-statement optimization refusal. Both literal and computed text constructors keep that refusal; adjacent scalar owners retain actual fast/slow versioning and sealed pooled-constant coverage. No pooling, loop-admission or constructor-cloning rule changes. |
| Move and cleanup boundary | Array headers remain Copy and own no text. Borrowing, copying and repeated reads allocate/free nothing. Owned input text and arenas retain their existing sole cleanup authority on normal/error exits. Move arrays, source nulling and recursive Drop are unchanged. |
| Calls and replay | Direct/imported/concrete-generic shared/exclusive helpers, whole/per-unit native execution and cold/hit/provider-edit cache replay agree. All MIR call variants use the common existing call validator; malformed mode/slot/path/source and unused peers reject before emission. No serialized proof or public format change. |
| Regression owners | `fixed_scalar_borrows` (including its new text owners), `borrowed_content_calls`, `fixed_array_fields` and existing loop-facts/producer unit owners close the changed boundary. Reuse their negative matrices instead of duplicating unchanged owned-family fixtures. No performance claim or benchmark. |

## Author-side boundary pass

This follows the already-reviewed Plan170 construction strategy and Plan195
authority split. The new element class carries readable borrowed text but no
Move cleanup. All applicable matrix cells must map to the final implementation
and discriminating owners before the one independent preflight review. Keep
the original scalar predicate for scalar-only algorithms; introduce one explicit
Copy construction predicate where both scalar and str constructors share proof.
Do not broaden owned/callable/record construction or weaken malformed-input
checks merely to admit a valid root call.

## Adjacent owner correction

The fixed dynamic-array negative owner still expected source rejection for an
ordinary helper result returned through a loop break. The prior merged release
already accepts whole/per-unit checking and MIR emission for that exact source.
Move the witness into its existing native lifecycle/cache owner, preserving
allocation and cleanup counts, and remove the obsolete rejection expectation.
This corrects a stale owner; it does not widen Move-array semantics here.

## Completed implementation and owners

`xml_copy_fixed_array_element` selects the scalar/str construction family.
Local literals use the existing complete temporary-value Store path; named roots
require an all-path complete publication at calls and whole loads. Root and field
publication authenticate anonymous fixed-array construction independently of
selected payload queries, so a readable text sibling cannot hide a missing slot.
Non-Load operands retain a path-empty READ query through the existing equation
worklist, so SSA forwarding cannot erase that independent construction proof.
The existing strict cardinality/type/dominance/writer scan is retained. Named
storage and each text producer keep separate authority; no loop rule changed.

`fixed_text_borrow_producer_matrix` covers empty/single/many, literal/computed,
shared/exclusive and copied arrays. Text binding over-alignment retains its source
rejection; the existing numeric alignment owner remains positive.
`fixed_text_borrows_reject_incomplete_or_forged_storage` discriminates missing,
duplicate, mistyped, late and out-of-range/SSA extra construction, incomplete
publication, Out text, forged slots, shared mutation, overlap and escaping views.
It rejects at both full/partition producer boundaries and LLVM emission.
`fixed_copy_array_forwarding_retains_complete_construction` pairs complete and
incomplete str/scalar constructors through Use/Select forwarding into root/field
publication. The producer controls distinguish whole-array construction from
readable selected leaves; incomplete inputs also reject before emission.
`fixed_text_borrows_execute_and_replay` runs imported generic helpers through
whole/per-unit and cold/hit/body-edit/restored caches, observes independent Copy
headers, covers if/match/else/?/map_err/termination and arena use, and checks
allocation/free stability plus post-function requested-live cleanup with a
positive owned-allocation witness. `fixed_text_construction_in_loops` executes
literal/computed loops and pins their existing optimization refusal. Adjacent
scalar tests retain real versioning, sealed pooling and malformed branch/field
publication coverage; owned-string/dynamic-array native owners retain lifecycle
and transfer coverage. Shared lifetime analysis and HIR admission are unchanged.
