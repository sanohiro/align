# SysV MEMORY C-record value passage

Status: implemented capability after plan138. Request32 demonstrates a 24-byte
native context argument and native record result. ARM64 now supports the full
existing flat C-record field domain. This capability removes Linux x86-64's
rejection of large records and small records whose argument registers are
exhausted, completing that SysV ABI domain. Bool/char FFI, native callbacks, variadics, consumer adoption
and K1/plan61 remain outside this boundary.

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

| Surface | Exact contract |
| --- | --- |
| Admission | Concrete, nonempty `layout(C)` records with existing naturally aligned integer i8/u8/i16/u16/i32/u32/i64/u64, f32/f64 and raw fields. No new field, type, annotation, default or syntax. Generic C records, nested records, bool/char and Move fields retain existing source and checked-HIR rejection. |
| Target | Little-endian LP64 Linux x86-64 SysV: x86_64 architecture, linux OS, gnu or musl environment, any vendor, 64-bit pointer layout. Reject x32/ILP32, Windows, x86-64 Darwin, Android, unknown environments and unrelated targets before any LLVM function declaration. Existing plan138 ARM64 targets and scalar-only externs retain their rules. |
| Signature and effects | Ordinary extern declarations and positional calls remain unsafe and impure. Small register records remain accepted. Larger records and small register-exhausted arguments become accepted. The complete argument/result plan applies identically to whole-program and imported/per-unit compilation. |
| Ownership and storage | Records remain Copy and raw fields remain non-owning pointer bits. A memory argument passes a complete by-value object copy; C writes cannot change the source or another live copy. Each call argument has independent aligned source scratch, and every memory result has separate caller-owned storage. Full padded storage and coercion width are covered. All scratch is entry-hoisted and stack-owned; no heap allocation, Drop, source nulling, pointee copy/free, lifetime extension, Result error or allocation policy is added. Padding contents remain unspecified. |
| Physical plan | The table below fixes classification, full storage, byval/sret attributes and physical ordinals. Both size-class MEMORY and register-exhaustion MEMORY use the same typed byval path. A byval argument consumes no GP/SSE register; hidden sret consumes the first GP register. Register assignment rolls back the entire aggregate if either class cannot fit. |
| Native compatibility and errors | Existing source/HIR validation precedes lowering. Reject fixed runtime symbols redeclared with source record values, including otherwise coincidentally equal pointer types, before emitting any LLVM function. Registered runtime rows retain their scalar/view type and attribute authority. Unsupported targets or unrepresentable checked storage produce diagnostics, not partial declarations or panics. No runtime row or native export is added. |
| Interface and cache | No new HIR/MIR variant, interface tag or persisted format. Existing nominal record identity, full field/alignment definitions, extern signatures, target identity and compiler identity govern caches. The same foreign symbol with changed imported alignment must miss and execute its new ABI, while unchanged warm and restored definitions preserve correctness. |
| Limits and prerequisites | Uses plan137 raw records and the shared extern attribute plan from plan138. No exported Align C function, callback, C++ object, packed/unaligned member, vector, variadic, cross-compilation CLI or additional OS ABI. Bool/char FFI and consumer adoption remain separate. No performance/resource improvement promise; no benchmark gate. |

## Physical plan

Let S be the complete native padded size, A the maximum of natural alignment and
explicit align(N), and B=max(A,8). The admitted field domain has only INTEGER,
SSE and padding-only NO_CLASS eightbytes. Every field occupies its natural
location; padding-only tail eightbytes consume no registers. S>16 is MEMORY.

| Position and class | LLVM plan and obligation |
| --- | --- |
| Register result, S<=16 | Existing byte-compatible per-occupied-eightbyte coercion: i64 for INTEGER, double for SSE; a single scalar or two-member result aggregate. No argument register pressure changes a result's class. Scratch retains the complete padded object. |
| Memory result, S>16 | void result with first physical pointer parameter carrying typed sret(record) and align(A). It precedes every source parameter and consumes one of six GP argument registers. Result scratch is at least S bytes, aligned at least A, distinct from all argument scratch. |
| Register argument, S<=16 and every class fits | Existing flattened occupied-eightbyte operands. Consume only the required GP and SSE counts. Six GP and eight SSE registers are available before the hidden-result adjustment. |
| Memory argument, S>16 or either required class does not fit | One pointer with typed byval(record) and align(B), on both declaration and call at the same physical ordinal. Scratch contains the complete padded source value and is aligned at least B. LLVM copies it to the ABI stack area. This pointer consumes no argument register. Never use ARM64's ordinary caller-copy pointer convention here. |
| Remaining parameters | Scalars retain their existing direct LLVM types; str/slice views retain their one data-pointer ABI and count as one GP argument. A spilled scalar saturates its register class at the maximum. Following aggregates still use all remaining registers of both classes. |

