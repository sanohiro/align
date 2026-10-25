# Fixed numeric arrays in C records

## Selection and evidence

Media AM-R17 uses eight dimension/stride entries and a 41-element convolution key.
Its reduced `layout(C) Shape { dims: [i64; 8] }` raw-store/load program is refused
by the current scalar-only C-field validator. The explicit scalar workaround is
valid; no speedup is claimed. LLM R32's raw fields and voice V006's heap-record
fixed fields are distinct delivered capabilities, not adoption of this request.
All three consumer registers were reconciled; consumer changes remain external.

The useful capability is the existing C-record contract with fixed numeric fields:
formation, storage, Copy transport and every supported by-value foreign ABI.
A raw-only exception would make the same type's C representation contradictory.
This is composition under the existing fixed-array layout and C scalar model;
it introduces no error, ownership, lifetime or unsafe model. The independent
boundary review accepted it against Plan23 and the former flat-field exclusion.

## Public-contract ledger

| Surface | Exact contract |
| --- | --- |
| Formation | A concrete `layout(C)` record field is an existing C integer/float/raw scalar, or `[P; N]` with P one of i8/u8/i16/u16/i32/u32/i64/u64/f32/f64 and N a positive u32 literal. Existing fixed-array parsing, cardinality and type validation remain authoritative. N=0 is rejected specifically at the C-field boundary: standard C has no zero-length array member. Ordinary Align zero-length arrays remain unchanged. No raw-pointer arrays, bool/char arrays, nested arrays/records, enums, views, owners, handles, callables or generic C declarations. Negative excluded element types stay excluded even at zero length. Empty C records keep their prior raw-storage behavior and remain excluded from by-value extern passage. |
| Layout | Declaration-order fields, each numeric array containing N contiguous P values with P's native size/alignment and no header or child padding. Normal C inter-field/tail padding and explicit aggregate align(N) remain. Semantic size/alignment, LLVM layout and independent C sizeof/alignof/offsetof agree on supported targets. All size/count arithmetic and metadata validation finish before emission. Padding bytes remain unspecified, not serialized data. |
| Source operations | Ordinary construction, Copy binding/assignment, field/index read/write, stable-place slice, function parameter/result and control joins retain fixed-field semantics. Existing `raw.store(address: raw, offset: i64, value: CRecord) -> ()` and annotated `raw.load(address: raw, offset: i64) -> CRecord` copy the whole record at byte offset, under unsafe. The caller supplies valid live writable/readable storage of sufficient size; raw loads/stores remain unaligned operations with alignment 1. No array-to-pointer decay or naked fixed-array extern parameter/result is added. |
| Ownership and effects | Records remain Copy. Fixed numeric fields allocate no heap/arena storage, have no Drop/nulling, and retain no backing. Existing direct raw pointer fields remain non-owning and confer neither lifetime nor access authority. Raw operations and extern calls retain current unsafe/effect rules. ABI caller scratch is full-size, aligned, entry-hoisted and independent of live source records; native mutation of a by-value argument cannot modify the caller source. |
| By-value targets | Preserve the closed little-endian LP64 x86-64 Linux GNU/musl and AArch64 Linux/ARM64 macOS target sets. SysV classifies every numeric array element at its actual byte offset, merging INTEGER over SSE in each occupied eightbyte; tail-only padding consumes no register. Records >16 bytes and whole-aggregate register exhaustion use existing stack/sret rules. ARM HFA classification flattens arrays and scalar fields into 1..4 identical f32/f64 members with no excess object padding; all other shapes retain existing integer/coercion/indirect plans, Linux alignstack and Darwin differences. Arguments/results and exact-fit/spill behavior ship together. |
| Validation and errors | Existing parsing/type width/cardinality precedes C-field admission in declaration order; unsupported fields fail before raw/extern lowering. Shared field admission is separate from scalar `is_ffi_safe`, so extern arrays are not accidentally admitted. Checked-HIR struct and extern validation replay the same field rule plus ordinary width/cardinality checks. Invalid IDs/types/widths/N and unsupported targets fail diagnostically before native declarations; runtime-symbol ABI preflight remains fail-closed. No native symbol/export changes. |
| Identity and encoding | Existing nominal record/interface identity includes field order, primitive kind/width, fixed length and explicit aggregate alignment; full reachable definitions and compiler/target identity already separate caches. No HIR/MIR variant, interface tag, runtime ABI or persisted/wire format changes. Existing interface encoding retains u32 fixed length and current scalar tags; existing corruption owners remain. Cold/hit/changed element width or length/exact revert must observe correct native values and identity. |
| Other dimensions | No new default, CLI/configuration input, text encoding/NUL boundary, global state or reflection/inspection table. No throughput/resource promise beyond existing no-child-allocation Copy semantics. No benchmark gate. |
| Agreement | draft section15, language-spec, design-notes, Settled FFI entry, HIR and library-boundary ledgers, Plan73 exclusion, Plan137/138/139 field-domain notes, unsafe/FFI guide EN/ja, Plan230/HANDOFF and media register. Update only normative promises/status boundary; unchanged runtime rows are not restated. |

