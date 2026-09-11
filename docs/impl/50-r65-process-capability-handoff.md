# R65: process capability implementation handoff

The [R82 shared carrier](56-r77-r83-composition-plan.md) implementation composes
an optional retained namespace with this plan's existing shared inheritance
operation. It changes no constructor, descriptor transaction, platform contract
or native ABI.

Status: **IMPLEMENTING — common and signal capabilities implemented; verified launch acceptance in progress**.
Baseline: Align `4cb14895a06e67f32ee72383b53afe72cd8e5555`.
This is the plan of record for completing R65. It selects the scope and remaining
native mechanisms; [plan 49](49-native-process-contract.md) owns the exact common
process operations. Plans 40, 46 and 48 are assessment/qualification history where
they conflict with this record. Capability order and acceptance remain defined below.

## 1. Decision and request coverage

Align provides composable OS operations, typed authority and deterministic
ownership. Application Align code provides supervision, resource comparisons,
first-error retention and workspace policy. An existing external sandbox remains
an explicit application-selected executable, argument list and namespace input.
The standard library neither implements that sandbox nor adds an installed helper,
privileged service, dedicated user, VM, background reaper or hidden keeper.

Native Linux and macOS both implement the common process workflow. Linux-only
kernel capabilities retain explicit Linux-only contracts: their typed operations
are available to type checking on both hosts, but return Error.Code(ENOTSUP) on
macOS before acquiring resources or starting a process. The native ENOTSUP value
is used, as with other errno errors; there is no new core Error variant. This is
the supported-platform decision, not an unfinished Mac backend or a refusal of
ordinary Mac process execution. It follows the actual consumer mapping in plan
48; the Python sealed-input/subreaper paths are already Linux-specific.

| R65 requirement | Selected capability / acceptance |
| --- | --- |
| Live bytes, EOF, timeout after EOF, exit versus signal, maximum RSS, cancellation, file output | Common Linux/macOS operations and exact ledger in plan 49. Application loops choose policy. |
| Verify the bytes later supplied to a child | Explicit streaming memory-file construction and OS sealing; hash the sealed file with caller scratch and existing crypto.digest; inherit the same object. |
| Hash and execute the same admitted object | Executable-kind sealed file, native ELF admission, command_image and fd-selected exec; no later path lookup. |
| Bind observation to the running producer | current_image opens the OS's running-main-image object, not a caller pathname; no claim that this alone attests libraries or source provenance. |
| Controlled descriptors and prepared namespace authority | Typed sealed-file and user-namespace bindings to explicit child slots; duplicate-and-remap transaction. The external tool performs setns. |
| Nested setsid and adopted children | Explicit exclusive child_scope using Linux subreaping, with one root launch and kernel-authenticated waitable child handles. |
| Zombie leader with a live thread | Process pidfds without PIDFD_THREAD and wait4/waitid final-process semantics; a leader's Z state alone never completes a process. |
| Individual resource observations | Explicit bounded process-table snapshots; application computes ancestry and aggregates values. Unavailable fields are Option, not zero. |
| Complete absence and release | Only kernel ECHILD under the scope's exclusive creation/reap/adoption contract certifies absence. Scans, root exit and pipe EOF do not. |
| Overlapping owners | A second scope, any ordinary Align launch while it is active, or admission with existing children fails before changing the first owner. No invocation adopts/kills another invocation's children. |
| Cancellation and cleanup errors | Explicit signal observation, authenticated signalling, bounded reap and retryable release; application preserves execution and cleanup errors separately. Drop is a final blocking native cleanup, never a success report. |
| Unsupported host | Common owners execute on both hosts. Linux-specific acquisition refuses on macOS before effects; the consumer keeps its existing host-specific path selection. |

The stronger proposal to create an extra Align-owned PID namespace/init is
withdrawn. It duplicated the existing sandbox, introduced namespace-ancestry,
bootstrap-artifact and parent-death protocols, and was not needed to preserve the
consumer's actual contract. A process-global subreaper is acceptable only as the
explicit exclusive owner below; no runtime initialization silently enables it.

No resource owner can run Drop after the owning process is killed with SIGKILL,
replaces itself with exec, or aborts. This design does not invent such a guarantee.
Catchable cancellation uses the explicit signal owner and application cleanup.
Where the application requires sandbox teardown on parent death, its existing
`--die-with-parent` sandbox policy remains explicit. This is not a new Align
service or an assertion that the ordinary child API owns arbitrary descendants.

## 2. Exact public ledger

All operations below are Impure except the explicitly marked cached getters.
Native failures use the existing shared errno classifier: ENOENT becomes
Error.NotFound, EACCES/EPERM become Error.Denied, EINVAL becomes Error.Invalid,
and other errors become Error.Code(errno). Contract violations use Error.Invalid.
Unsupported facilities use Error.Code(native ENOTSUP). Argument ranges and modes validate
before platform checks, then before allocation/native effects. Runtime heap OOM
retains the settled fatal policy. No invalid operation returns partial ownership.
The current 64-bit Linux/macOS targets and ordinary nominal/builtin name rules
apply. The common child/command and byte-read rules remain those of plan 49.

### File and executable authority

