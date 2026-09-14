# Decode optimization: evidence and implementation plan

Status: implemented compiler capability, 2026-09-14. Bounded local byte storage
and direct synchronous chunk iteration preserve existing APIs. The primary
consumer target is Apple Silicon with Metal; local x86-64 measurements are
reference evidence, not a claim about Mac inference latency.

Baseline: `881076ce56be96a3ea4905fe4ad59e12a4c903c8` on Linux x86-64,
LLVM 22.1.8, a freshly built `target/debug/alignc`. The compiler's Cargo profile
is independent of the generated program's profile: the probes use optimized
LLVM emission, which runs `default<O2>`, with baseline CPU and explicit
runtime LTO on/off. Both lenses produced the findings below. No original
before/after client revision, Apple Silicon timing, allocation trace, or
branch-miss profile accompanied the report. The client's reported slowdown
and “500+ allocations per token” remain unverified.

This extends [the inference audit](14-llm-inference-focus-audit.md) and
[the allocation audit](13-string-array-allocation-short-input-audit.md).
The existing language and library specifications remain authoritative.

## 1. Contract ledger and scope

These are existing public surfaces, preserved by the optimizations.
Implementation-private strategy is specified in sections 3–5. Experimental
Top-K policy in section 6 is a benchmark oracle, not a shipped API proposal.

| Surface | Inputs, result and error behavior | Ownership, lifetime and allocation | Owner, identity and prerequisites | Acceptance |
| --- | --- | --- | --- | --- |
| `buffer(cap: i64) -> buffer`; `b.put_<scalar>_<le\|be>(value) -> ()`; `b.append(bytes) -> ()`; `b.bytes() -> slice<u8>`; `b.len() -> i64` | Existing scalar widths and positional evaluation. Constructor capacity is a read window, not initialized length or a growth limit. Nonpositive capacity gives an empty window; the current reserve-failure behavior is retained on any observable runtime path. Scalar reads retain hard range failure. | Buffer remains Move. Bytes borrow its owner; no returned stack view, implicit clone or alternate public container. Elision is allowed only for a compiler-proven local object whose observable bytes/scalars can be preserved. No general guarantee for every buffer below a size threshold. | Sema/HIR retain existing signatures. MIR owns scalarization proof and replacement; LLVM only lowers it. No new native ABI, interface type or wire record is planned. Existing implementation/cache identity must invalidate changed lowering. Current M12 binary codec and producer validation are prerequisites. | Bounded scalarization owner, binary-codec owner, lifetime/cleanup negative controls and exact allocation evidence. |
| `xs.chunks(n: i64)` consumed immediately by a sequential pipeline terminal | Evaluate source and `n` once before callbacks. Existing nonpositive-`n` empty result, chunk order and final partial chunk remain. A scalar read of a short final chunk still fails if execution reaches it. | Virtual iteration creates borrowed chunk descriptors without an owned header array. Source/synthetic-owner lifetime covers every consumer; an explicitly materialized result keeps its existing ownership and allocation. | MIR chunk-source plan and all admitted sequential consumers ship together. Stored/boundary and explicit parallel policies retain their existing implementations. Reuse the existing virtual-range representation where its contracts fit. [Plan 09](09-explain-opt.md) records the actual selected strategy. No public/interface type or native ABI expansion. | Source effects, full-fold any/all, conditional reads, traps, owner expiry, terminal matrix, whole/per-unit and malformed-producer owners. Header-allocation and hot-loop IR evidence. |
| `bytes.f32_le(off: i64) -> f32`, and the existing binary-read family | Byte offsets; range validation before the load; exact endian interpretation. Alignment 1 remains valid. No numeric conversion of individual u8 values into float lanes. | Pure scalar result, no retained view or owned allocation. Removing a guard requires a range proof at that execution point. | MIR supplies existing range semantics; LLVM lowers alignment-1 loads/bitcasts. Current code assumes little-endian x86-64/AArch64 hosts; this plan adds no new supported target. | Width/endian, misalignment, overflow, partial tail and effect-before-trap owners. |
| `resource.view_from_raw(owner_ref, ptr, len) -> Option<slice<f32>>` under the existing expected-type rule | Existing unsafe, resource-declaring/internal boundary. `len` is an element count. None for detectable bad count, nonempty null or alignment; foreign extent, initialization and synchronization remain wrapper obligations. This is native f32 storage, not an arbitrary LE wire decoder. | Copy descriptor tied to the resource generation; no allocation or copy. Owner expiry/mutation rules remain. Read-only/writable authority cannot be inferred merely from pointer alignment. | Existing native resource mechanism, no new API. Safe native publication must obey the pending interprocedural access boundary in [plan 61](61-interprocedural-view-access-plan.md). This plan neither claims that boundary shipped nor adds an unchecked bypass. Consumer/backend adaptation is external work. | Existing resource owner plus eventual provider-specific f32 publication, synchronization, mutation and lifetime acceptance. |
| `s.load(i: i64) -> vecN<T>`; scalar/array reductions | Typed element indices; existing vector range guard and scalar lane semantics. Width remains explicit only in the kernel tier. | Copy vector value; existing borrowed input, no allocation. No reassociation, NaN or signed-zero relaxation is implied by optimization. | Existing MIR VecLoad / LLVM lowering and vec-mask design. No byte-specific SIMD method or new width family. | Existing vector load/store owner; typed-vs-byte oracle and actual target code inspection. |