## Implementation closure matrix

| Axis | Implementation and acceptance |
| --- | --- |
| Formation and malformed metadata | One shared C-field predicate separates numeric fixed fields from scalar extern values. Source and checked-HIR owner parameterizes ten primitive kinds, N=1/2/3/4/5/8/41, direct raw siblings and excluded zero/bool/char/raw-array/view/owned/nested/generic twins. Forged checked-HIR field/extern rows mutate zero length and scalar widths; malformed inputs must fail before codegen. Reuse fixed-array cardinality owner. |
| Storage and ordinary operations | Semantic/LLVM/C layout parity for packed-size-but-naturally-aligned fields, mixed widths, surrounding sentinel fields, explicit align16/32, 8 dimensions/strides and 41 keys. Native numeric-array raw round trips at deliberately unaligned offsets plus the existing aligned raw owner, construction/Copy, whole replacement, element mutation, function return and generic helper copies. Read every element/sentinel; no owning cleanup is added. |
| SysV physical ABI | Extend shared valid-field checks and register classifier to all occupied array leaves. Exact LLVM function/call types and attributes for SSE/SSE, INTEGER/SSE, SSE/INTEGER, INTEGER merge within one eightbyte, memory sizes and over-aligned tail padding. Independent C receives all members, mutates its local argument and returns changed values; caller verifies original copy. Parameterize zero/exact-fit/first-spill/full pressure, mixed following sentinels, multiple records and sret shifts. |
| ARM physical ABI | Same source domain; count homogeneous leaf cardinality without per-element iteration for huge arrays. HFA f32/f64 N=1..4, scalar-plus-array HFA, mixed/more-than-four and padded non-HFA twins. Keep Linux/Darwin argument/result coercions, alignstack/sret/extension ordinals, full scratch and entry-only allocas. Native exact-fit/first-spill/full pressure plus following GP/FP values. Cross-target LLVM verification and independent Clang22 prototypes pin shape. |
| Control and lifetime | Whole/per-unit native if/match/else/?/map_err/loop/early-return composition reuses existing Copy/FFI controls and includes new array-field results through those joins. Returned records outlive temporary call scratch; repeated calls and loop iterations preserve prior copies without stack growth. Stable-field views retain normal local lifetime; no new returned authority. |
| Imports and cache | Public concrete C record, extern declaration and generic helper imported in whole/per-unit modes; cold/hit/element-width-change/length-change/exact-revert with native execution. Existing schema/target identity and corrupt interface owners reused. |
| Siblings and exclusion | Existing c_layout_raw_fields, layout_c, ffi_byval, ffi_views, unsafe owners and SysV/ARM classifiers retain scalar/raw/target/runtime-symbol controls. Non-C fixed fields and heap-record grammar do not widen. |
| Local verification/resources | Reuse bounded private C-layout child lifecycle and shared owner fixtures; no consumer code changes. Qualify supported native platform calls locally before push, using Docker/emulation for Linux as needed, and inspect cross-target LLVM plans on host. No CI discovery. Check disk before large builds, share compatible caches, remove temporary cross artifacts afterward. |

