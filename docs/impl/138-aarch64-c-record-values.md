# ARM64 C-record value passage

Status: implemented capability after plan137. Request32 names two native signatures
that require record values: a 24-byte context parameter and a returned allocator
record. Both supported ARM64 hosts currently reject every record value. This
capability implements the complete existing flat C-record domain on little-endian
64-bit Linux AAPCS64 and macOS DarwinPCS. SysV MEMORY, bool/char FFI, native
callbacks, variadics and K1/plan61 remain outside this boundary.

## Public-contract ledger

Declarations:

```align
layout(C) NativeParams { length: u64, data: raw, flags: u8 }
extern "C" fn probe_params(value: NativeParams, after: i64) -> NativeParams
```

Call within an existing unsafe block:

```align
result := probe_params(NativeParams { length: 42, data: raw.null(), flags: 1 }, 77)
```

| Surface | Exact contract and authority |
| --- | --- |
| Formation and target | Existing concrete nonempty layout(C) records with integer, float or raw fields are legal extern parameters and results on little-endian LP64 AArch64 Linux and ARM64 macOS. The exact target classifier admits aarch64 Linux and aarch64/arm64 Apple macOS spellings used by the driver; excludes big-endian, ILP32, Windows, iOS and unknown environments. Existing x86-64 Linux rules remain. Generic C records, non-C records, nested fields, bool/char/view/owned fields remain rejected by source and checked-HIR gates. |
| Signature and effects | The ordinary extern declaration and positional call above need no new annotation, option or default. Calls remain unsafe and impure. No new source function, runtime operation or package API. Whole-program and imported/per-unit calls use the same target-specific physical plan. |
| Ownership, storage and return | C records remain Copy. ABI passage copies each value including pointer bits but neither copies nor frees pointees and grants no access/lifetime authority. Large argument passage creates an aligned caller-owned stack copy; native writes to that by-value parameter cannot mutate the source record. A large return uses caller-owned aligned result storage. Scratch covers the complete padded object and any wider coercion, is hoisted to the function entry, and makes no heap allocation. Copy, assignment, return, branch/loop joins and early exits keep existing semantics. No new Drop, source nulling, failure Result or heap admission policy. |
| Physical plan | The target/shape/position table below governs exact LLVM types, physical ordinals and ABI attributes. Aggregate arguments stay one LLVM aggregate operand where required; register exhaustion is handled by the backend's aggregate ABI, never by flattening AArch64 fields into independent scalars. Full argument and return paths ship together. |
| Validation and native symbols | Source and checked-HIR formation finish before target lowering. Unsupported targets fail before LLVM function declarations. Existing physical native-symbol preflight must include new ABI obligations: a coincidentally equal pointer function type cannot authorize an indirect record or hidden sret signature. Registered runtime symbols remain governed by their fixed ABI rows; ARM record-value declarations of those symbols fail closed; the registry supplies no source C-record contract. Failure produces a diagnostic and leaves no partial function declarations. No native symbol or row changes. |
| Scalar siblings | Darwin requires signed/unsigned extension of ordinary i8/i16/u8/u16 extern arguments/results, including their shifted ordinals after sret. One target-derived attribute authority applies to the declaration and call. Fixed runtime rows retain their attribute authority, with compatibility checked before reuse. Other scalar widths and views preserve their current ABI. |
| Serialization and identity | No new HIR/MIR type, operation, interface tag or persisted format. Interfaces already encode concrete records and extern signatures. Existing resolved-target/compiler identities select unit caches. Cache warm/change/restore tests must observe changed record fields/alignment and restore the original result; platform caches cannot share one compiled artifact. |
| Limits and prerequisites | Uses shipped plan137 raw fields, native record layout and existing copy lowering. No aggregate callback/function-value surface, exported Align C entry point, cross-compilation CLI, Windows/big-endian/ILP32 ABI or SysV MEMORY extension. Integer-represented C flags remain the one existing way; this does not complete Request32's bool request or perform consumer adoption. No performance/resource improvement is promised, so no benchmark gate. |

## Physical plan table

Let S be the native padded size, A the greater of natural field alignment and
explicit aggregate align(N), and F the semantic field list. Every admitted
field is naturally aligned and at most eight bytes. LLVM's nominal record
alignment alone does not encode align(N); use A for scratch and sret.