There is no new CLI input, environment setting, serialized schema, reflected
type, process-global state, foreign text boundary or package dependency.
There is consequently no new tag/byte format to specify. If implementation
needs one, reopen the relevant ledger before coding it.

## 2. Assessment of the five suggestions

| Suggestion | What the current source and optimized probes establish | Decision |
| --- | --- | --- |
| 64-byte SSO removes all allocation | `align_rt_buffer_new` reserves a `Vec<u8>` and boxes `Buffer`. A positive four-byte request normally allocates twice; an empty Vec does not allocate a payload. Buffer calls survive O2 with runtime LTO on and off. Putting bytes inside the existing Box would still allocate the Box. Existing stack-header selection covers `builder`/`array_builder`, not `buffer`. | Optimize proven local byte construction in MIR first. Do not introduce a general inline/heap tagged representation. General SSO is explicitly rejected in Settled; reopening that decision needs plan 23 evidence. A hardcoded 64-byte source promise is not selected. |
| Bytes cannot reach a numeric pipeline without copying | Ordinary safe byte-slice-to-float-slice reinterpretation is absent. However `f32_le` already lowers to a bounds guard and alignment-1 integer load/float bitcast; it is not a per-element runtime decoder call. Existing resource-owned native typed views are another, narrower route. | Improve existing binary traversal and virtual chunk sources; keep native typed publication at its resource owner. Do not add `as_f32_le()` pretending all byte storage has native alignment, byte order and access authority. |
| Native Top-K would automatically fuse and vectorize | No Top-K terminal exists. Selection has state-dependent insertion/heap/partition work; a syntax addition cannot establish a profitable SIMD algorithm. A value-only Top-K also loses token identity unless index retention is specified. | Qualify a stable indexed selection kernel before choosing a core terminal. Define exact benchmark semantics, caller storage and algorithm alternatives in section 6. |
| Fixed `values[39]` is checked on every iteration because LICM is absent | The fixed 40-element f64 witness loses all bounds calls and array storage at O2. The f32 `out slice` witness checks length >= 40 once on the nonempty loop-entry path and retains a changing threshold in SSA. LLVM's stock optimization already handles these cases. | Obtain the real residual loop before adding a pass. Preserve the first failing access and zero-iteration behavior. A changing kth value cannot be loaded once as an invariant constant. |
| A byte-specific `load_f32x8` guarantees fast NEON | Existing VecLoad requires a typed slice. Typed `slice<f32>.max()` in the baseline probe remains scalar too: type admission alone does not establish a legal profitable vector reduction. | Share existing byte-load/range and typed SIMD machinery. A new scalar/width/endian method family is not selected. NEON f32 vectors are four lanes per 128-bit vector; vec8 can lower to multiple instructions. |

Source anchors: `align_runtime/src/lib.rs::align_rt_buffer_new`,
`align_runtime/src/buffer_storage.rs`,
`align_codegen_llvm/src/lib.rs::{stack_header_plan, Rvalue::BytesRead, Rvalue::VecLoad}`,
and `align_mir/src/lib.rs::{chunks_plan, emit_bounds_check, lower_bytes_read}`
(all under `crates/`).

