# Caller result placement for indirectly returned aggregates

Status: implemented in the shared LLVM emission path. Reproduction baseline:
`241035ba97b2a679ce6589f2df66630cdcc2c691`, locally LLVM 22.1.8. Issue:
[1047](https://github.com/sanohiro/align/issues/1047), specifically its
[Apple caller evidence](https://github.com/sanohiro/align/issues/1047#issuecomment-5675111986).
This document owns the caller-copy follow-up to plan 65 section 7. Foreign
query effects, imported-body exposure and source type-system requests remain
in plan 65. The pending plan 61/65/66 storage capability is independent.

## 1. Evidence and selected improvement

The new report distinguishes a constructor writing through the ABI result
pointer from a caller copying that result into another stack region. The
previous constructor-only x86 probe did not test the second operation.

A provider-owned two-module witness now reproduces this class on current main:
`Big` contains an i64 and thirteen str fields (216 bytes); two calls construct
two simultaneously live records, then two borrowed calls consume them. The
source checks and emits ordinary release/O2 LLVM IR with runtime LTO disabled.
The entry unit retains both imported calls, aggregate results and destination
stores. Its x86-64-v2 assembly has two 216-byte return-buffer-to-local copies.
Retargeting that entry IR with LLVM 22 llc to apple-m1 gives the same copies.
Retargeting removes the original target layout/triple/CPU/feature attributes;
it is a compiler experiment, not a native Apple execution or client benchmark.

| Provider experiment | Result |
| --- | --- |
| Current aggregate-valued calls | Two logical records occupy four 216-byte regions: 864 bytes of aggregate storage. Both x86 and retargeted ARM copy 432 bytes after calls. |
| Explicit result pointers targeting the final slots | Two 216-byte regions; no return-to-local copy in either target's inspected assembly. |
| Explicit result pointers, temporary slots, aggregate load/store, ordinary O2 | Copies remain. Merely adding sret is insufficient. |
| Explicit result pointers, temporary slots, full-size memcpy, ordinary O2 | LLVM forwards the call destinations; only the two final slots remain and both copies disappear in retargeted ARM assembly. |

A concrete x86 ABI cross-link probe also passes in both directions: an implicit
caller with an explicit-result constructor, and an explicit-result caller with
the original constructor, both return the expected 47. This covers only the
216-byte witness and does not discharge the type/target/call-edge gate below.

Stack spills and alignment differ between forms. The ARM baseline reserves
864 bytes in addition to 48 bytes of saved registers; the explicit-result
comparison reserves 464 bytes including 32 bytes of saved registers. These
are measurements of this witness, not a promised client frame size or latency.
Local sources, complete IR, assembly and commands are retained under
`.git/issue-1047-caller-design/`. Implementation must promote the minimal
witness into its owning regression test; a local packet is not permanent CI
coverage. The exact original Mac binary's revisions, full flags and IR remain
requested on the issue, but the provider reproduction permits work to start.

**Selected design:** expose already-indirect result storage to LLVM before
optimization, and preserve a full aggregate transfer as memcpy when its sole
consumer is an adjacent materializing store. Let LLVM's existing alias-aware
call-slot optimization decide whether the final slot may replace the temporary.
A compiler-created result slot remains the correct fallback. No source syntax,
public destination argument, foreign purity annotation or size heuristic is
introduced. Do not implement a 216-byte, NodeTable, or Apple-only special case.

[Plan 71](71-aggregate-layout-transport-drop-state-plan.md) extends this
implemented result-only capability after tagged layout and drop-state effects
stabilize. Its transport contract splits cleanup-bearing results, classifies
program-owned parameters with the same target authority, and permits explicit
placement only for fresh whole locals and caller result slots. Existing-value
replacement, record fields, indexed elements, joins and observable aliases
retain this plan's temporary fallback so RHS, bounds-error and Drop order do
not change.

## 2. Contract and ownership ledger

These are compiler-internal records, not new language or runtime APIs. The
semantic MIR signature, value semantics and existing cleanup result stay fixed.

| Surface | Exact proposed contract |
| --- | --- |
| Return classification | `Direct` or `Indirect { result_type, size_bytes: u64, align_bytes: u64, address_space: u32 }`, derived from the complete existing physical LLVM result type, calling convention, return attributes and selected TargetMachine. Types are invocation-local LLVM handles. Size is fixed and checked; alignment is a nonzero power of two. |
| Classifier owner | LLVM 22 `GetReturnInfo` plus the selected target's `CanLowerReturn`, matching LLVM's own implicit demotion. Use the same full calling context at definitions, declarations and uses; do not approximate this with `sizeof(T) > 16` or the warning threshold. A target/context disagreement is a codegen refusal before object/bitcode publication. |
| Explicit indirect signature | LLVM return becomes void; pointer parameter index 0 has `sret(result_type)` and the exact alignment/address space; existing parameters shift by one with modes and parameter attributes preserved. The physical result includes any existing cleanup payload. Direct signatures remain exact. |
| Result-pointer effects | The compiler-owned parameter is only the destination of the completed physical return. It cannot be read or published by source code. `captures(none)` is justified only for this generated parameter. Do not infer noalias, readonly, willreturn or nounwind for ordinary arguments or foreign calls. |
| Call result storage | One correctly sized/aligned entry-block stack slot per static indirect-return call site, using an insertion builder at the entry block, alongside the existing `alloca_at_entry` invariant. Reuse it across loop iterations only after reconstructing or transferring the prior result; simultaneously live materializations have independent slots. No dynamic alloca may execute inside the loop. No heap allocation or new runtime export. A load after normal call completion reconstructs the original SSA result for general consumers. Terminating calls publish no result. |
| Materializing store | An immediately following whole-result, nonvolatile, nonatomic store that is the call result's only use may become a memcpy from the fresh result slot. Copy the exact LLVM allocation size with proven source/destination alignment; neither buffer may be smaller. There is no intervening instruction with effects and no lifetime start may be crossed. Other uses retain the SSA load. |
| Coalescing owner | Ordinary LLVM MemCpyOpt proves forwarding from the fresh slot. Align does not redirect an existing variable's address based on name, unique SSA use, Borrow mode, or coarse storage roots. If the proof fails, the original slot and transfer remain. |
| Callee completion | Evaluate the original return operand and execute the existing cleanup schedule before the store at the original return instruction; store the complete physical result and return void. No early field publication or reordered Drop is introduced. |
| Host allocation/errors | Classification and rewrite tables are compiler-host allocations scoped to emission. No HIR pointer escapes to an artifact. Invalid types, foreign-module handles, unsupported storage, unsupported ABI attributes or inconsistent signatures return the existing codegen error path before mutation/publication; no assertion on malformed input. |
| Artifact identity | Reuse the existing complete codegen key: compiler binary hash, dynamically loaded LLVM identity, target triple/object format, resolved CPU/features, profile/LTO mode and structural codegen-input identity. No new independently mutable persisted certificate or interface record. In-process reuse must be scoped to that same emission context. |
| Prerequisite | Existing checked MIR, native callable qualification and current target selection. No plan 61 source-access fact authorizes this transformation. No imported implementation body is required to lower an authenticated signature. |
| Public agreement | Source types, ownership, allocation/error behavior, purity, diagnostics and native externally observable ABI stay unchanged. `draft.md`, the language digest and mirrors therefore need no new promise. The owning implementation prose is this plan, plan 65 and the backend strategy document if implementation lands. |

The native ABI equivalence is a required proof, not an assumption from the word
sret. On every supported target, compile implicit and explicit definitions and
callers independently and link them in both directions. Include direct-register
reverse controls, mixed floats/integers, nested records, fixed arrays, vectors,
unit/zero-sized shapes and over-aligned admitted types. A return not demoted by
LLVM stays Direct even when its allocation size is large. If LLVM's classifier
cannot be reused faithfully, stop this strategy and reopen this ledger; do not
substitute a hand-maintained target size table.

Existing foreign/runtime ABI owners remain authoritative. Only compiler-owned
callable signatures are candidates, identified by the qualified callable
catalog rather than symbol spelling. A function pointer whose ABI owner is a
native contract retains that contract, including the implicit result slot's
function lifetime and the absence of a capture assertion. Checked descriptor
edges may expose the same machine ABI as sret but are not coalescing candidates. Compiler-generated native bridges and
callbacks require both their generator and their actual caller ABI to agree;
if that proof is unavailable they are explicitly outside normalization. Native
calls never gain the source-owned materialization proof. A generic signature cannot
silently choose a different LLVM function type at an indirect use.

## 3. Lowering and integration design

1. Finish and verify the ordinary LLVM module, then build one immutable
   return-transport table before creating rewritten declarations. Derive it
   from complete physical signatures, including cleanup-bearing results and
   closure environments. Plan the entire declaration/call/return rewrite before
   mutation. The matched LLVM 22 C++ bridge owns temporary MachineFunctions and
   caches classification by function type, calling convention and complete
   effective attributes for this invocation only.
2. Rewrite declarations, definition arguments, returns, direct calls, function
   values and lifted closures together. Rust's original LLVM function/argument
   handles are no longer consumed after this normalization. The qualified
   program catalog supplies imported owners; compiler-generated definitions
   are known before runtime bitcode linking. A private call-site marker selects
   checked function-value and task/closure edges and is erased before emission.
   Checked raw descriptor calls have a distinct native marker: their physical
   ABI is normalized too, so devirtualization to a generated definition sees
   one LLVM signature. They acquire no capture assertion, no shortened result
   lifetime and no materialization rewrite. Runtime/extern declarations and
   ordinary foreign calls remain unselected. Unknown native pointers cannot
   become source-owned calls merely by sharing a result type.
   Parameter-index changes preserve attributes and debug locations.
   No caller-only sret bitcast or adapter around an incompatible signature is
   legal. Tail/musttail attributes must be revalidated; never retain a tail
   promise for a pointer into the current stack frame.
3. Preserve the original MIR call result/store relationship while mechanically
   lowering it. The adjacent single-use materialization case emits the fresh
   result slot and memcpy described above. For a field destination, a join,
   replacement, multiple uses or an uncertain size, retain the general result
   load and original stores. This is an optimization eligibility rule, not a
   restriction on accepted programs. Existing by-value isolation stays intact.
4. The original optimizer order remains `default<O*>`. The paired memcpy
   experiment succeeds without a custom pass pipeline. Verify LLVM both after
   lowering and after any runtime module merge as today. O0 keeps correct
   temporary storage; O2/O3 must eliminate the simple witness copies.
5. Ordinary object emission, raw/optimized IR, optimization remarks, PGO,
   runtime LTO, ThinLTO prelink/backend, whole-program and per-function
   partitions must all consume the same classification. A prelink phase
   without a TargetMachine cannot guess the result transport: supply its
   existing selected target context to this mechanical lowering first.

The representation change alone is not a mergeable producer capability. Ship
classification, every applicable call/return consumer, the materialization
consumer and owner coverage together. This may exceed 1,000 hand-written lines:
one capability avoids incompatible LLVM signatures between independently
landing producers/consumers and duplicates less ABI proof. Split only a truly
independent detector/tooling change, not a dormant return convention.

## 4. Implementation closure matrix

Each row names the acceptance owner. The closure map below connects these
obligations to the implementation; native client measurements remain external.

| Axis | Required invariant and owning checks |
| --- | --- |
| Formation and validation | Complete physical type graph, alignment/size/overflow, selected target and call attributes agree. `align_codegen_llvm` return-transport unit owner: valid matrix plus malformed/multi-invalid records; LLVM verifier on every emitter. |
| Construction and return | Copy literals, nested records and call-return chains preserve evaluation count/order. `struct_by_value` minimal 216-byte two-call witness, direct constructor and scalar/small-record reverse controls. |
| Move-in/out, nulling, Drop | Existing ownership-bit and cleanup payloads are unchanged; exactly one final release. Reuse `large_drop_codegen`, `move_return_cleanup`, `owned_temporaries`; add only a case not already detecting a changed result-transport defect. |
| Replacement | Old value remains intact throughout RHS evaluation and failed/early-return RHS; source nulling follows the committed transfer. `reassign_drop`, `borrowed_replacement`, `struct_by_value`. A destination whose old contents the call can observably access must keep a temporary. Previous exposure alone does not forbid LLVM from proving safe forwarding. |
| Control flow | if/match/else/?/map_err, branch and loop joins, break/return and divergence retain their existing reached effects and cleanup. `value_control_flow`, `owned_temporaries`, `move_return_cleanup`; phi/multiple-use/loop-result joins stay on the general materialization path. The backend return-transport loop and two-live-result owners extend the same entry-alloca invariant with fallback materialization. |
| Call edges | Direct/imported/generic, indirect function values, captured closures, recursion, cleanup returns, entry/export wrappers and compiler-generated support calls agree. `lambda`, `ffi_byval`, `main_abi`, `per_unit_codegen`, `partition`; backend signature assertions enumerate every physical call kind. |
| Native boundary | Existing extern/runtime contracts and typed DB callback/descriptor conventions remain exact. Reuse `ffi_byval`, `ffi_link`, `pkg_db_q1`, `pkg_db_q3` and applicable DB owners; full local DB verification when the classifier says this diff reaches that boundary. |
| Body transport and identity | Nongeneric imported declarations and concrete generic bodies use the same result mapping; no new interface serialization field. `per_unit_codegen`, `interface_param_modes`, `inprocess_memo`, cache and ThinLTO owners cover cold/warm, compiler/LLVM/CPU changes and imported type-definition edits. |
| Runtime provenance/allocation | Result slots cannot become heap owners or extend borrowed lifetimes; existing heap/arena ownership bits pass unchanged. `return_provenance`, `owned_temporaries`, `large_drop_codegen` with allocation/free parity. |
| Source-access events | Physical lowering cannot erase, reorder or synthesize source effects. If integrated after the storage branch, consume its finalized native program and reuse its event-erasure/native rejection owners. Neither capability depends on publishing the other first. |
| Negative forwarding controls | Multiple result uses; call-observable reads of the old destination through an argument or escaped alias; intervening effects; field/over-aligned destination; by-value mutation independence; early exits. Require a retained temporary for observable aliasing. Previously exposed but provably unobserved destinations test semantic independence without mandating a copy or instruction. Parameterize the materialization owner. |
| Performance/resource evidence | On the fixed witness, two 216-byte logical objects need two aggregate regions, with no post-call 216-byte transfer at O2/O3. `struct_by_value` code-shape owner and `target_cpu_isa` native disassembly helper cover ELF/Mach-O. Do not assert one whole-frame byte count or one instruction mnemonic. |

No latency threshold belongs in correctness tests. A local benchmark, required
because this plan reduces physical copy/storage cost, compares repeated
construction-and-borrow consumption with unchanged calls and output digest.
Record compiler/LLVM identities, CPU, profile/LTO flags, repetitions, timings,
stack regions and copied bytes. Include small-register-return and multiple-use
controls; reject a claimed overall speedup if those dominate a workload and
regress. Native Mac client timing remains external qualification.

## 5. Sequence and completion criteria

- The provider source/IR reproduction is complete. Preserve it in the owner
  before implementation. Obtain the original Mac packet in parallel, without
  making provider progress depend on its arrival.
- Qualify the target classifier and implicit/explicit ABI equivalence with a
  bounded prototype. This is a mandatory implementation entry condition; the
  hand-written explicit-result IR is only a feasibility probe.
- Implement the complete return/materialization capability and close the matrix.
  Perform the author-side matrix-to-diff pass, compiler self-review, one fresh
  independent full-diff review, relevant local owners, bounded gate and Clippy.
  Run native target checks and local DB verification where applicable before
  push; use normal final-SHA preflight and PR wrappers.
- Close the caller-copy subfinding after the provider invariant passes on x86
  and native Apple ARM. Keep #1047 open for foreign query certification and any
  still-unresolved separately tracked subfinding. Report client measurements
  separately from compiler delivery.

The additional FFI claim does not select another fix: expanding cached_f16
still leaves two reached opaque reads. The shown helpers share a source module
and the assembly already targets the query directly. Imported helper exposure
requires the independent body/cache contract in plan 65; it cannot justify CSE
of the query. Traits and structural subtyping retain their existing disposition.

## 6. Upstream references and author checks

LLVM 22's [SelectionDAGBuilder](https://github.com/llvm/llvm-project/blob/llvmorg-22.1.0/llvm/lib/CodeGen/SelectionDAG/SelectionDAGBuilder.cpp)
uses `GetReturnInfo`/`CanLowerReturn` and creates an aligned hidden stack result
when demotion is necessary. Its later load explains the candidate copy path.
The [language reference](https://releases.llvm.org/22.1.0/docs/LangRef.html#parameter-attributes)
requires an sret function to return void and the caller to provide valid aligned
storage; ABI attributes must agree at definitions/declarations and call sites.
[MemCpyOptimizer](https://github.com/llvm/llvm-project/blob/llvmorg-22.1.0/llvm/lib/Transforms/Scalar/MemCpyOptimizer.cpp)
owns the call-slot alias/lifetime proof. These source records support the design;
they are not a proof of all-target ABI equivalence for a future implementation.

Author pass: every selected transformation has an owner, a general-path fallback
and negative controls above. No source/public ABI widening, native purity fact,
new persisted wire format, changed diagnostic threshold or hidden allocation is
introduced. Source witnesses syntax-check with the baseline compiler, and the
comparison IR passes the LLVM verifier. Independent review is recorded below.

## 7. Independent design review closure

One fresh inspection-only review reported two P2 findings and no P1 finding.
The author resolved the complete set in one pass:

- Result storage now has one entry-block slot per static call site, with explicit
  loop reuse and independent live-result rules. The existing entry-alloca owner
  closes both the optimized and fallback paths.
- Required-copy negative controls now depend on observable access to the old
  destination, not prior exposure alone. This agrees with the selected LLVM
  ModRef/alias proof while preserving observable by-value behavior.

The review accepted the classifier/prototype boundary as a coherent candidate;
it did not certify all-target ABI equivalence or review an implementation.

## 8. Implementation closure map

The author-side ledger extraction covers every `must` / `exact` / `every` /
`before` / `reject` / `required` obligation in sections 2–5. The complete
physical return remains the semantic MIR value plus its existing cleanup bit;
this change introduces no HIR/MIR variant, source-access fact, serialized field,
new runtime owner or independently reusable certificate.

| Obligation | Implementation and discriminating owner |
| --- | --- |
| Exact target/type/attribute decision; pre-mutation refusal | `return_transport.cpp::Classifier` uses LLVM's own classifier, the module/TargetMachine context check, fixed storage checks and complete planning. `target_mismatch_and_nonlocal_materialization_do_not_guess` and `unsupported_call_edges_refuse_before_module_mutation` cover context, owner and musttail refusals without mutation. Module verification precedes target/owner checks; then definitions and calls are checked in module order. |
| Definition/call/argument/return agreement | `indirectType`, `indirectAttributes` and the shared module rewrite shift existing attributes and arguments together. `native_implicit_explicit_return_abi_matrix` independently emits and cross-links both directions over scalar, mixed, HFA, padded/nested/array/vector and empty shapes, including trailing integer/float arguments. `scripts/test-codegen-performance.sh` runs the return-transport owners on every native CI platform; the full backend library gate additionally runs on Linux x86-64. |
| Storage size, alignment, lifetime and aliasing | One entry alloca per static call, lifetime around reconstruction/transfer, full-size memcpy only for the immediate simple whole-local store. `materialized_results_forward_without_changing_call_count`, `loop_result_storage_is_entry_allocated_on_the_fallback_path`, `observable_aliases_and_native_calls_keep_their_contracts` and the nonlocal/volatile/intervening-effect matrix distinguish forwardable and fallback cases. LLVM MemCpyOpt owns alias proof. |
| Direct, imported, generic and indirect consumers | `lower_prepared_module` installs one normalizer for all emit modes; `build_indirect_call` marks only checked program edges. `result_transport_preserves_parameter_contracts_and_indirect_edges` checks shifted contracts, erased markers and idempotence. `struct_by_value` adds imported large records, two simultaneous live values, named function values, captured closures and a generic return through whole/per-unit compilation. Existing `fn_values`, `lambda`, `partition`, `per_unit_codegen`, `thin_lto` and `function_thin_lto` own the sibling routes. |
| Cleanup, replacement, joins and early exit | Stores occur only at original return instructions, after original cleanup; the call reconstruction preserves the complete SSA value. Existing `move_return_cleanup`, `large_drop_codegen`, `owned_temporaries`, `reassign_drop`, `borrowed_replacement`, `return_provenance` and `value_control_flow` cover selected flags, exactly-once Drop, lifetime roots and reached effects. The return-completeness owner accepts a physical void return only with a typed sret destination; its semantic MIR and executable checks remain. |
| Native ownership and artifact context | Runtime/extern declarations are unselected. Checked raw descriptor calls retain the implicit slot's function lifetime and receive no capture or memcpy-forwarding fact; `native_descriptor_edges_match_generated_definitions_without_new_effect_facts` checks same-module devirtualization and absent lifetime/capture assertions. Generated bridges retain machine ABI as checked by native cross-links and the existing `ffi_byval`, `ffi_link`, `main_abi` and local DB suite. `interface_param_modes`, `inprocess_memo` and ThinLTO/cache owners reuse the unchanged semantic signatures and existing compiler/LLVM/CPU identity. No result slot escapes as a heap owner. |

The two-call `struct_by_value` IR owner is a regression against the original
caller: the baseline has aggregate-valued calls and duplicated result storage;
the candidate has two typed result calls into the two final records and no
memcpy. Backend fallback owners prevent extending that guarantee to observable
aliasing or non-materializing consumers. O0 remains a correct explicit temporary
path; no code-shape claim applies to O0. The native ABI matrix's separate object
emission deliberately performs no IR inlining across the boundary.

## 9. Local resource measurement

The source witness uses the baseline compiler hash
`5f54da0a066d0b0d3f9ccbdcff56c3b2dd5b4ccfd2aa1337b6c301fe03cb685c`
and candidate hash
`99b906b73f5d82ca0eded8af36dde1cb5cf0689b6f14c4dfb0b525700e8de422`.
Both emit release/O2 IR with runtime LTO disabled for x86-64-v2, using LLVM
22.1.8. Object emission explicitly uses `llc -O2 -mcpu=x86-64-v2
-filetype=obj -relocation-model=pic`; `cc -O2` links the independently emitted
objects. The host is a Ryzen 9 5950X under WSL2. Each execution
performs 3,000,000 calls to `probe`, retaining 6,000,000 constructor and
6,000,000 borrowed-consumer calls. All nine alternating pairs print digest
`9000078000000`. Median elapsed time is 66.4 ms before and 24.2 ms after.
The actual candidate's retargeted apple-m1 caller also has two final aggregate
regions instead of four, with both 216-byte transfers removed.

The multiple-SSA-use control normalizes the baseline's already-optimized
caller through the reconstruction path, then runs ordinary O2. It keeps
fallback storage; the median is 67.0 ms before and 57.3 ms after. This is an
LLVM mechanism control, not an additional source-language optimization claim.
The one-i64 record reverse control has byte-identical before/after objects;
its measured medians are 14.4 and 14.5 ms with identical digest `9000000000000`.
These short runs overlap unrelated local verification and show scheduling
noise even for identical objects. No overall workload latency improvement or
native Mac timing is claimed. The stable acceptance property is the eliminated
432 bytes of transfer and 432 bytes of duplicate aggregate storage per probe.
The local packet retains `benchmark.py`, `controls.py`, complete IR, assembly,
individual timings and compiler identities.