| Declaration | Inputs, ownership, allocation and failure |
| --- | --- |
| `fs.memory_kind` | Copy sum `Data`, `Executable`, in that order. Explicit kind; no default. |
| `fs.memory_file(kind: fs.memory_kind, max_bytes: i64) -> Result<fs.memory_writer, Error>` | Linux native memory file; nonnegative explicit byte cap. Empty initial content, no name/path input and no whole-input allocation. Own one CLOEXEC memfd and fixed writer state. |
| `w.write(data: bytes) -> Result<(), Error>` on memory_writer | Exclusive borrowed receiver, call-scoped readonly bytes under existing str/string/slice<u8> admission. Check cumulative length <= max_bytes before writing. Zero bytes succeed. Append all bytes, retrying EINTR/short writes. A native write error poisons the writer; a later write/seal returns Invalid. No success with a partial chunk. Range/cap rejection before writing leaves it usable. |
| `w.seal() -> Result<fs.sealed_file, Error>` | Consume writer on either result. Freeze content, length and executable-mode bits at the OS boundary, verify required seals, then transfer one fd to the sealed owner. Failure closes the file and publishes no sealed owner. |
| `sealed.len() -> i64` | Pure cached nonnegative final length. |
| `sealed.read_at(offset: i64, out destination: slice<u8>) -> Result<i64, Error>` | Existing writable-place/out/no-alias admission. Offset >= 0 and nonempty destination. One pread attempt, 0 at EOF, otherwise exact filled prefix; rest unchanged. EINTR is a native error. No offset mutation, retained buffer or allocation. |
| `process.executable(borrow file: fs.sealed_file) -> Result<process.image, Error>` | Admit only Executable-kind native ELF; duplicate the sealed object, do not copy its bytes or reopen a pathname. Failure leaves the source unchanged. Image is an independent Move fd owner. |
| `image.len() -> i64` | Pure cached length of the admitted object. |
| `image.read_at(offset: i64, out destination: slice<u8>) -> Result<i64, Error>` | Same bounded positional-read contract as sealed_file, permitting hashing after admission. |
| `process.command_image(borrow image: process.image, args: array<str>) -> Result<command, Error>` | Existing full argv, including explicit argv[0], and existing array/slice/literal argument forms. Validate argv, duplicate image fd, and return the existing independently owned, rerunnable command. No PATH fallback. Environment/cwd/live/synchronous configuration is otherwise shared with ordinary command. |
| `process.current_image() -> Result<reader, Error>` | Linux: open /proc/self/exe directly as a CLOEXEC read owner, fstat a regular main-image object, and return the existing reader. Never substitute argv[0], an environment variable or a pathname supplied by the caller. No copy/seal is hidden here. The descriptor binds the running main-image object; it does not certify immutable file contents, all executable mappings, dynamic libraries or source provenance. A caller requiring immutable observation checks/launches the sealed-image path. |

File storage is std.fs; selection of an executable is std.process. The builder
never offers an mmap or a raw writable descriptor, and sealed owners have no
mutation method. The Linux backend uses MFD_CLOEXEC | MFD_ALLOW_SEALING plus
MFD_NOEXEC_SEAL for Data and MFD_EXEC for Executable. Before sealing, set mode
0400 or 0500 respectively. Add F_SEAL_WRITE | F_SEAL_GROW | F_SEAL_SHRINK |
F_SEAL_EXEC | F_SEAL_SEAL and verify all required bits. Executable sealing may
also set F_SEAL_FUTURE_WRITE. F_SEAL_EXEC freezes executable bits, not every
permission bit; actual write/grow/shrink denial comes from the content seals.
An externally retained writable mmap makes final sealing fail, never weakens it.
After seals are installed, fstat the actual final length and require 0 <= length
<= max_bytes before caching/publishing it; do not trust a stale writer count in
the presence of native aliases.
Require these facilities; there is no older-kernel chmod-only fallback.

ELF admission checks a complete ELF64 header, little-endian encoding, current
ELF version, ET_EXEC or ET_DYN, and e_machine matching x86_64 or AArch64. Reject
scripts, shebangs, other architectures and malformed headers before launch.
The kernel loader validates the rest and supplies an exec error through the
shared launch channel. Dynamic loaders/libraries remain explicit deployment and
environment inputs, not part of the main-image digest. Use execveat with empty
path and AT_EMPTY_PATH on the retained fd; there is no /proc-path execution
fallback. Closing the command or original file after successful launch cannot
invalidate the executed object.

### Explicit inheritance

| Declaration | Exact contract |
| --- | --- |
| `process.user_namespace(path: str) -> Result<process.user_namespace, Error>` | Linux, nonempty UTF-8 without NUL. Open one CLOEXEC fd and require NS_GET_NSTYPE == CLONE_NEWUSER. A named path is only admission input; subsequent operations own the opened namespace object. Do not create a namespace, alter credentials or enter it. |
| `c.inherit_file(borrow file: fs.sealed_file, slot: i64) -> Result<(), Error>` | Explicit slot 3..1023. Atomically install an independently owned read-only descriptor for this sealed object at that child slot. A duplicate slot is Invalid; do not replace silently. The inherited open-file description starts at offset 0 independently for each launch. |
| `c.inherit_namespace(borrow ns: process.user_namespace, slot: i64) -> Result<(), Error>` | Same slot/admission transaction, retaining typed namespace authority. The child receives an independent descriptor; the command/source fd flags remain unchanged. Namespace access modes have their native meaning. |