A further confirmed cost is the current sequential chunk source:
`bytes.chunks(4).map(fn word { word.f32_le(0) }).max()` keeps
`align_rt_chunks`, a range check per chunk, and a final header-array free.
At 50,000 f32 elements its metadata alone is 50,000 * 16 = 800,000 requested
bytes, in addition to the existing 200,000-byte input. This is derived from
`align_rt_chunks`'s header layout, not a measured RSS or latency claim.
Document 13 section 8.2 already records sequential pipeline virtualization as
unfinished; direct `.len()`/index are different, already virtualized consumers.

A hand-written byte loop guarded by `src.len() % 4 == 0`, using
`i < src.len() / 4` and `src.f32_le(i * 4)`, also retains a range-failure edge
inside its O2 loop. That is a concrete range-proof candidate; it does not
establish the cause of the client's Top-K regression.

## 3. Capability A — scalarize bounded local byte construction

Use compiler analysis to remove an unnecessary byte owner when only scalar
observations escape. Keep ordinary buffer representation for other uses.
This follows the existing nonescaping-builder approach, with the additional
proof needed to remove the payload and native calls, not just the shell.

The selected representation is a MIR byte object with a statically bounded
initialized prefix, exact scalar writes and byte observations. Its contents
can lower to integer SSA fragments or fixed local byte storage. It has no
runtime inline/heap discriminator and never masquerades as a Rust Vec over
stack memory. Once scalarized, it must not be passed to any Buffer native ABI.

Admission requires a complete def-use/CFG proof for each object lifetime:

1. Construction and all writes have a finite compiler-known maximum extent.
   Preserve evaluation of every capacity, value and offset expression.
2. Every use is an audited scalar put, bounded append of known bytes, local
   byte view/range, scalar binary read or length observation. Mixed-width and
   mixed-endian sequences are part of the same byte semantics.
3. Every read has the same initialized-byte and failure behavior as the source.
   A conditional read cannot cause an earlier unconditional trap. Use the
   existing MIR range failure when proof does not eliminate it.
4. No buffer handle or derived view crosses a user/native call, return, capture,
   aggregate store or unknown use. The scalar result may cross those boundaries.
   Native readers/writers and capacity observations disqualify the object.
5. Construction in an outer loop gets distinct logical lifetimes; replacement
   and early exit preserve all unrelated owners. Cyclic byte state or ambiguous
   joins that lack a finite proof retain ordinary lowering as a whole object.

Do not recognize only `put_f32` followed by `u32_le(0)`. Implement the shared
byte-object proof, or defer it: a spelling-specific peephole would create the
special-case language this task aims to avoid. The demonstrated four/eight-byte
scalar conversions are acceptance witnesses for that general bounded rule.
The implementation must state its deterministic stack/code-size budget and
prove that large or escaping objects follow existing lowering. No budget value
becomes a public small-buffer allocation guarantee.

This capability can remove both allocations and call overhead in eligible
conversion helpers. It does not promise zero-copy I/O, eliminate caller-required
byte encoding, or establish the number of such helpers per generated token.
Actual allocator-event instrumentation is required: the current requested-live
buffer accounting combines shell and payload and cannot by itself count the
two Rust allocation events.

## 4. Capability B — virtual sequential chunks and binary traversal

Extend the existing source plan to feed each direct sequential consumer from
`(base, source_len, chunk_len, chunk_count)` instead of an array of headers.
Compute the borrowed chunk at ordinal `j` from the checked remaining range.
Avoid overflow in ceil division and `j * chunk_len`; derive the address only
on the edge that proves a nonempty valid range.

All directly consuming sequential terminals must share this source owner:
reductions (including full-fold any/all), materialization/map_into, partition and sort
where currently admitted. Preserve each terminal's callback order and allocation
contract. An explicit array result still allocates that result; only transient
chunk headers disappear. Direct stage chains carry the same virtual source.
A stored chunks value and existing parallel eligibility policy remain separate
because they expose an owned header collection or worker lifetime boundary.

