# OS memory observation

Status: **IMPLEMENTED — R50.**

The existing consumer's resident-weight preflight needs RAM observations inside
ordinary Align. This belongs to std.os alongside host and identity observations.
Keep model placement, allocation admission, GPU accounting and consumer adoption
application-owned. This capability is independent of deferred K1 and plan 61.

## Public-contract ledger

| Surface | Exact contract |
| --- | --- |
| Signatures | `os.physical_memory() -> Result<i64, Error>` and `os.available_memory() -> Result<i64, Error>`. Require `import std.os`; nullary direct builtin calls; Impure. No arguments, defaults, environment configuration or alternative API. |
| Result | Byte counts in signed i64. Physical success is strictly positive; available success is nonnegative and no greater than the total queried within that operation. Each call is independent; there is no cross-call atomic snapshot. Results are ordinary Copy values with no owner, borrowed lifetime, Drop, heap payload or retained native handle. |
| Linux/WSL2 total | Read the current process's `/proc/meminfo` view. `MemTotal` in kernel `kB` units times 1024, with checked arithmetic. This is the kernel's usable RAM total, including the guest's view under WSL2/virtualization; it does not enumerate installed hardware or apply cgroup/rlimit constraints. |
| Linux/WSL2 available | Query the same file for both MemTotal and MemAvailable. Validate total first, then available, then available <= total. Use the kernel's MemAvailable estimate, without a fallback to MemFree or a userspace estimate. Missing/duplicate/malformed required fields or out-of-range counts return Invalid. Physical-only queries ignore MemAvailable. |
| Linux native admission | One fixed native pathname, no caller path. O_RDONLY|O_CLOEXEC; one owned descriptor, closed once by ordinary RAII after acquisition on every exit. Read at most 8192 bytes plus a one-byte overflow probe into fixed stack storage, retrying EINTR. Any read/open failure precedes parsing and uses the existing errno mapping; a ninth KiB returns Invalid. EOF at 8192 is accepted. No file-size allocation, process launch, cache or language-source/configuration I/O. |
| Linux field grammar | Split bytes at LF; a final unterminated line is allowed. Required keys start at column zero and are exactly `MemTotal:` / `MemAvailable:`. After the colon require at least one SP or TAB, one or more decimal ASCII digits, at least one SP or TAB, exact `kB`, then zero or more SP/TAB and optional CR. No sign, internal digit whitespace, Unicode, embedded NUL or other suffix is accepted in a required field. Unknown lines are ignored as bytes. Scan the entire bounded file; duplicate required keys reject regardless of position. For available queries, total validation precedes available validation regardless of line order. |
| macOS total | Query `sysctlbyname("hw.memsize")` into initialized u64 scratch, requiring exactly eight returned bytes and a positive i64-representable value. Native sysctl failures use the existing errno mapping. No fallback, subprocess, search or environment input. |
| macOS available | First query/validate total. Acquire one host send right via mach_host_self; query host_page_size, then HOST_VM_INFO64 into the libc native revision's initialized structure. Require successful queries, positive page size, and the exact requested structure word count. Compute `(free_count + inactive_count) * page_size` with checked addition/multiplication and i64 conversion; reject a result above queried total. This is Align's explicit free-plus-inactive advisory estimate, not Linux MemAvailable or an immediately allocatable/reservation guarantee. Speculative pages are already included in free_count; do not add speculative or purgeable counts again. |
| macOS cleanup/errors | Arm a right guard immediately after valid acquisition. Release exactly one acquired host send-right reference via mach_port_deallocate using the borrowed task-self port, including failed page/statistics queries. Query error wins over release error; otherwise release failure rejects before returning data. Mach failures, invalid port/count/page size and arithmetic/semantic rejection are Invalid; do not reinterpret Mach status as POSIX errno. Release precedes arithmetic validation. No task-self deallocation, cached port or process-global native mutation. |
| Unsupported platforms | Return Invalid without a fabricated successful zero. The supported qualification targets are Linux x86_64/ARM64 and macOS ARM64; native macOS x86_64 compilation must use the same declared native ABI. |
| Limits | Advisory OS observations do not reserve or commit RAM, guarantee allocation success, report VRAM/per-model usage, enforce a budget, adjust for cgroups/address-space limits, or certify a snapshot. A container can see kernel/VM totals above its allocation limit. Consumers retain explicit caps and checked buffer capacity; fallible allocation remains deferred by plan 87. |
| Allocation/effects | No Align/Rust heap allocation on native query paths. Linux uses 8193 stack bytes; macOS uses fixed native scalar/statistics/message storage. Native OS/kernel/provider storage is outside this claim. Calls may wait for native OS operations; the byte cap is not a time budget. No benchmark or latency/throughput/RSS promise. Overlapping calls have independent scratch/descriptor/right owners and no exclusion requirement. |
| Compiler/native owner | Two HIR/MIR nullary selectors reuse ordinary Result<i64,Error> formation and cleanup. One MIR producer family has a compile-time selector and an i64 output slot. Two runtime keys select `align_rt_os_physical_memory` / `align_rt_os_available_memory`, both existing A03 `i32(ptr)` status/output ABI. No new scalar, record, Error, native dependency or generic/ownership rule. |
| Native output | Output is fresh exclusive writable eight-byte scratch aligned to eight. Check null, alignment and address-plus-eight overflow before access/query; invalid address geometry leaves output untouched. Otherwise initialize zero before querying, return status zero and publish the count only after validation/cleanup. Every nonzero status leaves zero. Dangling/undersized native storage violates the existing ABI precondition. No native pointer is retained or returned. |
| Artifact/cache identity | Existing compiler/runtime source fingerprints and runtime-key/declaration inventories change together. No new persistent/wire format, serialized type tag, runtime cache or consumer schema. Existing interface function bodies reconstruct the same nullary operations and impurity facts. |
| Prerequisites/boundary | Existing std.os, i64 Result status/output lowering, native OS calls, checked HIR/MIR validation, whole/per-unit compilation and runtime ABI table are shipped. Both operations, native queries and validators ship together as one usable capability. Expected cross-layer implementation/tests may exceed 1000 handwritten lines: splitting native production from typed validation would duplicate the ABI proof and leave no useful stable consumer. |
| Source agreement | This ledger owns the exact contract. Propagate the public summary to draft.md, docs/language-spec.md, docs/design-notes.md, Settled in docs/open-questions.md, std-design/os.md and ja/os.md. Update affected HIR/runtime ledgers and ABI/export goldens; guide18 English/ja gets one syntax-checked example. Record one capability status in HANDOFF at completion, and the Align answer in the external R50 register uncommitted. No consumer code/pin/adoption changes. |