The C ABI and raw storage consumer must land together. If the complete parameterized
native/LLVM owner and contract exceed 1,000 handwritten lines, keep this single
capability: splitting its dormant type producer from layout/ABI consumers would
duplicate the same formation, identity and by-value proof and expose inconsistent
C types. Prefer compact shape/pressure generators and existing test infrastructure.

## Author evidence and consistency pass

ARM AAPCS64 arrays use element alignment and count-times-element size, and HFA
uses flattened homogeneous fundamental members. Clang22.1.8 cross-target probes
confirm `[f32;3]`, `[f32;2]+f32`, `[f64;4]`, five-float and mixed array/integer
records on all three supported ABIs, including align16 non-HFA divergence between
Linux and Darwin. SysV Clang source recursively classifies each array element and
merges its eightbyte classes; no new representation rule is needed.

Primary references:
- https://github.com/ARM-software/abi-aa/blob/main/aapcs64/aapcs64.rst
- https://github.com/llvm/llvm-project/blob/llvmorg-22.1.8/clang/lib/CodeGen/Targets/X86.cpp
- https://github.com/llvm/llvm-project/blob/llvmorg-22.1.8/clang/lib/CodeGen/Targets/AArch64.cpp
- https://developer.apple.com/documentation/xcode/writing-arm64-code-for-apple-platforms

The author pass checked current raw lowering: aggregate loads/stores explicitly
use alignment 1, byte offsets and ordinary scalar-to-record lowering. The shared
field classifier returns each valid scalar leaf and its positive multiplicity;
source, checked HIR and both ABI consumers use that one domain without changing
scalar extern admission. No new persisted field, process state or native API is
missing from the ledger.

Exact owners: `align_sema::c_record_field_domain` for the shared predicate;
`align_mir` checked-HIR C-field mutations in the existing validation owner;
`align_codegen_llvm::ffi_fixed_array_tests` for semantic/LLVM layout, all three
physical plans, scratch, native collision preflight and declaration/call attributes;
`align_driver::c_layout_raw_fields` adds a separately bounded `fixed-arrays` role
using shared `helpers/ffi_fixed_array_cases.rs` and native C round trips, source
Copy/control/import checks. A separate `fixed-array-cache` role checks cache
identity and native execution for cold/hit/width/count/revert. Existing lifecycle harness remains unchanged.
The native fixtures make ordinary C calls and do not require a performance claim.
The driver owners run on the ARM64 macOS host. The same generated whole-program
objects and C oracles also execute in local Linux x86-64 and ARM64 containers,
linked by GCC 13.3 and 12.2 respectively. No runtime stubs are needed; the only
non-oracle undefined symbol is memcpy. These cross-target runs qualify native C
layout and ABI; host driver owners cover actual whole/per-unit runtime linkage.
The emitted fixture and C oracle must be the same shared source as the committed
native owner, including whole/per-unit object production where supported.

The former C-field restrictions are forward-linked to this ledger in one normative
pass after plan review. Implementation followed that accepted boundary review.


## Independent boundary review

Fresh inspection accepted the full capability without P1/P2 findings. Positive
numeric fixed fields are Category A composition under existing C representation
and Copy rules; no locked language model or K1 dependency is reopened. The review
checked source/HIR domain separation, flattened ABI classification, scratch and
pressure, malformed metadata, native collisions and imported/cache identity.

## Author closure pass

### Raw-loaded return correction (AM-R24)

The 2026-10-25 consumer reproducer rejects both direct and local-bound raw-load
returns of the admitted C record, while reconstruction from its Copy fields
passes. The storage classifier still assigns `RawLoad` the missing-producer
policy; numeric fixed fields therefore become unknown local storage. Classify
the existing raw-load result as fresh content without borrowed dependencies.
Its inline storage remains tracked for views, replacement and expiry. This
implements the existing source/Copy contract; no representation, API, ABI,
interface format, lifetime rule or allocation behavior changes.