No generic integer-to-fd/namespace authority cast is added. Both setters own a
CLOEXEC duplicate and retain no source lifetime. Check command, source kind,
slot, duplicate-slot conflict, then allocate/duplicate; failure preserves the
complete prior map. On every launch, build all source duplicates and private
control/image descriptors above the maximum destination before any remapping.
Remap destinations in ascending slot order and close every unlisted fd. All
inputs are admitted before any child exists. Standard 0/1/2 retain plan 49's
capture/redirection behavior. No descriptor names or private fd integers appear
in a persisted format or cache key.

A sealed-file child descriptor must have O_RDONLY access and an independent
open-file offset. On Linux reopen through /proc/self/fd/N while the retained
sealed fd is pinned, then verify device/inode/length and required seals against
that exact retained object. This is a bound-fd reopen, never a mutable pathname
lookup. Namespace descriptors use CLOEXEC duplication until the final remap.
The child slot becomes inheritable only in the child. The executable fd and
error writer remain private and CLOEXEC. The external sandbox receives the
namespace slot in its explicit argv and performs its own setns exactly once.
This removes the earlier prepared-namespace ancestry conflict.

### Exclusive child scope and observations

| Declaration | Exact contract |
| --- | --- |
| `c.start_scope() -> Result<process.child_scope, Error>` | Linux, live launch with the same timeout/capture-cap rejection and output defaults as start. Acquire exclusive process-wide child creation/reap/adoption rights, enable subreaping, and launch one root. Return one Move owner of root state, capture reads and the scope lease. It is a distinct resource from ordinary direct-child ownership, while reusing its native implementation. No extra process is created. |
| `scope.id() -> i64` | Pure cached original root PID. |
| `scope.owner_id() -> i64` | Pure cached supervising process PID, useful as the application ancestry root after orphan adoption. |
| `scope.status/try_wait/wait/read_stdout/read_stderr/poll/kill/kill_group` | Exact signatures and semantics from child in plan 49, with child_scope receiver. These concern the root and its capture/group only. Root wait never releases the scope lease. wait result/cache and native reap are shared with scope.reap. |
| `scope.children(max_scan: i64) -> Result<array<process.member_info>, Error>` | max_scan > 0. Explicit bounded observation of currently waitable direct/adopted children. Count each task-list entry and PID candidate inspected; over-budget returns Invalid and drops the partial result. Admit each returned process with kernel wait ownership before constructing a stable process pidfd. Sorted ascending unique PID observations. This is not a snapshot of every still-parented grandchild and is not an absence certificate. |
| `process.member_info` | Natural Move record, fields in order `handle: process.member`, `pid: i64`. pid is a cached observation matching the handle at construction; modifying the copied number grants no authority. The record makes the existing owned-record array carrier useful without permitting bare Move-handle arrays. |
| `member.kill(signal: i64) -> Result<(), Error>` | Shared borrowed immutable member owner, native Linux signal domain including zero. Use pidfd_send_signal on its retained process pidfd, never a subsequently reused numeric PID. No handle mutation or reap. ESRCH means that target cannot be signalled, not that the scope is empty. |
| `member.finished() -> Result<bool, Error>` | Shared borrowed receiver; nonblocking process-pidfd readiness, not a thread-leader Z test. Repeated terminal readiness stays true. It does not reap or release the scope. |
| `scope.reap(max_events: i64) -> Result<array<process.reaped>, Error>` | max_events > 0. Perform at most that many successful wait4(-1, WNOHANG \| __WALL) events, stopping on pending, ECHILD or EINTR. Return each newly reaped child's PID and typed wait_result once. Root events fill the same root cache used by wait/status. A native hard error returns Err, preserves already completed internal root/ownership state, and exposes no partial result. |
| `process.reaped` | Copy natural record: `pid: i64`, `status: process.wait_result`, in order. A copied PID is observation only. |
| `scope.release() -> Result<bool, Error>` | Non-consuming, retryable. True only after a kernel no-children witness and successful subreaper restoration/global-lease release. False means some live or unreaped child remains; performs no implicit reap. Err retains ownership and the restoration work. Closed repeats return true. |
| `process.snapshot` | Copy natural record, fields in order `pid: i64`, `parent_pid: i64`, `rss_bytes: Option<i64>`, `cpu_ns: Option<i64>`, `threads: Option<i64>`. IDs are positive except parent_pid may be zero. Optional values are nonnegative and representable. No authority is carried. |
| `process.table(max_scan: i64) -> Result<array<process.snapshot>, Error>` | Linux/macOS common bounded process-table observation. max_scan is 1..536870910. Count each PID candidate, including vanished candidates. Sorted unique PID rows; inaccessible/malformed essential identity is an error, unavailable optional usage is None. The observation is non-atomic and cannot certify full absence or an exact aggregate resource limit. Application code builds ancestry and computes totals. |

