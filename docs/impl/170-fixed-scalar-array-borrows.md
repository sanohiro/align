# Borrowed local fixed scalar arrays

Request148's `[i64; 4]` mutable local passes source checking but fails MIR
producer certification at an ordinary `borrow mut` call. Shared local borrows,
computed literals and zero-length locals have the same refusal. Arrays reached
through record fields, returned whole values and borrowed parameters already
work. Plan159 records this independent whole-storage descriptor limitation.

The existing fixed-array and explicit-borrow contracts in draft.md and plan73
already admit these calls. Preserve syntax, exact type, inline layout, Copy
classification, exclusive access, bounds and lifetime rules. There is no new
allocation, runtime ABI, IR variant, interface record or source admission rule.
K1, general aggregate provenance and owned/view/callable array expansion remain
deferred. This capability covers the existing plain scalar family: integers,
floats, bool and char; it must not infer payload authority for other families.

## Canonical construction boundary

Separate literal construction from later mutable storage in the existing MIR.
Every non-pooled plain scalar ArrayLit local binding first constructs an
anonymous inline array, loads its complete Copy value, then stores that value
into the named local. Empty literals follow this complete-value route. Reuse the existing array-literal materializer used by field construction.
Keep nonempty pooled bindings as direct StoreConstArray, preserve the named local's
explicit alignment, and publish neither the whole Load nor Store after an element
expression terminates. This introduces a compiler temporary, not a heap owner;
ordinary whole Copy initialization already has this value semantics.

Keep the original strict check_fixed_array_construction_operand for anonymous
construction storage. It still requires exact typed cardinality, unique in-range
constant initialization before its whole load, every readable element input and
no additional writer. Missing, duplicate, mistyped, late, non-dominating, dynamic
or out-of-range construction writes must reject. Do not weaken this predicate to
accept later mutations: those now target the separately initialized named root.

A canonical root borrow of a non-parameter plain array requires exact slot/type
and unique reachable use identity, plus complete typed whole-value Store nodes
intersecting every path before each borrow or whole Load. Both sides of a branch join may supply
that complete value. Require the existing readable Element graph for nonempty
arrays and whole-value graph for empty arrays. These graph edges retain every
root operand's complete construction proof and every later typed writer. No new
empty-array seed, current-writer reconstruction or recursive analyzer is needed.
Separate storage write/exclusive authority remains mandatory for borrow mut.

Nonempty pooled roots have a disjoint sealed constant-only proof: if any writer
is StoreConstArray, every writer must be an exact typed, complete StoreConstArray.
Reject root Store, indexed, field and native-out writes rather than falling back
to the ordinary-root route. An all-path initialization cut proves a complete
store precedes each use. Multiple complete constant stores are allowed because
loop_facts duplicates initializers into fast/slow copies while retaining slots.
This retains direct rodata access without certifying arbitrary indexed writers.

Named roots retain the existing post-construction mutation proof. In particular,
loop_facts may remove guards from a proved fast loop without invalidating array
construction. Source lowering and the existing optimization rederivation still
own runtime bounds; this capability does not add a general indexed-store bounds
validator or trust serialized optimization metadata. Anonymous construction
storage must never be reused as a mutation destination.

Loop versioning clones an isolated nonempty plain-array construction slot along
with its statements. Qualify it from the actual pre-rewrite MIR: no parameter or
cleanup-slot identity, no borrowed/address operand, only exact constant-index
StoreIndex writes and one exact-typed whole Load, all accesses inside the cloned
body, one complete set of typed in-range writes, and each writer preceding or
dominating that reachable read. Preheader and function metadata references also
exclude isolation.
An exhaustive direct-slot inventory complements the existing operand inventory;
unknown operations fail closed. Preserve type and alignment, allocate checked
fresh slot IDs, and rewrite only the admitted StoreIndex/Load fields in the
clone. Other slots remain shared. This preserves the original strict anonymous
proof independently in both fast and slow bodies, with no trusted metadata.

Every StoreField consumer separately checks the complete plain-array operand,
even when the current producer query selects an owned sibling field. Selecting
String ownership must not discard the scalar array's typed/readable obligations.
Whole SSA values alone retain copied-bits normalization; borrowed storage does
not gain independent Copy ownership. Field and parameter descriptors keep their
existing paths. All four direct/indirect MIR call forms share the call check;
source function values with fixed-array parameters and generic `[T; N]` element
assignment retain their existing refusal. Out parameters gain no read authority.

## Implementation closure matrix

