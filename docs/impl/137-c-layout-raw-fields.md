# Raw pointer fields in C-layout records

Status: implemented capability. Request 32 supplies the concrete consumer:
`ggml_init_params` contains `size_t`, `void *`, and an integer-represented C
boolean. The present field gate prevents even the existing pointer-based FFI
from representing that record. This capability closes that field-formation
gap independently of K1 and plan61.

The existing diagnostic explicitly defers other C field types to a later FFI
slice. This extends the existing opaque pointer representation; it does not
reopen the deliberate bool/char FFI restriction or create pointer ownership.
Plan138 and plan139 own the subsequent ARM64 and SysV MEMORY-class value-passage capabilities. A usable
consumer ships here: construct, inspect, copy, update, return, raw-load/store,
and exchange pointer-bearing records with C through the existing raw boundary.
That consumer works on every supported host without a future ABI producer.

## Public-contract ledger

Declaration:

```align
layout(C) NativeWindow { length: u64, data: raw, flags: u8 }
```

Construction inside an existing unsafe block:

```align
window := NativeWindow { length: 0, data: raw.null(), flags: 0 }
```

| Surface | Exact contract and owner |
| --- | --- |
| Formation | A concrete `layout(C)` field accepts an integer, float, or `raw`. Existing integer/float widths and align(N) rules remain. A direct raw field in an ordinary struct remains rejected; generic layout(C), nested C fields, bool/char, views and owned values remain rejected as C fields. A generic function may take/return an already concrete C record. Sema and checked-HIR validation enforce the same field predicate. |
| Representation | `raw` is the target's existing opaque data pointer, with the native pointer size/alignment and declaration-order field offset. No integer conversion, new pointer type, hidden length or tag. Natural padding and existing explicit aggregate alignment compose exactly as for integer/float C fields. No canonical wire encoding is promised by native layout. |
| Ownership/lifetime | A pointer-bearing C record is Copy and has no Drop. Copy, field replacement, control joins and return copy pointer values only. They neither copy/free a pointee nor extend its lifetime or infer non-null/no-alias/read/write authority. Null fields are valid. Pointee validity, effective type, initialization and lifetime remain the caller's existing unsafe responsibility. |
| Operations/effects | Record construction, projection, assignment and ordinary helper returns use existing struct semantics. raw.load/store, raw allocation/free/offset and foreign calls retain their existing unsafe gates and effects. Merely containing a raw pointer grants no safe dereference operation. There is no new allocation, runtime operation, default value or failure result. |
| Foreign passage | Existing pointer-based FFI can read/write the complete record or selected fields. Existing x86-64 Linux register-class by-value rules classify pointer fields as INTEGER. Padding-only trailing eightbytes consume no register, including align(16) single-pointer/integer/float records; coercion/reconstruction scratch still covers the complete padded struct size. Size and register-pressure rejection use the existing C ABI rule. Unsupported targets/large value signatures keep their diagnostics. This capability does not claim direct ARM64 ggml_init adoption. |
| Validation/error order | Resolve each field in declaration order, apply general field placement including the C-only raw admission, then the existing C-field restriction; retain existing invalid-type sentinels and later layout checks. Checked HIR rejects an otherwise valid raw field when c_repr is absent and rejects every non-C scalar/aggregate in a C field before MIR. Existing graph/type/layout validation precedence is unchanged. |
| Serialization/cache | No new Ty/Scalar/IR tag or interface version. Existing raw field type, c_repr, ordered fields and align values round-trip in concrete interfaces and structural MIR fingerprints. Cold/warm/change/restore imported builds must distinguish pointer-vs-integer fields through existing identity; explicit alignment retains its existing serialized field. Compiler identity changes invalidate old artifacts normally. |
| Owners/prerequisites | Sema owns admission; checked HIR owns independent revalidation; existing MIR/LLVM own pointer storage, copying and layout. No runtime/package signature changes. Uses shipped raw operations and concrete struct imports only. No latency/throughput/resource improvement is promised and no benchmark is required. |

## Implementation closure matrix

