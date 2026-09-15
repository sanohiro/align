# Array prefix mutation and UTF-8 boundary inspection

Status: U/P0 implemented; T and the safe-storage capability remain pending. Audit date: 2026-09-15.
Provider baseline: `61b2de79576fde043d5f310c1300370c02250fc3` (PR 1056).
Consumer inspected: `b00d3fa9ec0130d762eff6c34712f022ca06172e`.
This document extends [plan 65](65-open-issue-batch-plan.md). It owns the new
public records below; plan 61 remains authoritative for interprocedural access.
It does not reopen settled syntax or claim that pending methods are shipped.

## Public-contract ledger

Declarations below are signatures, not passing source examples. Receiver and
arguments evaluate once in source order. A terminating child prevents the
operation. All operands are type-checked before receiver/domain admission;
malformed checked records reject before source analysis or native emission.

| ID and exact surface | Inputs, defaults and errors | Ownership, lifetime, allocation and effects | Owner, prerequisite and acceptance |
| --- | --- | --- | --- |
| U: `str.is_char_boundary(index: i64) -> bool` | One exact i64 byte offset, no default or implicit numeric conversion. False outside `[0, len]`; true at 0 and len; interior true exactly when `(byte & 0xc0) != 0x80`. No Result, runtime bounds failure or UTF-8 boundary failure for any i64 input on a valid live string. | Shared text receiver; existing owned-string receiver borrowing also applies. Copy bool result; Pure; no allocation, mutation or retained view. Source text must be valid at the reached operation. | Sema/HIR and MIR own semantics; LLVM only lowers guarded MIR. Existing text validity and source-completion machinery, with plan 61 transfer registration when that capability integrates. No prerequisite on completing plan 61 for this read-only operation. U owner below. |
| T: `array<T>.truncate(new_len: i64) -> ()` | One exact i64 count, no default. Dynamic arrays only; stable mutable local or an already-admitted exclusive whole-array/record-field place. `0 <= new_len <= old_len`; failure is the existing terminal collection-range failure before Drop or header mutation. No growth, clamping, returned array or recoverable error. | Exclusive receiver; preserve backing pointer, allocation provenance, physical allocation and retained prefix. Drop each removed owned element exactly once, then publish new length. No allocation, copying, compaction or realloc. Mutation rooted in an owned local or explicit `borrow mut` input remains Pure absent another Impure operation; captured mutation and cleanup/native effects keep their existing classification. Source loans and retained observations obey the exclusive-operation contract below. | Existing dynamic-array layout and complete recursive Drop owner, plans 19/61 and plan 65 A/O. Implement with the pending safe mutable-storage capability, not ahead of its access consumers. T owner below. |
| C0: existing `slice<T>.to_array() -> array<T>` | Existing supported element grammar, allocation errors, evaluation and result semantics remain. No API addition. | Fresh materialization retains existing ownership and lifetime obligations. Do not infer deep-copy permission or source independence from Copy alone. | Existing `lower_array_collect` and LLVM optimization. The i64 witness already uses optimized memcpy; preserve it with the narrow C0 owner. Other layouts are separate evidence, not a universal memcpy promise. |
| P0: existing `str.starts_with(prefix: str) -> bool` / `ends_with(suffix: str) -> bool` | Compare prefix/suffix bytes; empty needle true, longer needle false. No arbitrary interior slice or boundary trap. | Borrow both valid live strings; Pure; no allocation, returned view or retained argument. Worst-case compared bytes bounded by needle length; no exact libc symbol or SIMD speed promise. | Existing string primitive/runtime and `m5::str_predicates_are_byte_oriented_utf8`; clarify the English/Japanese string guide with U. |