A later range-proof capability may expose the full-width prefix separately
from the possible partial final chunk. This implementation preserves the
existing per-read range guard; it removes descriptor materialization only. Within the prefix the existing
`word.f32_le(0)` can use its proved four-byte extent, retaining an alignment-1
load. A reached short final chunk still traps at that read; do not reject the
whole input eagerly. An earlier `any=true` or `all=false` does not stop iteration: predicates run
for every surviving element, so a reached short-tail read still traps. A `where`
predicate that discards the tail or an actual `&&`/`||` expression that skips
the read must not acquire a new trap. Source and chunk-size effects happen
before iteration exactly once. Preserve source byte mutations in callbacks;
never pre-read data or memoize loaded values merely because descriptor extents
are invariant.

Reuse the same range facts for the demonstrated guarded byte loop if this can
be done without a new general speculative-hoisting pass. Otherwise close the
virtual pipeline capability first and retain the byte-loop witness as a named
follow-up. Independent capabilities are justified by distinct source-formation
versus general loop-analysis boundaries, not a line target.

Keep LLVM's stock pipeline. Remove only proved guards; do not add global
`noalias`, `inbounds`, alignment or fast-math claims to force vectorization.
An exact IEEE reduction may remain scalar. Vectorizing independent decode/map
loads is useful even when a final ordered reduction is not widened.

## 5. Implementation closure matrix

The independent proof review and its closure are recorded below. The delivered
owners and explicit deferrals following this matrix delimit implementation scope.
No standalone dormant metadata producer is a capability PR.

| Axis | Capability A: bounded byte object | Capability B: virtual source / range proof |
| --- | --- | --- |
| Formation and validation | Shared MIR producer qualification; wrong width, offset, initialized-prefix and unaccounted use reject optimization. Extend binary-codec/MIR owner. | Actual checked chunk source and consumer, valid element layout/count; forged range facts and unrelated operands reject. Extend chunks/MIR owner. |
| Construction and argument order | Capacity, scalar values and append source run once, in order, including effectful/diverging expressions. | Source then chunk size then terminal arguments/callbacks retain existing order, including invalid size and effectful owned source. |
| Move-in/out, return and source nulling | Buffer/view move, return, capture or aggregate storage selects ordinary owner path. Scalar result return keeps no byte owner. Unknown use never drops only half an object. | Retain source owners and view roots; explicit result ownership and source nulling use the ordinary terminal rules. No returned view into transient descriptor storage. |
| Drop, replacement and provenance | No runtime free of stack/SSA bytes; unselected objects drop once; repeated local construction/replacement and unrelated owners remain balanced. No change to BufferStorage pointer provenance. | Header array has no allocation/free only when virtual; materialized path retains its matching free. Synthetic source and terminal output cleanup remain exact. |
| Control-flow product | if/match/else/?, map_err, branch and loop joins, value-carrying break, early return and malformed input: prove or retain ordinary lowering. | Same product plus full-fold any/all predicates, callback mutation, final partial chunk, where-suppressed reads and terminal-specific trap order. |
| Generics and unit boundaries | Concrete generic body is requalified; imported helper may return a scalar. No proof inferred from a function name or stale interface. | Instantiated source/terminal and callback bodies, whole/per-unit and ThinLTO have equivalent results and source lifetimes. |
| Access safety | Read-only origin and local validation rules remain; no optimization bypass of plans 52/57/60. Calls/views escaping scalarization are conservative. | Derived chunks keep source region, read-only and validated-observation facts; native/interprocedural broadening remains plan 61. |
| Allocation and ABI | Eligible object loses shell and payload events; keep noneligible behavior. No Buffer ABI row is added or changed. Cross-check event counts against a deliberate forced materialization control. | Zero header allocation for virtual consumption; expected result allocation still occurs. No chunks runtime-call/free in the admitted MIR/IR path. Existing runtime ABI unchanged. |
| Numeric/byte semantics | All scalar widths, both endian directions, overlapping reads, signed zero, infinities and NaN payload bit patterns; no numeric `as` substitution. | Same decoding results at byte alignments 0..7; n<=0, empty, exact/partial tail, wrap boundaries; source-order floating results unchanged. |
| Performance/resource owner | Local allocation-inclusive 0/1/4/8/32/64/65/256-byte conversion/construction workloads, escaping and large-buffer controls, native call counts and stack budget. | 0/1/39/40/41/50,000 values, full and partial chunks, allocation counts/bytes and O2 hot-loop shape; baseline/native CPU on x86-64 and Apple Silicon. |