## Implementation closure matrix

| Invariant | Implementation and discriminating owner |
| --- | --- |
| Formation, arity, import and impurity | Existing std.os dispatch gains the two nullary calls. `m11_os_memory` parameterizes both selectors over import/arity/result typing and impure parallel refusal; wrong arity produces no typed native operation. |
| Scalar construction/transport | Both return ordinary Copy Result<i64,Error>, requiring no new allocation/Drop/move rule. `m11_os_memory` executes direct/imported/generic callers, if/match/else/?/map_err/loop/return and arena contexts through whole/per-unit compilation. Source owners retain both success and error arms; no resource view or region carrier exists. |
| Linux grammar/ranges/precedence | `os_memory::tests` runs both selectors over independent meminfo byte vectors: zero/one/max, KiB conversion, overflow, missing/duplicate/reversed/unknown fields, every whitespace/unit/sign/NUL case, available above total and multi-invalid order. Mutating the required-unit or bound check must fail its parameterized owner. |
| Bounded reads and descriptor cleanup | Injected native read/open seam covers open refusal, acquired descriptor with short read, EINTR, read refusal after a valid prefix, 8192 EOF and overflow probe. Count reads/close and assert no query/publication after failure. Real Linux query owner independently reads /proc and verifies total plus available range; constrained container confirms the declared host-view scope. |
| macOS query/cleanup/units | Injected native ops cover sysctl failure/length/value, invalid host port, page query error/zero, statistics error/count, right-release failure, checked page arithmetic, total bound and multi-invalid precedence. Count every acquire/release and query; omission/double release and duplicate speculative pages must fail their owner. Independent C/SDK native observation verifies total, declared VM revision and kernel page size on macOS; live results require range/page-unit checks, not equality or temporal bracketing of changing counts. Exact synthetic vectors own arithmetic independent of live changes. |
| Allocation/ownership | Feature-enabled native allocation owner has a positive allocating control, then checks native success/error query paths have zero Align/Rust allocator calls. The same owner runs on Linux/macOS. Native provider/kernel work is excluded; no process-RSS benchmark. |
| HIR formation/validation | Gate-1 nullary-operation sweeps include the new family. `validate_hir_tests` rejects wrong output Result success/error type and malformed nested producers for both selectors through all checked-HIR ingress owners. Existing no-operand walks preserve effects, region/escape, generic monomorphization and interface serialization. |
| MIR/native ABI | MIR gate rejects wrong result-status and out-slot scalar width/shape before LLVM for both selectors. Runtime exact-output geometry/zero-on-failure owner and independent LLVM call declaration/out-slot owner cover A03 i32(ptr)/eight-byte scratch. Runtime ABI declaration golden, effect table, export inventory and existing callable/callback certifier remain synchronized. |
| Cache/interfaces | `m11_os_memory` checks imported wrappers and compiler cache cold/hit source parity; private type/lookalike shadows cannot substitute scalar/error identity. No serialized type-graph addition. |
| Existing behavior | Existing `m11_os_host` / `m11_os_identity` keep their owner tests; no host/identity record or native ABI changes. Owner tests plus bounded PR gate and library/binary Clippy run locally before publication; CI is the platform guard. |