T admits all existing well-formed contiguous dynamic-array representations,
without introducing new element types: `DynArray`, AoS `DynStructArray`,
`DynSliceArray`, `DynVecArray`, `DynMaskArray`, `DynFixedArray`,
`DynFixedStructArray`, and `DynResponseArray`. Their exact element and Drop
interpretation comes from the existing type/array owner, not a method whitelist
or a fresh Copy test. SoA, fixed-length array values, slices, builders and buffers
are not dynamic arrays and do not acquire this operation. A missing complete
Drop implementation for an otherwise admitted representation blocks the unified
capability; do not ship a numeric-only truncate with silent omissions.

The array's region and individual-cleanup bit remain unchanged. The cost is
O(1) when removal needs no per-element destruction, including numeric Copy
arrays; otherwise it is the cost of the existing Drop plans for the removed
suffix. Dropping nested owners may free their storage. The outer allocation is
retained even at length zero. An individually owned heap allocation is eventually
freed once; arena storage is released only by arena teardown, and stack backing
by its frame. Neither arena nor stack array Drop individually frees that outer
storage. T makes no promise to return memory to the allocator or reduce RSS.

U/T add compiler records, not native public exports, flags, ambient configuration
or persisted user data. Their exact semantic fields are:

```text
HIR StrCharBoundary { receiver: Expr<str>, index: Expr<i64> } -> bool
HIR ArrayTruncate { receiver: exclusive typed place, new_len: Expr<i64> } -> ()
MIR ArrayTruncate { destination: validated typed place, new_len: Operand<i64> }
```

The string record owns the existing source receiver borrow; its initial MIR
expands to comparisons, CFG and one guarded byte load. T's element type and
cleanup provenance are derived from its validated destination, never supplied
as a second overridable type. Canonical transport uses the owning HIR codec and
plan 61 statement schema; no independent serializer or opaque raw-call escape.
Plan 61 reserves AccessInstruction tag 28 for T, followed by its Place subject
and i64 operand in that order; all nested tags, little-endian widths and sequence
rules come from that one codec. U expands to existing MIR records and adds no
access-wire tag. T extends the still-unpublished interface-13 schema; do not
introduce a separately published version-13 placeholder. If the actual base
changes before implementation, reconcile the entire bundle/version/golden set.
For T at root slot 3 with `new_len = Value(7)`, the statement-only golden is
`1c0103000000000000000107000000` (15 bytes). It contains no optional fields.
The independent author encoder/reader checks both directions, every proper
prefix, a Value destination and trailing bytes. Full-bundle reference/type and
source-admission negative controls remain implementation owners.

Compiler identities invalidate frontend/object/ThinLTO caches for changed HIR
records or semantic lowering. The physical array ABI remains `{ptr, len}`;
resource/type identity retains the existing nominal and reachable-definition
rules. Imported body and drop-thunk dependencies use the existing source bundle
and partition identities. No target-private body may disappear from the key.
No package milestone is advanced by this plan; U uses shipped prerequisites,
and T consumes the complete plan 61 capability at the existing milestone.

## Issue audit and evidence