An HFA is one through four fields all of exactly the same f32 or f64 type,
with no object padding beyond those fields (S equals field count times field
size). Explicit alignment does not disqualify a no-padding HFA. Padding that
increases S does. Test both sides of this distinction.

| Shape / position | Linux AAPCS64 | macOS DarwinPCS |
| --- | --- | --- |
| HFA argument | One [N x float/double] operand; alignstack(8) attribute (flat supported fields have natural alignment <=8) on declaration and call | Same array operand, no alignstack attribute |
| HFA result | Native homogeneous record type, reconstructed/used as the source record | Same |
| Non-HFA argument S<=8 | i64, or ptr when every semantic field is raw | Same |
| Non-HFA argument 8<S<=16 | [2 x i64], or [2 x ptr] when every semantic field is raw, including explicit tail padding | i128 when A>=16; otherwise same two-element array |
| Non-HFA result S<=8 | i(S*8), carrying native low-address bytes | Same |
| Non-HFA result 8<S<=16 | i128 when A>=16; otherwise [2 x i64] | Same |
| Non-HFA argument S>16 | Pointer to an independently owned aligned caller copy, without byval; one ordinary physical operand | Same |
| Non-HFA result S>16 | void return plus first physical pointer parameter with typed sret(record) and align(A), before every source argument; dedicated native return register is selected by sret | Same |
| Scalar narrow integer | Existing direct scalar representation | signext for i8/i16, zeroext for u8/u16 on result or exact physical argument ordinal |

Every coercion slot is entry-hoisted, sized to max(S, coercion store size), and
aligned to max(A, coercion alignment). Initialized semantic bytes must survive
coercion; unspecified padding is not an observable contract. The caller copy
and result slot are distinct from each other and from every still-live source
record. No byval attribute is substituted for AArch64's caller-copy pointer.

## Implementation closure matrix

| Axis | Implementation and exact acceptance owner |
| --- | --- |
| Classification, width and malformed inputs | Private codegen target/record plan uses validated field/layout metadata. Parameterized align_codegen_llvm owner crosses both targets, integer widths/signs, mixed integer/float/raw records, raw-only records, 1..4 f32/f64 HFAs and 5-member non-HFA, sizes 1/3/8/12/16/24/32/40, natural/16/32 alignment, and refused triples. LLVM verify and exact function types/attributes pin each class. Existing checked-HIR C-field and ffi owners close formation and unsafe rejection. |
| Construction, copy, replacement, return, Drop | Extend the existing bounded c_layout_raw_fields C harness with native ARM record-value cases. C reads every supplied field, changes its local argument, and returns a changed record; Align verifies the returned value plus unchanged original and copied source. Include null/non-null pointer identity and integer-represented flags. Static pointees make accidental free observable. Copy has no Drop/nulling; existing whole/per-unit control composition reuses if/match/else/?/map_err/early exit/loop/generic helper records. |
| Register pressure, ordering and attributes | Native whole/per-unit cases use zero, exact-fit (8 minus the aggregate register width), first-spill (exact-fit plus one), fully exhausted (8), and already-spilled (9) preceding GP or FP scalars for each applicable family: ordinary two-register and aligned16 records, every 1..4-member HFA, and indirect records. Following narrow/wide/FP sentinels and multiple record arguments pin order. Exact-fit and exhausted aggregates must reach C unchanged. Include HFA pressure with following FP and GP values, Darwin packed narrow stack arguments, and sret plus mixed ordinary parameters. LLVM owner asserts single-aggregate argument shape, declaration/call alignstack, sret and narrow scalar attributes and ordinals. |
| Allocation parity, alignment and loop lifetime | Existing raw-layout parity plus codegen owners assert full aligned scratch and entry-only alloca sites, including explicit tail padding. Native large/over-aligned arguments and results check alignment and source preservation. Repeated loop calls reuse fixed scratch without stack growth. No runtime allocator path is added; inspect MIR/LLVM and reuse Copy/Drop owners. |
| Native declaration preflight | Codegen owner for runtime-symbol collisions crosses direct pointer-looking record arguments, indirect record arguments and hidden sret with otherwise matching pointer function types. Assert a diagnostic before any function declaration when the source signature uses a record value. Existing compatible scalar/native attributes owners retain positive coverage. |
| Generic, imports and artifact identity | Existing bounded C harness compiles concrete public records and generic helper through whole/per-unit paths. Imported record and extern interfaces round-trip; cold/warm/change/restore native execution exercises field and alignment identity. Existing target-identity/cache owner retains target separation; no persisted schema changes. |
| Platform siblings and invalid domains | Reuse full SysV ffi_byval/raw/layout/ffi_views/unsafe owners; replace the old ARM64 blanket refusal test with supported ARM cases and target-independent unsupported-target negatives. Run new native owners on local Apple Silicon and local Linux ARM64 Docker before push; codegen owner checks both target plans independent of host. Existing Linux x86 native owners remain unchanged. |
| Test resources and final gates | Reuse c_layout_raw_fields' private ArtifactStage, immediately armed bounded process group, exited-leader and unwind cleanup owners. No ambient env mutation or unowned C compiler/link/run children. Run narrow codegen/native owners, independent inspection, final preflight and literal release workspace build after merge. |

