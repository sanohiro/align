# Host observation

Status: O1/R66 implementation candidate for the capability selected in plan 40.
This capability is independent of retained filesystem and contained-process work.

## Public-contract ledger

| Surface | Exact contract |
| --- | --- |
| `os.host() -> Result<os.host_info, Error>` | Requires `import std.os`; no arguments, defaults or configuration. Impure. Query the current process's OS view using uname, then the online logical CPU query. Return an independently owned record. No subprocess, environment read, distribution-file parsing, CPU-model inference, cache or process-global mutation. |
| `os.host_info` | Qualified-only natural-layout record, with fields in declaration order: `system: string`, `release: string`, `machine: string`, `cpu: Option<string>`, `logical_cpu_count: Option<i64>`. Ordinary Move ownership, construction, partial-field moves and recursive Drop. No separate host-handle type, reflection or inspection lifetime. The existing admissible record/collection/carrier rules apply. |
| `system`, `release`, `machine` | Exact uname sysname, release and machine text, respectively, with no case folding, architecture aliases or distribution inference. All three must be NUL-terminated within their native field capacity, nonempty and valid UTF-8. Strip only the native terminator and ignore padding after it. Missing terminator, empty text or invalid UTF-8 is Error.Invalid. No lossy conversion. |
| `cpu` | Always None in this capability on Linux/macOS; absence means no CPU description is supplied. Do not substitute machine architecture. The type permits a future separately specified observation without making this implementation promise one. |
| `logical_cpu_count` | `sysconf(_SC_NPROCESSORS_ONLN)` on Linux/macOS. A strictly positive representable count is Some(i64); unavailable, nonpositive or unrepresentable results are None. This is the online-count query visible to the process, not a physical-machine guarantee, affinity/quota adjustment, core count or `process.cpu_count()` alias. Optional query failure does not discard successful uname fields. |
| Failures and ownership | Uname failure uses the existing fixed errno-to-Error table. Validate mandatory native fields in order system/release/machine before the optional count query or text allocation. OOM is the existing immediate-abort policy, with no recoverable partial successful record. On recoverable failure, no text allocation or partial owner escapes. |

Supported native platforms are the existing 64-bit Linux and macOS targets.
An unsupported platform returns Error.Invalid without fabricated identity. Tests
must distinguish refusal from a successful observation. No platform-dependent
field omission: every successful record has all five fields in the same order.

The observation is neither an authenticated machine identity nor an atomic
snapshot with process.cpu_count, application identity, filesystem state or later
queries. The consumer owns its environment schema version, interpretation and
digest; replacing Python observations requires an explicit new consumer schema.
No consumer code is part of this Align capability.

## Native ABI and exact layout

`OsHost` selects `align_rt_os_host`, reusing A03: `i32 (ptr)`, with no extra
LLVM attributes. The pointer addresses one writable, aligned, exclusive HostInfo
scratch record; return zero means success and nonzero is the existing runtime
status encoding. This scratch is compiler-owned, never a source raw pointer.

For 64-bit targets the native record is exactly 88 bytes, alignment 8:

| Offset | Native field |
| --- | --- |
| 0 | system `{ptr,i64}` |
| 16 | release `{ptr,i64}` |
| 32 | machine `{ptr,i64}` |
| 48 | cpu Option tag `u8` (zero for None), followed by seven zero padding bytes |
| 56 | cpu dormant string payload `{ptr,i64}` (null, zero) |
| 72 | logical_cpu_count Option tag `u8` (zero None, one Some), followed by seven zero padding bytes |
| 80 | logical_cpu_count `i64` payload (zero for None, positive for Some) |

The runtime checks a null pointer, alignment and pointer-plus-88 overflow before
writing the entire empty record or observing OS state. Non-null dangling or
undersized native allocations violate the normal ABI precondition. After all
mandatory field validations, query the optional count, allocate ordinary owned
strings, and publish the complete record. Error paths leave the initialized
empty scratch and never publish an Ok value. No source-visible allocation is
region-backed, including calls inside arenas. Dropping the ordinary success
record releases exactly its three returned strings; CPU has no allocation.

The public field types and layout are producer-independent checked-HIR and MIR
schema obligations, not facts trusted merely because a slot names a struct.
The runtime repr(C) size/alignment/offset owner and independent LLVM layout owner
must agree on every field, including dormant Option payloads.

## Compiler identity and closure matrix

This adds one builtin nominal record plus one HIR/MIR native operation, not a new
Ty/Scalar ownership class or new canonical scalar tag. Existing canonical graph
serialization records the complete nominal definition and ABI. Interfaces must
reconstruct the std.os import and exact reserved record identity; source aliases
must not substitute a lookalike user record. A malformed native producer with a
wrong name, order, width, ownership field, optional payload, result or output-slot
type rejects before emission. Update runtime key ordering, declaration golden,
export inventory and compiler/runtime fingerprints together.