Implemented owner mapping:

- `runway_a2_binary_codec::bounded_byte_object_control_matrix` closes A's
  evaluation, lifetime, replacement, fallback and whole/per-unit cells;
  `bounded_byte_object_numeric_matrix` closes its byte/IEEE cells.
- `align_mir::tests::bounded_byte_object_rejects_unaudited_uses` closes A's
  proof-validation and unknown-use negatives.
- `chunks::virtual_sequential_consumer_matrix` closes B's terminal, effect,
  source-lifetime, output-allocation and whole/per-unit cells;
  `virtual_chunks_binary_tail_matrix` closes its binary/range cells.
- Existing MIR `current_plan_catalog_and_record_corruption_fail_closed` and
  driver `malformed_current_plan_precedes_warnings_codegen_and_stdout` close
  reporting validation. Existing checked-HIR chunks Move-element refusal and
  producer validation remain in force; no new imported range-proof record exists.
- Extend these same owners with IR and allocation observations; reuse
  `resource_ownership`, `vec_simd` and existing chunks/binary-codec cases
  where they already fail for the changed invariant.

The any/all control must include an early full chunk establishing true/false,
then a reached short final chunk whose scalar read still traps. A paired where
control discards that chunk before the read. Effectful predicate counts pin the
full fold independently of the return value. No timing assertion
belongs in the bounded correctness gate. Follow the normal self-review,
committed-candidate independent review and final-SHA preflight for code work.
If a capability exceeds roughly 1,000 handwritten lines, record why its one
producer-to-consumer boundary avoids duplicated lifetime and cleanup proof.

## 6. Top-K qualification design

First reproduce the client's current output policy. It owns RNG state, temperature,
repetition penalties, softmax and token selection. This plan does not move those
policies into core or alter them to earn a faster comparison.

Use a provider-local benchmark function with this existing-type shape:

```text
fn select_indices(scores: slice<f32>, out indices: slice<i64>) -> i64
```

This is a benchmark harness interface, not a new library declaration. Destination
length is requested k; result `m = min(scores.len(), indices.len())` counts the
initialized prefix. No allocation, retained borrow, mutation of scores or write
to `indices[m..]` is permitted. Empty source/destination returns zero without
loads. The ordinary out no-alias rule applies. The benchmark order is descending
numeric score, NaNs after all numbers, with original index ascending for equal
scores (including signed zeros) and for NaNs. Output preserves original indices;
selected float bits are read from the input. These rules define a total order on
indices without changing scalar IEEE comparisons or Align's existing Ord rules.
If the client's contract differs, compare under that contract separately.

Compare three complete algorithms, including final output ordering:

- Thresholded sorted insertion: O(V*k), O(k) caller storage; useful reference
  and possible winner for tiny k or a low candidate-admission rate.
- Bounded min-heap of indices: O(V log k + k log k), O(k) caller storage;
  the root denotes the worst retained candidate including the index tie-break.
- Block threshold filtering feeding the same selector: vectorize independent
  comparisons only when their complete NaN/tie mask is equivalent. A threshold
  snapshot may safely admit extra candidates while the true threshold rises;
  recheck each admitted candidate against the current state in original order.
  Never let a stale threshold discard a candidate that should win a tie.

For each use k=0/1/2/8/40/64/128, k>V, ties, NaNs, infinities, signed zeros,
ascending/descending/random/all-equal and captured real logits. Compare the
entire ordered index prefix against an independent full-sort oracle. Feed the
winning kernel into unchanged sampling with a fixed seed to verify token/RNG
parity. Include any byte-to-typed copy, GPU synchronization, transfer and scratch
initialization in the relevant end-to-end timing.

No “branchless Top-K” or eight-lane NEON speed promise follows from this plan.
Choose a reusable public terminal only after this experiment demonstrates both
the missing composition and its profitable algorithm. That separate public
ledger must fix k/error behavior, key evaluation order, ties/NaNs, index retention,
result ownership, allocation, all terminal interactions and generic/type domains.
A materializing Top-K terminal would already return its result array; requiring
an additional `.to_array()` would misstate the existing terminal model.