| Closure cell | Implementation and owner |
| --- | --- |
| Valid formation and complete copied contents | `storage_variant_policy` supplies fresh empty content to both EscapeCheck and MoveCheck; `raw_loaded_fixed_c_records_are_independent` covers ten numeric types, raw siblings, direct/bound/arena returns and tagged carriers. Existing `c_record_field_domain` and checked-HIR C-field owners retain malformed-domain rejection. |
| Construction, copying, replacement and return | The existing 28-record native fixture adds direct/bound raw-load helpers and checks every returned element after mutation of the source and another Copy. The same fixture runs whole/per-unit and through width/count/cache-hit/revert rounds. |
| Imported result and source release | `raw_loaded_fixed_records_return_across_units` returns copied numeric fields after freeing the source allocation, then checks direct/bound/imported/generic results and independent-copy mutation in both compilation modes. |
| Borrowed negatives and control paths | The sema owner retains local fixed-field slice escape rejection, including direct loaded fields; existing fixed-field exclusion and Copy/FFI control-join owners remain. Inline views retain lifetime/generation tracking. |
| Drop and other boundaries | C records contain no owning children or Drop action; raw pointer fields convey no pointee authority. Existing raw unsafe checks, native ABI owners and independent checked-HIR replay remain authoritative. Partial owning fields, external writable views and R132/R142 are outside this correction. |

This is the already-reviewed Plan233 strategy with a missing producer repaired.
The author matrix pass and one preflight diff review cover the correction; no
new safety strategy or separate broad design review is required.

The extracted ledger obligations map to the following changed sites and owners.
No IR variant, ownership transfer, cleanup strategy or serialized format changes.

| Obligation | Implementation and regression owner |
| --- | --- |
| Closed field domain, scalar widths, positive count, naked extern exclusion | `align_sema::c_record_field_scalar`, source declaration gate and checked-HIR `struct_fields_valid`/`ffi_struct_ok`; `c_record_field_domain`, `c_layout_fixed_fields_require_positive_numeric_elements`, `fixed_c_fields_preserve_the_excluded_domains` |
| C storage, explicit alignment, all numeric leaves, Copy and controls | Existing fixed-field layout/Copy/raw lowering; shared 28-record fixture, independent C sizeof/alignof/offsetof assertions and all-element reads/writes; `fixed_numeric_fields_cross_c_storage_and_values` whole/per-unit. Existing `layout_c`, `unsafe_raw` and `fixed_array_fields` retain aligned/raw/ordinary-array controls. |
| SysV and ARM plans, scratch, pressure, returns and native preflight | `classify_struct_abi`, `SysvMemory::new`, `ffi_aarch64::Record::classify`; `fixed_numeric_c_record_abis` verifies all three targets, 392 actual calls and INTEGER/SSE/HFA/memory native-symbol collision twins. Shared native C verifies received values, local argument mutation, returned values and preserved caller copies. Existing `ffi_aarch64_tests`, `ffi_sysv_tests`, `c_layout_raw_and_sysv_padding` and `ffi_byval` retain target, padding, malformed and loop-scratch controls. |
| Imports and identity | Existing interface/cache representation; `fixed_numeric_field_cache_tracks_width_count_and_revert` checks both imported units' hit state, exact MIR/type snapshots and native per-unit replay across five rounds. |
| Prose agreement | The author pass reconciled draft/spec/Settled/rationale, current HIR and library-boundary field predicates, historical plan forward links, EN/ja guide and the capability queue. The consumer register is updated only with actual provider delivery after merge. |

The self-review checks all numeric kinds and both ABI consumers through the same
field-domain helper. SysV bounds classification by the validated <=16-byte object;
ARM counts HFA leaves with checked arithmetic and early cardinality rejection.
Existing raw operations retain alignment 1 and ordinary Copy values never acquire
backing authority. Parameterized source/HIR negatives and pre-declaration native
collision owners preserve fail-closed behavior. The tests reuse the existing
private child lifecycle; exported cross-target artifacts are exclusively acquired
and removed after local qualification. No benchmark or new allocation claim is added.
