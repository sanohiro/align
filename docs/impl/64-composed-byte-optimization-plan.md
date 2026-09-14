# Composed byte-loop optimization: issue 1043 follow-up

Status: implemented capability, 2026-09-14; native macOS qualification and
consumer adoption remain external. [Plan 63](63-codegen-performance-audit.md) records the shipped
baseline; this document owns the next capability and its explicit deferrals.

## 1. Evidence and dispositions

Inputs are the [native lifetime clarification](https://github.com/sanohiro/align/issues/1043#issuecomment-5662787330)
and [ARM binary inspection](https://github.com/sanohiro/align/issues/1043#issuecomment-5663319777).
Provider reference is `6cd95b159d77a762c1b213417493708787cc1590`.
Consumer source was read at `8a890d169f40fe030c05635a280d3c52fdbf6370`, including
merged [sampler PR 247](https://github.com/sanohiro/align-llm/pull/247).
The sibling checkout is older (`f4f5a9cf`); it was not pulled or edited except
for the permitted request-register answer. The inspected consumer's managed
`.align-revision` still names `21d0cf27`. This does not disprove a manual build
with `6cd95b15`, but addresses in a comment cannot identify its compiler or
linked artifact. Record that binary's hash and complete command before timing it.

The current release compiler was used for temporary, exported source probes at
release/baseline with runtime LTO off. Sources, MIR, raw/optimized IR and downloaded
pinned native source are preserved locally under `.git/issue-1043-followup/`.
This is Linux compiler/IR evidence; the reported Mac addresses and timings were
not independently reproduced on this host.

| Finding | Confirmed evidence | Disposition |
| --- | --- | --- |
| Whole-function range rejection | A pure byte-sum loses its range trap. Adding only a builder after the loop retains it. `Facts::new` rejects unlisted operations anywhere in the function. | Replace function-wide effect rejection with candidate-local, root-specific invalidation. Keep whole-function structural validation. |
| Additional admission failures | An unused `borrow mut other: i64` also retains the trap. Naming the bound `n := src.len()/4` retains it without either of the other changes. | Relevant roots and reaching definitions, not parameter-mode-wide rejection or expression spelling. |
| Actual sampler composition | The pinned sampler includes `borrow mut random`, a named bound, an offset local, `finite_f32` inside the scan, nested insertion loops and writes to fresh arrays. Allocation/math/RNG outside the scan is only part of the problem. | All these shapes belong to the acceptance fixture. A pure-loop-only patch does not close this request. |
| Repeated descriptor loads | Reproduced in optimized sampler IR. Raw IR already has `nonnull readonly captures(none)` on the shared descriptor parameter, including the local reader. A diagnostic source copy of the descriptor moves its loads to entry while the range trap remains. | Snapshot a proved-stable descriptor in MIR; do not add blanket `noalias`, and do not conflate descriptor and backing-byte effects. |
| Foreign update allocation | The wrapper graph ends at `align_gpu_input_update`, which is opaque to `CallEscapeSummary`; cross-unit calls are also opaque. | Native lifetime evidence is now available. Compiler-carried certification and cross-unit dependencies remain separate missing mechanisms; section 5 records the exact boundary. |
| f32 widened to f64 | Source explicitly uses `as f64`, f64 candidate storage and a f64 threshold. Optimized IR contains `fpext`. | This is faithful lowering. Precision-preserving comparison optimization and vector scanning are later capabilities, not an implicit sampler policy change. |

The descriptor is passed by pointer for an Align `borrow slice` parameter.
An ordinary by-value Align slice is an aggregate; an extern slice argument is
only its data pointer, with length passed separately. The native declaration in
`ggml_ffi.align` is `data: slice<u8>`, **not** `borrow data: slice<u8>`; the
shared-borrow parameters are on the Align wrappers. Result shape, `const void *`,
absence of an out-parameter, and `borrow` spelling do not certify foreign retention. See the
[LLVM parameter-attribute contract](https://releases.llvm.org/22.1.0/docs/LangRef.html#parameter-attributes)
for the distinct alias, access and capture meanings.

## 2. Selected capability and unchanged public contract

Implement composed byte-loop proofs, stable descriptor snapshots and local
read-leaf exposure as one useful capability. It must optimize the unmodified
shape of the current sampler, without asking the consumer to extract its loop,
copy a descriptor, remove a helper, replace a builder, or change precision.
Do not wait for foreign-call certification or the broad plan 61 implementation.

| Existing public surface | Inputs/defaults/errors | Ownership, allocation and owner | Identity, prerequisite and acceptance |
| --- | --- | --- | --- |
| `slice<u8>.{u8,i8,u16_le,u32_le,u64_le,i16_le,i32_le,i64_le,f32_le,f64_le}` and existing big-endian siblings | Existing offset types, width rules and reached range errors. No new syntax or unchecked permission. | Same view lifetime and alignment-1 reads. No byte allocation/copy. MIR proves guards, LLVM lowers existing operations. | Recompute from actual validated MIR. Current producer validation is prerequisite; source/CFG/width owners below qualify it. |
| Existing `loop` and mutable local indices | Zero-based positive single-step recurrence and equivalent named bound/offset expressions; unsupported shapes keep checks. Evaluation order, wrapping, early exit and first reached failure remain. | No owner movement, extra source effect, speculative read or trap hoisting. | All emission routes use the shared owner; no serialized range certificate or runtime format. |
| Existing shared slice parameter | Descriptor snapshot only after its binding is valid and its stability is proved for every rewritten use. Empty views do not authorize reading their data pointer. | Copies only the existing Copy descriptor into compiler SSA, never backing bytes; source generation/lifetime and ABI stay unchanged. | Root/effect and operand-order proof. Raw/optimized IR and actual native instructions own acceptance. |
| Existing local scalar byte-reader calls | Concrete nonrecursive read-only leaf with a scalar result and existing byte guards; no foreign/indirect/cleanup-bearing call. | Actual arguments evaluated once, in order; local binding/return and trap semantics preserved. No allocation or ownership-bearing temporary is introduced. | Consume only bodies already covered by the emitted unit/partition identity; opaque imports remain calls. Local, concrete-generic, whole/per-unit and partition fallback owners are mandatory. |

No public API, CLI default, native calling convention, runtime symbol, persisted
format or new MIR variant changes in this capability. Draft/spec/mirrors therefore
retain their contracts. Update only this plan and the adjacent implementation
status when it ships. The deferred native certificate in section 5 is not a
public API introduced by this document.

## 3. MIR design

### 3.0 Preparation scope and cache boundary

Keep the existing function-local range simplification in shared lowering. Local
leaf exposure must not run there: that stage precedes emission-scope selection.
Add one separate MIR-owned preparation step after the emitter has selected
Whole, Test or Function scope and before pure LLVM module construction. Its
input is immutable original MIR plus the exact defined-function inventory; its
result owns any rewritten bodies. Reuse original MIR when there is no candidate.
Never mutate the driver's memoized Program or publish prepared MIR into that
scope-independent cache.

The shared preparation wrapper must serve `build_program_module` and the
separate `collect_opt_remarks` construction route, including raw/optimized IR,
ordinary/PGO objects, whole/test emission and function prelink. Support-only
partitions contain no candidate body. LLVM lowering consumes the prepared view;
all cloning, range decisions and structural validation remain owned by MIR.
`emit-mir` continues to show pre-emission MIR and does not claim to display a
scope-specific exposed leaf. Native/object/optimized-IR owners observe the final
prepared body; direct MIR owners invoke preparation with an explicit inventory.

Whole/per-unit keys already include the unit's private implementation; Test keys
include the selected program. Function partition keys intentionally omit peer
bodies. Therefore Function preparation defines only the selected function and
cannot expose a peer leaf, even when original MIR still contains that body.
Run exposure, descriptor/root analysis and composed range simplification on the
owned view, then validate it before lowering. Failed applicability retains the
original candidate; malformed generated MIR is a compiler rejection, not a
silently trusted optimization. Compiler identity invalidates older artifacts.

### 3.1 Structure and reaching definitions

Separate validated structure from optimization applicability. Validate all block
ids, targets, statement/value types, parameter binding, SSA order and cleanup
structure before deriving an optimization. Unknown operation semantics reject
only the affected candidate proof; malformed MIR remains invalid globally.

Compute reachability, block/edge dominance and the unique natural-loop latch
from actual CFG edges. These facts may be queried on demand rather than stored
as a separate serialized loop inventory. The initial recurrence admits one header and one incrementing
latch per candidate; nested loops that do not modify the outer induction are
allowed. Irreducible or multiple-entry candidates, ambiguous latches and
unsupported joins retain their checks. Do not erase unrelated operations merely
to fit a recognized shape.

Resolve `Use`, immutable local `Store`/`Load`, named bounds and offset copies with
reaching definitions at the exact use. A value defined once syntactically is not
a timeless fact: binding must dominate the use, no replacing write may intervene,
and every incoming path must agree. Mutable induction loads remain fresh on each
iteration. Do not recursively chase cycles on the Rust stack.

### 3.2 Root-specific invalidation

Track three distinct identities: descriptor storage, backing-byte owner/generation,
and index/bound slots. Derive proof relevance through copies, slices and known
place projections. Array element writes are not descriptor writes. Admit writes
to fresh local candidate arrays only when their ownership provenance proves them
disjoint from the descriptor and source owner; distinct variable names alone do
not prove disjointness.

A `borrow mut random` used after the scan does not invalidate logits during the
scan. A mutable call, raw operation, unknown call or escaped alias that may
replace the relevant descriptor/index or invalidate its owner does. Analyze every
path between snapshot/bound definition and each use, including loop re-entry;
source publication before the loop can invalidate later stability. An operation
outside the syntactic loop is not automatically harmless. Facts meet by
intersection at joins. Independent roots may keep their independent facts.

For this capability, use a restricted body-derived effect classifier for concrete
local scalar/read-only leaves and explicit MIR store/place operations. This is
an ephemeral optimization prerequisite, not plan 61's source-access permission
or its public wire program. Do not infer no-write from Pure or no-retention from
an LLVM descriptor attribute. Unknown effects keep the existing execution.

### 3.3 Local read-leaf exposure

`finite_f32` contains the first byte guard; the caller's later `f32_le` is not the
only check to remove. LLVM inlining happens after the current MIR proof. Merely
widening `Facts::new` cannot remove a guard still inside a separate MIR function.

Expose eligible local leaf bodies before range reasoning using existing MIR:
clone a validated acyclic scalar/read-only body at the original call position,
remap every block/value/slot/type/source coordinate, bind already evaluated
arguments once and route scalar returns to the original continuation. The cloned
range trap stays at its original reached point until independently proved safe.
The descriptor snapshot can be passed through the existing Copy view binding.

Initial eligibility is deterministic: at most 16 blocks and 128 statements per
leaf, no loops/call graph recursion, owning locals, Drop, cleanup-return ABI,
foreign/indirect calls or escaping view results. At most 32 eligible sites and
2,048 cloned statements per caller are exposed, in stable block/statement order;
exhaustion preserves remaining calls. These are compiler work/code-growth limits,
not user allocation guarantees. Revalidate the transformed body before proving
ranges. A private callee outside the current partition cannot supply this proof
without its body entering cache identity; keep the call in that mode.

The ordinary per-unit sampler includes its private `finite_f32`, so this boundary
has a useful shipped consumer. Whole and per-unit success must be shown; ThinLTO
may conservatively retain an inter-partition check and must retain correct output.
No consumer-invisible private-body assumption enters an existing partition key.

### 3.4 Descriptor snapshots and arithmetic

Place one descriptor `Load` at the earliest existing dominating descriptor read
that is valid for all selected uses, after required parameter/local initialization.
If no such reached point is available, keep the original loads; do not speculate
an otherwise unexecuted data access or add a preheader trap. Rewrite only uses in
the proved stability interval, including eligible leaf argument bindings. Keep
all backing-byte loads at their original execution points. Distinguish a reusable
descriptor from mutable bytes behind it.

For widths 1/2/4/8, prove `0 <= i < floor(len/w)` on the exact admitting branch
arm. Both Ge-exit and Lt-admission spellings must pass through named definitions.
The same arm must dominate the guard and increment. Check no increment or
reinitialization intervenes between admission and read; no rejected-arm rejoin,
equal successors, stale load, wrapped multiply or inclusive bound qualifies.

Remove only the proved range branch. Keep genuine array insertion bounds and
all error paths. This is not an attempt to hoist `values[39]` across updates:
the threshold changes and must remain current. No new `assume`, `inbounds`,
`nuw`, `nsw`, `noalias`, or fast-math assertion is minted from source spelling.

## 4. Closure matrix and verification

The closure matrix below defines the capability boundary. Its concrete owners
and explicit conservative fallbacks follow the table. Reuse `runway_a2_binary_codec` for the source/MIR
owners and `target_cpu_isa` for native instruction controls; keep them in
`scripts/test-codegen-performance.sh` on Linux x86/ARM and macOS Apple Silicon.

| Axis | Required implementation / discriminating owner |
| --- | --- |
| Actual consumer | Frozen, attributed PR-247 sampler shape: named limit/offset, finite reader, nested array insertion, unrelated mutable RNG and postprocessing; original optimized IR retains the inner logits trap and descriptor reload. |
| Independent rejection causes | Pure positive plus post-loop builder only, unused mutable parameter only, named bound only, local reader only; each fails the old optimization expectation independently. |
| CFG admission | Ge/Lt, equal-successor and rejected-rejoin mutations, multiple-entry/irreducible loops, extra latch, step-before-read and initialization inside loop; original reached trap retained for each invalidated proof. |
| Definition order / wrap | Slot copies, branch/loop joins, bound replacement, stale header load, negative/overflowing offsets, step 0/2 and widths 1/2/4/8; correct branch and actual read types must agree. |
| Aliasing / effects | Shared source aliases allowed; source replacement, hidden mutable callee, raw escape and callback rejected; independently allocated insertion arrays allowed. Test descriptor mutation separately from payload mutation and owner expiry. |
| Native and unknown effects | Calls after a completed scan do not invalidate it; relevant calls before/between reads do. Unknown imported/foreign facts never become empty effects. |
| Leaf construction and control | Exact argument evaluation, repeated arguments, source span/value/block remapping, scalar return joins, early failure; owning/cleanup/recursive/cyclic malformed candidates not exposed. |
| Ownership, moves and Drop | Descriptor is Copy, owner remains in place; construction/move-in/move-out/nulling/replacement/return/Drop are unchanged. Existing whole/per-unit cleanup owners must fail if exposure duplicates or skips cleanup. |
| `if`, `match`, `else`, `?`, `map_err`, joins and exits | Surround the scan with each existing control shape and retain identical outputs, first reached error and owner cleanup; no proof moves across its validity interval. |
| Generics / interfaces / partitions | Concrete local generic read leaf; ordinary per-unit reader definition; opaque import and other ThinLTO partition fallback; source check, IR/object, runtime-LTO on/off and final executable parity. |
| Cache | Same-process Whole-then-Function preparation on the same original Program must leave original MIR unchanged and retain the Function peer-call fallback; cold/warm/private-leaf-edit/revert controls; every consumed body already belongs to the artifact dependency identity. No serialized optimizer plan or silent proof from absent body. |
| Resource and code size | Bound leaf exposure as above; no new runtime allocations/copies. Existing compiler gate remains below its hard budget; record compile time and text size for unchanged and sampler fixtures. |
| Native machine code | Inspect loop-scoped loads/branches on generic and named Apple ARM targets, native Mac execution, and x86 v2/v3 reverse controls; assertions identify the loop, not absence of every bounds symbol in the function. |
| Sampling oracle | Empty/short/tail/unaligned, vocabulary below/equal/above k, duplicate logits, signed zero, finite extremes, NaN/Inf in every group position, ascending/descending/random inputs; exact token identity, tie order, errors and RNG advancement. |

Deliver this as one compiler capability PR. Loop composition, descriptor stability
and local leaf exposure share proof inputs and jointly close the actual sampler.
If the diff exceeds roughly 1,000 handwritten lines, this is preferable to a
sequence whose intermediate metadata has no accepting consumer; the matrix has
one validation/identity owner and one real composition fixture. It does not pull
in the separate foreign contract or SIMD algorithm boundary.

### 4.1 Implementation closure

`align_mir::byte_prepare::prepare` owns emission-specific, immutable-input leaf
exposure and block ordering. `byte_ranges::Facts` owns typed copy resolution,
exact-arm recurrence proofs and fresh-builder disjointness. Its effect query
covers the complete reachable prefix to each use, including loop re-entry;
unknown effects on that prefix invalidate the candidate. `snapshot_descriptors`
reuses a previously reached dominating load after parameter initialization.
`build_module` is the shared preparation boundary for ordinary module emission
and optimization remarks; `lower_prepared_module` validates the prepared view
before LLVM construction. Existing producer validators also validate the input.

The first admitted descriptor root is an incoming ByValue/shared byte slice,
including its typed local copies. Fresh primitive heap builders and their built
arrays are the disjoint writable roots. Nonempty place projections, arbitrary
locally owned source generations, region builders, payload mutation of the input,
unknown/native/indirect calls on the use's prefix and other unclassified effects
retain checks/snapshots. They do not acquire an empty effect summary. These are
conservative applicability boundaries; ownership and cleanup execution remains
unchanged. In particular, a scalar leaf is never an owning or cleanup-bearing
function, and exposing it adds only Copy slots and values. The sampler's separate
owning builders and early error cleanups remain in their original control paths.

Concrete owners in `runway_a2_binary_codec`:

- `composed_byte_sampler_exposes_both_guards_without_changing_cached_mir`: the
  frozen PR-247 body, both reads, one source descriptor load, real array checks,
  explicit f64 and same-process Whole/Function immutability/opaque-peer control.
- `composed_byte_proofs_accept_named_copies_and_ignore_post_loop_effects`:
  independent postprocessing, mutable-parameter, named-copy and local-leaf cases.
- `byte_range_malformed_and_invalidated_proofs_fail_closed`: Ge/Lt same-arm and
  rejoin controls, all widths, stale types, nonzero initialization and wrong steps.
- `composed_byte_effects_and_reaching_definitions_fail_closed`: mutable calls,
  unknown prior calls, replaced bounds, inclusive bounds, extra updates,
  overflowing and stale offsets. The complete-prefix classifier rejects raw,
  callback, owner-expiry and unproved writes by the same default arm.
- `composed_leaf_exposure_covers_generics_returns_and_has_bounded_growth` and
  `composed_leaf_malformed_and_effectful_candidates_remain_calls`: concrete
  generics, scalar return joins, site cap, cyclic/malformed/owning leaves,
  duplicate definitions/bindings, invalid slots and argument arity.
- `composed_leaf_arguments_execute_once_and_keep_early_returns`: argument order,
  exactly-once evaluation and an early return over an empty byte view.
- `composed_reader_per_unit_and_thin_cache_bind_private_body_edits`: ordinary
  per-unit preparation plus cold/warm/private-edit/revert ThinLTO execution.
  `function_thin_lto` retains the existing peer-body-independent prelink and
  backend dependency owner; `emit_llvm_stage`/`explain_opt` cover both emitters.
- `composed_sampler_matches_checked_execution_and_rng_advancement`: unchanged
  sampler against an independently guarded equivalent, 0–81 words across nine
  input distributions, nonfinite values at all 41 positions, unaligned input,
  short/tail rejection, every Selection field and the next RNG value.

`target_cpu_isa::composed_sampler_native_cpu_controls_preserve_scalar_policy_without_byte_traps`
checks the exported sampler's descriptor load and actual object relocations and
floating-point conversion instructions on x86 v2/v3 or ARM generic/apple-m1.
It runs in the existing shared Linux x86/ARM and macOS native owner. This is not
a SIMD speedup claim and does not measure Metal inference latency.

Local x86-64 measurement uses the frozen sampler with 50,000 deterministic
logits and 2,000 sampling calls, release/baseline, runtime LTO disabled, identical
production runtime and no allocation instrumentation. Five fresh process runs:
median 120.006 ms before and 99.022 ms after (17.5% less wall time). Both print
`1605244` and next RNG `8423113656621481766`. Executable text decreases from
321,915 to 321,659 bytes. These are local workload measurements, not a portable
percentage promise. Optimized-IR emission medians over five runs are 415.8/419.4
ms for the sampler and 20.5/19.2 ms for the unchanged pure byte-sum control.
Sources, commands, compiler/executable hashes, IR and raw samples are preserved
under the local Git evidence directory `issue-1043-followup`.

## 5. Foreign-call promotion: evidence closed, contract still required

The source lifetime evidence is materially stronger than at plan 63's first
investigation. At ggml pin `bb4caa7540188872173c44d161602d9271386413`:

- The shim's `align_gpu_row_input_check` and `align_gpu_row_input_accept` copy
  scalar values and do not publish `data`.
- [Generic tensor set](https://github.com/ggml-org/llama.cpp/blob/bb4caa7540188872173c44d161602d9271386413/ggml/src/ggml-backend.cpp)
  dispatches through the actual tensor buffer implementation.
- [Metal buffer set](https://github.com/ggml-org/llama.cpp/blob/bb4caa7540188872173c44d161602d9271386413/ggml/src/ggml-metal/ggml-metal-device.m)
  uses memcpy for shared storage; its private-buffer path wraps the source and
  waits on completion before returning. That latter path also has native
  no-copy buffer alignment/size prerequisites; lifetime evidence alone does
  not qualify arbitrary four-byte stack addresses for it. The
  [Apple API contract](https://developer.apple.com/documentation/metal/mtldevice/makebuffer%28bytesnocopy%3Alength%3Aoptions%3Adeallocator%3A%29?language=objc)
  requires a page-aligned pointer and region. At this pin, unified-memory devices
  normally select shared buffers, but `GGML_METAL_SHARED_BUFFERS_DISABLE` can
  select private storage (`ENABLE` overrides it). The shim allocates inputs from
  the backend default type, so record actual placement and these overrides.
- [CUDA buffer set](https://github.com/ggml-org/llama.cpp/blob/bb4caa7540188872173c44d161602d9271386413/ggml/src/ggml-cuda/ggml-cuda.cu)
  enqueues the host-to-device copy and synchronizes its stream before return.

This supports synchronous source consumption for these inspected implementations,
not an annotation the current compiler can consume or a claim about every backend,
custom buffer, native build option or asynchronous API. Source retention is no
longer simply “information not supplied.” The remaining barrier is transporting
an explicit, justified contract into compiler proofs, plus the actual native
storage preconditions for the selected tensor placement.

The selected next design boundary is a native call-scoped **backing-storage**
certificate together with its cross-unit consumer. It must specify all of:

| Required contract dimension | Fixed safety requirement |
| --- | --- |
| Root | Exact C symbol, logical parameter ordinal, lowered pointer role and source view; not merely a descriptor pointer. |
| Time | No byte/address/provenance retention beyond normal return, including success/error exits and callbacks, tasks, queues and device work; before-return completion is required. |
| Access | Read/write, free/ownership transfer, pointer identity observation and alignment/extent constraints are separate from escape. Stack placement needs all applicable facts; `const` alone proves none. |
| Source authority | Explicit native-package-owned unsafe obligation bound to the declared native operation; a GitHub comment, naming whitelist, return type or arbitrary cache blob is not compiler authority. |
| ABI | Preserve C data-pointer lowering and explicit length; a new contract must not accidentally turn the argument into an Align descriptor pointer or add a second length. |
| Graph | Derived facts pass through wrapper `else`/Result paths, resource ownership, private helpers and concrete generics. Import/ThinLTO transport must bind the complete relevant private-body and native-contract dependencies. |
| Failure | Missing/unknown/changed/unverifiable facts retain heap allocation; malformed supplied metadata rejects before emission. A stale positive certificate never survives a dependency change. |
| Proof owner | Extend the shared interprocedural analysis/transport owner in plan 61 when shipped, or revise that owner and its prerequisites coherently. Do not create a competing serialized noescape schema. |
| Acceptance | Three-unit resource wrapper and synchronous C sink; retaining/error/callback/asynchronous/changed-contract controls; cold/warm/private-edit/revert and allocation counts; real Metal/CUDA placement qualification remains native-owned. |

This document does **not** invent `foreign_borrow`, infer a contract from a
Result return, or silently strengthen existing `borrow`. Choosing a public
certificate spelling, its authoritative artifact/validation format and its full
native trust contract is explicitly deferred. That is a separate public-contract
design gate, not implementation-ready permission for a special-case whitelist.
Before implementing it, its own exact ledger must cover signatures, diagnostics,
encoding/validation order, all publication/cache paths and specification mirrors.
The loop capability above can ship independently while this boundary is completed.

For the four update buffers, measure shell and payload allocation/free events
separately. Four `buffer_new` calls do not establish four allocator events, and
zero allocations for those objects does not establish zero allocations for all
input preparation or for the native backend.

## 6. Precision, SIMD and measurement order

Keep the sampler's source f64 policy in the first capability. Finite f32 values
widen exactly, but the f64 threshold initializer is not itself an exact f32
constant; later thresholds originate from candidate storage. A future generic
comparison rewrite must prove both operands' representable f32 origins on the
reached path, including phi/array stores and sentinels. A caller's explicit
f64 arithmetic, normalization, RNG quantization and error handling stay unchanged.
F32 and F64 share ARM's physical SIMD/FP registers; removing `fcvt` does not by
itself prove that physical register pressure halves.

Qualify vector scanning after the scalar proof closes. Stateful sorted insertion
is not an associative maximum reduction. A later vector filter could snapshot a
threshold to select a superset of candidates, then replay lanes in ascending token
order against the current threshold. It must validate nonfinite lanes, partial
tails, ties and errors without speculative out-of-range loads, and preserve exact
state/RNG behavior. Do not replace this with `fmaxnm` or claim four candidates per
cycle or 3x speedup from an instruction listing. Plan 62's oracle remains the
semantic acceptance owner; no Top-K terminal or vector source API is selected here.

For each proposed optimization, compare fixed compiler/client SHAs, binary hash,
CPU/model/OS, exact commands and runtime-LTO/ThinLTO choices. Hold logits/k/seed,
sampling policy and model fixed. Record native Mac update, readback/wait,
selection/normalization, GPU and end-to-end timing separately; Linux CUDA/CPU
results are separate. Allocation instrumentation and timing use different fresh
processes. Include short inputs, random/reverse/tied distributions, code size,
compile time and unchanged typed reductions as reverse controls.

The comment's 85.38 ms/token and 0.278 ms CPU sampling are reported measurements,
not measurements reproduced here. Its 53 GB/s estimate already applies a 78%
efficiency factor to 68.25 GB/s; agreement with that estimate is not 99.4% of
physical peak. Roughly 0.33% relative CPU time does not prove CPU/GPU overlap or
that every CPU cost is hidden. These qualifications do not reduce the priority
of making ordinary composed Align code optimize correctly.

## 7. Author closure and follow-up boundaries

The implementation author pass extracts the normative obligations from sections
2–4 and maps them to section 4.1's code and owner tests. The selected three pieces
ship together. No consumer source, pin or build machinery changes; the external
request-register answer is separate and uncommitted. Native Metal latency remains
consumer-owned, and the local CPU measurement does not substitute for it.

The next distinct capabilities are the explicit native certificate and its
cross-unit proof transport (section 5), followed by separately justified
precision-preserving comparison/vector filtering (section 6). Neither is a
hidden prerequisite of the composed scalar sampler delivered here.

One fresh independent design review found one P2: the leaf-exposure scope seam
was not concrete enough to protect a shared memoized MIR instance. Section 3.0
now fixes preparation after scope selection, immutable original MIR, the separate
remarks path and the same-process Whole-to-Function negative owner. The author
checked the finding against `ModuleScope::defines`, `build_program_module`,
`collect_opt_remarks`, `thin_view_hash` and the unit cache inputs. No P1 or other
independent finding was reported. This is a finding-to-design correction, not an
implementation verification result.