All scope state-changing methods require its exclusive receiver; Pure ID reads
permit shared borrow. The scope's root controls share one closed operation ledger
with ordinary child, not a second measured-child implementation or native
supervision loop. No automatic timeout/TERM grace or first-error selector is
introduced. member handles own only stable signal/liveness authority and may
outlive the scope; successful release means those targets are gone, and later
handle calls cannot hit a recycled PID.
After successful release, children and reap return Invalid before any native
observation or wait. Root cached status/wait results, cached IDs and capture
draining retain their ordinary child semantics; kill/group controls reject the
reaped root. Drop then closes only remaining local descriptors/state, never
reacquires adoption rights or waits for children of a later owner.
member.kill and member.finished do not mutate the handle. They are Impure OS
operations but permit shared borrowing, as existing directory metadata methods
do. Thus an ordinary shared helper can act on a member_info borrowed from its
owned array through the existing R67 path; no mutable indexed borrow is added.

On Linux, table uses procfs stat identity/PPID, resident pages converted with the
native page size, utime+stime converted from native clock ticks to nanoseconds,
and the process thread count. Every multiplication/addition checks overflow;
unavailable/unrepresentable values are None. On macOS use unprivileged short BSD
identity/PPID plus optional task-info resource fields. Task CPU counters are
Mach absolute-time units: checked-sum pti_total_user and pti_total_system, then
convert with mach_timebase_info numer/denom using widened integer arithmetic and
floor division; unavailable/overflow is None. Use byte RSS and pti_threadnum. Access denial for the optional task
record produces None fields without discarding an otherwise valid identity row.
A vanished entry is skipped on either host. The initial platform PID list uses
one bounded max_scan+1 request on Darwin, as in plan 49. Neither implementation
silently grows a sizing-and-retry buffer beyond the explicit scan budget.

## 3. Scope proof, state and failure order

```text
Unreserved -> Reserved -> SubreaperEnabled -> RootOwned -> Empty -> Released
                         \-> failed launch cleanup -> restored/unreserved
```

One runtime process-global reservation coordinates every Align child launch and
reap owner. Scope admission requires no active scope, no in-flight or unreaped
ordinary Align child, PR_GET_CHILD_SUBREAPER == 0, admissible SIGCHLD disposition,
and kernel ECHILD for waitid(P_ALL, WEXITED | WNOHANG | WNOWAIT | __WALL).
A successful waitid call with no terminal siginfo still means live children may
exist and must reject admission. A terminal child also rejects; do not consume an
unowned status to make admission pass. Only after these checks set subreaping.
A second scope or ordinary spawn/start/run while reserved returns Invalid before
forking. Closed/reaped ordinary handles containing only caches are not live
native children. Reservation and launch-in-flight counters are exception-safe.

The entire lifetime, including failed-construction cleanup, requires exclusive
native child creation and reaping. Foreign FFI must not fork/clone children into
this process, wildcard-reap, ptrace/reparent its children, change SIGCHLD to
SIG_IGN/SA_NOCLDWAIT, or change subreaper state. Safe Align exposes no bypass.
This is the same explicit native-interop boundary as an owned fd: foreign code
must not invalidate its ownership. It is not a claim that constructor inspection
can enforce exclusivity against a later arbitrary native violation.

The root is launched with PR_SET_NO_NEW_PRIVS in the child before exec. This
prevents a setuid/file-capability executable from silently changing the host
privilege boundary; it does not prohibit an explicitly supplied user namespace.
No caller credential or signal mask is changed permanently. Kernel denial of
signal/reap/metadata remains a real error, with the owner retained.

