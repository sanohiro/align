# Command-line text admission

Native argv may contain arbitrary non-NUL bytes. The current wrapper publishes
them as `str`; an executable passed `ff c0` prints those bytes from its Align
body on both macOS and Linux. Admit UTF-8 before constructing the argument
array, preserving the settled text invariant and the existing entry signatures.

## Public contract ledger

| Field | Contract |
| --- | --- |
| Surface | Existing `main(args: array<str>) -> Result<(), Error>`; no new API, option, environment setting, or source signature. `args[0]` remains the program name. |
| Text | Every argument, including the program name, must be valid UTF-8. Empty text is valid. Preserve accepted bytes exactly; no normalization or replacement decoding. Native NUL terminates an argument and is not part of its view. |
| Failure | Invalid native argument text prevents entry into the Align body. The wrapper reports builtin `Error.Invalid` with the existing reporter (`error: code 2` and newline on stderr) and returns exit 2. It produces no argument array or body side effects. No-argument main forms do not inspect argv. |
| Ordering | Validate output slot, argc/argv extent, and each entry in ordinal order, then allocate the complete header array, then publish it, then invoke the Align body. All invalid metadata/text returns the same Invalid status. No allocation or partial publication before complete admission. |
| Ownership and allocation | Success with argc > 0 owns one allocation of exactly argc `AlignStr` headers; contained text borrows stable process-lifetime argv bytes. No byte copy or transient owned collection. argc == 0 succeeds with canonical null/zero output and no allocation. Existing array move/nulling/Drop releases only the header allocation. Rejection has canonical null/zero output and no allocation. |
| Native boundary | Change private A92 atomically to `i32 @align_rt_args_build(i32, ptr, ptr)`: arguments are argc, argv, writable AlignStr output. Return 0 on success, 2 (`AL_INVALID`) on failure. Negative argc, null positive-count argv, null individual entries, or unrepresentable extents reject. Null argv is allowed for zero argc. Null output rejects before access. Non-null input/output pointers must be aligned, readable/writable for their extents, non-overlapping, and each argument NUL-terminated; argument bytes remain stable throughout the call and subsequent use. |
| Extents | Check count conversion and both pointer-table/header-array multiplication against `isize::MAX` and native/i64 representability before traversal/allocation. C-string extent remains the unsafe caller's obligation; each observed text length must fit i64. No reading the optional argv[argc] sentinel. |
| Effects and access | Keep `Unkeyed::ArgsBuild`, IndirectStorage, argv escape ordinal 1, no fresh-return claim, no curated output-write attribute or rt-LTO admission. Explicitly reject source-extern reuse even with the now-expressible exact i32/ptr/ptr shape; ArgsBuild remains wrapper-only. No new concurrency or access-analysis strategy. |
| Owners and identity | Runtime owns decoding/header allocation; existing generated C-main wrapper only marshals, branches on status, reports and calls the Align body. MIR continues to own source semantics and array cleanup. No new MIR/HIR type or operation. Existing compiler build identity and runtime-source fingerprint invalidate stale artifacts; no interface serialization change. |
| Sources of truth | Update draft, language digest, design rationale, settled entry rule, roadmap entry record, runtime startup outline, native-library boundary and ABI ledger/declaration golden, and English/Japanese error guide. Plan 61 remains deferred; its borrowed-byte ownership description is unchanged. |

## Implementation closure matrix

| Axis | Implementation and acceptance owner |
| --- | --- |
| Type formation and validation | Existing closed main-signature checks remain unchanged; `main_abi` owns accepted entry behavior; the bounded gate retains `main_signature_matrix` and `main_abi_matrix` for source/malformed-MIR rejection. New native owner rejects negative/count/null/entry/text states before construction, including invalid first/middle/last and late invalid bytes. |
| Construction and publication | Two passes over stable native entries: full UTF-8 admission, then one header allocation and borrowed-view fill. Native owner checks exact bytes/pointers/counts, empty arguments, repeated aliases, canonical output and allocation parity. No intermediate Vec. |
| Move-in/out, nulling, Drop, replacement and return | Successful array transfers once to existing Align main parameter and normal MIR cleanup. Native owner verifies one allocation/free; compiled owner moves the argument array through imported/generic helpers and success/error/early exit, with scalar branch and loop joins. Whole-array self-replacement and array-valued loop breaks are rejected by the current frontend; those source-analysis shapes remain deferred with K1, without weakening the native admission boundary. Text views remain process-lifetime; no argv byte is freed. |
| If/match/else/?/map_err and failures | Existing source Result cleanup is unchanged. Compiled owner exercises representative success and Error returns, `?`, `map_err`, and an early exit with owned args; invalid native input never reaches any source control flow. Existing tagged/array cleanup owners close remaining carrier paths. |
| Whole/per-unit, generic and interface | Same generated imported program through both build paths; generic ordinary array/string-view paths require no codec extension. Raw LLVM owner verifies the status branch precedes payload load/body call; optimized linked execution proves rejection behavior. Execute with runtime LTO off/on to retain private native boundary parity. |
| ABI and native provenance | Exact A92 return/parameter order changes in runtime and typed registry together. Declaration golden, independent complete extern type matrix, wrapper-only source rejection (new exact shape plus old forms), main wrapper registry owner, and linked execution discriminate mismatch. Keep effects conservative. |
| Test isolation and portability | Native allocation counters run in one exact-filter child with an immediate bounded child guard. Compiled fixtures exclusively acquire `ArtifactStage`; source/object/executable/output files stay inside it. Bounded children use scoped argv/arg0 bytes without process-global mutation. Run native and compiled owners on macOS and Linux. |

One capability joins runtime and wrapper producer/consumer ABI changes; neither
is useful independently. Expected diff is below 1,000 handwritten lines. No
performance claim or benchmark gate. Author ledger-to-prose and matrix-to-diff
passes precede a fresh strategy review and later implementation review.

## Acceptance bindings

- `align_runtime --features alloc-count --lib args_tests`: all argument positions,
  malformed count/null cases, canonical outputs, UTF-8 matrix, exact allocation/free
  parity, repeated borrowed pointers and byte preservation after Drop.
- `align_driver --test argv_text`: imported/generic whole/per-unit execution with
  runtime LTO off/on, empty/non-ASCII/invalid program and user arguments, pre-body
  rejection, unused-args admission, early and later Result exits, raw wrapper
  branch/load shape, and all three no-argument entry forms.
- `align_driver --test main_abi`: existing exact Unit/i32/Result entry behavior,
  raw/optimized signatures and ordinary non-entry generic main functions.
- `align_codegen_llvm --lib runtime_abi`: declaration golden and exact type/effect
  inventory, every extern ordinal, explicit private ArgsBuild source rejection,
  and typed main-wrapper consumer inventory.

The strategy review found no contract or boundary defect. Existing frontend
rejection of whole-array self-replacement and array-valued loop breaks was
observed while forming the owner fixture; those source-analysis shapes remain
outside this capability under the explicit K1 deferral. Native and wrapper
admission, ownership and ABI coverage do not depend on those forms.
