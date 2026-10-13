# Borrowed local fixed scalar arrays

Request148's `[i64; 4]` mutable local passes source checking but fails MIR
producer certification at an ordinary `borrow mut` call. Shared local borrows,
computed literals and zero-length locals have the same refusal. Arrays reached
through record fields, returned whole values and borrowed parameters already
work. Plan159 records this independent whole-storage descriptor limitation.

The existing fixed-array and explicit-borrow contracts in draft.md and plan73
already admit these calls. Preserve their syntax, exact type, inline layout,
Copy classification, exclusive access, bounds and lifetime rules. No allocation,
copy, runtime ABI, IR variant, interface record or source admission rule changes.
K1, general aggregate provenance and owned/view/callable array expansion remain
deferred. This capability covers the existing plain scalar family: integers,
floats, bool and char; it must not infer payload authority for other families.

## Proof boundary

The producer graph currently asks for a whole slot's empty path even though
literal stores supply element paths. Plain arrays need both complete initialized
storage at the particular call and readable reaching element producers. Neither
proof alone is sufficient. Keep storage write/exclusive authority separate from
payload readability; reading constants must not manufacture exclusive access.

Use one pure initialized-at-use helper for canonical root borrows and whole
plain scalar Rvalue::Load values at their unique reachable use site. A mutable
local can be copied after earlier indexed writes and before later writes; it
is not an anonymous literal whose entire writer set must be initialization.
For either use, require an initialized parameter, complete typed whole-value/bulk
stores intersecting every path to the use (including both sides of a branch join),
or in-range constant stores for every element that each dominate the use. Reuse
the existing CFG reachability/dominance machinery. Later indexed writes are
mutations, not duplicate literal initialization; their types and input producers
still participate in the element graph. Missing cells, a repeated index replacing
a required cell, wrong types/cardinality and initialization occurring only after
or on one path around the use must reject. Whole loads keep their existing
copied-bits normalization only after all element dependencies pass; borrowed
storage never receives that normalization. The plain-scalar branch of
check_fixed_array_construction_operand delegates to this helper, including its
StoreField caller; publishing a mutated local into a record field receives the
same proof. The helper does not recursively build another producer analyzer.
No array-length-sized allocation is needed: count distinct observed in-range
indices and compare against N.

Nonempty payload proof follows the existing Element projection, including all
root stores and later element writes. Empty plain arrays have no element edge:
retain whole-value dependencies for parameters/root stores; only bare zero-bit
storage or exact zero-element constant stores with no other incoming producers
or writes can supply their own readable seed. Preserve the existing exact empty
bulk-construction form; malformed bulk records receive no seed.
Cycles, malformed writes and unreadable inputs must never gain that seed.
The existing call-shape/type and shared/exclusive storage checks still apply to
all four direct/indirect MIR call variants. Source function values still reject
fixed-array parameters; generic element assignment through `[T; N]` also retains
its existing refusal. Generic source coverage uses a concrete fixed-array
parameter beside a generic marker. Out parameters do not acquire read authority.
Field paths retain their existing producer proof.

## Implementation closure matrix

| Axis | Implementation and exact owner obligation |
| --- | --- |
| Formation and construction | Retain exact Ty::Array element/N and descriptor slot/type checks. `fixed_scalar_borrows` parameterizes int/float/bool/char, constant/computed/pool construction and N=0/1/4/37. Existing HIR validation remains unchanged. |
| Initialization versus use | One shared load/call-site proof requires complete dominating initialization. The same owner mutates MIR to remove, duplicate, mistype, delay or branch an initializing store, corrupt bulk cardinality/type, forge the root/type/SSA identity, and introduce an ungrounded cycle. Whole and partition certification plus codegen must refuse. |
| Read and exclusive authority | Element payload readability and descriptor storage authority remain separate. Shared and mutable positives, borrowed-parameter forwarding, unreadable out-parameter inputs, shared-to-mutable controls close the new matrix. The unchanged closed plain-element predicate excludes views and owned headers; existing owned_fixed_record and fixed_dynamic_arrays rejection matrices close those siblings. No storage is normalized into an independent Copy owner. |
| Mutation and replacement | Repeated indexed mutation before/after calls, whole Copy loads and whole replacement retain every typed producer. The native owner copies a previously mutated local before subsequent loop writes and publishes that local into a record field. Reuse unchanged fixed-array field and owned-record owners; native source checks observe the original caller's storage and independent by-value copies. Move-in/out, source nulling and Drop are unchanged because the repaired family is Copy-only. |
| Calls and control | Local/imported/direct calls and generic-marker helpers share the proof. All four MIR call variants retain the same argument-validation closure; source function-value and generic-element assignment exclusions remain negative controls. Source owners cover if/match joins, loop zero/one/multiple iterations, early returns and Result else/?/map_err around completed borrows. Malformed non-dominating initialization and unreadable arguments remain failures. No general join algorithm changes. |
| Lifetime and overlap | Mutation through a shared parameter and overlapping peer arguments retain sema refusal in whole/per-unit modes. Returning a view of the local must still fail. This correction adds neither view retention nor owner provenance. |
| Interfaces and replay | A small imported fixture checks/builds/runs in whole-program and per-unit modes, including cold/hit checked-artifact replay. Compiler identity invalidates old artifacts; interface format and public signatures remain unchanged. |
| Runtime and allocation | Native execution verifies exact values after mutation, replacement and early exits. Inspect generated LLVM for unchanged inline storage and absence of new allocation calls. No allocation or performance improvement is claimed; no timing benchmark is required. |
| Regression boundary | Run the new owner, owned_fixed_record, fixed_array_fields and fixed_dynamic_arrays. Existing copied-bit, borrowed-producer and malformed-call owners remain in the bounded MIR/codegen gate. K1 and general owned/view aggregate certification are explicitly outside this matrix. |

The author consistency pass checks this matrix against the existing scalar-copy
and borrowed-storage rules before one independent adversarial boundary review.
Resolve that review before implementation, then use one fresh full-diff preflight
review after the compiling owner-backed candidate. Source specifications need
no restatement because this restores their existing promise.

## Boundary review and author refinement

The independent preimplementation review accepted the initialized-at-use and
separate-storage-authority strategy. Its supplemental inspection closed the
StoreField caller of the same plain-array construction predicate. Source-shape
checks retained the callable/generic-element exclusions. The owner-backed
implementation additionally preserves all-path whole-value initialization at
branch joins and the already valid exact zero-element bulk constructor. These
are the same completeness invariant, not a new provenance model.

## Qualification

`fixed_scalar_borrows` covers the seven scalar representatives at N=0/1/4/37,
imported native execution, whole/per-unit emission and cold/hit artifact replay.
Its malformed MIR matrix checks full and partition certification, with direct
LLVM refusal for invalid construction and descriptors. Source negatives retain
shared-write, shared-to-exclusive, overlapping-argument, escape, callable and
generic-element restrictions in both modes. The raw LLVM owner observes inline
`[4 x i64]` storage and no allocator, free or dynamic-array builder calls.

The existing `owned_fixed_record`, `fixed_array_fields` and `fixed_dynamic_arrays`
owners retain zero-bit copy cycles, malformed owned-header construction, lifetime
and cleanup coverage. The old fixed-field formation table still rejected
`[array<i64>; 2]`, which plan165 already admits; it now positively checks that
existing type while keeping the nested-array and native-owner exclusions. This
was a stale test expectation, not a semantic admission change in this capability.