## 7. Measurement and delivery order

1. Capture client before/after revision, compiler/runtime pin, generated-code
   profile, target CPU, runtime/ThinLTO flags, model, vocabulary/k, seed and
   exact input logits. Split GPU completion/readback, decode update, selection,
   normalization/sampling and output publication. Determine whether the reported
   total is wall time, host CPU time or asynchronous GPU work.
2. Implement A and B as independently useful capabilities after their proof
   review, ordered by that phase profile. They need no public syntax addition.
   Keep the current buffer and scalar-loop probes as negative baselines.
3. Run section 6's selection qualification after removing avoidable representation
   work. Native typed publication is an existing resource integration option,
   conditional on producer ownership/access/synchronization, not a mandatory
   general byte cast or an authorized align-llm code change.
4. Fix a residual bounds/vectorization defect only against its optimized witness.
   Keep the successful fixed-array and slice-hoisting probes as controls.
5. Client adoption and final token/s verification remain align-llm-owned. Record
   provider results in its request register without editing its implementation.

For local performance decisions use warmed repeated trials in balanced AB/BA
order, identical data, a consumed result/checksum, and enough repeated calls to
amortize the timer. Report median and dispersion; end-to-end request samples
also report tail latency. Collect actual allocation/free events, requested bytes,
cycles/instructions and branch misses where supported. Identify unavailable
counters. A predictable bounds branch is not evidence of branch misprediction.
No fixed millisecond gain or acceptance percentage is selected without a paired
baseline. Correctness requires exact outputs; performance adoption requires a
repeatable improvement above observed noise and no material regression in the
named controls. Do not raise test timeouts or turn the benchmark into CI.

## 8. Reproduction and author verification

Local artifacts are retained under `.git/decode-optimization-audit/`: probe
sources, raw/optimized LLVM with LTO on/off, and `summary.json`. This directory
is investigation evidence, not portable checked-in test coverage. Reproduce
with a freshly built compiler:

```text
scripts/cargo.sh build -p align_driver --bin alignc
# Save the following source as probe.align.
target/debug/alignc check probe.align
target/debug/alignc check-per-unit probe.align
target/debug/alignc emit-llvm probe.align --stage raw --no-rt-lto --export byte_max --export bits_roundtrip
target/debug/alignc emit-llvm probe.align --stage optimized --no-rt-lto --export byte_max --export bits_roundtrip
target/debug/alignc emit-llvm probe.align --stage optimized --rt-lto --export byte_max --export bits_roundtrip
```

```align
module decode_optimization_probes
pub fn fixed_threshold(src: slice<f64>) -> f64 {
  mut values := [0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0]
  mut i: i64 := 0
  loop {
    if i >= src.len() { break }
    if src[i] > values[39] { values[39] = src[i] }
    i = i + 1
  }
  return values[39]
}
pub fn slice_threshold(src: slice<f32>, out values: slice<f32>) -> f32 {
  mut i: i64 := 0
  loop {
    if i >= src.len() { break }
    if src[i] > values[39] { values[39] = src[i] }
    i = i + 1
  }
  return 0.0
}
pub fn byte_max(src: slice<u8>) -> f32 {
  return src.chunks(4).map(fn word { word.f32_le(0) }).max()
}
pub fn bits_roundtrip(x: f32) -> u32 {
  mut b := buffer(4)
  b.put_f32_le(x)
  return b.bytes().u32_le(0)
}

pub fn byte_loop(src: slice<u8>) -> f32 {
  if src.len() % 4 != 0 { return 0.0 }
  mut best: f32 := -1.0 / 0.0
  mut i: i64 := 0
  loop {
    if i >= src.len() / 4 { break }
    x := src.f32_le(i * 4)
    if x > best { best = x }
    i = i + 1
  }
  return best
}
pub fn typed_max(src: slice<f32>) -> f32 = src.max()
```

