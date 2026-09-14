# Idiomatic execution performance: issue 1043

Status: configuration, ThinLTO entry repair, local byte-call promotion and
restricted byte-range proof implemented, 2026-09-14. Build defaults are unchanged.
Follow-up comments, composed sampler proofs and the newly supplied native lifetime
evidence are tracked in [plan 64](64-composed-byte-optimization-plan.md).
Reference revision: `21d0cf27fb92166370b2705d5c366c2b269d17a3`.
Input: [issue 1043](https://github.com/sanohiro/align/issues/1043), the
align-llm request register, and the owner's request to investigate both missed
optimizations and optimizations that make execution worse. The primary target
is native Apple Silicon with Metal; Linux CPU/CUDA is a separate qualification.

## 1. Recommended order

1. Repair the reproduced ThinLTO entry-wrapper defect and make CPU/profile
   requests trustworthy. These are useful correctness and diagnostic boundaries
   before changing optimization policy.
2. Extend bounded byte storage through audited unrelated operations and proved
   synchronous calls. Do not treat the word `borrow` as a no-escape certificate.
   Imported/foreign consumers need the separate proof described below.
3. Remove the reproduced redundant byte-read check by preserving an actual
   range proof through existing MIR. Keep the existing single `loop` syntax.
4. Qualify the existing native CPU and ThinLTO modes on the real Mac workload.
   Recommend explicit local native builds; retain portable defaults in this
   implementation plan. Automatic local selection remains a policy proposal,
   with its decision conditions recorded in section 5.

The reported 47x sampler improvement is evidence for thresholded candidate
selection. It does not establish a 47x compiler optimization opportunity.
The issue does not include the complete measurement commands, client revisions,
raw profiles, or foreign retention/synchronization contract. Those limits do
not prevent provider investigation, but prevent claiming the actual decode
path is qualified or choosing a universal faster default.

## 2. Public-contract ledger

This is the authoritative boundary ledger for this proposal. Existing language
and library contracts remain in force until an implementation changes the
specification set. No new language syntax, public byte cast, SIMD intrinsic,
parameter mode, inline annotation, runtime Buffer ABI, or package API is selected.

| Surface | Inputs, defaults, errors | Ownership, allocation, owner | Identity, prerequisite, acceptance |
| --- | --- | --- | --- |
| Existing `buffer(capacity: i64) -> buffer`, scalar puts, `.bytes() -> slice<u8>` and ordinary function calls | Plan 62's capacity/initialized-extent limits remain. Existing source diagnostics and reached range failures remain. Unknown proof means ordinary allocation. | The buffer remains Move and the view Copy with its existing source lifetime. Proved objects avoid both runtime allocations; there is no source guarantee of universal stack placement. MIR owns the proof; LLVM only lowers it. | A body-derived plan is ephemeral and never trusted from serialized optimization metadata. Local selection needs no ABI/schema change. Cross-unit selection waits for an authenticated dependency proof; section 4 fixes its limits. Allocation and escaped-view negative controls own acceptance. |
| Existing `loop`, indexing, and endian byte reads | Preserve wrapping arithmetic, first reached failure, zero-iteration behavior, partial tails, evaluation order, and IEEE float behavior. | Borrowed storage and alignment-1 byte reads remain. No speculative load, new array, or copy. MIR owns range reasoning. | Recompute proofs from actual validated MIR. No new `RangeLoop` or unchecked-read IR variant is selected. Byte-range and loop owners plus malformed-MIR controls qualify the change. |
| `--target-cpu baseline\|native\|CPU` | Omitted remains baseline. Correction: explicit empty, missing, unknown, or wrong-architecture CPU is a diagnostic, before cache lookup, codegen, linking, or execution. CPU names come from the linked LLVM target, not a hard-coded M-series list. | Ordinary CLI string input; no native side effect on rejection. Driver parses; the shared codegen target resolver validates. | Use the existing resolved triple/CPU/features/relocation/code-model identity. Native must never key as the literal string `native`. CLI, direct resolver, cache, and machine-code owners cover both architectures. |
| `--profile dev\|release\|fast\|small\|tiny` on emission/inspection | Correction: optimized IR and LLVM remarks use the selected profile, default release, including pipeline, TargetMachine level and size attributes. Section 2.1 defines the raw stage and each verb's runtime-LTO scope. Existing incompatibilities still reject. | Inspection remains ephemeral and side-effect-free with respect to artifact publication/cache population. Driver and LLVM emitter own propagation. | No new profile or custom LLVM pipeline. The exact Profile-bearing signatures and parity conditions are in section 2.1; per-unit inspection does not represent a ThinLTO-linked executable. |
| Existing `build/run/size --thin-lto` at release/fast | Remains explicit. Existing rejection with PGO and unsupported verbs/profiles remains. Unit/Result main must link and have the ordinary exit/argv behavior. | One C entry wrapper belongs to the entry function partition. Its original argv/result cleanup remains; dependency partitions emit no duplicate wrapper. | Function partition identity must cover the wrapper's inputs and reachable support dependencies. Existing compiler/cache identity invalidates old artifacts on implementation change. Partition, entry-ABI, warm-cache and link owners qualify the repair. |

CPU option validation is lexical first, then profile parsing/compatibility,
then target resolution before artifact work. Duplicate well-formed selectors
retain the current last-value rule. A missing/empty selector is still invalid
when followed by another selector. A following option is not a CPU value.
CLI values must be nonempty valid text without embedded NUL; non-text, NUL,
unknown names and names for another target cannot reach LLVM's fatal path.
The reusable resolver must enforce the same rules for direct library callers.
No new ambient environment setting or persisted format is introduced.

The source-of-truth changes for a future implementation are proportional:
plan 62 and the allocation/backend owner for byte storage; MIR/backend docs for
range proofs; plan 09 and the English/Japanese toolchain guide for inspection;
plan 21's function-partition ledger for wrappers. A changed CPU/default or
public CLI contract also updates `draft.md`, `docs/language-spec.md`,
`docs/design-notes.md` and the applicable Settled record. This proposal does
not prematurely rewrite those current contracts.

### 2.1 Complete inspection configuration

One shared profile configuration supplies all three columns below to object
emission, raw/optimized IR and the LLVM remark lens. Do not thread only the
pipeline string while leaving TargetMachine level or function attributes fixed.

| Profile | Optimized pipeline | TargetMachine level | Definition attributes | `emit-llvm` default runtime LTO |
| --- | --- | --- | --- | --- |
| dev | `default<O0>` | None | no size attributes | off |
| release | `default<O2>` | Default | no size attributes | on |
| fast | `default<O3>` | Aggressive | no size attributes | on |
| small | `default<Os>` | Default | `optsize` | off |
| tiny | `default<Oz>` | Default | `optsize`, `minsize` | off |

Raw IR is the verified module after construction, any requested runtime-bitcode
merge/normalization and profile size-attribute application, before the optimizer
pipeline. It therefore exposes the selected size attributes even in raw mode.
Optimized IR runs the selected pipeline on that same module. Existing explicit
runtime-LTO on/off selection, profile rejection and diagnosed fallback remain.

`explain-opt` retains runtime LTO off at every profile and rejects both explicit
runtime-LTO switches, as it does today. It adds debug locations and uses the
table's pipeline, TargetMachine level and definition attributes; its existing
process-global remark-option initialization and context-handler cleanup remain.
MIR current-plan rows remain target/profile-independent. LLVM remarks must be
described as per-unit, selected-profile, runtime-LTO-off observations. Compare
them with an object built with the same profile/CPU and `--no-rt-lto`, not with
an ordinary release object containing runtime LTO or a linked ThinLTO program.
Inspecting dev/O0 need not yield a vectorization/inlining remark.

Exact Rust library signatures, replacing the previous signatures
outright. `Program`, `BuildTarget`, `Profile`, `DebugInfo` and `CodegenError`
retain their existing definitions; borrowed inputs are call-scoped, output
strings/vectors are ordinary owned Rust allocations, and no input is retained.

```text
align_codegen_llvm::emit_llvm_ir(
    program: &Program, target: &BuildTarget, profile: Profile,
    optimized: bool, exports: &[String], rt_lto: Option<&[u8]>
) -> Result<String, CodegenError>
align_codegen_llvm::collect_opt_remarks(
    program: &Program, target: &BuildTarget, profile: Profile, debug: &DebugInfo
) -> Result<Vec<String>, CodegenError>
align_driver::emit_llvm_ir(
    mir: &align_mir::Program, target: BuildTarget, profile: Profile,
    optimized: bool, exports: &[String], rt_lto: bool
) -> Result<String, String>
align_driver::collect_opt_remarks(
    mir: &align_mir::Program, target: BuildTarget, profile: Profile, debug: &DebugInfo
) -> Result<Vec<String>, String>
```

`build_profiles`, `emit_llvm_stage`, `explain_opt` and `rt_lto` own the
five-profile/raw-optimized/verb-scope product. Check settings and meaningful
generated behavior rather than assuming O2 and O3 must differ on every kernel.
The existing remark handler is detached on success and error. These API changes
introduce no interface/cache/wire schema or new diagnostic error type.

## 3. Findings and contrary evidence

| Item | Verified at the reference revision | Consequence |
| --- | --- | --- |
| I1: calls block byte promotion | `byte_storage::nonescaping` rejects a related argument at `Rvalue::Call`. A named borrowed-byte reader retains `buffer_new/put/bytes/free` calls even after O2 with runtime LTO off; the direct scalar reader becomes its input value. | Confirmed optimization gap; parameter-mode-only admission is insufficient. Four runtime calls are not four allocation events. Measure allocation and free events separately. |
| I1: `borrow` cannot return a view | False. `fn identity(borrow bytes: slice<u8>) -> slice<u8> = bytes` passes both checkers. The LLVM `captures(none)` on a borrowed descriptor address does not prove that the descriptor's data pointer cannot escape. | Track backing storage through return, mutable destinations, aggregate/callback capture and foreign uses. Existing return-borrow and mutable-retention records are relevant but not a complete foreign-effect certificate. |
| I1: issue snippet is exact source | Its temporary `.bytes()` argument is rejected for an explicit `borrow` parameter: a stable local/place is required. Binding `bytes := b.bytes()` makes the witness valid. | Obtain the actual signature and source before attributing the client allocation count to this exact call shape. By-value Copy slice parameters also need analysis; do not optimize only a spelling. |
| I2: ordinary loops cannot vectorize | False as a general claim. `ordinary_sum` below loses its per-index bounds failure and emits `<2 x i64>` loads/adds at baseline, `<4 x i64>` at native/x86-64-v3. | No `for` syntax or forced vectorization metadata is justified. LLVM can recognize the existing CFG. |
| I2: byte-offset proof is lost | Confirmed. A divisible-by-four guard, `i < src.len()/4`, and `src.u32_le(i*4)` still leave an inner range-failure edge at O2. Plan 62's f32 witness has the same gap. | Target this range/arithmetic relationship; do not move a changing Top-K threshold load outside its update loop. |
| I3: AArch64 baseline is generic | Confirmed in `resolve_cpu_features`; x86 baseline is x86-64-v2. Native already resolves host CPU and features, and named CPU alone enables LLVM's associated ISA. | Generic AArch64 still has baseline NEON. Native is CPU tuning, not a Metal/CUDA kernel compiler switch. The issue does not show an f16 or dot-product CPU operation that would benefit from the named extensions. |
| I4: default build is per-unit | Confirmed. Ordinary build/run use per-unit objects; explicit ThinLTO uses function partitions. In the two-unit i32-main control, the helper call exists without ThinLTO and disappears with it; both print 42. | Cross-module inlining already exists. Qualify its cost and correctness rather than introducing an inline annotation. Runtime LTO is a different mechanism. |
| A1: unrelated code disables byte promotion | Confirmed. Adding an unrelated `xs[0]` after the direct local byte read restores all four Buffer runtime calls. The unknown-operation arm rejects the entire candidate even when that operation does not use the buffer. | Audit operand/slot footprints. Keep unknown operations conservative, but do not require ordinary unrelated indexing to destroy a proved object's eligibility. |
| A2: malformed CPU requests | Trailing `--target-cpu` exits successfully with a warning and baseline output; `--target-cpu=` also succeeds. An x86 build with `apple-m1` or an invented name exits through LLVM's fatal unsupported-subtarget diagnostic. | Make invalid explicit choices fail early and consistently. This is a separate defect from choosing a portable default. |
| A3: inspection ignores profile | `emit-llvm --stage optimized --profile dev` and `--profile fast` produce byte-identical O2 IR with `--no-rt-lto`. `run_emit_llvm` drops the parsed profile; the LLVM emitter and remark path hard-code O2. | Inspection can mislead an O3/O0 investigation. Thread the shared profile contract and label the per-unit scope accurately. |
| A4: ThinLTO omits C main | Unit, Result Ok/Err and argv-main fail when a second function or source unit selects partitioned ThinLTO; the one-function shortcut succeeds. Explicit i32 main succeeds in all three shapes. Unit failure also reproduces with cache off/on and runtime LTO off/on. `ModuleScope::emits_main_wrapper` admits only `Whole`, although the partition preserve set includes C `main`. | Fix wrapper production before any ThinLTO-default proposal. The defect is broader than multi-file compilation; qualify every entry ABI and both partition modes. |
| A5: incomplete target context at prelink | The ThinLTO prelink shim constructs `PassBuilder PB` without a TargetMachine, while its backend and PGO use `PB(TM)`. Prelink's C ABI has no TM parameter. Emitted textual IR carries triple/layout but not CPU/features for independent replay. | A target-cost qualification candidate, not a demonstrated slowdown or generic final machine code. Compare prelink decisions with the selected TM before proposing a change. Replay exported IR with the matching CPU/features; triple alone cannot reproduce native tuning. |

The loop evidence agrees with LLVM's
[canonical-loop documentation](https://llvm.org/docs/LoopTerminology.html)
and [vectorizer documentation](https://llvm.org/docs/Vectorizers.html): source
syntax is not the proof boundary, and forcing a vectorization hint does not
remove data dependencies or grant floating-point reassociation.

## 4. Byte storage and range design

### 4.1 Audited unrelated operations and synchronous calls

Keep plan 62's 64-byte object and 1,024-byte per-function budgets, known
initialized extent and all-or-nothing object selection. Derive may-origins for
the buffer handle, every view, local carrier and projection. Separate an
operation's operand/place footprint from its transfer effects. An explicitly
audited unrelated scalar/index operation leaves the candidate unchanged; an
unknown variant, hidden place write or untracked carrier still rejects it.
An exhaustive operation classification must prevent a future MIR variant from
silently obtaining an empty footprint.

A synchronous call qualifies only if every use of the candidate's backing
storage is confined to the call and remains within the initialized view. It
must neither return/retain a derived view nor store it into another parameter,
aggregate, callback, task, resource or external state. Reject buffer-handle
arguments, raw-pointer publication, unqualified indirect/foreign calls and
unknown effects. Audit all call representations, including cleanup-bearing
calls; the absence of a `Rvalue::Call` is not absence of a call.

For available, structurally validated concrete MIR bodies, compute a finite
per-parameter may-escape summary to a fixed point over the reachable call
graph. Roots are logical argument ordinals, not LLVM physical argument slots.
Seed unrecognized/native/indirect effects as escaping; union all reachable
effects, including branch and loop joins. Cycles with no escaping operation
can converge to nonescaping; never convert analysis exhaustion into a proof.
The result is private, body-derived and ephemeral. Existing validation runs
before storage selection. This is an optimization certificate, not a new
borrow/access permission or a replacement for source lifetime validation.

The first useful capability may admit available local/concrete generic bodies
and unrelated operations together. Imported bodyless declarations remain a
documented fallback in that capability. It must not claim to optimize the
modular `runtime_inputs.update` example. Do not introduce a second serialized
effect system just for four-byte buffers: cross-unit admission must reuse
plan 61's authenticated interprocedural access-program transport once that
producer/consumer capability ships, with a separately checked escape projection.
Plan 61 is currently unimplemented; access qualification alone also does not
certify arbitrary foreign no-retention behavior.

The actual Metal/CUDA update wrapper remains a consumer-dependent boundary.
Its provider must establish whether input bytes are copied synchronously,
retained until command completion, or stored for later use. Existing audited
runtime operations can supply a compiler-owned transfer rule; an arbitrary C
declaration or `borrow` mode cannot. If that proof is unavailable, retain heap
storage. Neither a symbol-name whitelist for ggml nor an unsafe noescape
assertion is selected here. Any required new foreign-contract mechanism needs
its own complete public ledger before implementation. ThinLTO inlining occurs
after the MIR storage decision and cannot be assumed to repair this admission.

### 4.2 Byte-range proof through existing loops

Start with widths 1/2/4/8 and the already-supported endian reads. Derive a
private checked-access plan from validated MIR: source identity and stable
length, dominating branch facts, index recurrence, byte width, and absence of
overflow. For `0 <= i < floor(len/w)`, `len >= 0`, and positive literal `w`,
prove `i*w <= len-w` without relying on wrapped multiplication. The selected
edge permits the existing read to omit its redundant guard. Otherwise keep
the original checked operation.

Today `lower_bytes_read` already emits the range-check CFG before an unguarded
`BytesRead`. Therefore this is a MIR CFG simplification of the proved branch,
not a new unchecked load flag in LLVM. Validate the original MIR first; after
rewriting, validate the resulting CFG/value/ownership structure again. Preserve
the original failure block for every edge not independently proved safe.

No semantic operation belongs in a new LLVM-only pass. The MIR proof owner
must be common to object, IR, PGO and ThinLTO emission, recomputed for the
actual body rather than deserialized as trusted metadata. Keep actual source
and LLVM operand-type checks at the load boundary. Do not add global `nuw`,
`nsw`, `inbounds`, `assume`, alignment or no-alias facts from source syntax.

Only remove a guard whose failure is impossible on its original reached edge.
Do not hoist a trap to loop entry, pre-read a partial chunk, run a suppressed
predicate, or change any/all's full-fold behavior. Kill facts on source
replacement, changed index/length, mutable aliases and opaque effects. Join
with intersection of proved facts. Unknown or wrapped arithmetic falls back.
The scalar byte-sum and f32 witnesses must lose the redundant edge; existing
ordinary-sum and fixed-threshold controls must remain optimized.

Additional negative/positive controls narrow the missing fact: explicitly
checking `src.len() >= 0`, or expressing the loop limit with unsigned casts,
enables a vector main loop for the integer byte-sum witness. Its scalar remainder
still has a range-failure edge. This is not complete bounds-check elimination.
Use the valid-view length invariant in the proof owner rather than requiring
callers to write these extra conditions. Do not generalize the observation to
arbitrary integer expressions or assume the LLVM backend lacks vectorization.

Top-K remains plan 62's indexed/oracle experiment. Threshold pruning changes
algorithmic work; vectorization of a stateful insertion loop is not its
semantic replacement. Preserve tie/NaN/signed-zero policy and sampling/RNG
sequence when comparing the real client.

## 5. CPU, optimizer and reverse-regression policy

For native source builds on a Mac, the current explicit recipe to qualify is:

```text
alignc build app.align --profile release --target-cpu native
```

Measure `fast` and `--thin-lto` as separate factors after the wrapper repair.
Do not bundle three changes and attribute the result to one. A named
`--target-cpu apple-m1` is a candidate floor for a known Apple fleet when
building on the corresponding target; it is not an architectural cross-compile
selector on an x86 machine. Portable Linux arm64 and portable x86 retain their
own baselines. The native settings of precompiled runtime/native libraries
remain independent of the generated Align code's CPU selection.

The owner's concern about generic local ARM execution is recorded. An automatic
native `run` default would change the Settled portability policy; an automatic
native `build` would additionally affect distributed artifacts. No new
`apple-silicon` alias, hidden host-detected baseline or ambient profile is
selected. Before proposing that policy change, record native Mac results and
the intended distribution boundary, account for identical `build/run/test`
resolution and target cache keys, and use plan 23's reopen protocol where the
locked decision applies. The existing native flag supplies the experiment now.

Other audited axes and reverse controls:

| Axis | Finding / selected response |
| --- | --- |
| x86 native vs portable | Native/v3 enables wider integer vectors in the witness; portable v2 deliberately does not. The empty feature string for a named CPU is correct: LLVM derives that CPU's features. Do not "fix" it by enabling x86 features on ARM or vice versa. |
| ARM machine-code coverage | `target_cpu_isa.rs` currently gates its instruction checks to x86. Add native Apple/AArch64 instruction-selection owners for baseline and a supported named CPU, with portable fallback execution. An LLVM IR vector width is not sufficient machine-code evidence. |
| O3 vs O2 | More inlining/unrolling may cost code size and instruction-cache capacity. In the local decode probe the rt-off O2/O3 object hashes are identical for each CPU choice; no O3 speedup is established. Keep IEEE semantics, including reduction order where required. |
| Runtime LTO | Already default-on at release/fast. It exposes selected runtime bodies, not every Rust or external GPU-library function. The decode probe gains no consistent improvement; retain its established string-kernel controls and check fallback diagnostics. |
| ThinLTO target propagation | Audit the target-free prelink builder against a builder supplied with the same resolved TM as backend emission. Measure actual changed prelink decisions, final instructions, cold/edit build costs and execution on ARM and x86. If no relevant difference exists, retain the current path. Any implementation threads one call-scoped TM through all prelink callers and the Rust/C++ ABI together; a source CPU name alone is not a native-hardware proof. |
| ThinLTO and iteration cost | Cross-unit import can remove calls but adds prelink/thin-link/backend work. Plan 21 records that the ThinLTO path does not reuse the ordinary packaged frontend/codegen cache. Measure cold, warm, private-edit and public-edit builds before changing defaults. PGO and ThinLTO are currently mutually exclusive. |
| SIMD can lose on ARM | The existing native M1 UTF-8 audit rejected a NEON candidate after regressions on ASCII and early/late-invalid inputs. Current `validate_utf8` still uses the scalar reference on AArch64; x86 dispatches AVX2 above its measured threshold. This is a deliberate measured choice, not another missed generic-CPU switch. |
| Parallel work can lose | The runtime has work/size thresholds and a serial path. Do not turn the 152,064-value sampler into parallel work merely because its vocabulary is large; measure scheduling, per-token latency and concurrent-request throughput independently. |
| Views/copies/FFI | Stored chunks still materialize; direct synchronous chunks do not. Typed native views still require backing lifetime, alignment and completion. Removing an explicit staging copy is not valid if an asynchronous GPU operation retains its pointer. Consumer/backend work remains external. |

The historical ARM UTF-8 result is in
[allocation audit section 6.2](13-string-array-allocation-short-input-audit.md#62-measured-2026-07-16--x86-64-crossover-shipped-apple-m1-neon-rejected).
It is prior native evidence, not a new measurement in this investigation.

## 6. Implementation closure matrix

Each row is planned acceptance, not a passed implementation test. Reuse the
named owner where possible. The local/available-body storage capability, range
proof, CLI/inspection correction and ThinLTO wrapper repair are distinct useful
failure domains; avoid dormant producer-only PRs. A cross-unit escape expansion
cannot ship its producer, transport and consumer independently. If that later
boundary exceeds 1,000 handwritten lines, record its actual proof-sharing
reason in the owning ledger before coding.

| Axis | Storage / range closure | CLI / partition closure | Owner |
| --- | --- | --- | --- |
| Formation and malformed input | Validate source types, MIR values/places and initialized extents before deriving proofs; forged result/operand widths never authorize fixed writes or unchecked loads. | Reject invalid CPU input before LLVM; profile reaches every supported lens; malformed partition/ABI records cannot emit a wrapper. | `runway_a2_binary_codec`, MIR/codegen malformed owners; `build_target`, `build_profiles`, `function_thin_lto` |
| Full profile / observation scope | Range/storage semantics remain the same across all five profiles; resource selection is not justified by a diagnostic-only pipeline. | Section 2.1's raw stage, optimizer, TM level, size attrs and runtime-LTO scope agree with object controls. Remark handler cleanup and per-unit/MIR-plan scope remain. Target-free prelink is a measurement candidate only. | `build_profiles`, `emit_llvm_stage`, `explain_opt`, `rt_lto`; existing ThinLTO/PGO owners if prelink changes |
| Construction, replacement, move-in/out, source nulling, Drop, return | Fresh loop instances and local calls preserve storage lifetime. Returning/retaining views, handle replacement and unknown uses fall back. No runtime free of stack memory; ordinary buffer frees exactly once. | C wrapper emitted once for the actual entry; i32, Unit, Result Ok/Err, argv allocation and cleanup preserve ordinary behavior. | `runway_a2_binary_codec`, `borrow_liveness`, `borrowed_replacement`; `main_abi`, `unit_main_exit_code`, `function_thin_lto` |
| `if`, `match`, `else`, `?`, `map_err`, joins, break, early exits | Escape is a may-union; initialized extent/range proof is a must-intersection. Unknown recursive effects and source invalidation cannot disappear at joins. Trap timing is unchanged. | Wrapper Result exits and early returns preserve exit status and cleanup; unsupported option combinations fail deterministically. | `loop_expr`, `borrowed_params`, `owned_borrowed_composition`, binary-codec owners; profile/entry owners |
| Calls and native boundary | Include direct, indirect, cleanup-bearing, mutable-output and captured calls. Prove backing storage, not descriptor address. Unknown foreign retention blocks promotion. Existing read-only/validated-text checks remain separate. | Wrapper extern declarations and support ABI exist in its partition; dependency units never create an entry wrapper. | `consumer_borrow_boundaries`, `borrowed_params`, codegen validation; `function_thin_lto`, `thin_lto` |
| Generic and whole/per-unit transport | Available concrete bodies are reanalysed. Bodyless imports fall back until authenticated access transport and escape projection are implemented together. No native noescape promise is inferred from plan 61. | Whole-unit shortcut and actual partitioned single/multi-unit graphs agree; entry body, preserve set and wrapper dependencies agree. | concrete-generic/interface owners; `function_thin_lto`, `thin_lto_sv` |
| Cache / artifact identity | No trusted cached storage/range plan. Future interprocedural proof dependencies must invalidate consumers on relevant private-body effects, not only public signature changes. | CPU/profile/runtime-LTO combinations remain distinct; wrapper changes and private/public edits invalidate appropriate partitions; cold/warm/revert/parallel paths agree. | `cache_codegen`, `unit_cache`, `thin_lto_cache`, `function_thin_lto`, `target_cpu_isa` |
| Resource/performance claim | Selected local object: zero allocation/free events; escaping/65-byte controls stay allocated. Proved byte read: no per-read range-failure edge. No universal latency/SIMD/Top-K promise. | Correct entry is mandatory; inspect actual target instructions and compare build/runtime costs separately. | existing decode-storage harness plus native Mac qualification; allocation counters are separate from timing |

Plan 61's unimplemented access repair, arbitrary foreign-retention contracts,
new iteration syntax, general Top-K recognition, and a default-CPU/ThinLTO
flip are explicitly outside the first implementation boundaries.

## 7. Reproduction and measurement record

The source probes below pass both `check` and `check-per-unit` after binding
the borrowed argument to a local. Export the named kernels when inspecting a
standalone object; otherwise dead-code elimination removes unused entry-file
functions. These are investigation witnesses, not new library APIs.

```align
fn read_word(borrow bytes: slice<u8>) -> u32 = bytes.u32_le(0)
pub fn local_bytes(x: u32) -> u32 {
  mut b := buffer(4)
  b.put_u32_le(x)
  return b.bytes().u32_le(0)
}
pub fn call_bytes(x: u32) -> u32 {
  mut b := buffer(4)
  b.put_u32_le(x)
  bytes := b.bytes()
  return read_word(bytes)
}
pub fn borrow_identity(borrow bytes: slice<u8>) -> slice<u8> = bytes
pub fn unrelated_index(x: u32, xs: slice<u32>) -> u32 {
  mut b := buffer(4)
  b.put_u32_le(x)
  return b.bytes().u32_le(0) + xs[0]
}
pub fn ordinary_sum(src: slice<i64>) -> i64 {
  mut sum := 0
  mut i := 0
  loop {
    if i >= src.len() { break }
    sum = sum + src[i]
    i = i + 1
  }
  return sum
}
pub fn byte_sum(src: slice<u8>) -> u32 {
  if src.len() % 4 != 0 { return 0 }
  mut sum: u32 := 0
  mut i := 0
  loop {
    if i >= src.len() / 4 { break }
    sum = sum + src.u32_le(i * 4)
    i = i + 1
  }
  return sum
}
```

For the ThinLTO witness, put this declaration in `helpers.align`:

```align
module helpers
pub fn add1(x: i64) -> i64 = x + 1
```

Put the caller in `app.align`; compare ordinary build with `--thin-lto`:

```align
import helpers
fn main() { print(helpers.add1(41)) }
```

The positive comparison uses `fn main() -> i32`, prints the same value, and
returns zero. It builds in both modes; disassembly has one helper call in the
ordinary case and zero with ThinLTO. All failures and outputs were preserved
under the local Git directory's `issue-1043-audit/`, including sources, raw/O2
IR, CPU-option diagnostics, assembly, and measurement JSON.

### Local option sweep

Used the existing `bench/decode_storage` harness and one release runtime on
Linux x86-64 / Ryzen 9 5950X / LLVM 22.1.8. Eight configurations cross
baseline/native, release/fast and runtime LTO off/on. Five fresh processes per
point run in deterministically shuffled configuration order; no allocator
instrumentation or concurrent compiler build runs during timing. CPU frequency
and affinity were not pinned. Checksums agree across all configurations.

Selected median times, runtime LTO off:

| CPU/profile | Byte max, 50,000 f32 | Typed max, 50,000 f32 | Object size |
| --- | ---: | ---: | ---: |
| baseline/release | 12.636 us | 10.594 us | 1,944 B |
| baseline/fast | 12.638 us | 10.613 us | 1,944 B |
| native/release | 10.804 us | 10.663 us | 2,160 B |
| native/fast | 10.767 us | 10.601 us | 2,160 B |

Native reduces this byte-kernel time by about 15% and increases this object by
216 bytes. The typed control does not establish a benefit. O2/O3 object bytes
are identical within each CPU choice. Runtime LTO adds 120 object bytes here
and gives no consistent timing gain; small timing differences are not a proved
regression. This experiment does not measure Metal, CUDA, native Mac execution,
the client's Top-K code or end-to-end decode. One-shot compilation times in
the raw record are not a compile-performance comparison.

### Native acceptance before policy changes

Capture compiler/client SHAs, complete commands, CPU/OS/LLVM and runtime/native
library identities. Compare fixed logits/model/vocabulary/k/seed and unchanged
sampler/RNG policy. Measure CPU update, GPU completion/readback, selection and
normalization, publication, end-to-end token latency, and concurrent throughput
separately. Count allocation and free events independently, excluding benchmark
setup and including a forced-heap positive control.

Sweep empty/tiny/boundary/real sizes, partial tails, ordered/random/equal and
nonfinite scores, and cold/warm/private-edit/public-edit builds. Report per-case
medians and spread, code size and memory; do not hide a negative case in a mean.
Native Apple results decide Mac policy. Linux/CUDA results decide that target's
policy. Syntax checks, cross-target assembly, and CI compilation alone cannot
establish either end-to-end performance claim.

## 8. Design verification

The author ledger-to-prose and matrix-to-source pass completed. One fresh
independent inspection reviewed the complete design and relevant implementation.
It found one P2 contract omission: profile-aware inspection must include the
TargetMachine level, size attributes, raw stage and runtime-LTO scope, not only
the optimizer pipeline. Section 2.1 and the matching matrix row close that
finding together; there was no P1 finding or requested safety redesign.
The review is design evidence, not implementation acceptance.

Both checkers accept the complete documented Align witnesses. Provider-local
probes established the findings above; documentation links and whitespace were
checked. The configuration capability implements section 2.1 and entry repair;
client source and build defaults are unchanged.
Native Mac measurement and the actual foreign update contract remain pending.

## 9. Configuration capability closure

The first capability repairs entry emission and configuration observation as one
independently useful compiler boundary. Byte storage and range reasoning form
the second capability; foreign retention and native policy qualification stay
deferred under sections 4 and 5.

| Applicable cell | Implementation | Regression owner |
| --- | --- | --- |
| Entry construction, return, cleanup, partition and warm cache | `ModuleScope::emits_main_wrapper`; existing entry ABI lowering | `function_thin_lto::partitioned_entry_abis_match_the_whole_unit_shortcut`, `main_abi` |
| CPU lexical precedence and repeated options | `parse_target`, original-prefix validation in `main` | `cpu_selection_rejects_missing_values_before_flag_stripping` |
| CPU validation before target/cache construction; NUL and wrong architecture | `resolve_cpu_features`, compiler-private `align_target_cpu_valid` | `build_target::explicit_cpu_names_are_validated_before_emission_or_cache_identity` |
| Five profiles, raw/optimized attributes, optimizer and remark scope | shared `build_program_module`, Profile-bearing emission and remark APIs | `build_profiles::inspection_uses_complete_profile_configuration`, `emit_llvm_stage`, `explain_opt`, `rt_lto` |

The private CPU query borrows two NUL-terminated strings synchronously and retains
neither. Null/empty input returns 0, unavailable target metadata returns -1, and
a valid CPU returns 1. C++ owns and destroys temporary target metadata. Rust
rejects embedded NUL and every result other than 1 before constructing a target
machine. There is no runtime ABI change.

The LLVM observation preamble is exactly
`LLVM observation: per-unit; profile=PROFILE; runtime-lto=off`, after any existing
override banner and before current-plan rows. Those MIR rows keep their existing
profile-independent meaning.

The inspection signature migration reaches many existing owners mechanically.
Keeping those callers, the shared profile producer and the inspection consumers
in one capability avoids a temporarily inconsistent public API. Together with
the investigation ledger the diff may exceed 1,000 lines; splitting this boundary
would duplicate configuration proof without producing a useful intermediate API.

## 10. Byte-execution capability closure

`CallEscapeSummary` is a private-field, ephemeral compiler analysis over concrete
MIR bodies covered by the current artifact identity. Logical parameter ordinals
identify byte roots; the finite greatest fixed point removes confinement facts
when any body path returns, retains or forwards bytes to an unproved consumer.
Cycles converge without Rust recursion. Buffer-free artifacts skip this analysis.
Whole/unit objects may use their local concrete bodies, including instantiated
generics. Function partitions only use bodies they define; a peer's private body
is not in the caller's cache key. Imported/foreign effects remain deferred.

The range pass runs in the shared checked-HIR-to-MIR route. Its first admission
is deliberately restricted to scalar/read-only bodies with stable incoming byte
views and no mutable parameter aliases or opaque effects. It verifies unique
SSA definitions, participating operand types and dominance, one zero initializer
and one increment by one, and the original range-failure condition/read identity.
The step returns to the same admission header. The admitting branch arm must
dominate both the read guard and increment: removing exactly that arm must make
each unreachable from entry. Successor-node dominance is insufficient; equal
successors and rejected-arm rejoins preserve the original trap. Both `i >= len/w` exit guards and
`i < len/w` admission guards qualify, for widths 1/2/4/8. Valid view lengths are
nonnegative, so the admitted index and increment cannot overflow and
`i*w + w <= len`. The byte read keeps alignment 1.

Only the proved branch becomes `Goto`; statements, value/block IDs, source
coordinates and the original trap block remain. Structural admission is checked
again after rewriting, with rollback on rejection. No LLVM assumption, new IR
variant, speculative read or trap hoisting is introduced. Source replacement,
extra index writes, unproved aliases, inclusive bounds, different widths or
unknown operations preserve the original branch. Other valid recurrences and
source shapes remain optimization opportunities, not unchecked fallbacks.

| Matrix cell | Implementation and owner |
| --- | --- |
| Local/forwarded/recursive and concrete generic readers; logical argument ordinal | `CallEscapeSummary`, `argument_confined`; `byte_storage_crosses_proved_local_readers_and_unrelated_indexing` |
| Return/retention, opaque import and aggregate/cleanup/indirect unknown effects | Explicit admitted-operation match and conservative remainder; returned-view control and `imported_byte_readers_remain_opaque_to_per_unit_storage_selection`; existing ownership/producer rejection owners remain |
| Construction, initialized extent, scalar widths, replacement, fresh loop lifetime and Drop | Existing `object_plan` transfer and budgets unchanged; `bounded_byte_object_control_matrix`, `bounded_byte_object_rejects_forged_put_widths` |
| Range guard identity, SSA/type/CFG validity, initialization, wrap and alias invalidation | `byte_ranges::Facts`; `byte_range_malformed_and_invalidated_proofs_fail_closed` |
| Admission provenance for true/false arms, equal successors and rejected-arm rejoins | `Facts::arm_dominates` at both read and increment; parameterized Ge/Lt positive and bypass controls in `byte_range_malformed_and_invalidated_proofs_fail_closed` |
| Integer/float width, endian, zero/short/partial/unaligned inputs and reached trap | `byte_range_recurrence_preserves_tails_and_eliminates_only_proved_guards`; existing binary-codec trap owners |
| Whole/per-unit, profile and target execution | Shared lowering route and scope-filtered body summaries; `scripts/test-codegen-performance.sh` runs the same owner set locally and on Linux x86/ARM and native macOS CI |
| ARM baseline/named CPU instruction selection and x86 reverse direction | `target_cpu_isa`: generic/Apple NEON plus existing v2-without-AVX2 and v3/skylake-with-AVX2 controls; native execution qualification remains distinct |
| Allocation and generated-code performance | `bench/decode_storage` modes 5/6/7, with modes 0/1/2/3/8 as controls; section 11 records measurements and limitations |

The capability stays together because both changes consume the same byte-view
contract and the same binary-codec execution owner, and the native verification
script closes that consumer across architectures. The roughly 1,000-line diff
includes the restricted proof, its malformed-input matrix and shared CI/local
validation. Splitting those from their accepting consumers would duplicate the
byte-lifetime/range and verification proof without reducing a public ABI boundary.

## 11. Local byte-execution measurements

Linux x86-64 / Ryzen 9 5950X / LLVM 22.1.8, explicit baseline/release with runtime
LTO off, reference `0e236129`. Both objects link the same production release
runtime (SHA-256
`185897ec6cd5ca0be326a47f9286c40e21e6b0d02f4507da247e08ae35b51739`).
Allocation wrappers run in separate diagnostic executables; three local
reader conversions change from six allocations/six frees to zero/zero, while
the 65-byte control remains six/six. All checksums agree.

CPU-0-pinned timing uses five fresh processes per point in shuffled order. Sizes
0/1/39/40/41/50,000 are swept for scan kernels. Medians for the whole-module object:

| Kernel | Before | After |
| --- | ---: | ---: |
| Local reader conversion | 23.584 ns | 1.274 ns |
| Direct conversion control | 1.329 ns | 1.306 ns |
| 65-byte heap control | 23.647 ns | 23.476 ns |
| Explicit f32 byte max, 50,000 values | 10.863 us | 10.717 us |
| Divisibility-guarded byte-word sum, 50,000 values | 10.883 us | 1.778 us |
| Typed-word sum control, 50,000 values | 1.805 us | 1.925 us |

The typed control's adverse movement was investigated rather than discarded.
Its instruction sequence is unchanged, but its loop moves from a 32-byte to
a 16-byte boundary in the larger object. A second experiment emits unchanged
controls into a separate object and links it first; the before/after control
objects have identical SHA-256
`34ed17311ff92124e5758bfaab38617ae7bc15adac2f87a532ef86222a23e564`.
Seven pinned processes then measure typed-word sum at 2.750/2.691 us and typed
f32 max at 10.733/10.733 us. This supports a layout/noise qualification rather
than a changed typed-sum lowering; it does not promise every final link layout
improves. In that arrangement local conversion remains 23.482/1.294 ns and byte
word sum 10.889/1.719 us. The f32 loop varies much more with placement (21.530/
10.627 us), so no single f32 speedup ratio is claimed.

The optimized integer loop has alignment-1 vector loads and no reached byte-range
failure. Cross-target LLVM lowering emits NEON `ldp q`, `add.4s` and `addv.4s`
for both generic AArch64 and Apple M1. This is machine-code inspection, not native
Mac/Metal timing. Native workload, GPU synchronization and foreign-retention
qualification from sections 4/5 remain open. Raw commands, checksums, timings,
assembly and runtime identity are retained in the local issue-1043 audit record.