children reads Linux /proc/self/task/*/children incrementally with fixed scratch,
not an unbounded text read. Duplicate candidates collapse. For each PID, perform
waitid(P_PID, WEXITED | WNOHANG | WNOWAIT | __WALL) without reaping: success proves
current direct/adopted wait ownership, including pending children. The exclusive
lease prevents other reaping, so this PID cannot be reused before pidfd_open.
Acquire a process pidfd (never PIDFD_THREAD) and publish the handle only then.
ECHILD/vanished candidates are skipped; other errors fail without publishing the
partial array. A raw PID from a process-table snapshot is never accepted by a
public signal operation. Members become waitable as intermediate parents exit;
application cleanup can therefore signal current children, reap, and repeat.

Reap uses __WALL so clone children with a non-SIGCHLD exit signal cannot evade
ownership. Each successful native reap updates root/child state before subsequent
fallible work. Allocate bounded result capacity before reaping, so allocation
failure cannot lose a newly consumed status. Root termination must agree with its
WNOWAIT cache. Non-root resource results are observations of each native wait4,
not an aggregate maximum-RSS calculation.

Release requires waitid(P_ALL, WEXITED | WNOHANG | WNOWAIT | __WALL) == ECHILD
while the lease is still exclusive and subreaping is still enabled. Under that
contract every surviving descendant has a chain to a direct child, or is itself
adopted when that chain ends. No direct child therefore means no surviving or
unreaped descendant. New child creation is still excluded when restoring
PR_SET_CHILD_SUBREAPER to zero and releasing the reservation. A procfs scan is
never used for this final proof. Detected loss of ownership or contradictory
native owner state is latched; subsequent ECHILD cannot turn it into a successful
certificate. Argument/budget errors and retryable OS/restoration failures do not
by themselves assert lost ownership or poison a still-valid owner.

On constructor failure, close capture/control descriptors, kill the owned root
and any adopted children, reap them, restore subreaper state, then release the
reservation and return the original construction error. Cleanup failures cannot
return an unowned live scope: use the existing invariant-abort path if native
ownership/restoration cannot be recovered. Explicit release retains retryable
state on failure. Drop closes owned capture reads, repeatedly signals/reaps its
owned children to kernel-empty, then restores and releases; it may block and
never reports application success or removes a workspace. A hard unrecoverable
native cleanup/restoration failure aborts. These are resource finalization
mechanisms; all application grace, limits, reporting and cleanup authorization
remain source code.

## 4. Compiler, runtime, ABI and allocation closure

The six new opaque non-Send Move leaf types use the existing digest carrier
contract: owned/borrowed helpers, returns, natural records/sums/Option/Result and
existing owned-record arrays. Bare owner collections, tuple/box/out/global/
layout(C)/print/equality/hash and task/parallel capture reject. Moves null source;
replacement Drops the old owner; imported signatures and Drop-only reachability
retain the native entry. No dynamic dispatch or user-visible lifetime syntax.

| Type | Canonical Ty / Scalar leaf tags | Native Drop |
| --- | --- | --- |
| fs.memory_writer | 77 / 55 | Close unsealed/poisoned memfd. |
| fs.sealed_file | 78 / 56 | Close retained sealed fd. |
| process.image | 79 / 57 | Close retained image fd. |
| process.user_namespace | 80 / 58 | Close namespace fd. |
| process.child_scope | 81 / 59 | Complete scope cleanup/restoration above. |
| process.member | 82 / 60 | Close process pidfd only. |
| command (existing owner) | existing Ty 38 / new Scalar 61 | Existing command cleanup. |

command_image returns the existing command owner inside Result, so its Scalar
leaf is added to the ordinary native-owner carrier machinery. The existing
canonical Ty tag is unchanged. Scalar 61 follows the six reserved R65 leaves;
this does not create another command type or constructor ownership model.

Plan 49 reserves 76/54 for signal_subscription. Canonical leaf goldens are the
listed bytes; independent encode/decode checks reject unknown neighbors. Interface
12's named-type grammar remains sufficient. Full reachable nominal definitions,
method effects and changed runtime schemas participate in compiler/interface/MIR
and runtime ABI fingerprints; no cache may reuse a pre-R65 child-wait declaration.
No external helper artifact, installed binary, private CLI or new persisted
application format is introduced.

All opaque handles lower to ptr. Native result status is i32 using the existing
Error mapping, with exclusive checked scratch output. memory_kind is i32 (0/1).
Natural 64-bit record layouts are: member_info {ptr at 0, i64 at 8}, size 16/alignment
8; reaped {i64 at 0, wait_result at 8}, size 48/alignment 8; snapshot {i64 at 0,
i64 at 8, Option<i64> at 16/32/48}, size 64/alignment 8. Dynamic arrays use the
existing {ptr,i64} descriptor and runtime heap provenance. Zero inactive fields
and all padding before publication. User lookalike records are never native schemas.

| RuntimeKey / suffix after align_rt_ | Exact native shape |
| --- | --- |
| FsMemoryFile / fs_memory_file | i32(i32 kind, i64 cap, ptr out_handle) |
| FsMemoryWrite / fs_memory_write | i32(ptr writer, ptr data, i64 len) |
| FsMemorySeal / fs_memory_seal | i32(ptr consumed_writer, ptr out_handle) |
| FsMemoryFree / fs_memory_free | void(ptr) |
| FsSealedLen / fs_sealed_len | i64(ptr) |
| FsSealedReadAt / fs_sealed_read_at | i32(ptr, i64 offset, ptr data, i64 cap, ptr out_i64) |
| FsSealedFree / fs_sealed_free | void(ptr) |
| ProcessExecutable / process_executable | i32(ptr sealed, ptr out_image) |
| ProcessImageLen / process_image_len | i64(ptr) |
| ProcessImageReadAt / process_image_read_at | i32(ptr, i64 offset, ptr data, i64 cap, ptr out_i64) |
| ProcessImageFree / process_image_free | void(ptr) |
| CommandImage / command_image | i32(ptr image, ptr argv_views, i64 argc, ptr out_command) |
| ProcessCurrentImage / process_current_image | i32(ptr out_reader) |
| ProcessUserNamespace / process_user_namespace | i32(ptr path, i64 len, ptr out_namespace) |
| ProcessUserNamespaceFree / process_user_namespace_free | void(ptr) |
| CommandInheritFile / command_inherit_file | i32(ptr command, ptr sealed, i64 slot) |
| CommandInheritNamespace / command_inherit_namespace | i32(ptr command, ptr namespace, i64 slot) |
| CommandStartScope / command_start_scope | i32(ptr command, ptr out_scope) |
| ScopeOwnerId / scope_owner_id | i64(ptr scope) |
| ScopeChildren / scope_children | i32(ptr scope, i64 max_scan, ptr out_member_array) |
| ProcessMemberKill / process_member_kill | i32(ptr member, i64 signal) |
| ProcessMemberFinished / process_member_finished | i32(ptr member, ptr out_bool) |
| ProcessMemberFree / process_member_free | void(ptr member) |
| ScopeReap / scope_reap | i32(ptr scope, i64 max_events, ptr out_reaped_array) |
| ScopeRelease / scope_release | i32(ptr scope, ptr out_bool) |
| ScopeFree / scope_free | void(ptr scope) |
| ProcessTable / process_table | i32(i64 max_scan, ptr out_snapshot_array) |

Scope root methods use the existing Child* native signatures with a validated
common native root-state prefix at offset zero of child_scope; they never accept
an unrelated opaque handle. Sema/HIR/MIR fix the exact receiver family, result
nominal schema, writable out provenance and owner effects in one closed operation
ledger. LLVM only lowers validated MIR. Scope-specific Drop is selected by the
source type; a common root operation can never release the process-wide lease.
Malformed native pointers still owe valid native allocations; range/alignment,
length/overflow and forbidden-overlap checks precede scratch initialization or
state changes. Free(null) is a no-op; consuming seal frees its source shell on
both results and codegen nulls the source exactly once.

The file's kernel storage grows with explicitly written bytes. There is no
additional full-file application buffer or launch-time content copy. Snapshot,
member and reap arrays allocate according to explicit bounds, with checked
capacity/stride arithmetic before allocation. Ordinary cached status/read/poll
does no process scan. The scope adds fixed native owner state and no subprocess.
Measure increasing input/output with fixed caller scratch during implementation;
this checks the stated resource behavior, not a latency correctness gate.

## 5. Implementation closure and capability order

| Axis | Owning regression, including negative control |
| --- | --- |
| Common read/status/poll/group/signal/launch | Plan 49's matrix, including writable out, deadline, signal, metadata, frame and Darwin CLOEXEC-source corrections. Native Linux and macOS execute the common suite. |
| Formation/carriers/borrow/purity/Send | m11_process_verified::formation_and_carriers and m11_process_scope::formation_and_carriers; all six new leaves plus nested records/sums and imported signatures. |
| Construction, consume, move-in/out, source nulling, Drop, replacement, return | owned_control_flow for file/image/namespace/scope/member, structs/sums/Option/Result, allowed owned-record arrays, helpers and omitted-Drop negative controls. |
| if/match/else/?/map_err, joins, loops, early exits | Same invariant-level control-flow owner; terminating arguments must not call native code or consume an earlier live receiver. |
| Sealed bytes/executable | seal_matrix: cap/empty/chunk/poison, external writable mappings, write/grow/shrink/exec-bit attacks, aliases, data/ELF kind, wrong ELF arch/header, fd-selected exec after path replacement, no whole-copy growth. |
| Running-image observation | current_image_binding: caller-path mismatch, renamed/unlinked main image, sealed versus ordinary running object, loader/library limit and no guessed fallback. |
| Descriptor/namespace boundary | inheritance_matrix: duplicate/out-of-range slots, cycles, standard-fd collisions, independent per-launch offsets, source Drop, flags, high/sparse unlisted fd, sealed readonly mode, NS_GET_NSTYPE rejection, prepared namespace with original bwrap setns path. |
| Scope admission/global state | exclusive_scope: existing live/zombie/clone child, existing subreaper, second scope, concurrent ordinary launch, SIGCHLD modes, foreign-violation fault injection, failed constructor, restoration failure and no lost reservation; released-owner children/reap/Drop cannot observe or consume a later owner's children. |
| Descendant lifecycle/identity | lifecycle_matrix: nested setsid/double fork, adopted orphans, CLONE_PARENT/non-SIGCHLD children, child PID reuse sentinel, zombie leader/live thread, two-owner isolation, catchable cancellation, repeated reap/release, kernel ECHILD-only completion. |
| Individual observations/work bounds | process_table_matrix and children_matrix: PID/TID budget, duplicate/vanished candidates, host-independent ordering, optional usage/unrepresentable counters, no scan-as-absence and ordinary-user Darwin identity access. |
| ABI/IR/cache | runtime native_layout_and_ranges, independent LLVM size/offset checks, HIR/MIR mutation sweep for every new operation/type family, whole/per-unit/generic/import/cache parity and canonical type goldens. |
| Cleanup failure | kill/reap/restore fault injection, partial constructor phases, lost-ownership state never reports success, ordinary budget/native errors retain retryable ownership, Drop blocks/aborts as specified, no workspace deletion. |
| Shared error model | native_error_mapping: ENOENT/NotFound, EACCES and EPERM/Denied, EINVAL/Invalid, native ENOTSUP and remaining errno/Code, on every new operation family; no duplicate classifier. |

Implement the fewest useful capability PRs in this order:

1. Common native child workflow and process-table observations: plan 49's live
   I/O/status/group/file-output migration plus table. This is a useful native
   Linux/macOS consumer boundary.
2. Explicit signal subscription: its distinct process-global disposition state
   and already useful live-loop consumer; can proceed independently of sealing.
3. Verified launch authority: memory construction/sealing, image admission,
   current-image observation, namespace admission and the command inheritance/
   fd-exec consumer together. No dormant producer-only sealing milestone.
4. Exclusive child_scope: admission, root integration, member handles, reap,
   absence and restoration/Drop together. No constructor-before-release split.

The common, verified-launch and scope capabilities may exceed 1,000 handwritten
lines. Their native ownership producers and consumers must land together to avoid
duplicated ABI/lifetime proof and unsafe intermediate states; the boundaries
above isolate distinct failure domains, not line counts. All APIs and failure
strategies are selected here; the implementation model owns code and owner tests,
not another platform/helper/containment-design decision.

## 6. Composition example

The first helper materializes explicitly in fixed chunks; sealing and executable
admission are separate from hashing and command launch. The second performs one
cleanup pass with a borrowed scope. A caller loops with its own budget, preserves
errors, and retains the owner until release succeeds. No native policy supervisor
or new array-mutation feature is needed.

```align
import std.fs
import std.process

fn admit_image(borrow mut input: reader, max_bytes: i64) -> Result<process.image, Error> {
    mut pending := fs.memory_file(fs.memory_kind.Executable, max_bytes)?
    mut chunk := buffer(65536)
    loop {
        count := input.read(chunk)?
        if count == 0 { break }
        pending.write(chunk.bytes())?
    }
    sealed := pending.seal()?
    return process.executable(sealed)
}

fn signal_member(borrow member: process.member, signal: i64) -> Result<(), Error> {
    return member.kill(signal)
}

fn signal_entry(borrow entry: process.member_info, signal: i64) -> Result<(), Error> {
    return signal_member(entry.handle, signal)
}

fn cleanup_step(borrow mut scope: process.child_scope) -> Result<bool, Error> {
    entries := scope.children(4096)?
    mut index: i64 := 0
    loop {
        if index >= entries.len() { break }
        signal_entry(entries[index], process.signal_number(process.signal.Kill))?
        index = index + 1
    }
    scope.reap(4096)?
    return scope.release()
}
```

## 7. Qualification and completion criterion

Primary native contracts: [Linux subreaping](https://man7.org/linux/man-pages/man2/PR_SET_CHILD_SUBREAPER.2const.html),
[wait ownership](https://man7.org/linux/man-pages/man2/waitid.2.html),
[process pidfds](https://man7.org/linux/man-pages/man2/pidfd_open.2.html),
[memfd sealing](https://man7.org/linux/man-pages/man2/memfd_create.2.html),
[no_new_privs](https://man7.org/linux/man-pages/man2/PR_SET_NO_NEW_PRIVS.2const.html),
and [Darwin task counters](https://github.com/apple-oss-distributions/xnu/blob/main/osfmk/kern/bsd_kern.c).
These delimit native guarantees; the scope exclusivity and release proof above
are Align's design built on them.

Read-only consumer inspection uses revision
c9062c5fb51e137d6b78815c199c7935a732f7b6. No consumer code, tests, fixture, pin or
adoption command was modified or run. Local disposable probes ran as uid 1000 on
Linux 6.18.33.2-microsoft-standard-WSL2 x86_64:

- Exclusive subreaping adopted a double-setsid orphan, refused empty while it was
  alive, then reached kernel ECHILD after the owned reap. A foreign parent PID
  failed wait admission. A Z leader with a live worker remained nonwaitable and
  its process pidfd remained nonready until process termination.
- Data and executable memfds denied content/length/execute-bit mutation after
  sealing. Required seals were 47 for data and 63 for executable; executable
  fd-selected /bin/true ran successfully after chunked materialization.
- A prepared user namespace was opened and its creator reaped before workload
  launch. Existing bubblewrap 0.12.0 accepted that inherited namespace fd with
  --userns, --unshare-pid and --die-with-parent under no_new_privs, with exit 0.

These are mechanism qualification, not completed compiler/consumer acceptance.
The probes are /tmp/r65-exclusive-domain-probe.py and
/tmp/r65-authority-probe.py; the implementation must convert their invariants to
the repository owner suites above. Native macOS common-owner execution is an
implementation acceptance requirement, not claimed to have run on this Linux
host. Source evidence for common Darwin operations is in plan 49.

The completed author pass checked this ledger against plan 49 and every R65
request row, including unsupported-host behavior and the absence of an implicit
sandbox/supervisor. Independent review covered the changed scope/global-state
strategy, exact authority boundaries and the corrected common contract. Its
shared-error finding is closed by the existing errno classifier and category
owner. A focused Darwin source follow-up closed the SETEXEC/CLOEXEC-source gap
with setup-once stdio remapping and inherit-only SETEXEC, as recorded in plan 49.
The released-owner exclusions and ownership-loss/retryable-error distinction were
also checked. No finding or platform/helper decision remains assigned to the
implementation model.

Both composition examples pass the existing formatter/parser. Native layouts,
future API type checking and cross-platform execution belong to the specified
implementation owner tests; they are not claimed to have executed already.
The accepted planned contract is synchronized to the language specs, rationale,
Settled decisions, process/fs originals and Japanese mirrors, HIR/runtime ledgers,
HANDOFF and the external request register. R65 is design-ready (ACCEPTED in the
register); it becomes ALIGN_MERGED only when all four capabilities are implemented
and merged. Consumer verification remains consumer-owned. A request-batch release
build is required after implementation, not for this design-only handoff.

## Verified-launch implementation evidence

The capability uses process_verified.rs for the native sealed/image/namespace
owners and the shared process_launch Prepared/bootstrap path for their sole
execution consumer. ProcessLiveKind owns HIR/MIR/native operation identity;
MemorySeal consumes its receiver and MIR nulls that source once on both results.
All other owner inputs use the existing stable shared/exclusive borrow rules.

The closure matrix above is closed for capability 3 by these owners:

- m11_process_verified::formation_and_carriers covers all four new leaves plus
  command in imported generic record/sum/Option/Result carriers and forbidden
  direct collection/tuple/box/out/multiple-move forms.
- borrowed_arguments_remain_live_until_action rejects later-argument consumption
  of an earlier image/file/namespace loan and shared write/seal attempts.
- owned_control_flow_and_temporary_cleanup executes if/match/else/?/map_err,
  loops, early exits, replacement, owned helpers and native cleanup under both
  compilation modes. A MIR mutation removes a real sealed Drop and must expose
  residual fds; the unmodified programs retain the initial fd count.
- memory_seal_positional_read_and_consume and native seals_caps_poison_aliases_and_positional_prefix
  cover cap rejection without poisoning, actual hard-error poisoning, empty and
  positional reads, unchanged suffixes, mmap rejection, final native size and
  write/grow/shrink/execute-bit alias attacks.
- retained_executable_command_and_descriptor_authority and native inheritance_matrix
  cover retained sources, repeated fd-selected execution, independent readonly
  offsets, ordered/sparse slots, duplicate/range rejection and source fd flags.
  The native ELF owner rejects each header discriminator before descriptor exec.
- current_image_binding executes after unlinking only the fixture-owned running
  executable; opened_namespace_and_running_main_image checks the native main
  object and namespace type admission. Prepared namespace sandbox integration
  retains the unprivileged qualification recorded in section 7.
- native_layout_and_ranges checks alignment/overlap/ranges before output mutation.
  live_process_records_and_writable_backing and live_process_mir_contract_matrix
  mutate every new producer; canonical leaf goldens independently encode/decode
  all four types and command Scalar 61. The declaration/export owner checks all
  444 native signatures and each optional export profile.

The fixed_scratch_resource_measurement local probe writes 4 MiB and 64 MiB with
64 KiB scratch. RSS was 8072 -> 9404 KiB at the cold 4 MiB case and 9532 -> 9532
KiB at 64 MiB. This is resource evidence, not a timing or correctness threshold.
Native unsupported-acquisition cases are in the shared macOS suite; valid inputs
return the platform ENOTSUP code and invalid input still takes precedence.
Capability 4 owns the scope-specific matrix rows; this capability adds no scope
constructor or partially usable descendant owner.

Native-error encoding closure: raw OS errno values cross the shared
io_error_to_status classifier before entering an Align status result, including
unsupported-platform constants. The platform owner independently expects
AL_CODE + native ENOTSUP; it must not compare the status directly with errno.
Internal ESRCH/EINTR branches likewise compare raw errno before conversion or
compare the encoded status afterwards. This closes the native-error-encoding
axis of the matrix without changing the public Error contract.

## Exclusive-scope implementation evidence

`process_scope.rs` owns the lease, Linux subreaper restoration, bounded child
observations and process pidfds. `process_launch` serializes admission with every
ordinary launch, while the scope-specific bootstrap sets NO_NEW_PRIVS before exec.
The root uses the offset-zero NativeChild state and shared status/reap cache.
No member stores a numeric PID for signalling; its sole field is an owned pidfd.

The capability-4 matrix maps to these invariant-level owners:

- `exclusive_scope` and `foreign_children_and_failed_constructor` cover existing
  ordinary and clone children, cached handles, concurrent ordinary launch, second
  scope rejection, existing subreaper, SIGCHLD IGN/NOCLDWAIT and failed exec.
- `restoration_and_reap_failure_state` and
  `constructor_phases_and_partial_observations` inject every acquisition phase,
  partial member admission, kill denial, partial reap and restoration failure.
  The native lease remains retryable, completed root cache survives output
  failure, partial pidfds close and contradictory foreign state remains lost.
- `adoption_and_drop` uses nested setsid/orphan adoption and verifies the kernel
  ECHILD witness after Drop; intentionally retaining the raw owner is the negative
  control that leaves both lease and children present. Released-owner operations
  and Drop cannot touch a later scope. Retained pidfds remain finished after reap
  and return the shared encoded ESRCH instead of addressing a later PID.
- `m11_process_scope` runs whole/per-unit imported generic and owned-carrier
  transport, shared member_info helpers, record/sum/Option/Result, branches,
  loops, early error propagation, replacement and Drop with stable fd counts.
  Dedicated native C fixtures exercise CLONE_PARENT and a zombie
  thread leader with a live worker; process completion stays false until the
  worker exits, and the executed root independently observes NO_NEW_PRIVS.
  `non_sigchld_wait_domain` deliberately injects a direct clone with exit_signal=0
  in an isolated test scope: plain wait falsely reports ECHILD while the scope's
  __WALL admission, absence and reap owners retain and recover that child.
- Native layout/range owners, canonical leaves, every scope/member producer in
  the HIR/MIR mutation sweeps and all nine native declaration/export rows close
  ABI/cache and malformed-input cells. macOS performs source/interface checks
  and returns its encoded native ENOTSUP at scope acquisition.

The scope adds fixed owner state and no helper process. Children charges every
task entry and PID candidate and publishes sorted unique owned records only on
success; reap bounds successful events and publishes no partial result on error.
No throughput promise or application cleanup policy is introduced.