The source above also compares fixed 40-element f64 storage, an f32 out
slice threshold, guarded byte-loop decoding and typed f32 max. Add each function
name with its own `--export` flag to inspect these additional bodies. The fixed local
array is f64 because fixed literal element inference defaults to f64; no result
is reported as a fixed f32-array reproduction. Fixed-threshold guards disappear;
the slice threshold guard is before the loop but after its nonempty test.
Neither byte max nor typed max emitted vector float operations at baseline CPU.
A linked smoke probe passed at dev and release: the four finite-input maxima
agreed (9.0), a zero-iteration slice-threshold call did not access its short
destination, the 1.5 f32 bit round trip returned 1069547520, and unaligned
byte-loop/chunk loads agreed. Both source check modes and the document's source
example pass. LLVM llc assembly also retains no fixed-threshold bounds call and
places the slice threshold comparison before its loop. These observations
qualify the work, not the original client's cause or a timing improvement.
The implemented regression owners are listed in section 5.

Author ledger pass: no public signature, language decision, runtime ABI or
persisted format is changed by this plan. The two compiler capabilities preserve
existing evaluation, numeric, ownership and access contracts. Top-K is explicitly
an experimental oracle, and wider unsafe/access permission remains external to
these optimizations. Normative spec/mirror changes are therefore unnecessary now;
a later public API decision must update draft, digest, design notes, Settled and
the relevant English/Japanese library designs together. Implementation strategy
and selection-status updates belong in the owning optimization documents, once
per delivered capability.

## Independent design review closure

One inspection-only adversarial review found one P1 design error: the draft
assumed any/all short circuit despite their settled full-fold predicate rule.
The ledger, source-plan algorithm, entire control matrix and provider response
now preserve that rule. The paired tail-trap/where-suppression acceptance above
closes the missed axis before implementation. No second discovery review was
used; no other independent plan gap was reported. This is design review evidence,
not a code-review attestation or passed optimization implementation.

## References

