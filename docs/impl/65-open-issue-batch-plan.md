# Open issue batch: storage, byte operations and loop proofs

Status: first capability implemented and merged in PR 1056; safe mutable-storage
implementation remains pending. Initial audit: 2026-09-15, baseline
`da20aefe1e4054cd132fbbf852217d5ee2c240ac` (PRs 1044–1046).
[Plan 66](66-array-prefix-and-text-boundary-plan.md) adds the later issues 1054,
1055 and 1057 against `61b2de79`, bringing the current inventory to 11 open issues.
It owns their exact additional public ledger and revised delivery sequence.

This document owns the proposed batch, exact proposed public ledger and closure
matrix. Existing shipped contracts remain authoritative until implementation.
It does not reopen the settled loop syntax, nominal types, inferred purity,
explicit allocation or IEEE arithmetic rules. Plan 61 remains the authority for
interprocedural access analysis; this document consumes its complete design.

## 1. Complete issue inventory and disposition

The initial audit read all eight then-open issues and all eight comments on
issue 1043. The seven new issues had no comments at that snapshot. There were no open PRs. The consumer
register's new September headings reuse historical Request 63–70 numbers;
identify this batch by GitHub issue number and dated heading, not number alone.

| Issue | Finding at the audited baseline | Batch disposition and closure condition |
| --- | --- | --- |
| [1043](https://github.com/sanohiro/align/issues/1043) | CPU/profile diagnostics, ThinLTO entry wrappers, local-reader promotion and composed zero-based byte loops already shipped in 1044–1046. Foreign backing-storage certification, broader loops and precision-preserving vectorization remain distinct. | Extend the existing MIR proof through 1049. Keep portable CPU and explicit ThinLTO selection. Foreign certification and SIMD remain explicitly open under section 7; do not close the umbrella on this batch alone. |
| [1047](https://github.com/sanohiro/align/issues/1047) | The actual policy query reaches an ordinary extern; `borrow` is not a purity/repeatability certificate. A 216-byte exported Copy-record constructor already writes directly to the ABI result destination on local x86-64. Architecture duplication is real, but does not establish permission for traits/structural subtyping. | No blanket foreign CSE, ABI rewrite or new type system. Section 7 specifies the evidence and legal optimization boundary for each subfinding. Keep the issue open until those independently tracked subfindings receive evidence/disposition. |
| [1048](https://github.com/sanohiro/align/issues/1048) | Typed slice stores are absent. `array<i64>` field replacement explicitly rejects in both frontends. Per-byte stores cannot simply be merged across reached failing checks. SIMD argmax is a separate request in the same issue. | Implement all-width endian stores and general admitted owning-field replacement with the shared access/cleanup prerequisite. Close the two concrete API/ownership findings after their owners; keep SIMD visible rather than claiming the entire issue is fixed. |
| [1049](https://github.com/sanohiro/align/issues/1049) | Nonzero initial induction and independent offset recurrence retain checks. A zero-based scaled loop with an in-loop nonfinite `Err` already removes both byte checks. Masked casts still warn. | Extend recurrence facts and finalized-type known-bits diagnostics. Keep exact-edge dominance. Close with the eight-shape oracle and rejection controls; do not weaken dominance to fit the report's incorrect diagnosis. |
| [1050](https://github.com/sanohiro/align/issues/1050) | The direct path test is ORed with coarse storage roots. A valid scalar-field/byte-view witness reproduces the alias error. The issue's `raw` field is independently illegal. Sibling string replacement alone preserves the view in the corrected witness. | Path-aware ownership/backing origins and ordered call invalidation, using plan 61. Isolate the full client's after-call invalidation before attributing it to the sibling assignment. Close on the composed Session-shaped case plus overlapping-backing rejections. |
| [1051](https://github.com/sanohiro/align/issues/1051) | Bulk initialized buffers, byte fill/copy and repeated-word fill are missing. Aliased buffer append snapshots its source. A zeroed allocation is not guaranteed constant-time; byte memset cannot form `0xff800000` words. | One filled constructor, byte copy/fill and a uniform typed-fill family. No zeroed alias, capacity-equality or O(1) promise. Preserve ordinary append semantics. |
| [1052](https://github.com/sanohiro/align/issues/1052) | Float bit/classification methods reject. Existing `align_rt_*_to_bits` symbols are package-internal DB codec ABI helpers, not public methods. | Eight scalar methods, direct MIR/LLVM lowering, no new native float exports or buffer roundtrip. Close on exact-bit/classification and native-register owners. |
| [1053](https://github.com/sanohiro/align/issues/1053) | Capacity input rejects as a region mismatch. Heap and region headers start at capacity zero; heap growth chooses a minimum of four at first growth. Region builders use chunks and a separate contiguous freeze. | Capacity forms for the existing builder, preserving both storage modes and their type grammars. A repeat terminal is not selected: the issue explicitly permits a capacity constructor instead. Close on bounded push/growth and freeze/Drop owners. |

The follow-up inventory is owned by plan 66: 1054 adds exclusive dynamic-array
truncation (its numeric bulk-copy witness is already optimized); 1055 syntax is refused
under the current plan 23 widening rule; 1057 adds a total UTF-8 boundary
predicate and prefix/suffix guidance. All earlier partial/deferred issue parts
remain visible above and in section 7.

No GitHub comment, issue closure, consumer code, branch, pin or publication is
part of this design task. Align's answer belongs in the existing consumer
register as an uncommitted edit.

## 2. Reproduction and contrary evidence

The audit rebuilt `alignc` from the baseline with
`scripts/cargo.sh build --release -p align_driver --bin alignc` (passed).
Twenty initial source cases and three focused follow-ups were checked with both
`check` and `check-per-unit`.
Raw and optimized IR use `--profile release --no-rt-lto --export probe`.
The packet under `.git/issue-batch-20260915/` retains issue JSON, sources,
commands, compiler hash, diagnostics, IR and the object below. It is local
evidence, not a checked-in regression suite or native Apple qualification.

| Start | Address spelling | Nonfinite early return | Optimized range-failure call sites |
| --- | --- | --- | --- |
| 0 | `i * 4` | absent / present | 0 / 0 |
| 0 | `offset += 4` expressed as ordinary assignment | absent / present | 1 / 1 |
| 1 | `i * 4` | absent / present | 1 / 1 |
| 1 | `offset += 4` expressed as ordinary assignment | absent / present | 1 / 1 |

The fallible cases have two typed source reads. Raw IR retains one or two
checks; these are observations of final optimized IR, not a claim that raw MIR
already lacks every check. All eight loop sources type-check in both modes.
The full client greedy function additionally seeds a candidate, validates the
first element and resolves equal values to the smaller index; keep that shape
as a separate execution oracle.

The normalized 1050 witness uses `owner: i64`, since `raw` is not an admitted
ordinary record field. `worker(state.owner, view)` still rejects. Separately,
`view := state.slot.bytes(); state.key = "new".clone(); return view.len()`
passes both frontends, including after a local helper writes through
`borrow mut view`; subsequent length and byte reads both pass. A design must not
describe this second case as an independently reproduced failure. Audit
`completed_borrow_mut_invalidation_roots`
and the full client's helper calls before selecting the actual fix.

The 216-byte `Big` constructor produces `%Big @probe(i64)` in LLVM IR. Its
local x86 object writes fields through `%rdi`, returns that address, and has no
stack allocation or memcpy. The absence of an explicit LLVM `sret` parameter
does not prove a missing native indirect return. The warning is source-size
based and is not evidence of 216 bytes copied twice. The reported Mach-O
992-byte frame still needs its exact native function/call-chain owner. The
subsequent [plan 67 audit](67-caller-result-placement-plan.md) reproduces the
caller-side copy class and owns its result-placement implementation;
this earlier constructor-only result did not rule that class out.

## 3. Proposed public-contract ledger

These are declarations, not positional source examples. All arguments evaluate
once, left to right, with receiver completion first. A terminating operand
prevents later operands and the operation. Static checking resolves all operand
types once before domain/mutability checks and produces one root diagnostic.
Ordinary `mut`/`out`/`borrow mut`, lifetime and purity rules continue to apply.

All new allocation requests are explicit at a constructor. Fully initialized
materializers and builder capacity requests use the existing core terminal
allocation-failure policy, not a new recoverable allocator error model.
Invalid negative/overflowing counts abort before acquiring storage. This is
deliberately different from the issue's suggested `Result` spelling. The
existing one-argument `buffer(capacity)` keeps its best-effort reserve/read-window
contract; the new filled constructor guarantees initialized length on return.

| ID / exact surface | Inputs, results and errors | Ownership, lifetime, allocation and effect | Owner, prerequisite and acceptance |
| --- | --- | --- | --- |
| F: `f32.to_bits() -> u32`, `f64.to_bits() -> u64`; each float's `is_finite() -> bool`, `is_nan() -> bool`, `is_infinite() -> bool` | No arguments/defaults/errors. Exact same-width bits; classification truth table below. | Copy input/result, Pure; no memory or allocation required. | Sema/HIR/MIR/LLVM. Existing float and byte-codec types. F owner below. |
| W: `slice<u8>.set_u8(offset: i64, value: u8) -> ()`, `set_i8(offset: i64, value: i8) -> ()`; `set_S_E(offset: i64, value: S) -> ()` | Cartesian family `S = u16,i16,u32,i32,u64,i64,f32,f64`, `E = le,be`. No coercion/default. Single full-width range obligation, terminal range failure before any store. | Mutates existing writable bytes, leaves length/capacity and owner intact; no allocation. Source effect is the existing in-memory write effect. | Shared checked byte operation; complete access qualification in plan 61 is prerequisite. W owner. |
| B: `buffer.filled(length: i64, value: u8) -> buffer` | Exact initialized length on success; zero length valid. Count conversion/layout overflow precedes allocation; OOM aborts. Allocated capacity is at least length, not allocator-exact equality. | Fresh Move buffer; Pure explicit allocation. Zero length allocates no payload, but the existing handle may allocate. Nonempty construction acquires one payload; no growth sequence or temporary snapshot. O(n) initialization, no zero-page complexity promise. | Existing Buffer owner/Drop/runtime plus new typed constructor. B owner. |
| M: `slice<u8>.fill(value: u8) -> ()` | Entire current slice; zero length no-op. No count/default/error result. | Writable backing; O(n), no allocation. Existing descriptor/owner remain; overlapping validated observations end. | Plan 61 write transfer; MIR bulk operation, LLVM memset. M owner. |
| M: `slice<u8>.copy_from(source: slice<u8>) -> ()` | Source is borrowed, not consumed; exact equal lengths, mismatch aborts before writes. Empty/empty no-op. Ordinary argument alias admission must prove independent source/destination backing; unknown or overlapping backing rejects. | Explicit byte copy, no allocation/ownership transfer. Writes only destination, invalidates its overlapping validated observations. | Plan 61 source read plus destination write. LLVM memcpy only after nonoverlap proof. M owner. |
| P: `slice<u8>.fill_S_E(value: S) -> ()` | Same eight multi-byte scalars and two endian suffixes as W; slice length must be divisible by width, otherwise abort before writes. Empty slice valid. | Repeat exact scalar bits; no allocation. One validation plus O(n/width) stores. Byte `fill` is the width-one form; no duplicate `fill_u8` alias. | Same ByteFill record as M with scalar/byte-order discriminator. P owner. |
| C: typed `array_builder()` / `array_builder(capacity: i64)` / `array_builder(out: region)` / `array_builder(out: region, capacity: i64)` | Binding/expected type supplies `array_builder<T>` as today; no turbofish. Omitted capacity = 0. No defaults for region. Negative count, count × stride overflow, and target allocation-size overflow abort before allocation. | Length remains 0; no initialized T is invented. Heap/region admissibility is unchanged. At least capacity pushes fit the initial payload/chunk without another growth allocation. Heap build transfers backing; region build retains its current contiguous materialization. Pure, explicit allocation in named storage. | Existing builder type formation and Drop/region owners, all header representations. C owner. |
| O: `owner.field_path = value` for every already-admitted owning field type with an existing complete Drop plan | Same-type replacement on a stable writable local or exclusive borrowed record. Static ownership/lifetime errors; no new runtime Result. No new field/storage type becomes legal. | Evaluate and retain RHS first; transfer its ownership, clear selected moved sources, drop old live destination once, store replacement. Preserve siblings and external owner lifetime. No implicit clone/allocation. | Existing AssignField, Drop plan and producer validator, plus plan 61 path/backing facts. O owner. |
| A: shared/exclusive arguments to disjoint stable record fields | Permit only proved nonoverlap of both places and addressed storage. Same root is not by itself alias; different field names are not by themselves disjoint backing. | No new borrow mode, pointer permission or allocation. Same/ancestor/unknown backing conflicts remain errors. | Plan 61 caller instantiation, existing borrow checker and source staging. A owner. |
| L: existing byte reads in nonnegative-start / paired-recurrence `loop` | No API or trap/error change. Proved checks disappear; all unproved reached checks remain. | Wrapping semantics, scalar FP evaluation, owner lifetime, byte alignment and source effects remain. | MIR byte_ranges and emission preparation in plan 64. L owner. |
| K: existing integer `as` warning | Suppress only when finalized source expression facts prove every value fits the destination integer interval. Otherwise existing warning remains. | Diagnostic-only; no execution, ownership or allocation change. | Sema finalize_expr, finite known-bits helper. K owner. |

For `copy_from`, overlapping copies are not an alias-rule exception. A future
within-owner copy, if needed, needs its own source contract; this batch does not
silently allow it by choosing memmove. Two slices with unproved separation can
still be copied through an explicit independent owner, with the copy visible.

For float classification, both signed zeros, subnormals and finite normals are
finite; both infinities are infinite; every quiet/signaling NaN payload is NaN.
The three predicates partition all encodings. `to_bits` preserves the input
encoding, including sign and payload. It promises no canonical payload for an
earlier arithmetic operation whose existing IEEE contract leaves that payload
unspecified. Classification uses integer exponent/mantissa masks after bitcast;
it does not evaluate arithmetic on a signaling NaN or enable fast-math.

W validates `offset >= 0`, `length >= width`, then
`offset <= length - width` before forming the store address. Arithmetic used by
validation must not overflow. Store alignment is 1, including float bit stores;
byte order follows the method name on either host. A successful set changes
exactly width bytes. This atomic validation is the new operation's contract;
it does not authorize changing the partial-write behavior of old scalar code.

P addresses the actual `0xff800000` mask workload. A byte memset is used only
when all pattern bytes are equal; otherwise lower one pattern loop that LLVM
can vectorize. Do not add a resource-specific logits/argmax/mask API.

There is no text encoding or embedded-NUL validation: all W/B/M/P inputs are
bytes/scalars. Bytes from text still require writable backing proof; text
construction and UTF-8/codec observation validity remain separate checks.
No operation changes connection-global/process-global native state. No runtime
inspection table, new CLI option, ambient setting or user persisted format is
introduced. Performance claims are the bounded work/allocation claims above,
not a hardware speedup ratio.

### Artifact identity and agreement set

F/W/B/M/P/C introduce checked operation records or operands. Every enum visitor,
generic substitution, canonical serializer, producer validator, effect/access
transfer, Drop walk and emitter classifies them explicitly. The compiler-owned
record codec and interface version change once for the integrated change that
actually alters each format; no permissive old-record fallback is added.
Plan 61 owns its canonical access bundle, scalar widths/tags/order, malformed
rejection and two-direction golden vectors; do not invent a parallel summary.
Its interface 12→13 transition must be reconciled with the actual base before
implementation. Nominal type identity remains; reachable definitions and
access-program graphs enter structural artifact fingerprints as specified there.

Ordinary, frontend/object memo, rehydration, runtime LTO, PGO and ThinLTO caches
include the actual compiler/runtime ABI identity. A function partition cannot
consume a peer's private body absent the body dependency from its key. F/W/M/P
lower without new native symbols. The proposed B native row is
`ptr @align_rt_buffer_filled(i64 length, i8 value)`; positive length is checked
before allocation and success always returns one live Buffer handle. C adds
`i64 capacity` last to each existing heap-new, region-new and stack-init builder
row. All declarations, generated callers, runtime definitions, runtime-bitcode
inventory and probe variants migrate together; no compatibility exports.
The Buffer and ArrayBuilder physical owner layouts need not change.

Before implementation changes the public contract, propagate this one ledger
to `draft.md` (binary operations, scalar methods, builders, field assignment),
`docs/language-spec.md`, `docs/design-notes.md`, the appropriate Settled entries
in `docs/open-questions.md`, `core-design/string.md`,
`core-design/array-slice-pipeline.md` and their `ja/` mirrors. Update the exact
records in plans 19/20 with implementation; preserve their baseline inventories
until then. Plans 37/61 own projection/access rules, plan 64 owns optimization
preparation, and this plan owns their batch composition. The present proposal
does not relabel unimplemented methods as shipped in those documents.

## 4. Shared ownership and access design

The 1050 one-line suggestion is insufficient. A view local has a different
syntactic place from its owner, and distinct fields may contain descriptors
pointing to the same allocation. Removing storage-root intersection whenever
field paths differ can admit an invalidating call or dangling view.

Use the existing projected headers and plan 61 point state to distinguish:

1. The stable descriptor place: root plus typed field/tuple/payload path.
2. Each addressed owner/generation and byte range, with Unknown explicit.
3. Read/write/replace/capture access at the completed call action.
4. Read-only permission and live UTF-8/codec observations of that backing.

Prefix-overlapping paths conflict. Proved sibling owned allocations are
independent; copied descriptors keep their original shared backing. Dynamic
indices and unresolved joins use conservative intersections. Replacement follows
plan 19's existing per-leaf generation rules and plan 61's descriptor snapshots:

| Replaced leaf / action | Generation and observer transition |
| --- | --- |
| Copy descriptor rebind | Replace only that place's descriptor profile. Previously copied descriptors retain their original still-live backing and validation dependencies. Rebinding does not destroy the allocation they address. |
| Existing inline storage | Preserve the destination storage generation; update its selected contents. Existing aliases observe the new contents. A write ends overlapping UTF-8/codec observations, not the inline storage lifetime. A consumed inline source follows the existing source-lifetime rule. |
| Owned backing displaced and released | End the old allocation generation only when it is actually dropped/reallocated. Invalidate its observers wherever their descriptors are stored; preserve proved unrelated siblings. |
| Existing owned backing transferred into the field | Preserve the transferred allocation generation and contained dependencies; change its release owner and clear the consumed source. End any distinct displaced destination allocation. Source rebinding must not affect the moved allocation's observers. |
| Fresh allocation | Create a fresh generation from its producer. No descriptor name or destination field alone establishes freshness. |
| Exact self / selected branch returning the same owner | Preserve that allocation and its generation on the transfer edge; drop a displaced allocation only on an edge that actually replaces it. |

Root replacement applies these rules to its selected leaves. Root Drop ends
owned allocations and local inline lifetimes, not independently owned backing
merely referenced by a Copy descriptor. Unknown invalidating effects remain
conservative over their possible targets. A byte write ends overlapping
validation observations without reallocating the owner or making every
ordinary byte view dangle.

Call receiver and arguments are staged in source order. A later argument that
replaces an earlier argument's owner invalidates the earlier reservation before
pointer construction. Direct, indirect, captured, generic and imported calls
share the same instantiated proof. An empty effect, Pure, LLVM readonly, or a
missing header is never a no-retention/writable certificate.

Implement plan 61 as its complete producer/interface/consumer capability before
shipping W/M/P or widening A. Its existing plain-slice write and returned-view
laundering holes must not become the new methods' bypass. This dependency is
substantial: a local method whitelist or name-based caller check is not a
complete substitute. Reuse plan 61's accepted finite representation and DB/native
inventory rather than redesigning it in this batch.

Generalize O through the typed Drop plan, not a list of string/array exceptions.
Admissibility is exactly: already-legal stable record path, already-legal RHS
type, complete existing Drop/producer representation, and valid replacement
ownership. This includes admitted buffers, arrays, records, tagged owning
payloads and resources; it does not admit builder fields or new array elements.
Owner-specific leases/region provenance must still validate. Shared borrowed
records and moving other fields out of an exclusive borrowed owner remain
invalid; replacing one field does not require moving its siblings.

Exact same-place assignment is an ownership-preserving no-op. Other RHSs finish
before destination mutation. Branch-selected source nulling occurs only after
the value has been captured. A source equal to the destination through a
transparent branch is transferred before old-value Drop, so it is not freed
twice. A failed `?`, early return or divergent initializer must emit no later
store. Drop readiness and cleanup provenance move with the selected value;
do not infer them from a nonnull pointer or the containing record's single flag.

## 5. MIR recurrence and diagnostic design

Keep plan 64's structure validation, emission-scope preparation, exact admitting
edge dominance and root-specific invalidation. Extend recurrence recognition to
an invariant integer start `k` only when the reachable entry proves `k >= 0`.
Literal nonnegative starts are the first required case; an unknown signed start
retains its check unless an existing dominating guard establishes the fact.

For a paired byte offset, prove the entry relation `offset = width * i` and
the unique latch relation `i' = i + 1`, `offset' = offset + width` at every
backedge. Recognize copies/named values using reaching definitions, not variable
names. Updates may appear in either order after the last read, but neither
intermediate state may feed a read or side exit that observes a rewritten value.
Require the same admission `i < floor(length / width)` and stable descriptor
generation. Prove the entry product and all reached updates do not wrap;
derive the last offset from `length` without multiplying unchecked bounds.
Keep unknown stride, mismatched starts, multiple latches, skipped increments,
pre-read increments, stale loads, descriptor mutation and unproved joins checked.

A function-return error arm does not create a bypass of the admission edge.
Do not delete error arms or move nonfinite tests. Keep load coalescing separate:
same byte address, width and generation plus no intervening write/effect permits
LLVM to reuse the integer load for its float interpretation. Bitcast/classify
methods also let clients express this directly, but existing source must still
benefit when its proof is available. Never assume alignment 4 for byte views.

K runs after type inference. A bounded expression-local known-bits calculation
handles typed integer literals, bitwise AND/OR/XOR, checked constant shifts and
integer casts using each operand's fixed width. Unknown facts mean all values
possible. For signed destinations, prove the signed interval as well as width:
`u32 & 255` fits u8, but does not fit i8. Unsupported calls, variables without
a point-valid definition, overflow-sensitive arithmetic and float conversions
keep the existing warning. Do not copy the MIR optimizer's mutable-slot facts
into source diagnostics or change cast execution.

## 6. Implementation closure matrix and owner commands

Owners below are existing targets to extend; labels F/W/B/M/P/C/O/A/L/K are
planned invariant-level cases, not tests already present/passing. Before code
review, replace each applicable row's planned witness with the exact test name
and implementation site. Reuse coverage that fails on the original defect.

| Axis | Required cases and exact owning target |
| --- | --- |
| F formation/evaluation | f32/f64 receiver from local/field/call/parenthesized expression; wrong receiver/arity; signed zeros, subnormals, finite extremes, ±Inf, signaling/quiet NaN payloads. `align_driver --test runway_a2_binary_codec` (F). |
| W/P domains | All 18 setters and 16 typed fills; both endian golden bytes independent of the reader; offsets 0/last/negative/MAX, widths 1/2/4/8, unaligned addresses, empty/short/partial tail. Pre-write failure and operand-side-effect order. Same target (W/P). |
| B allocation | Zero/nonzero, invalid count/size/OOM failpoint, exact initialized bytes and length, capacity lower bound, no grow/snapshot sequence, ordinary Buffer Drop/rebind/return. `align_runtime --lib` filtered new buffer-filled owner plus codec target (B). |
| M access | Writable heap/stack/arena origins; readonly literal/string, retained aliases, returned/captured/imported/plain-parameter laundering; same/overlapping/unknown source-destination reject; unequal lengths do not modify bytes; disjoint equal-length copy. `align_driver --test consumer_borrow_boundaries` and codec target (M). |
| C storage grammar | Heap Copy/string/closed record; RegionPlain and rejected owning region elements; generic inference; capacity 0/1/4/40/128; count×stride overflow; header heap/stack/arena; zero initialized prefix, partial push Drop, full push/build, moved-source nulling, final region materialization. `align_driver --test m12_array_builder`, `--test large_drop_codegen`; `align_runtime --lib` filtered builder owner (C). |
| A places/backing | Direct/nested sibling owned fields, scalar/slot and resource/slot; same field, ancestor, copied sibling slices sharing backing, dynamic elements, unknown root and branch joins; full Session-shaped after-call case; later-argument invalidation. `align_driver --test consumer_borrow_boundaries`, `--test borrowed_params`, and plan 61's full access owners (A). |
| O lifecycle | Existing admitted Drop-plan classes; construction, move-in/out, source nulling, replacement, Drop/return; self, sibling and branch-selected transfer; owning record and exclusive borrowed record, readonly reject; no new element/field categories. `align_driver --test borrowed_replacement`, `--test owned_structs_arrays`, `--test struct_handle_fields` (O). |
| A/O generation preservation | Copy descriptor rebind preserves aliases of live old backing; inline destination aliases observe replacement contents; allocation transfer preserves generation and changes release owner; later source rebind cannot invalidate transferred storage; self/branch-selected transfer survives while actually dropped backing invalidates observers. `align_driver --test return_provenance` existing storage-generation matrix plus A/O targets above. |
| Control paths | `if`, `match`, `else`, `?`, `map_err`, branch/loop joins, zero iterations, early return/divergence, RHS error after temporary ownership acquisition, stale/overlapping view rejection. O/A owners plus `--test owned_borrowed_composition`. |
| L range proof | Eight baseline shapes, exact client greedy/candidate ties, widths 1/2/4/8, tails, zero/negative/unknown starts, paired update order and all invalidations in section 5. `align_mir --lib` byte-range owners; codec target (L). |
| K diagnostics | Mask fits/nonfits for signed/unsigned widths, inferred types, nested masks/shifts, unsafe-to-suppress controls and no change to result bits. `align_sema --lib` focused finalized-cast owner (K). |
| Malformed records | Every new discriminator/type/width/mode/operand; incomplete Drop/provenance and unavailable write permission reject before LLVM/native side effects. `align_sema --lib` checked-HIR and `align_mir --lib` producer owners; exhaustive variant tripwire. |
| Units/cache/ABI | Whole and per-unit positive/negative parity; concrete generics and indirect calls; raw/optimized IR, ordinary objects, runtime LTO and PGO; function/support ThinLTO, cold/warm/private-body edit/revert and shared-process scope reuse. `align_driver --test per_unit_codegen`, `--test cache_codegen`, `--test function_thin_lto`; plan 61 owners. |
| Native lowering | No buffer/runtime-call roundtrip for F; alignment-1 wide store for W; no per-element runtime calls for M/P; no false sret or SIMD assertions. `align_driver --test target_cpu_isa` and the shared performance script on x86-64 and ARM64. |
| Native ownership/DB | Runtime row/type/export parity across ordinary and probe features; full plan 61 native provenance inventory, DB callbacks and retained views. Runtime ABI owners plus `scripts/db-verify-local.sh` when that capability is implemented. |

Benchmark only the explicit performance paths, outside correctness gating:
40/128-element builders with counted growth, 607744-byte zero initialization,
131072-byte `0xff800000` pattern fill, 2048-token typed prefix publication, and
151936-logit greedy/PR-247 sampler kernels. Record optimized executable identity,
compiler/client SHAs, complete flags, cold/warm policy and distributions. Count
allocation calls and bytes separately from elapsed time; header allocation is
not payload growth. Require exact output, error/tie behavior and RNG parity.
Native Mac/Metal and Linux/CUDA end-to-end measurements remain consumer-owned;
local x86 code or cross-target assembly is not that evidence.

## 7. Boundaries kept open, with concrete resumption conditions

**Foreign CSE and stack promotion (1047/1043).** The inspected
`ggml_ffi.gpu_attention_policy` calls an extern through `resource.raw`; source
`borrow` does not certify repeatability. Plan 64 section 5 already records
synchronous-copy evidence and private Metal alignment/region constraints.
Promotion needs authenticated backing-storage retention/completion/alignment
facts for every actual backend and imported wrapper, bound to the native
artifact and cache identity. Query CSE additionally needs mutable/global state,
errno, synchronization, divergence and exception/trap effects, and invalidation
by `gpu_attention_select`. Plan 61 is access-only and is not that certificate.
Do not attach blanket readonly/willreturn/noalias or add a guessed `#[pure]`.
Resume after an exact native contract and its artifact transport are reviewed;
opaque calls remain optimization barriers meanwhile.

**Cross-module pure helpers (1047/1043).** Source single-expression spelling
is not an effect proof. Extend plan 64's bounded exposure only when a validated
body is actually in emission scope, or introduce a reviewed body dependency
transport covering every imported helper and partition key. Use existing
explicit ThinLTO for cross-unit qualification now. A broader import-inline
capability must supply its own size/work budget and private-edit invalidation
owner; it cannot consume an unavailable body because a declaration looks small.

**Large returned records (1047).** The new caller disassembly and a provider
reproduction establish a return-temporary-to-local copy class, separately from
the constructor's already-direct ABI result writes.
[Plan 67](67-caller-result-placement-plan.md) now owns the exact evidence,
return-transport/materialization implementation, all-call-edge closure matrix and
native ABI qualification gate. It exposes LLVM's existing indirect result convention
and presents an eligible materialization as memcpy to the existing optimizer;
sret alone was insufficient in the reverse control. LLVM's target classifier owns the decision; the native gate cross-links
implicit/explicit callers and definitions in both directions. No new
source return API, guessed size threshold or foreign effect fact is selected.
The original Mac build identity/IR and client timing remain pending.

**Ordered SIMD (1048/1043).** The scalar fixes and F do not guarantee LLVM
vectorizes a fallible argmax. A later vector candidate must validate every lane
and tail within proved live bytes, preserve the first invalid logical element,
candidate seeding and minimum-index tie rule, avoid speculative effects and
retain f64/NaN/signed-zero/RNG semantics. Stateful Top-K insertion is a different
algorithm from argmax. Resume after native reverse controls demonstrate benefit
against the now-correct scalar baseline; no `argmax_f32_le` API or 4–8× promise.

**Traits/structural subtyping (1047).** Do not introduce them. The report has
one consumer, and two model files are not two independent programs. Plan 23's
five mechanical sites/two-program threshold is not met, and customizable
behavior is outside its permitted widening. Existing nominal common records,
scalar arguments and ordinary functions remain the design route; changing the
consumer's architecture is separate consumer-owned work.

**macOS SIGPIPE (1043 comments).** Keep the previously reported
`test_runner::tests::controlled_write_sigpipe_process_owner` native failure on
the disposition list. It is not fixed by excluding it from a performance owner.
Reproduce that exact child mode on macOS, inspect inherited mask/disposition and
spawn restoration, then fix under the process lifecycle owner. Linux success
is not evidence of a macOS fix. This host did not reproduce or clear it.

## 8. Batch execution and review

The initial eight-issue batch has two independently useful capability boundaries.
Plan 66 extends this sequence with an independent text predicate and folds array
truncation into the second boundary; it does not reopen the first implementation:

1. **Scalar/constructor/loop capability:** F/B/C/L/K, covering 1052, 1053,
   1049 and the allocation part of 1051. No dependence on the unshipped access
   analyzer. Each new constructor, record, runtime producer and consumer lands
   together; optimize its allocation path only with complete identity/Drop proof.
2. **Safe mutable storage capability:** the complete accepted plan 61, A/O,
   W/M/P and the composed Session owner, covering 1050 and the concrete storage
   parts of 1048/1051. Integrate the producer/transport/consumer before publication;
   do not publish dormant access metadata or a local-only permission shortcut.

These failure domains justify the boundary: capability 1 is useful without any
borrow widening, and capability 2 changes cross-call access/ownership safety.
Both can be prepared in the same delivery cycle; the second is not a one-line
follow-up. Expect more than 1000 handwritten lines, especially in capability 2.
Keeping access records, path invalidation, typed Drop and new mutation consumers
together avoids duplicated proof, temporarily unsound call paths and repeated
ABI migrations. Do not split the strict producer-to-consumer chain for a line
target. Section 7 is a visible deferred list, not silently included acceptance.

The author ledger/prose pass checked every ledger row against sections 4–7 and
the matrix, including the extracted must/exact/every/before/reject obligations.
No fenced source example introduces unimplemented syntax as a passing program.
The inventory covers every issue subfinding, including the SIGPIPE comment.
One fresh independent adversarial inspection completed on 2026-09-15. It found
one P2: the original replacement paragraph unconditionally advanced generations.
The fix replaces that shorthand with the per-leaf transition table in section 4
and the A/O generation-preservation matrix row, following plans 19/61. The author
checked the complete generation/invalidation wording against that finding;
the revised design does not invent a new ownership model. The review found no
other verified defect. The original review verdict was FINDINGS, not CLEAN;
the local packet retains the reviewed candidate and finding-to-fix record.
Implementation uses `align-self-review`, the relevant owner targets, one fresh
full-diff inspection and one coherent finding-fix commit under repository rules.
Run final-SHA `scripts/pre-pr.sh` with the owner command, bounded gate and
isolated-target Clippy; the mutable capability also needs local DB verification.
The 30-minute test budget is unchanged. Finish normative English/mirror changes
before final attestation. Push/open only through repository wrappers when the
implementation task reaches publication; no publication is performed now.

Native facts used to constrain this design are defined by LLVM 22's
[bitcast](https://releases.llvm.org/22.1.0/docs/LangRef.html#bitcast-to-instruction),
[memory intrinsics](https://releases.llvm.org/22.1.0/docs/LangRef.html#llvm-memcpy-intrinsic)
and [parameter attributes](https://releases.llvm.org/22.1.0/docs/LangRef.html#parameter-attributes).
The [vectorizer guide](https://llvm.org/docs/Vectorizers.html) describes legality
and target profitability separately; a requested transformation is not evidence
of a native workload speedup.

## First capability implementation closure

F/B/C/L/K use the existing MathOp, BufferNew and ArrayBuilderNew families.
Their complete operands enter replay, depth, effects, escape, move, finalization,
producer validation and lowering. The HIR production projection is v2 because
MathFn tags and constructor record fields changed; interface format 12 remains
unchanged until the access capability actually changes its serialized schema.
Native ABI inventory is 445 keyed / 463 base. F has no native export; B adds
BufferFilled; C migrates all three constructor rows together.

| Cell | Implementation | Discriminating owner |
| --- | --- | --- |
| F width, bits, classification, import, inference | Sema float/result relation; exact HIR/MIR MathFn result; integer-mask LLVM lowering | `runway_a2_binary_codec::float_inspection_exact_bits_and_classification`, `float_inspection_inference_and_receiver_matrix`; malformed HIR scalar/capacity owner |
| B exact initialization, operand order, zero, overflow, growth, Drop | Optional BufferNew fill; ordered lowerer; runtime filled constructor | `filled_buffer_initialization_growth_and_operand_order`; native `explicit_constructor_capacity_preserves_payload_and_initialized_prefix` and `json_owned_terminal_allocation_failpoints_abort_in_child_processes`; `constructor_termination_stops_later_operands_and_allocation` |
| C heap/stack/region, capacity, initialized prefix, growth/freeze | Mandatory capacity child; constructor overload checker; exact initial allocation in every header mode | `m12_array_builder::explicit_capacity_keeps_empty_length_and_both_storage_modes`; existing full builder/region owners; native capacity owner |
| L constant nonnegative start and paired recurrence | Same unique latch and exact admission-arm proof; constant initial relation and no-wrap bound | `byte_range_nonzero_and_paired_recurrences`; existing malformed/admission-edge controls and tail oracle |
| K finalized diagnostic-only facts | Bounded expression-local known-bits helper | `lint_lossy_cast::finalized_known_bits_suppress_only_proved_lossless_integer_casts` and existing lossiness owners |
| Exact ABI and malformed records | RuntimeKey/registry/golden/export signature owner; checked HIR/producer validators | `scalar_inspection_and_capacity_records_reject_forged_operands`; `runtime_abi` owners; `scripts/test-runtime-abi-exports.sh` |

The region owner exposed a pre-existing rejection: its already-admitted
`array<Option<i64>>` element is stored as a Tagged descriptor but read as an
expanded Option value. Baseline release emission of
`fb_region_builder_aggregate.align` reproduces the failure. The physical-result
validator now normalizes this existing canonical wrapper on both sides while
requiring exact payload identity and Copy-read admission. No new element type
or owning element read is admitted.

The full diff exceeds 1,000 handwritten lines because one selected capability
includes its coordinated schema/ABI migration, all header-mode owners and
required specification/mirror changes. Keeping this boundary avoids repeatedly
migrating constructor operands and proving the same numeric byte-loop consumer.
A/O/W/M/P and complete plan 61 remain a separate, pending capability.


### First capability verification and finding closure

The independent implementation inspection found one P2: entering an unrelated
lambda prematurely defaulted pending `to_bits` relations in the enclosing
function. Relation propagation now preserves unconstrained components; only
actual lambda parameters/captures commit their boundary types. The inference
matrix covers unrelated lambdas, captured results, explicitly typed captures,
and pipeline parameters in whole-program and per-unit modes. The author swept
all relation solver/finalization boundaries for the same class. The original
review verdict remains FINDINGS. The fix also groups the two recurrence
constants into one argument to close the Clippy arity warning.

Local x86_64 Linux measurements use the production runtime built with Rust
1.96.0, `scripts/cargo.sh build --release -p align_runtime`, and an optimized
Rust FFI probe. Nine alternating samples compare identical construction,
push/initialization, length/value checks and destruction; builder samples run
10,000 iterations and 607,744-byte buffer samples run eight. Median nanoseconds
per operation (baseline / revised) are 291.241 / 240.735 for 40 builder elements,
746.416 / 694.824 for 128 elements, and 2,421,945 / 6,192.75 for byte-at-a-time
zero filling versus `buffer.filled`. These isolate constructor/runtime costs;
they are not native ARM, application or compiler-wide speedup promises.
Correctness owners separately check initialized contents and capacity behavior.
The local evidence packet retains all nine samples, source and artifact hashes:
probe `751d60b8e04a795b8a7e2c8300f6e3b37607117f29b146f2e9b8fe2813c4d7a8`,
runtime `4d4755e37eb6f63b092cc6457b618f685c3adae7502dfa80ca3a8f60f4ad5598`.