| Axis | Required owner |
| --- | --- |
| Source formation | `align_driver --test m11_os_host::host_formation`: arity, import, qualified-only identity, user construction including `cpu: Some(string)`, and field types. |
| Actual observation | `m11_os_host::host_observation`: independent uname/sysconf oracle on Linux/macOS, CPU None and count availability. |
| Native failure and encoding | `align_runtime::os_host::tests::host_validation_order`: injected uname failure, malformed terminators/UTF-8/empty fields, multi-invalid precedence and allocation-free mandatory validation; `host_optional_count` covers absent/nonpositive/positive counts. |
| Ownership/cleanup | `m11_os_host::host_owned_control_flow`: whole/per-unit, imported signature, arena return, partial moves and source nulling, record/Option/Result/generic transport, replacement, early exits, positive allocation witness and final balance with omitted-Drop negative control. Include a user-constructed Some(cpu) owner. |
| Control and purity | `m11_os_host::host_owned_control_flow` covers if/match/else/?/map_err/loop joins; `host_formation` rejects impure parallel calls. |
| Native ABI | Runtime `os_host::tests::host_native_layout` and `host_invalid_out`; LLVM `host_layout_matches_native_contract`; runtime declaration golden owner and `scripts/test-runtime-abi-exports.sh`. |
| HIR/MIR | `align_mir::validate_hir_tests::host_schema_rejects_malformed_records` and LLVM `host_mir_gate_rejects_forged_schema`: parameterized schema/result/out-slot mutations across entrypoints, including malformed builtin records in programs with no OsHost producer. |
| Cache/interface | `m11_os_host::host_interfaces_and_cache`: imported nominal record, cold/hit compilation, and canonical graph reconstruction. |

No performance/resource benchmark is required: this is a fixed observation,
with no latency, throughput or process-RSS promise. Counts prove ownership,
not performance. One complete capability avoids a builtin record whose native
producer or Drop path is still dormant. The anticipated cross-layer diff is
about 800–1,300 handwritten lines; keeping schema, out-slot proof and consumer
together avoids duplicating the exact native layout proof.

Required source-of-truth propagation: draft.md, docs/language-spec.md,
docs/design-notes.md, Settled in docs/open-questions.md, std-design/os.md and its
Japanese mirror, HIR/runtime ledgers, module/design indexes that enumerate std,
and plan 40's O1 boundary. Record status once at capability completion and
write Align's answer to the sibling request register uncommitted.

Author pass must close all mandatory/optional success/failure combinations,
validation order, native layout, exact type/import identity and allocation
provenance before independent design review and before production code.

Author ledger-to-prose pass (2026-09-10): all five public fields, the mandatory
validation order and optional-state product are fixed above. The native Option
tag is u8, as verified against option_struct_type; seven explicit zero padding
bytes preserve the 8-byte payload alignment. HIR OsHost has no children and
returns the exact builtin Result; MIR OsHost owns one typed scratch out-slot and
returns i32 status before ordinary Result construction. Schema certification
checks the owning builtin nominal identity plus every field recursively; it
cannot infer trust from the output slot's asserted type. No new type/scalar tags
are needed. The one new ordered RuntimeKey changes 365/383 to 366 keyed/384 base
records, after R29. The ledger passed independent inspection before production implementation.

Independent inspection (2026-09-10): the reviewer found one P2 closure-owner naming gap. The matrix now names every target and parameterized owner and explicitly includes user-constructed Some(cpu), source nulling, and malformed records without a host producer. No public contract or strategy changed.

## Implementation closure

The exact owners in the matrix are implemented. Native size/alignment/offsets and
independent LLVM physical offsets pass; native error/encoding/optional-count
injection passes. Malformed schema owners exercise programs both with and without
an OsHost producer. Whole/per-unit driver owners pass actual uname/sysconf parity,
ordinary Some(cpu) construction, field moves, arena return, joins, early exits and
imported generic transport. The allocation counter witness requires at least 25
allocations and final allocation/free balance; removing the host Drop in the early
return function changes both compilation modes to failure. Cold/hit ThinLTO
compilation verifies record reconstruction and cache reuse. No performance claim
or benchmark is added.

Author matrix-to-diff pass: every native result leaf is authenticated against the
reserved schema before nested owned-field certification. The interface's builtin
ownership bridge derives Move/return-cleanup from that same ordinary record schema.
FreshEmpty provenance and individual allocation preserve arena-return ownership.
Nullary HIR sweeps cover purity, replay, depth, movement and storage. No new type or
scalar class widens existing carrier rules. Native pointers are checked before
writing; mandatory fields validate before the optional query or text allocation.
The native out scratch is initialized empty on recoverable failure. Exact runtime
registry membership, declarations and export totals advance together.