All 11 current open issues were checked against plan 65's eight-issue snapshot.
The additions are [1054](https://github.com/sanohiro/align/issues/1054),
[1055](https://github.com/sanohiro/align/issues/1055), and
[1057](https://github.com/sanohiro/align/issues/1057); all three have no comments.
The other issue update timestamps have not advanced beyond the earlier audit.
Requests 89/90 in the external register correspond to 1054/1055. Issue 1057
has no numbered request in the inspected register; identify it by issue number.

| Issue | Verified current behavior | Decision |
| --- | --- | --- |
| 1054 | `truncate` is absent in whole/per-unit checking. `provider_runtime::truncate_ids` materializes `ids[0..count]`; the caller conditionally selects it after EOG. Direct `slice<i64>.to_array()` already becomes one optimized `llvm.memcpy` at release/no-runtime-LTO on local x86-64. | Add T with complete ownership and access closure. C0 is existing behavior to preserve, not a missing numeric-copy implementation. Retain the distinction between avoiding an allocation (T) and accelerating a required copy (C0). |
| 1055 | Integer match rejects in the parser. The real `direct_byte`/`byte_scalar` functions optimize to range comparisons, branches and a select; the helper is inlined. This is not an unchanged chain of all source conditions, and it does not establish a jump-table speedup. | Record the syntax request under plan 23, with two inspected dispatch workarounds in one program. Keep the settled sum-type match boundary. Compiler-derived optimization of existing if expressions remains available without a syntax extension. |
| 1057 | `is_char_boundary` and `get_slice` are absent in both checking modes. UTF-8 slicing calls `emit_utf8_boundary_check` after range validation. Existing prefix/suffix primitives compare bounded bytes. Native positive/negative Japanese prefix/suffix checks pass. Current `joined_equal` already uses byte loops, so the original aborting expression is a reported historical shape. | Add U and document P0. Do not add a second slicing method or aggregate `==` in this capability. Ordinary slicing keeps its existing terminal contract. |

Local evidence is preserved under `.git/issue-batch-20260915/followup-design/`:
issue JSON, checked source files and diagnostics, binary SHA-256, raw/optimized
IR and the prefix/suffix executable result. The 16 whole/per-unit checks have
the expected accept/reject outcomes. This is author investigation, not regression
coverage for the proposed methods or native Apple qualification.

`align_rt_alloc`/`align_rt_free` use a size-independent C allocation/free pair;
a frozen builder already transfers its allocation to a two-field array header.
Consequently, reducing the logical length does not require inventing a capacity
field or passing the shortened size to a sized deallocator. The element
cleanup obligation is separate. In particular, owned string/response arrays
and arrays of owning records must not lose suffix cleanup when length shrinks.
Borrowed `str` element descriptors and owned `string` elements are distinct.
The checked `slice<str>.to_array()` probe retains a descriptor-copy loop and
borrow-dependent return metadata; it is not an owned-string cloning operation.

## U: guarded text inspection

Validation order is source lifetime/observation use at the reached action,
then signed range classification, then endpoint classification, then at most
one interior byte load. No pointer addition or load occurs for negative,
out-of-range, zero or end offsets. Empty strings are valid; only index zero is
true. Embedded NUL is ordinary ASCII and has no terminator meaning. U queries
an already valid UTF-8 view and does not validate arbitrary bytes or repair
text invalidated by an intervening write.

Factor a MIR-owned guarded boundary predicate with the existing slice-boundary
owner. Keep existing slicing error precedence: range checks precede start/end
boundary failures, and each source operand still evaluates once. Do not convert
a boundary trap into false for ordinary slicing. Do not introduce unchecked
LLVM pointer arithmetic as the semantic owner or tag the predicate as evidence
that the string will remain valid after a later mutation.

The HIR source-completion classifier gives U an ordinary eager scalar result:
check receiver obligations after index evaluation, publish bool, and retire the
completed child byte-validation obligations under the existing scalar cutoff.
An index expression that writes through an alias of the receiver does not become
safe merely because the resulting index would return false. Direct, generic,
imported and captured calls must retain the existing observation discipline.

Safe prefix/suffix composition is already available. This complete example uses
only current syntax and methods and avoids sum overflow:

```align
fn joined_equal(candidate: str, left: str, right: str) -> bool {
  if left.len() > candidate.len() { return false }
  if right.len() != candidate.len() - left.len() { return false }
  return candidate.starts_with(left) && candidate.ends_with(right)
}
```

This is documentation guidance, not an edit to the external tokenizer. P0
promises byte-comparison behavior and bounds, not that the final executable
calls `memcmp` or outperforms a specific custom loop.

## T: exclusive prefix mutation and cleanup

1. Complete and reserve the destination place before evaluating `new_len`.
   A later argument cannot move, drop or retarget that place. Use the same
   source-evaluation index and exact-place validation as other exclusive calls.
2. At the reached action, require ordinary whole-array exclusive access.
   No live overlapping array view, indexed borrowed place or eager snapshot may
   survive the action. Unknown escaping observers prevent admission. Sibling
   independently owned fields follow plan 65 A; distinct names alone prove
   nothing. This rule applies even to a runtime no-op count. No new retained-prefix
   slice exception is introduced. Liveness ending before the action is allowed.
3. Validate `new_len >= 0` and `new_len <= old_len` with signed, nonoverflowing
   comparisons. Invalid counts leave both the outer header and all elements
   unchanged before the terminal range failure.
4. For a nonempty removed suffix, visit indices in ascending order and invoke
   each element's existing active-payload recursive Drop plan, using its normal
   field order and cleanup provenance. Borrowed referents are not freed. Null
   each removed owned payload as required by that Drop plan. No callbacks or
   custom destructor hooks are newly introduced by truncate; existing resource
   cleanup behavior and effects remain authoritative.
5. Publish only the new logical length after suffix cleanup. Preserve pointer,
   allocation identity, region and owner bit. A same-length call changes no
   bytes or cleanup state. A zero-length result retains its outer allocation.
   Subsequent truncation, assignment, return and Drop must see only the retained
   initialized prefix.

Plan 61 keeps the retained prefix's allocation and contained dependencies.
Removing an owning element ends the allocations actually released by its Drop;
removing a borrowed descriptor releases no independently owned referent. Outer
backing is neither freed nor reallocated. This is a source-visible exclusive
mutation with retained-prefix contents, not a fresh allocation or a generic
whole-value replacement. The no-surviving-overlapping-view precondition avoids
admitting an old view with a longer length after suffix Drop. A mere LLVM
`readonly` attribute, Pure label or empty imported summary cannot prove it.

One MIR operation carries this behavior through source admission. Native
preparation expands its bounds, existing element Drop and length publication
only after admission, with ordinary proof validation before LLVM. Codegen does
not invent the destruction loop from a guessed element ABI. No native truncate
function, Rust Vec reconstruction, new capacity word or byte-pattern zeroing
substitutes for the typed owner. Correctness for prefix-removed elements must
hold through resources, owned nested arrays and region storage, not just i64.

## 1055: settled syntax and compiler optimization

The Settled sum-type item in `docs/open-questions.md` says match is for variants
and value conditions use if. Plan 23 therefore applies. The inspected
`byte_scalar` and `scalar_byte` ladders are two mechanical interval dispatches
in one external program; `direct_byte` is a boolean predicate and is not counted
again as a replacement dispatch. This audit does not claim a full-corpus count.
The evidence prerequisite is five sites across two independent real programs.
Explicit new pattern syntax also fails the current compiler-derived widening
shape, so the provider disposition is **refused under the current plan 23
protocol**, independently of that count. Keep Request 90's administrative status
PROPOSED with the refusal explicit; more sites alone cannot authorize this shape.
No parser/AST change is scheduled.

There is adjacent historical wording in draft section 5 and the integer-literal
Settled item reserving wrap semantics for hypothetical integer patterns. It
specifies a value rule if such patterns are ever admitted; it does not repeal
the explicit sum-only match decision. A future authorized reopening must
reconcile both in one public ledger, including signed literal normalization,
range endpoints, overlaps, exhaustiveness and char's non-surrogate domain.
This plan does not choose conflicting new range/error semantics.

The optimization request can be investigated within existing syntax. An eligible
future MIR transform must derive one already-evaluated scalar discriminator,
constant comparisons and exact reached leaves; prove no reordered effect,
fallible evaluation, wrapping arithmetic or changed selected result; and retain
all source events until admission. Signedness, min/max values, overlapping
source conditions and first-taken branch order are semantic inputs. Any interval
representation must be bounded by source cases, not enumerate every value of a
wide integer range. The backend selects a profitable branch/switch form; no
jump table is promised. The actual extracted byte mapping already simplifies
substantially. Do not add a new pass until an exact local benchmark shows a
remaining cost and its owner can compare all 256 byte inputs plus out-of-domain
controls. Such optimization is a follow-up investigation, not selected code in
this batch and not a way to claim the syntax request implemented.

## Implementation closure matrix

The following are required owner cells, not claims of existing coverage. One
parameterized owner may close many cells; add no one-test-per-row bureaucracy.

| Axis | U / P0 owner | T / C0 owner |
| --- | --- | --- |
| Formation and diagnostics | New `align_driver --test text_boundary` plus sema/checked-HIR owners: str/borrowed string, exact i64 argument, missing/extra/wrong type, malformed source/ids. | New `align_driver --test array_truncate` plus sema/checked-HIR: every listed dynamic representation; immutable/shared/fixed/slice/SoA rejection; exact count type and field-place admission. |
| Construction, move-in/out, source nulling, replacement and return | Shared receiver survives; bool retains no storage; temporary owning receiver cleanup once; expired text rejection. | Heap/builder/arena/stack-promoted origins; scalar, borrowed descriptors, owning strings/records/nested arrays, responses and aggregate elements. Prefix identity, removed payload Drop, field replacement, later whole move/return/Drop and zero-length owner-specific release. Reuse `reassign_drop`, `m12_array_builder`, `response_builder_payload`, `move_record_slices`. |
| Value and boundary matrix | Empty, ASCII, NUL, 2/3/4-byte scalars, CJK/emoji/BPE scalar boundaries; every interior byte, -1, len+1, i64 min/max; endpoints with no load. P0 longer/empty/false-collision controls. | 0, old length, one removed, all removed, repeated truncate, negative/overlarge/extreme counts; invalid count has zero suffix Drops/header writes. Counts are logical elements, never bytes. |
| Control and source order | Receiver/index once; `if`, `match`, `else`, `?`, `map_err`, branch/loop joins, early exits, alias write during index. Existing UTF-8 slicing traps unchanged. | The same control product for count evaluation and operation placement; destination retarget/drop in count rejects, escaping views and eager snapshots reject, prior-ended loans pass. Original EOG branch executes both paths without a second owner or copy. |
| Effect summaries | U remains Pure; validate receiver/index effects separately. | Owned-local and explicit `borrow mut` rooted helpers remain Pure without another Impure operation; captured mutation and ordinary Impure operations remain Impure. Check direct/imported/indirect summaries and parallel eligibility, including existing recursive cleanup effects. |
| Access and malformed MIR | Existing 52/57 owners plus plan 61 event/transfer registration; unguarded byte-load mutations reject. | Plan 61 exact exclusive access, descriptor contents, retained/suffix ownership and UnknownObservers; forged place/type/cleanup/Drop/length fields reject before LLVM. No missed new-variant path. |
| Monomorphs, interfaces and caches | Concrete generic wrappers, named/indirect/imported calls; whole/per-unit parity; codec and cache edit/revert tests. | Same product plus returned/retained owners and per-unit Drop thunk dependency; immutable source/native finalization and every partition route in plan 61. Canonical statement goldens in both directions. |
| Native/runtime parity | Raw/optimized MIR/LLVM proves no edge load or failure call for U. Existing m5 prefix/suffix owner. | Enabled allocation probe: unchanged outer pointer/requested bytes, zero new allocation/realloc/copy, heap-owned suffix payloads freed exactly once and one eventual heap outer free. Arena-owned storage has zero individual frees and ordinary arena teardown; include two arrays sharing a chunk and an early exit. Stack-promoted backing has zero heap outer frees and ordinary frame lifetime. Borrowed referents are never freed. No new native symbols; run existing ABI/provenance owner for touched rows. C0 pins the numeric optimized memcpy while preserving zero length and allocation failure. |
| Performance promise | Constant number of guards and at most one byte load; structural owner suffices. No timing speedup claimed. | Local 128/2048-element EOG prefix benchmark: compare materialized prefix versus truncate, exact output parity; count allocations separately from production-runtime timing. Nontrivial Drop benchmark covers only claimed suffix-dependent work. No CPU/GPU or RSS claim. |

Use `scripts/cargo.sh` for the named owner packages, then the normal tier-selected
preflight. The safe-storage implementation retains plan 61's local DB verification
because that complete capability crosses its native descriptor boundary; U alone
does not inherit a database test requirement from an unrelated issue. Keep the
30-minute hard budget. Design-only validation is the author ledger-to-prose pass,
syntax-check of the existing-method example, `git diff --check`, and one independent
inspection of the new contract and capability boundary.

## Delivery and agreement

The first plan 65 capability is merged in PR 1056. The revised sequence is:

1. U with P0 documentation, a small independently useful text capability.
2. Finish the existing plan 61 + plan 65 A/O/W/M/P capability, adding T to its
   complete ownership/access closure. Preserve C0 with the narrow numeric owner.
3. Record 1055 syntax as refused under the current restriction protocol. Keep
   optimization research, aggregate byte equality and a fallible slicing API
   outside selected implementation; the earlier foreign/SIMD/native-target
   investigations remain in plan 65.

This adds one independently usable text boundary, not one PR per issue. T remains
in the shared safety failure domain; separating it from its producer/transport/
consumer would duplicate ownership proof or create a dormant prerequisite.
The existing greater-than-1000-line justification in plan 65 applies to that
capability. Neither new method needs a new numeric loop syntax or type system.

Before implementation publishes these methods, synchronize `draft.md`,
`docs/language-spec.md`, `docs/design-notes.md`, applicable Settled records,
`core-design/string.md`, `core-design/array-slice-pipeline.md` and both Japanese
mirrors with this exact ledger. Update plans 19/20/61 only where their actual
validation, ABI inventory or access schema changes; do not inflate runtime export
counts for compiler-only methods. This proposal leaves the currently shipped
specification truthful and records the future agreement set rather than listing
unimplemented methods among verified signatures.

Update the external request register with the provider decision and remaining
consumer-owned acceptance. No consumer code, fixtures, branches, pin, commit,
GitHub comment, issue closure, release or PR publication belongs to this design
request. The safe-storage implementation worktree remains intact.

## Design review closure

One fresh independent inspection reviewed this addition and its capability
boundaries after the author ledger-to-prose pass. It returned FINDINGS: two P2
and one P3. All three were verified and addressed together:

| Finding | Contract and owner correction |
| --- | --- |
| Explicit-input purity | T's authoritative ledger and effect-summary matrix retain Pure mutation through explicit `borrow mut`, with ordinary independent Impure effects unchanged. |
| Region/stack ownership | The resource contract and lifecycle/native matrix separate individual heap frees from arena teardown and stack lifetime, including shared-chunk and early-exit owners. |
| Refusal status | Plan 23 B5 and the delivery/register disposition record refused syntax separately from deferred compiler optimization research. |

The author swept all effect, free/release and refusal wording after the fixes.
No new strategy or language exception was introduced. The preserved review
verdict remains FINDINGS, not a claim of independently reviewed CLEAN. The packet
retains the reviewed draft, full log and finding-to-fix ledger. Compiler owner
and native resource tests above remain work for implementation, not design-test
results.

## U implementation closure

`StrCharBoundary` retains exact str/i64 children through shared HIR traversal,
replay cloning, finalization, source borrow/completion checks and strict checked-HIR
validation. MIR derives the total range split and calls the shared guarded
UTF-8 boundary predicate; existing slicing adds its original failure action.
LLVM sees existing typed comparisons/loads only. The physical ABI and runtime
export inventory are unchanged. Ordinary compiler identity covers the new HIR
shape; no access-wire format is published by this independent capability.

The `text_boundary` owner covers the Unicode oracle, every byte position and
extreme i64 counts, NUL/empty text, owned fields and temporary owners, generic
imported wrappers, both compilation modes, source ordering/termination, stale
observations/scalar cutoff, exact types and optimized boundary lowering.
Enabled native allocation/free counters own temporary receiver cleanup on both
normal and early-return paths. The checked-HIR owner forges receiver/index/result
types across every lowering entrypoint. Existing `m5` text/slicing owners cover
the shared lowering change. T's matrix cells are explicitly pending.