- LLVM's [LICM description](https://llvm.org/docs/Passes.html#licm-loop-invariant-code-motion)
  and [alias analysis](https://llvm.org/docs/AliasAnalysis.html) explain the proof
  required for memory motion; Align already uses the stock optimization pipeline.
- LLVM's [vectorization documentation](https://llvm.org/docs/Vectorizers.html)
  distinguishes scalar loop form, legality and floating-point reduction constraints.
- Arm's [Neon intrinsics](https://arm-software.github.io/acle/neon_intrinsics/advsimd.html)
  defines the native f32 lane shapes; source vec8 is not an eight-lane NEON instruction.
- Rust's [slice construction preconditions](https://doc.rust-lang.org/stable/core/slice/fn.from_raw_parts.html)
  illustrate why an arbitrary byte allocation cannot simply become an aligned typed
  native slice. Align's own unsafe resource-view contract remains its authority.

## Implementation boundary refinement

The two selected capabilities ship together as one decode representation-cost
PR. The combined change is expected to exceed 1,000 handwritten lines including
owners: sharing the scalar/byte/chunk correctness and benchmark fixtures avoids
duplicated source-lifetime, numeric and whole/per-unit proof. Both provide useful
independent behavior; there is no dormant consumer prerequisite.

Capability A derives an ephemeral, read-only storage plan from the actual MIR
body, before backend instruction selection. Existing Buffer operations retain
their source/producer-validation meaning; LLVM lowers only the certified plan
into fixed byte storage and ordinary scalar bit loads/stores. No pointer into
that storage reaches a Buffer runtime operation. The plan is recomputed for each
whole/per-unit/function-partition body and is not serialized or accepted from
an interface. This avoids new IR/wire variants and preserves all existing
validation before representation selection. Bound individual byte objects to
64 bytes and cumulative selected local storage to 1,024 bytes per function;
these are deterministic compiler resource budgets, not public SSO semantics.
Unknown uses, mixed extents at a read/write, escapes and over-budget objects
retain the ordinary runtime representation. All allocation and byte-conversion
claims are qualified against the actual selected plan.


## Delivered closure and measured limits

`align_mir::byte_storage` recomputes escape and initialized-prefix facts over the
actual function CFG. The backend applies the complete plan to constructor,
writes, views, lengths and Drop together. Unknown operations refuse selection;
parameter slots and escaped views cannot select local storage. Fixedpoint joins
retain equal lengths only; ambiguous writes/observations, replacement and
unsupported control carriers use the ordinary ABI. The object and function
stack budgets have exact-limit and rejected-next MIR owners. Scalar arithmetic,
branch joins and repeated construction have direct whole/per-unit execution
owners; the per-unit control fixture also executes O0.

`setup_source` consumes the chunks selector once and retains the base's synthetic
owners. Reduce, collect/scan/sort, partition and the admitted source loop machinery
share `lower_virtual_chunk`; source layout restrictions on zip, map_into and
SoA are unchanged. Whole/per-unit fixtures cover generic callbacks, source/width
side effects, borrowed/fresh inputs, reductions, collect, scan, sort, partition,
nonpositive/huge widths and full-fold any/all. The tail matrix independently
pins reached traps and where/boolean-suppressed reads. Native allocation
measurement of `fresh_sum` observes exactly one source allocation and one free;
the baseline also allocates/frees a header array. Stored chunks still materialize.

The numeric owner checks both endian directions against independent expected
byte reversals, mixed widths at an unaligned offset, signed zero, infinities and
quiet/signaling NaN payloads. Dynamic offsets preserve negative, last-invalid
and maximal-offset failure. Native ABI/layout and source-region validators run
on the original MIR before storage selection. Existing binary-codec, resource
and SIMD owners remain the authority for source rejection and native access.
No new semantic type or IR variant requires an interface schema change.

Deferred cells are explicit: new buffer/view escape admission, variable extents,
more than 64 initialized bytes, general replacement scalarization, speculative
range hoisting/full-prefix loop splitting, new Top-K or byte-SIMD APIs, and
native typed Metal publication. The general guarded-byte-loop witness retains
its per-read check. ThinLTO-specific optimization and Apple Silicon/Metal timing
are not claimed by these local results; ordinary per-unit selection is tested.

Reference measurements and reproducible source are in
[`bench/decode_storage`](../../bench/decode_storage/README.md). On Linux x86-64
Ryzen 9 5950X with baseline CPU, LLVM 22.1.8 O2, runtime LTO disabled and the
same production release runtime archive for both revisions:

| Kernel | Baseline median | Candidate median | Native allocation/free events per call, baseline → candidate |
| --- | ---: | ---: | --- |
| 4-byte f32 bit conversion | 26.351 ns | 1.439 ns | 2/2 → 0/0 |
| 65-byte-capacity forced heap control | 26.355 ns | 25.626 ns | 2/2 → 2/2 |
| Fresh five-element source/chunk sum | 22.262 ns | 8.261 ns | 2/2 → 1/1 |
| 50,000 f32 byte-chunk max | 59.503 us | 14.784 us | 1/1 → 0/0; 800,000 header bytes removed |
| 50,000 typed f32 max control | 11.704 us | 11.644 us | 0/0 → 0/0 |

These are five-process medians, without allocation instrumentation in timing
binaries. A separate ten-process alternating CLI check of an unaffected typed
max export took 27.844 ms baseline versus 30.091 ms candidate with debug-built
compilers; this noisy startup-dominated check establishes no compiler-speed win. Allocation diagnostics use separate linker-wrapped malloc/calloc/
realloc/free binaries with a positive heap control. Output checksums agree at
0/1/39/40/41/50,000 elements. Timing was not CPU-isolated and is not a gate.
AArch64 Apple M1 backend inspection (`llc-22 -mtriple=arm64-apple-macosx
-mcpu=apple-m1`) lowers the optimized conversion to `fmov w0, s0; ret`, and
byte max to scalar `fcmp`/`fcsel` traversal without chunk materialization calls.
This cross-target instruction check is neither native Mac execution nor a
Metal synchronization/transfer benchmark. The implementer must measure complete
sampling with its original logits, model, seed and Metal completion boundary.


### Code-review finding closure

The independent committed-candidate review found one P2 producer-width gap:
a supported but inconsistent `BufferPut.scalar` could budget one byte while the
actual LLVM integer operand stored eight. The closure checks the constructor,
put, append, view and length operand/result types before selection, then checks
the actual generated integer/float class and width before any fixed-storage put.
Declared value types alone never authorize a store. No ABI or strategy changes.
`runway_a2_binary_codec::bounded_byte_object_rejects_forged_put_widths` owns
narrow/wide and integer/float mismatches, forged matching value-type tables with
real wider Load producers, and every selected operation's result shape. This
closes the complete reported class in one fix against the original review.