## Primary platform evidence

[Linux proc documentation](https://www.kernel.org/doc/html/latest/filesystems/proc.html)
defines MemTotal and the kernel MemAvailable estimate. WSL2 sees its Linux VM's
kernel view. Cgroup-relative allocation admission is a distinct contract.
[Apple VM statistics](https://github.com/apple-oss-distributions/xnu/blob/main/osfmk/mach/vm_statistics.h)
defines free/inactive counts and includes speculative pages in free count.
[Apple host-self contract](https://github.com/apple-oss-distributions/xnu/blob/main/osfmk/man/mach_host_self.html)
returns a send right; it has one release owner. The macOS free-plus-inactive
formula is an explicit Align estimate, not a claimed OS MemAvailable field.

## Author consistency pass and review boundary

Before production changes, verify every row above against the public summaries,
native API width/ownership evidence and planned owners. Exhaust both selectors,
supported/unsupported platform, successful/failed query and output geometry;
there are no optional fields, response detail modes or user options. Keep input
grammar and validation/cleanup order in this one ledger. Syntax-check the guide
example using temporary declarations for the proposed functions until implemented.
Request one fresh independent adversarial design/matrix review, resolve its whole
finding set, then implement. The final committed capability gets one fresh full-
diff preflight review. Do not use review as the primary design-completion loop.

Author ledger-to-prose/matrix pass (2026-10-05): the two public calls, all native
inputs, range/grammar/error ordering, independent ownership and exact output
ABI appear in the ledger and public English/ja summaries. No optional field,
wire format, cache mode or user-option product exists. Gate-1/ABI owners are
named before adding IR. The guide example parses through alignc fmt. Local
SDK C queries qualify native total, host page size, VM counts and successful
send-right release; a second C query requests SDK REV1 (38 words), matching
libc 0.2.186's 152-byte structure, and receives exactly 38. No production
implementation or timing claim exists yet. Evidence is retained outside the
repository in align-os-memory-evidence.

Independent adversarial design/matrix review (2026-10-05): CLEAN. Public/native
contract, platform scope and complete output/compiler/cleanup boundaries inspected
before production changes. Live C and Align observations are separate samples;
only synthetic vectors assert exact page-count arithmetic.


## Implemented closure

`align_runtime::os_memory` owns both query paths, checked byte/page arithmetic,
Linux descriptor and Mac send-right cleanup, and exact output admission.
`os_memory_tests` closes grammar, multi-invalid precedence, bounded I/O,
EINTR/failed acquisition, query/release ordering, VM revision/units, output
publication and actual allocation parity (with a positive allocating control).
Fake Mac inspection storage is reserved before its allocation measurement.
Live native observations are qualified against independent proc/sysctl and
SDK C queries without equating fluctuating available values.

`ExprKind::OsMemory` is explicit in purity, region, replay, depth, storage and
checked-HIR walks; its Copy scalar payload has no source owner to null.
`Rvalue::OsMemory` shares status/Result lowering and authenticates i32 status
plus exclusive i64 scratch through the native producer contract. Both keyed
A03 records have non-retaining output effects; the existing machinery derives
captures(none), without nonnull/read-only/output alias promises.

`m11_os_memory` qualifies direct/imported/generic calls, scalar Copy transport,
replacement, if/match/else/?/map_err/loop/return/arena, invalid formation and
imported impurity through whole/per-unit checking and executable compilation.
Its independent IR owner pins i32 calls to eight-byte alignment-eight output;
its cold/hit object owner proves cached compilation preserves the callable
surface. Forged checked-HIR Result/error/width and MIR status/output owners
reject before lowering. Existing host/identity owners, declaration/effect
and native source/export inventories cover surrounding behavior. Guide examples
and both language/library mirrors use the same public signatures and limits.

The public source ledgers agree with this record. Native allocation counts
qualify the only resource promise; no time/RSS benchmark or stronger memory
admission claim is made. The complete native-to-typed consumer boundary ships
in one capability, as justified above; no producer-only path remains dormant.