Class accounting follows source order after reserving hidden sret. A failed
aggregate consumes neither class: e.g. five GP scalars then a two-GP record
leaves the sixth GP register available for a following one-GP record/scalar.
An exhausted SSE class must not consume the GP half of a mixed record, and the
converse also holds. Already-MEMORY records do not consume either class.
Attribute ordinals count physical flattened arguments and the hidden result,
not source indices. One stored plan supplies declarations and calls.

The uniform typed byval form also handles single-eightbyte exhausted records.
Clang may coerce such a stack argument to an integer when no GP register is
left; that optimization is not needed for ABI compatibility. Native execution
must cover both GP-free and GP-exhausted single-SSE spill cases.

## Implementation closure matrix

| Axis | Implementation and exact acceptance owner |
| --- | --- |
| Formation, malformed input and target closure | Reuse source/checked-HIR raw/C-layout/unsafe formation owners. Add a private checked SysV memory-storage plan and closed target classifier. A target-independent codegen owner crosses GNU/musl, x32, wrong architecture/OS/environment, pointer width and endian; malformed storage metadata yields a diagnostic. Actual refused TargetMachine emission leaves zero function declarations. |
| Size, alignment and full copy | Native parameterized C owner crosses sizes 1/3/8/12/16/17/24/32/40, integer widths/signs, packed f32 eightbytes, mixed integer/SSE/raw, raw first/middle/last, padding-only alignment16 and MEMORY alignment32/64/4096. C checks every field, two copies of one source, unchanged source after mutation, static pointee identity and actual parameter alignment. LLVM owner asserts full scratch sizes/alignments and typed byval/sret. |
| Register exhaustion and rollback | Cross GP and SSE families at zero, exact fit, first spill, full and already-exhausted budgets. Cover INTEGER/INTEGER, SSE/SSE, INTEGER/SSE and SSE/INTEGER; mix opposite-class pressure. Follow each failed aggregate with a scalar and smaller record in the still-free class. Include large byval before register records. Native values and LLVM exact parameter/attribute ordinals must both prove all-or-nothing rollback. |
| Hidden result and siblings | Cross large returns with small register arguments at adjusted GP exact-fit/first-spill and normal SSE boundaries, preceding/following scalar/view arguments, multiple records and large memory arguments. Native code checks returned fields and source preservation. LLVM checks sret at ordinal zero, exact byval ordinals after flattening and no byval on ARM64 indirect arguments. Existing native scalar/runtime-row owners retain positive coverage. |
| Construction, copy, replacement, joins and returns | Existing plan137 whole/per-unit records exercise if/match/else/?/map_err, loop joins, early return and generic identity. Reuse that owner and add actual large foreign returns used through a generic imported helper. Copy has no Drop or source-null action. Caller source, result and second argument storage remain independent. |
| Loop and allocation lifetime | Native repeated calls to large/over-aligned records preserve exact call counts and values. A source-loop codegen owner asserts all scratch is in the entry block and distinct for every static argument/result. No dynamic alloca, heap allocator or runtime ownership path is introduced. |
| Native preflight | Codegen owner crosses byval pointer-looking arguments, hidden sret with no source arguments, and direct register records redeclaring fixed runtime symbols. Reject before any function emission; preserve compatible scalar/native declaration and call attribute owners. |
| Generic, serialization and cache | Bounded native harness compiles imported concrete records and generic returns through whole/per-unit paths. Same-symbol alignment16 -> alignment32 -> alignment16 cycle changes register/byval/sret form and checks native results, cold/warm misses/hits and actual metadata. No persisted schema changes; existing target-cache identity owner remains. |
| Platform parity and resources | Linux x86-64 real C executions are required locally before push, plus target-independent production LLVM verification. On an ARM host, use a bounded temporary x86 ELF generated by the actual production build_module path and execute it in local x86 Docker emulation; Nightly CI runs the native host owner; local qualification also executes actual imported/per-unit outputs before push. Keep generated C/Align inputs identical to the checked-in native matrix. Reuse the C-layout owner's ArtifactStage, armed process-group deadline and exited-leader/unwind cleanup. ARM64 siblings retain their native owner. |
| Final gates | Replace previous MEMORY rejection owners with positive native assertions; keep unsupported-target and malformed-formation negatives. Run relevant codegen/ffi_byval/C-layout owners, self-review, one independent code review, final preflight and locally required DB verification. After merge run the literal release workspace build, update only the external request register and clean owned artifacts. |