| Axis | Implementation and exact owner obligation |
| --- | --- |
| Formation and construction | Lower non-pooled plain scalar local literals through complete anonymous value construction and named-root Store; sealed pooled locals retain direct constant storage. `fixed_scalar_borrows` parameterizes seven scalar representatives and N=0/1/4/37, constant/computed/pool construction, explicit destination alignment and terminating element expressions. Owned/view/callable families retain their existing lowering and strict proof. |
| Construction versus mutation | Keep the original anonymous writer scan. Append invalid constant N/-1 and SSA/runtime-index writers after otherwise complete construction, before/after its Load; mutate payload type, cardinality, bulk records, SSA and input authority. Whole/partition/codegen must reject. Named-root mutations remain valid and cannot be mistaken for construction. |
| Loop-local construction | `fixed_scalar_construction_inside_versioned_loop` requires actual versioning for pooled/non-pooled local construction and producer/native success. Loop-facts unit owners reject isolation for outside uses, direct indexing/slices, borrowed/cleanup operands, incomplete or non-dominating initialization and unknown writers; preserve explicit slot alignment. |
| Initialization at each use | One unique reachable call/whole Load; exact descriptor/root type; every path crosses a complete typed whole-value Store before the use. Remove one branch's store, delay publication or create an ungrounded cycle; reject forged slot/type/SSA identities. Empty values retain complete root dependencies and the existing validated anonymous empty literal. |
| Read and exclusive authority | Element readability and descriptor storage authority remain separate. Shared/mutable positives, parameter forwarding, unreadable Out inputs and shared-to-exclusive negatives close the new matrix. Existing owned_fixed_record/fixed_dynamic_arrays owners retain owned/view sibling restrictions. |
| Projected consumers | StoreField attaches the complete plain-array operand even for an unselected sibling. A mixed String/array record tests type, unreadable input, malformed indexed/bulk/root construction and empty cycles. Publishing a mutated named array into that record remains a native positive. |
| Mutation and replacement | Repeated indexed mutation before/after calls, whole Copy loads, record publication and whole replacement preserve exact values and independent copies. An affine mutation loop followed by borrowing/copying must retain actual loop versioning and native correctness. Move-in/out, source nulling and Drop are unchanged because the repaired family is Copy-only. |
| Calls and control | Local/imported/direct calls, generic markers, if/match joins, loop zero/one/multiple iterations, early return and Result else/?/map_err retain existing rules. All four MIR call variants use the actual Let identity. Source function-value/generic-element exclusions remain negative controls. Guarded dynamic and constant out-of-range source updates retain runtime bounds failure, including calls with bare empty locals. |
| Lifetime and overlap | Whole/per-unit checking rejects shared mutation, overlapping peer arguments and returning a view of local storage. No view retention or owner-provenance expansion. |
| Interfaces and replay | Imported whole/per-unit native execution and cold/hit checked-artifact replay agree. Compiler identity invalidates old artifacts; interfaces and public signatures are unchanged. |
| Runtime and allocation | LLVM retains inline storage without allocator/free/dynamic-builder calls. Inspect optimized initialization and explicit destination alignment. No portable performance or universal copy-elimination promise; no benchmark is required. |
| Sealed pooling | Exact constant-only stores and all-path initialization; malformed constant records and any additional indexed/field/root/native-out writer reject, including an extra well-typed whole Store that must not escape through fallback. A pooled binding inside an actually versioned loop remains valid. Existing const_pool S3 raw/optimized checks retain direct rodata access. |
| Regression boundary | Run fixed_scalar_borrows, owned_fixed_record, fixed_array_fields, fixed_dynamic_arrays, aligned_binding and const_pool. Existing copied-bits, borrowed producers, malformed calls, loop-fact and lowering owners remain in the bounded gate. K1 and general aggregate certification are outside this matrix. |

## Review-driven boundary redesign

The initial candidate generalized the anonymous construction predicate to allow
later writers. Its full-diff review found that complete required cells could hide
an additional out-of-range write, and a StoreField query selecting a sibling
could lose payload checks. A proposed blanket guard check then conflicted with
valid loop_facts fast copies whose checks had already been proved and removed.
Rejecting only literal indices would leave SSA-wrapped invalid writers unproved.

The revised boundary preserves the original construction predicate and expresses
named mutable storage through the already supported whole-value initialization
path. It removes the need for a second bounds-proof system and adds the missing
complete StoreField obligation. Before coding, the author matrix pass and focused
independent review check pooled storage, destination alignment, terminating
elements, anonymous/root identity, optimized loops and empty dependencies. The
final candidate receives a fresh full-diff review because this changes the safety
strategy. If its second review finds a new equivalent correctness issue, reopen
the matrix rather than continuing a local patch loop.

This capability may exceed roughly 1,000 hand-written changed lines because the
new malformed, mixed-sibling and optimized-loop owners close the same construction
to borrow/publication boundary. Splitting a dormant constructor producer from its
consumer would duplicate validation and leave this defect open between PRs.

## Existing owner correction

The fixed-field formation table still rejected `[array<i64>; 2]`, which plan165
already admits. It now positively checks that existing type while retaining the
nested-array and native-owner exclusions. This is a stale test expectation, not
a semantic admission change in this capability.