## Author source and boundary pass

Primary references are the ARM AAPCS64 parameter/result rules, Apple's ARM64
platform differences, and LLVM22 Clang's AArch64 classification. Independent
local Clang22 probes record real physical signatures for both platforms,
including padded float records, aligned HFAs, pure-pointer records and narrow
integer attributes. These are implementation oracles, not a new user promise.

- https://github.com/ARM-software/abi-aa/blob/main/aapcs64/aapcs64.rst
- https://developer.apple.com/documentation/xcode/writing-arm64-code-for-apple-platforms
- https://github.com/llvm/llvm-project/blob/llvmorg-22.1.8/clang/lib/CodeGen/Targets/AArch64.cpp

Required public agreement: draft.md section15, language-spec, design-notes,
Settled/open FFI entries, roadmap and English/Japanese unsafe/FFI guide. HIR and
runtime ledgers change only if their normative contract changes; unchanged
formation and fixed native rows do not need status narration. HANDOFF records
one implemented boundary; the external request register receives a partial
provider answer without consumer code edits or commits.

One capability must connect classification, attributes, aligned copy and result
reconstruction: a dormant producer cannot be tested by an actual C caller.
Darwin and Linux are sibling rules for the supported ARM64 platform family.
SysV MEMORY is a separate useful ABI domain and can follow independently. The
implementation exceeds 1,000 handwritten lines because the two platform plans,
24-shape exact LLVM owner, shared native pressure matrix and public ledger form
one value-passage capability. Retaining this boundary avoids duplicating all
record/control/cache and native preflight proof;
parameterized owners and the existing bounded harness keep the proof shared.

Author ledger-to-prose pass: every admitted target, record shape, position,
attribute, ownership and cache rule is enumerated above. No wire/text input,
inspection table, configuration, process-global state or native API is added.
The matrix closes the observable exactness claims, with existing owners for
unchanged formation/control invariants. Fresh independent adversarial plan
review is required before implementation.

## Independent plan review

Fresh inspection accepted the capability boundary without P1/P2 findings. The
author tightened the pressure row to each family's exact-fit/first-spill
boundary; a scoped continuation confirmed that row before implementation.

## Author implementation closure

The private ffi_aarch64 module selects the closed target domain and record plan.
The shared extern declaration/call plan carries physical attributes and sret
ordinals; every ARM record argument and result receives a distinct full aligned
entry slot. Fixed native record redeclarations fail before any function is emitted.
The 24-shape codegen owner checks both targets, exact type/attribute ordinals,
full storage and loop hoisting. The existing bounded C-layout process owner
contains the native record/pressure matrix, two copies of each argument, native
field/alignment checks, exact observed call count and 4096 repeated large calls.
Whole/per-unit imports and generic return helpers execute every case. An additional
16-to-32-to-16 alignment cache cycle crosses direct/indirect ABI forms with the
same foreign name; actual native results and retained record metadata qualify
miss/hit/change/restore. Existing raw/control/HIR/unsafe/SysV owners close unchanged
formation and control invariants. Both native ARM64 platforms passed locally.