## Author source and boundary pass

Independent Clang22 probes confirm typed byval alignment max(A,8), sret
alignment A, the hidden-result GP adjustment, mixed-class rollback and
padding-only tails. A 17-byte byte-field record specifically distinguishes
sret align(1) from byval align(8). The SysV psABI and Clang22 source provide
the primary reference rules:

- https://gitlab.com/x86-psABIs/x86-64-ABI/-/blob/master/x86-64-ABI/low-level-sys-info.tex
- https://github.com/llvm/llvm-project/blob/llvmorg-22.1.8/clang/lib/CodeGen/Targets/X86.cpp

Required public agreement: draft.md section15, language-spec, design-notes,
Settled/open FFI entries, roadmap and English/Japanese unsafe/FFI guide.
Plan137/138 historical exclusions point to this subsequent capability without
claiming it belonged to their shipped boundaries. HIR/runtime ledgers retain
unchanged normative contracts. HANDOFF records one implemented boundary.

One PR connects MEMORY classification, rollback, typed attributes, full aligned
storage and result reconstruction. Splitting large records from exhausted small
records would leave the same byval mechanism with two sets of proof and an
unnecessary partial ABI. If the complete native matrix and exact LLVM owner
take the change beyond 1,000 handwritten lines, retain this useful capability
boundary: shared source generation and storage planning avoid duplicated proof
and integration risk.

Author ledger-to-prose pass: every admitted target/shape/position, register
budget, attribute ordinal, storage and identity rule is enumerated. Existing
control/formation owners close unchanged semantics; explicit native and LLVM
owners close new obligations. No wire format, text/native encoding, public
inspection table, ambient option or process-global operation is added. A fresh
independent adversarial review must accept this plan before implementation.

## Independent plan review

Fresh independent adversarial inspection accepted this contract, closure matrix
and capability boundary without P1/P2 findings before implementation. The author
clarified that the native driver owner belongs to nightly CI; local cross-target
qualification must also execute actual imported/per-unit outputs before push.

## Author implementation closure

`SysvMemory::new` validates full storage and alignment. `assign_sysv_struct_args`
reserves hidden sret, saturates scalar budgets and replaces an entire exhausted
record with the common byval plan without charging either class. The shared
extern attribute vector supplies exact declaration/call ordinals; each memory
argument/result has its own full aligned entry slot. All supported source record
redeclarations of fixed native rows fail before any LLVM function is emitted.

`ffi_sysv_tests::sysv_record_target_domain_is_closed` and
`sysv_memory_types_attributes_storage_and_native_preflight` own target refusal,
19 independent physical-signature goldens, typed byval/sret size/alignment and
call parity, distinct entry scratch, oversized/malformed metadata and native
symbol preflight. The latter compiles the same pure `ffi_sysv_cases` source
matrix as the bounded `c_layout_raw_fields` native owner. Its 33 record shapes,
class-relative pressure cases, sret-adjusted boundaries, following smaller
records, view argument, large-before-small argument and repeated aligned calls
execute 4,336 native calls in each whole/per-unit program. The existing imported
control and generic owners remain; shared cache preparation now serves both
ARM64 and SysV without changing the ARM64 cases.

Local production-generated x86 ELF executed every whole/per-unit case and the
four same-symbol alignment cache rounds against independently compiled Clang22 C.
All six executables returned zero. Temporary target/export instrumentation was
removed with exact hash restoration before final checks. The native driver owner
remains a nightly detector, not a new PR CI requirement. Mutation controls detect
omitted hidden-sret GP accounting and under-aligned byval passage; both mutations
were restored. No new IR/interface/runtime row or allocation owner was introduced.