| Axis | Implementation and exact acceptance owner |
| --- | --- |
| Formation and malformed producers | `align_driver --test c_layout_raw_fields`: accepted direct pointer fields, mixed integer/float/pointer records, null and generic helper; refused ordinary raw field, generic C record, bool/char/view/nested/owned C fields. `align_mir` parameterized placement owner independently mutates c_repr and refused scalar/view/owner/tagged field families around one valid pointer record. |
| Layout and native passage | `c_layout_raw_fields` compiles a real C helper; independent C sizeof/field reads/writes and whole-buffer boundary sentinels qualify pointer-first/middle/last, mixed-width padding and align(32), including unaligned whole-record raw.load/store. Pointer identity is checked in C without an integer round trip. TypeLayoutCache gains the explicit existing 8-byte raw pointer leaf, qualified against LLVM/native layout. The bounded native owner adds SysV pointer/integer positives and preserves pressure/size/target negatives. |
| SysV padding/storage cross-product | `align_codegen_llvm` ABI owner crosses Raw/integer/float with natural/align(16) layouts: semantic fields determine register classes, trailing padding consumes no class, and scratch includes all padded bytes on argument and result paths. `c_layout_raw_fields` native SysV owner places a following scalar after each aligned one-field record, returns it, and exercises exact-fit GP/SSE boundaries; the codegen owner checks exhaustion for all three field families. The same root-cause correction covers existing integer/float siblings. No blanket rejection of otherwise supported over-aligned records. |
| Construction, copy, source, replacement, return and Drop | Whole/per-unit source owner returns concrete records from helpers, copies/replaces raw fields and uses both original/copied records. Static C-owned pointees remain alive through all source scopes; source copies have no implicit free or nulling. Native static-pointee identity across completed source scopes rejects implicit pointee cleanup; the target-independent layout/class owner pins pointer storage. |
| Control | One whole/per-unit composition crosses if/match/Option else/Result ?/map_err, branch and loop joins, early return, with concrete pointer records. No new control-flow algorithm; reuse cumulative struct/unsafe owners. Invalid raw load/store/extern outside unsafe remain diagnosed. |
| Generic/import/interface/cache | Imported public record and ordinary/generic helpers execute through whole and per-unit compilation; concrete interface encode/decode retains raw, layout and alignment. Cached cold/warm/type-change/restore uses an observable native record result. Generic C declaration remains forbidden. |
| Safety siblings | Ordinary/direct raw field, raw tagged/collection field, C bool/char/view/owned/nested field, safe raw dereference, JSON encode/decode, SoA and heap/region builder admission remain rejected. Fixed arrays and ordinary enclosing structs retain their existing concrete Copy-record behavior. Existing record-contained raw pointer does not acquire a pointee ownership or inferred lifetime contract. Audit struct field extraction, assignment, Drop and scalar/layout conversion matches for the existing Raw branch. |
| Platforms and test resources | Run focused native and compiler owners locally on macOS and Linux before push. Test artifacts use ArtifactStage and every spawned process has a bounded, immediately armed process-group kill/reap owner, including setup failure and exited leaders with live descendants. Reuse existing qualified test machinery where possible. Required final batch build is literal cargo build --release --workspace. |

This is one formation-to-pointer-consumer capability, expected below 1,000
handwritten lines. It needs one fresh independent adversarial plan review
before implementation because the admitted FFI field domain changes. The final
candidate receives the normal one code review and local gates.

## Author ledger-to-prose pass

Every new admitted field, representation, operation, error, ownership and
artifact rule is above. There is no new text/wire input, process-global state,
inspection table, configuration, public function, or runtime ABI row. Native
addresses/padding are deliberately not a persisted byte format. Required
agreement: draft.md section15, language-spec, design-notes, Settled
open-questions, implementation boundary ledger17, HIR ledger19, roadmap and
English/Japanese unsafe/FFI guides. Status is recorded once in HANDOFF at the
capability boundary. Request32 receives a partial provider answer and PR;
consumer verification stays external and its register edit remains uncommitted.

## Independent plan review

The review found one P1 in the newly admitted Raw × align(16) × existing SysV
domain: padding-only eightbytes currently consume an extra INTEGER register,
shifting following arguments. The ledger and matrix now separate meaningful
register classes from complete storage size, require the same correction for
integer/float siblings, and own following arguments, returns and pressure.
Author inspection separately found the missing Raw leaf in TypeLayoutCache
and the independent checked-HIR FFI struct predicate; both are included above.

Scoped continuation accepted the revised ledger/matrix as CLEAN before implementation.

## Author implementation closure

Sema and the two checked-HIR admission paths share is_ffi_safe; the ordinary
field gate has only the concrete C/raw exception. TypeLayoutCache gives Raw its
existing pointer-sized leaf. The SysV classifier drops NO_CLASS tail padding
and retains storage_eightbytes for both argument and return scratch. No IR,
runtime signature, pointee ownership or serialization tag changes.

The c_layout_raw_fields parent owns every native helper, linker, execution and
artifact through a bounded process group; its exited-leader and unwind owner
qualifies retirement. Whole/per-unit execution checks unaligned C storage in
four field/alignment shapes, pointer identity, null/replacement/copy, ordinary
wrappers and fixed arrays, every selected control alternative and cache
cold/warm/type-change/restore. Source negatives retain unsafe and other field/
collection restrictions. The HIR field mutation owner removes externs when
testing field-placement rejection, so an unrelated FFI-signature error cannot
mask an accepted invalid field. The valid raw extern record separately owns
that sibling. The target-independent x86 codegen owner checks Raw/int/float
layout, occupied register classes, every coercion slot size and GP/SSE pressure.

Local native SysV qualification additionally links the production-generated
x86_64 Linux LLVM with a Clang22 C helper and runs the ELF executable under
Docker linux/amd64. It covers following arguments, returned records and exact
GP/SSE fit; the checked-in native driver owner repeats the equivalent calls
on a Linux x86-64 host. This is correctness evidence, not a performance claim.
