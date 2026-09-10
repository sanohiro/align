# Align-owned boundaries for the align-llm product cutover

Status: **implementation authorized by the owner, 2026-09-10**.
This is an adoption assessment and capability plan. The candidate interfaces below
are design notation, not shipped declarations. H1 proceeds under the exact
[incremental SHA-256 ledger](41-incremental-sha256-plan.md). R65 has explicit
unclosed platform and lifecycle decisions; it is not ready for implementation.

## 1. Evidence and scope

Assessed Align `59bbfe5d` and the request register at align-llm `c9062c5f`.
The latest wave consists of R29, R53, and R63–67. The earlier
[all-60 audit](align-llm-request-audit-2026-09-07.md) remains the inventory for
older requests; this wave does not select all remaining proposals.

The register reports unpublished candidates `3805787e`, `c1f99817`, and
`d66a3107`, and plans named `39-host-observation-plan.md` and
`40-retained-tree-plan.md`. Neither those objects nor those plans exist in this
checkout. Their reported tests/reviews are external checkpoint evidence, not
verification of this tree. The local plan numbered 39 concerns fixed-array
length. Do not copy the other environment's numbering or mark its capabilities
merged. Recoverable candidate code may later be evaluated against the accepted
contract; it cannot define that contract by existing first.

Read-only consumer evidence:

- `scripts/prompt-snapshot-helper.py`: `walk_tree`, `digest_file`, and
  `environment_probe` establish raw-name observation, streaming and host facts.
- `scripts/prompt-source-verifier.py`: `RetainedExecutable`, `fixed_git`, and
  `parse_file_set_manifest` establish retained identity and explicit execution.
- `scripts/prompt-fixed-adapter.py`: sealed memfd input construction establishes
  immutable input, not merely a read-only descriptor.
- `eval/runners/run-coding-task.py`: subreaping, namespace descriptor admission,
  bounded capture and task-resource decisions establish the supervision boundary.

New `run-align-product-cutover` targets and modules in the register are planned
consumer acceptance, unavailable here. None was run. Consumer code, tests and
pin adoption remain consumer-owned.

## 2. Selection

| Request | Align assessment | Proposed answer |
| --- | --- | --- |
| R29 incremental digest | Adopt. Incremental cryptographic hashing is a general library capability. | One SHA-256 Move context using the existing OpenSSL engine; input chunks are borrowed and finalization consumes the context. No file-hashing or evidence-schema API. |
| R53 ordinary directory operations | Adopt the remaining general operations. `read_dir` already ships. | Ordinary creation and a fallible type observation; retained creation/type checks share R64's native machinery. A type predicate never certifies writability. |
| R64 retained raw tree | Adopt the requirement and the directory/cursor/metadata separation. | Complete byte-name enumeration and descriptor-relative operations. Traversal order, manifest building, size limits and recursive cleanup are Align application code. |
| R65 verified process lifecycle | Adopt the missing OS capabilities; redesign the unclosed execution/containment contract before coding. | Verified inputs, explicit inheritance, nonblocking I/O/status, authenticated process observations and exclusive ownership. Application Align code implements supervision. No native coding-runner or product report API. |
| R66 host observation | Adopt with corrected semantics. | OS-observed owned facts and explicit unavailable values. An online logical CPU count is not guaranteed to describe the physical host outside a VM/container. |
| R67 borrowed Move-record slices | Admit as an implementation-gap candidate, independently of the cutover. | Extend existing `slice<T>` formation and existing borrowed-place reads; keep element ownership in the collection. Do not introduce reference syntax or implicit element copies. |
| R63 owned JSON f64 | Defer under the current settled-decision protocol. | The requested use is reasonable, but recursive-owned JSON explicitly excludes floats. Reopen only with qualifying evidence; if admitted later, extend the existing codec structurally, not with an application-specific record exception. |

R67 follows the general array/slice view promise in `draft.md` §Array/§Slice.
Plan 30 explicitly deferred the wider Move-collection surface; its closed R22
indexing feature does not supply this proof. Confirm the complete readable-place
domain in the R67 contract review rather than deleting the Move guard globally.
R63 is different: `docs/open-questions.md`'s recursive-owned JSON Settled entry
explicitly excludes floats. [The friction protocol](23-friction-ledger.md)
requires five mechanical workaround sites across two independent real programs.
This request does not establish that threshold; copied examples or two modules in
align-llm do not supply a second program.

## 3. Public-surface decision ledger

This ledger precedes detailed contract authoring. **Candidate** rows fix the
recommended interface direction; **unclosed** rows enumerate the decisions that
must be resolved in the owning capability ledger before implementation. They are
not placeholders an implementer may fill by inventing runtime behavior.

Common rules for candidate rows: normal `Error`/`Option` and Move/borrow rules;
no implicit clone; no manual raw-handle close API; ordinary fatal allocation
failure unless the accepted owner specifies a fallible native error. All native
observations and digest mutations are Impure. Each new native owner must support
bound locals, move/return, shared or exclusive borrowed helpers as applicable,
and ordinary struct/Option/Result cleanup. Unsupported carrier classes must be
listed once by a structural classifier, not discovered one use at a time.
No new global, capture, parallel or extern transport is implied.

| ID / state | Exact candidate surface or unresolved signature decision | Ownership, lifetime, allocation, errors | Owner / prerequisites / acceptance |
| --- | --- | --- | --- |
| H1 candidate | `crypto.sha256_stream() -> crypto.digest`; exclusive `d.update(data: bytes) -> ()`; consuming `d.finish() -> array<u8>` | Opaque Move context, no reset/clone. Existing byte-view coercions; NUL and arbitrary bytes accepted. Update retains no input. Finish returns exactly 32 owned bytes and releases context. Preserve one-shot SHA's total-or-abort engine-error/OOM policy; no second recoverable digest error path. Fixed context storage, no retained preimage. See §4.1 for length limit. | `std.crypto`, existing EVP dependency; proposed `m11_crypto_stream` owner plus native context lifecycle and existing `m11_crypto` vectors. |
| F1 candidate | `fs.create_dir(path: str) -> Result<(), Error>`; `fs.is_dir(path: str) -> Result<bool, Error>` | Borrow UTF-8 path only for call; embedded NUL/empty path Invalid before I/O. Creation is one directory, no missing-ancestor creation, existing entry is an error. Candidate mode 0777 filtered by process umask, with no process-global umask mutation. `is_dir`: successful followed metadata query gives kind equality; absent/denied/traversal failures remain errors. Neither observes write authority. | `std.fs`; [plan 43](43-ordinary-directory-plan.md) and `m9_fs` create/type/permission owners. Cutover uses F2 for retained paths. |
| F2 unclosed | `fs.directory` Move owner; independent `fs.dir_cursor` Move owner; `fs.dir_entry { name: array<u8> }`; Copy `fs.entry_kind` and `fs.metadata`. Candidate cursor terminal: exclusive `cursor.next() -> Result<Option<fs.dir_entry>, Error>`. Constructor and relative-operation names, complete metadata fields and widths must be closed as one R64 ledger. | Cursor owns an independent enumeration descriptor; directory Drop closes only its descriptor. Entry name owns exact bytes, excluding `.` and `..`; EOF is only None, failure never EOF. Returned ordinary reader/writer owners independently retain opened files. Relative paths use borrowed bytes. Metadata is an observation, not authority or a filesystem snapshot. | `std.fs`; F1 is not a semantic prerequisite. Native Linux/macOS retained-path owner and whole/per-unit `m9_fs` coverage; §5. |
| O1 candidate | `os.host() -> Result<os.host_info, Error>`; record fields in declaration order: `system: string`, `release: string`, `machine: string`, `cpu: Option<string>`, `logical_cpu_count: Option<i64>` | Owned text; uname fields are exact OS strings, strict UTF-8, no lowercasing/localized formatting. Mandatory query failure is Error; invalid UTF-8 is Invalid. Optional CPU description is None in the first boundary. Positive online CPU query gives Some(n); unavailable/nonpositive/unrepresentable gives None, never zero or fabricated 1. | New `std.os`, existing owned-record/Option machinery. Exact O1 contract and owners: [plan 42](42-host-observation-plan.md), `m11_os_host`, native injection and HIR/MIR schema gates. No R50 memory query. |
| V1 candidate | For an existing readable AoS `array<T>` with Move record T: `view: slice<T> := rows` and `rows[a..b]`; `view[i].field`; `inspect(view[i])` where the parameter is `borrow row: T`. Re-slicing and helper-returned field views use the same rule. | Copy view descriptor, source generation and transitive view roots retained. Receiver then bounds evaluated exactly once; existing bounds abort. No element copy, move, source nulling or allocation. Whole Move-element value use and write access through this view reject. | `align_sema`/HIR/MIR/LLVM borrowed-place path; existing `borrowed_params`, `struct_slice_fields`, `consumer_borrow_boundaries`, plus proposed Move-slice parameterized owner. |
| P1 unclosed | Retained executable and immutable input constructors; exact command-builder method selecting that executable; typed inherited descriptor slots; current-executable observation. | Pinning pathname identity and sealing bytes are different contracts. Explicitly allocated immutable snapshots, byte readers and launch borrows; no raw integer establishes authority. Failure may publish no partially prepared owner. See §6. | `std.fs`/`std.process`, H1/F2 as consumers require; verified launch/descriptor tests. |
| P2 unclosed | Piped launch returning an owned child; exclusive `try_wait() -> Result<Option<i64>, Error>` candidate; bounded read into caller storage; readiness with explicit finite timeout. Exact pipe owner/read-result and command configuration interaction must be fixed together. | Pending, EOF, data and failure are distinct. No retained scratch view or hidden unbounded capture. EOF does not end the application deadline. Repeat wait reports a cached exit code without reaping again. | Existing child/command boundary; proposed `m11_process_live` owner and unchanged synchronous `m11_process_command` owner. |
| P3 unclosed | Exclusive containment/subreaper lease; authenticated member cursor/handles, individual observation, signal, reap, absence observation and fallible release. Namespace-descriptor admission is a separate typed operation. | No raw PID signalling authority. Launch admission and process-global exclusion start before a child exists; no automatic successful release on partial cleanup. Native operations expose facts. Policy is in Align. Full lifecycle and platform proof in §6 must precede exact signatures. | Linux qualification first; proposed `m11_process_containment` plus native failure/overlap owners. R65 cannot close with P1/P2 alone. |
| J1 deferred | Existing `json.decode`/`encode`/`encode_bounded` grammar only, if R63 later passes the reopen protocol. | Sweep f64 direct/Option/array leaves, scalar semantics and owned-prefix cleanup together. No current accepted grammar change. | Plan 25 and `m5_owned_json`; outside this implementation wave. |

Candidate notation is `text`/table notation because several types are not yet
accepted by the compiler. It makes no claim of syntax-checked new API examples.
Executable Align examples enter the capability ledger only when their syntax is
checked and declarations are separated from calls.

## 4. Independently useful library and compiler capabilities

### 4.1 Incremental SHA-256

H1 is a good match for Align. Constructor allocation and a consuming finisher
are visible; the application controls the read loop and chunk size. The runtime
wraps EVP context construction/update/finalization/free and never retains input
chunks. Reuse the existing SHA-256 engine and digest byte representation; do not
implement SHA in Align or Rust, expose arbitrary algorithm strings, add a generic
Hasher interface, or expand this consumer into SHA-512 streaming without need.
EVP directly supplies the incremental operations and reports failures; mapping
those failures to abort is Align's existing digest policy, not an OpenSSL
guarantee of infallibility. [OpenSSL EVP documentation](https://docs.openssl.org/3.0/man3/EVP_DigestInit/).

Track total input bytes with checked unsigned arithmetic and reject a total
greater than `2^61 - 1` with the same hard-error policy before the failing update.
This avoids promising a standard SHA-256 message beyond its length domain.
Zero updates and empty updates are valid. A helper may borrow the context
exclusively and borrow a chunk simultaneously; the chunk expires immediately
after the call. `finish` moves and nulls the source on its evaluated path;
early exits before the consuming call retain ordinary cleanup ownership.

The resource promise is **context storage independent of total input length**,
not zero provider allocations or a specific throughput. Context/provider setup
is charged, and the 32-byte result is separately owned. Measure retained bytes
over increasing total input with a fixed chunk and inspect wrapper allocations;
do not claim exact process RSS from those counters. Use known empty/abc vectors,
chunk partitions around 55/56/63/64/65 bytes and repeated updates; a handful of
partition-invariance cases replaces the request's quadratic suggestion of
testing every boundary of every multi-megabyte message. Hashing logical file
content by positional writes remains application ordering work: update order is
message order, not automatically final file-offset order.

### 4.2 Ordinary filesystem and host facts

F1 preserves the distinction between missing path, non-directory and denied
observation. The original R53 `store[denied]` acceptance cannot follow solely
from `is_dir`: an accessible directory may still reject a create due to mode,
ACL, read-only mount, quota or a later race. The consumer must perform the actual
write admission it needs, or report the eventual create failure. No library
predicate guarantees future write success.

O1 exposes the kernel/system identity as seen by this process. It does not parse
`/etc/os-release`, invoke subprocesses or infer a CPU model from architecture.
`system`, `release`, `machine`, optional description and online count have
independent meanings. Use `uname` and the platform online-count query, with
native Linux/macOS oracles. The application owns lowercase/canonical mappings
and a new environment schema/digest when those meanings differ from the Python
probe. Keep `process.cpu_count()` for available parallelism. The OS query's
online count is not a claim about an enclosing physical machine.
[uname](https://man7.org/linux/man-pages/man2/uname.2.html),
[sysconf](https://man7.org/linux/man-pages/man3/sysconf.3.html).

### 4.3 Borrowed Move-element slices

V1 should be a representation/provenance extension, not a new reference model.
The physical source still owns each record/string. Slicing copies only the
existing pointer/length view. Indexed access projects a readable place, then
uses the existing Copy-leaf or owned-string-to-`str` rule; explicit shared
`borrow` helpers receive the address of that place. A whole Move record cannot
be loaded into a value, returned by value, stored by value or passed to a
Copy-element pipeline merely because a view now exists.

Close formation and consumers together: type annotation, array-to-slice
coercion, range slicing, re-slicing, field projection, borrowed helper, helper
return and generic/per-unit transport. Include the sibling `array<string>` /
`slice<string>` physical-String/logical-Str case in the classification sweep.
No new owning array element family follows from admitting a view. Move resource
elements with no existing readable-place operation remain outside the boundary;
the ledger must distinguish that from refusing already-supported record leaves.

The lifetime includes the collection storage generation and transitive field
owners. Source replacement, element replacement, move, Drop and exclusive
borrowing invalidate overlapping live views under existing conservative rules.
Later eager operands must not invalidate a receiver/index/helper argument that
has already been evaluated. Read-only slicing must never manufacture `out`
authority, a mutable field place, element Drop or a dynamic-array owner.
Representation reuse is subject to HIR/MIR validator proof; a necessary IR
change must be recorded before implementation, not smuggled through a cast.

## 5. Retained filesystem design boundary

F2 is one end-to-end capability: directory ownership, complete enumeration,
metadata and relative opens/mutations. A directory-handle-only precursor would
be dormant for this consumer and duplicate ownership proof. Before coding,
complete these exact fields in its own ledger:

| Axis | Recommended contract to close |
| --- | --- |
| Root admission | Explicit path opens one no-follow directory owner; no process cwd change. Preserve plan 29's component-by-component no-follow observation, directory-kind rejection, open and descriptor type/identity revalidation; every later operation is relative to a retained descriptor. |
| Byte grammar | Nonempty relative bytes, no NUL, leading slash, empty component, `.` or `..`; reject the complete lexical path before I/O. Accept all other bytes, including invalid UTF-8. No normalization, decoding fallback or cwd resolution. Linux/macOS resource limits may still return an OS error. |
| Enumeration | A fresh independently positioned directory stream, not `dup` sharing another cursor's seek position. Each next call owns one raw basename. Skip only `.`/`..`; unspecified OS order; no automatic sorting or stat. Returned entries survive cursor advance/Drop. Error is terminal for that cursor until Drop and is never represented as None; EOF remains EOF. |
| Completeness | Enumerate all entries of an unchanged, accessible directory. Concurrent mutation is not an atomic tree snapshot and may change enumeration. Never silently omit an unrepresentable name. Linux must qualify raw non-UTF-8 creation; native macOS rejection of some names does not justify filtering names already returned by its OS. |
| Metadata | Separate no-follow named observation and descriptor observation on directory/reader/writer/file. Close exact kind variants, unsigned identity/link widths, mode bits, signed size and timestamp units, overflow rules and unavailable fields. Device/inode identities are local observations, not portable persisted identities or unforgeable deletion tokens. Do not trust `d_type` as complete metadata. |
| Opens | Relative directory/regular reader/exclusive writer constructors; reuse existing reader/writer ownership and plan 29's admission sequence. For a regular reader, first observe the final name no-follow and reject every observed nonregular kind before open. Then open read-only/nonblocking/no-follow/CLOEXEC, revalidate descriptor kind and device/inode identity, restore blocking mode, and only then construct the reader. Named pre-observation rejects unsafe candidates; it never authorizes an unchecked open. Reuse R55's final descriptor single-link predicate when required. Exclusive creation uses the existing non-replacing native create, with its own output-kind/identity checks. |
| Mutation | One nested-directory creation at a time; explicit mode application; separate file and empty-directory removal. Close permissions, umask interaction, symlink handling and failure order per operation. No recursive traversal/removal in native code and no process-global umask changes. Descriptor-relative operations must not silently fall back to full path reopening. |
| Allocation | One handle and bounded native enumeration storage per cursor, one owned name per returned entry; no native array of the complete tree. Native path marshalling can allocate proportional to explicit path length. Application-held paths, sorting and manifests are application allocations. |
| Error precedence | Complete lexical validation, then root/ancestor admission in order. A regular-reader final path follows no-follow observation, pre-open regular-kind rejection, open, descriptor type/identity revalidation, blocking-mode restoration, optional single-link predicate, result publication. Named metadata and exclusive creation have their own operation-specific branches, without a fabricated pre-stat success. Record exact errno mapping for each operation; do not turn denied/vanished into false/EOF. Cleanup preserves the operation's error while closing partial owners. |

An open descriptor retains an object across rename; it does not prevent writes
to that object by another descriptor. Metadata sampled before/after reading can
detect changes but is not proof of an atomic immutable snapshot. FILE_SET
identity and acceptance must state the trusted source immutability/quiescence
precondition or use an explicitly frozen copy. App byte sorting, traversal limits
and streaming H1 produce the manifest; `std.fs` knows nothing about FILE_SET.
[openat rationale](https://man7.org/linux/man-pages/man2/openat.2.html).

Preserve plan 29's separate device-open limit: pre-observation prevents opening
an already-observed special entry, and nonblocking open prevents a FIFO swap from
waiting for a peer. A hostile device substitution after observation can still
have effects during native open before descriptor validation rejects it.
No special descriptor is published and no file bytes are read on that rejection;
side-effect-free hostile device admission requires the existing single-writer
directory precondition or stronger deployment isolation. The retained-executable
input path inherits these admission limits as well as its later immutability proof.

Cleanup is explicit application control flow: prove the owned processes absent,
retain root and parent identities, exclude other writers to that private parent,
then enumerate and remove bottom-up. Directory Drop only closes descriptors.
Mode 0700 excludes other users, not same-UID processes. Neither a pre-stat check
nor an open directory fd turns `unlinkat(parent, name)` into atomic
identity-conditional deletion; the existing plan 36 already records that limit.
If child absence or exclusive-parent control fails, keep the workspace and report
cleanup failure. An observed matching inode does not justify deleting a later
replacement. The parent must not be writable or otherwise controllable by the
sandboxed workload. External same-UID/root interference requires an explicit
threat-model exclusion or stronger host isolation.

## 6. Process architecture and unresolved proof

### 6.1 Responsibility boundary

| Align runtime / standard library | Align application / consumer deployment |
| --- | --- |
| OS descriptor ownership, immutable-object sealing, exec from admitted identity, exact inheritance mapping | Expected digests, approved tools, command arguments, env/cwd selection, payload formats |
| Bounded nonblocking read, readiness, nonblocking wait, stable signalling identity | Two-stream draining, byte caps, deadline checks through EOF/reap, cancellation policy |
| Individual authenticated process facts, reaping, exclusive lease ownership and release facts | Resource summation, maximum comparisons, scan schedule, first execution error and separately recorded cleanup error |
| Typed namespace/containment admission and OS teardown operations | Sandbox command assembly, installed tool trust, platform qualification, success publication and workspace cleanup |

Expose operations, not a native `run_supervised`, scope report, Python-runner
translation or product-specific limit bundle. Also avoid one global generic
event/iterator framework or a second child ownership implementation. P2 should
extend the existing child owner and share pipe/launch primitives with command
capture. Whether existing synchronous `run` is later expressible in Align is a
separate implementation decision, not a prerequisite for this request.

### 6.2 Retained identity and immutable inputs

A verified pathname must resolve once. Hash and execute the same admitted object;
do not re-open the original pathname after verification. `execveat` with
`AT_EMPTY_PATH` executes an fd-selected image, but does not freeze its bytes.
Linux sealing can prevent writes/growth/shrink through every descriptor to the
sealed inode. Copying into a sealable object is therefore an explicit operation
with explicit storage cost, followed by hashing the sealed contents, then launch.
It is not a hidden side effect of `command` or `borrow`.
[execveat](https://man7.org/linux/man-pages/man2/execveat.2.html),
[file seals](https://man7.org/linux/man-pages/man2/F_GET_SEALS.2const.html).

For large inputs, the constructor must support application-driven chunk writes
and a consuming seal transition; requiring a whole additional input-sized byte
array defeats H1. Finalized inputs provide bounded reads for hashing and no
mutable view. Specify whether each inherited read has a new open-file position
or shares one; `dup` alone must not masquerade as independent reader positions.
Explicit inheritance validates slot range, reserved slots and duplicates before
launch, installs the complete mapping safely even through cycles, and closes all
unlisted descriptors in the child. Parent-owned descriptor flags/offsets must not
be mutated as an inheritance shortcut. Raw fd integers passed to a tool are
labels for already-authorized slots, not constructors for authority.

Executable freezing needs separate qualification from data sealing: memfd exec
policy, executable mode, set-ID/capability treatment, dynamic-loader dependencies
and origin-sensitive loading may affect behavior. The first verified-exec
contract should admit native binaries and reject script magic; scripts need an
explicit admitted interpreter and sealed script input. The ELF interpreter and
shared libraries remain the deployment's admitted runtime closure, not something
the main executable digest attests. No silent `/proc/self/fd` or pathname fallback
may weaken selected guarantees. Seal flags and executable-versus-data permissions
must be fixed from the supported platform profile.

Observing the currently executing product requires an OS-derived source, such
as Linux `/proc/self/exe`, not a caller-supplied path or argv[0]. This identifies
the main executable backing object, not all loaded libraries, memory patches or
source provenance. If access is unavailable, return an error; a pathname guessed
from application configuration is not equivalent evidence.
[proc_pid_exe](https://man7.org/linux/man-pages/man5/proc_pid_exe.5.html).

### 6.3 Live-child primitives

Use caller-supplied bounded storage. A pipe read result must distinguish positive
byte count, WouldBlock and EOF with ordinary sum/Option types; zero-capacity
buffer behavior and hard I/O errors must be specified. Readiness is only a hint;
the caller still handles WouldBlock. A finite wait uses monotonic remaining
time and the existing overflow-safe poll conversion. Signal/reap handle state
must prevent signalling a recycled PID. Waiting is idempotent only through a
stored completed status, not repeated `waitpid` on the old PID.

P2 must close what `timeout_ns` and `max_capture_bytes` mean on an asynchronous
launch: silently ignoring existing command settings is invalid. Recommended
design is to reject incompatible configured capture/timeout settings before
spawn, because the application loop explicitly owns those policies. Select the
exact fallible launch shape and diagnostic/error in the public ledger. Include
an exec-error handshake; the strong admitted launch must not report a failed
exec as a successful child exit 127. Preserve the documented existing synchronous
surface unless its behavior is explicitly changed in that same ledger.

The application loop's completion condition includes both EOFs, direct-child
completion and successful containment cleanup. It checks the deadline even
after both pipes reach EOF. It bounds each drain step so one stream cannot
starve the other or postpone a timeout. The application retains its first
execution error and records cleanup failure separately; no runtime product
result selects that precedence for it.

### 6.4 Containment mechanism selection

The current process-group capture contract explicitly excludes `setsid`
descendants. R65 requires more. A subreaper changes orphan adoption; it does not
itself isolate invocations or prevent descendants escaping a procfs scan.
Use process pidfds without `PIDFD_THREAD` for process completion: the OS documents
readiness after the final thread exits, unlike the thread-specific handle.
An observed zombie leader or an empty output pipe is not full child absence.
[subreaper](https://man7.org/linux/man-pages/man2/PR_SET_CHILD_SUBREAPER.2const.html),
[pidfd_open](https://man7.org/linux/man-pages/man2/pidfd_open.2.html).

| Mechanism | Decision |
| --- | --- |
| Process group + kill/wait | Retain for the existing documented command contract; insufficient for R65. |
| Process-wide subreaper + repeated raw PID/PPID scans | Useful observation/adoption, insufficient alone as authenticated membership and isolation. Do not reproduce the Python algorithm verbatim as a claimed safe primitive. |
| Dedicated PID namespace with retained, authenticated init/process ownership | Preferred qualification path because the consumer already uses a Linux namespace sandbox. Prove admission/handshake, init identity, all descendants/threads, launch failure and teardown. The launcher process may differ from namespace init; an arbitrary nsfd or launcher exit cannot certify absence. |
| Delegated cgroup v2 plus exclusive reaper ownership | Alternative only if the real deployment explicitly provides trusted delegation and forbids migration out. Kernel `cgroup.kill` handles the subtree, but cgroup emptiness does not prove zombies reaped. Do not impose this new host prerequisite silently or use it as an automatic fallback. |

The kernel kills processes in a PID namespace when its init terminates. That
is a useful kernel boundary, not by itself a complete library release protocol:
bind the correct namespace/init, prevent admission bypass, observe completion,
reap what the owner must reap, and cover failure before init exists.
[PID namespaces](https://man7.org/linux/man-pages/man7/pid_namespaces.7.html).
The cgroup alternative likewise needs admission and reaping beyond kill/empty;
zombies are omitted from `cgroup.procs`.
[cgroup v2](https://www.kernel.org/doc/html/latest/admin-guide/cgroup-v2.html).

Prefer an application Align worker dedicated to one containment owner when
process-global subreaping would otherwise conflict with unrelated work. If the
public primitive leases the caller's process-global subreaper state, reserve it
atomically before querying/changing native state, record the previous value,
reject a second lease before any change, and prevent unrelated Align child
launch/reap APIs from bypassing the lease. Existing unmanaged children, foreign
FFI spawning/reaping, SIGCHLD configuration and outstanding live-child owners
need explicit admission/exclusion rules. A mutex around constructor calls alone
is insufficient. Successful release restores prior state only after no further
adoption can occur and all owned reaping is complete; failed construction
restores whatever state it changed without releasing another lease's reservation.

This is the principal remaining design gate. Qualify one selected mechanism
against the actual supported Linux x86-64/ARM64 deployment before freezing P3.
The generic pipe/status layer can support macOS, but R65's strict Linux
containment must return an explicit unsupported error on macOS until an equally
strong native profile exists. A portable-looking process-group fallback is not
an acceptable implementation. Do not promise bounded successful cleanup for a
kernel task stuck in uninterruptible I/O.

### 6.5 Drop and failed release

Explicit cleanup belongs in the Align loop so it can report failure and retain
the workspace. Drop remains a safety net, not an acceptance result. Its exact
rule must be selected before P3 implementation: signal only authenticated owned
members, close owned pipes/descriptors, and preserve exclusive reaper ownership
until absence/reaping/restoration are proven. It must never restore global state
early, silently detach children, unlock another owner or delete a workspace.
An inability to finish needs a specified fail-stop or retained/quarantined-owner
policy; ordinary fallible explicit release must keep a retryable owner on error
rather than consuming away the only cleanup authority. Existing `child` Drop
blocks in waitpid and cannot be reused unchanged while claiming bounded Drop.

Abrupt cancellation means normal cancellation/early exit while the owner can run
cleanup. SIGKILL, abort and fatal OOM have no language Drop guarantee. If the
consumer requires absence when the supervisor itself is killed, qualify an
external/kernel parent-death mechanism as part of the selected deployment; never
claim that RAII alone provides it.

## 7. Implementation closure matrix

This is the author-side matrix for the proposed boundaries. It names proof
owners, not completed tests. New owner names below are **planned**; reuse existing
parameterized coverage where it already fails for the changed defect. Each
capability's exact ledger must bind these cells to its implementation and owner
before its code review. No unchecked runtime-only prototype closes a compiler
transport cell.

| Axis | Required closure | Proposed owner / deferred boundary |
| --- | --- | --- |
| Formation and validation | Exact type/import/arity/mode, finite types, shared carrier classifier; kind/handle confusion rejects. F2/P1–P3 signatures and full native value domains remain unclosed. | `m11_crypto_stream::formation_and_carriers`, `m9_fs` retained-tree formation matrix, `m11_os::host_contract`, `m11_process_live` / `m11_process_containment` formation matrix; sema checked-HIR mutation owners. |
| Construction and partial failure | Fail each native allocation/open/dup/pipe/seal/exec preparation stage; no half-owned result, stray fd, child or changed global state. After fork, the owner must retain cleanup authority even if launch admission fails. | Native `digest_context_failures`, `retained_tree_constructor_failures`, `host_query_failures`, `verified_launch_failure_matrix`, `subreaper_lease_failure_matrix`. |
| Move-in/out and source nulling | Ordinary transfers through parameters/results/records/Option/Result null exactly the selected source; overwrite drops previous initialized value once. Borrowed reads never null or create ownership. | Driver carrier matrix in each new owner, reused `borrowed_replacement` / `borrowed_params`; allocation/fd counters with positive witnesses. |
| Control flow | `if`, `match`, `else`, `?`, `map_err`, branch/loop joins, return/break/early exit; both successful and failed partial constructions. Cross reachability, terminating alternatives, late expected types and already-invalid children where relevant. | Parameterized `owned_boundary_control_flow` in the owning target; existing bound-if/else regressions reused where they exercise the same carrier. |
| Borrowed lifetime and eager operands | Source expiry/replacement, independent source positive, helper return substitution, later-operand invalidation, readonly versus writable capability. Include String and owned-record sibling projections. | Existing `consumer_borrow_boundaries`, `borrow_liveness`, `struct_slice_fields`; new `move_slice_place_matrix` and digest/launch borrowed-input cases. |
| Drop/replacement/return | Exact recursive cleanup, cursor independence, finish consumption, live-child fallback, failed release and restore ordering. P3 Drop policy is explicitly unclosed; no implementation until selected. | Native drop counters; `m11_process_containment::release_and_drop_matrix`; move-slice Drop parity proving only the source drops elements. |
| Malformed HIR/MIR | Forged receiver/result/discriminator, byte-view extent, ownership carrier, cursor/reader mix-up and physical/logical element type reject before LLVM. No unchecked enum wildcard or lossy native length conversion. | `align_sema` variant sweep and HIR owner; MIR validation/mutation owner; `align_codegen_llvm` validation entrypoints, per runtime operation family. |
| Generic/interface/cache | Imported borrowed helpers and returned owned records; generic instantiations; whole-program versus per-unit cold/hit/private edit/restore parity. Native handle type identity is nominal; any structural record fingerprint includes the complete reachable graph. | Driver whole/per-unit parameter in each capability owner; existing frontend-cache owner with one relevant new type/carrier witness. |
| Native ABI/provenance | Every symbol has typed RuntimeKey/declaration/signature/effect/link capability; output slots initialized before fallible work; allocator/deallocator match; source views valid throughout native call. | `19-hir-validation-ledger.md`, `20-runtime-abi-ledger.md` and their structural owners; no handwritten symbol escape. |
| Filesystem adversary | Raw names, disappearance/denial, symlinks/FIFO/device, hardlinks, ancestor/final replacement, independent cursors, modes and explicit bottom-up cleanup preconditions. An observed special file never reaches native open; a post-observation replacement is revalidated and never published/read, within the stated device-open threat boundary. | `m9_fs` retained-tree matrix and native race/failure owners with an open-call witness; Linux raw-name oracle, macOS admitted-name/refusal oracle. |
| Process adversary | Executable replacement versus same-inode mutation, seal failure, fd remap cycles/leaks, spawn/exec failure, dual-stream pressure, EOF-live timeout, setsid, adopted zombie and live thread group, cancellation, two-owner isolation, current-exe mismatch. | `m11_process_live`, `m11_process_containment`, native owned-process failpoints; explicit independent external process observer in the qualification harness. |
| Resource observation | Each process sample identifies exactly one authenticated process, units and unavailable fields; sampling is not an atomic aggregate or guaranteed peak. Consumer sums/limits require conservative failure behavior. | Process observation native oracle; consumer `resource-ceilings`, with a separate oracle rather than assertions against its own formatter. Exact fields remain P3 contract work. |
| Storage/performance | H1 retains no growing preimage; F2 native cursor stores no complete tree; P2 stores only caller-selected read/capture capacities. No throughput, task-start latency or cleanup-time claim. | One fixed-chunk increasing-input digest measurement; cursor allocation and pipe-capacity invariants. No benchmark for ordinary host fields or prose changes. |
| Consumer adoption | Original pack/KV owners stay distinct from new cutover owners. Align merge alone never proves adoption or closes the requests. | Registered `alignpack-qualification` / `make alignpack-smoke`, R53 R6 owners, planned product-cutover functional/containment/no-python owners, and R67 edit-admission owner when available. |

No public wire format, CLI option or application cache format is added by this
plan. Native types/operations still affect compiler/runtime artifacts. At each
implementation boundary update canonical type/interface serialization and
runtime/build cache identities as required, with old incompatible artifacts
rejected. Allocate tags and version changes in the exact ledger before writing
encoders; if its persisted format changes, provide independent semantic-to-byte
and byte-to-semantic golden vectors. Do not predict a version number here or
add compatibility decoders. Environment/FILE_SET/report canonical bytes remain
consumer-owned and cannot silently change during a provider pin update.

## 8. Delivery plan and readiness gates

| Order | Work and useful completion boundary | Required condition before code |
| --- | --- | --- |
| 0 | Finish design closure, with R65 Linux mechanism qualification selected first because it can invalidate the consumer cutover. Recover external candidates only as reviewable inputs if available. | Confirm the real deployment's namespace/reaper permissions and absence proof; fix P1–P3 fields, state machine, global-state/Drop policy and errors. Close F2's signature/metadata ledger. This current proposal is not permission to bypass those steps. |
| 1 | Incremental SHA-256, compiler-to-runtime plus owners (H1). Useful immediately to source hashing and existing pack/KV clients. | Exact digest ledger and carrier/ABI matrix reviewed; no file/pack format change. |
| 2 | Retained filesystem with ordinary directory creation/type observation (F1/F2). Useful to source/snapshot traversal and nested workspace construction. | Complete one filesystem ledger including byte-path rules, metadata widths, mode policy and concurrency preconditions. Implement producer, validators, native consumers and tests together. |
| 3 | Host facts (O1), a small independent capability. May be scheduled before 2 if it does not delay the blocking filesystem/process work. | Close Linux/macOS query mapping, ownership, failures and schema changes required in the consumer. |
| 4 | Verified asynchronous contained-child capability (P1/P2/P3), including a small Align application composition fixture. | The mechanism proof and exact public ledger from 0 pass a fresh adversarial design review. The fixture must express dual-stream drain, deadlines, observation and cleanup without native policy code. |
| 5 | Read-only Move-element slice capability (V1), independent and nonblocking. | Complete the physical-element/readable-place classifier and matrix before removing any rejection. R63 is not a prerequisite. |

These are capability boundaries, not one PR per request or compiler layer.
Filesystem F1/F2 should share one proof rather than duplicate native admission.
The process work will likely exceed 1,000 handwritten lines: keeping launch,
membership authentication, reaping and release together avoids a dormant owner
with no safe terminal operation and repeated proofs across ownership layers.
Record the actual estimate and boundary rationale before coding. Split only if a
smaller boundary has a stable real consumer and a separate failure domain; P2's
general streaming caller could justify such a boundary, while a private P3
producer with no usable cleanup consumer cannot.

For each accepted capability, first finish the author ledger-to-prose pass and
one independent adversarial design review when the public/safety contract changes.
Then implement one coherent candidate, run the narrow owner and Align self-review,
and commit. One fresh inspection review may run alongside the required owner,
bounded gate and Clippy on that same candidate. Fix the complete finding class in
one pass, run final-SHA preflight, then use the normal PR wrappers if publication
is authorized. Do not use CI as the investigation loop. Service/platform-dependent
checks need their matching local harness before any new required CI job.

Owner commands use `scripts/cargo.sh test -p align_driver --test TARGET`, plus
only the changed library/runtime owners. Code preflight supplies
`scripts/test-pr.sh` and Clippy with `CARGO_TARGET_DIR=target/clippy`; keep the
30-minute hard suite budget. Linux-specific containment tests must state the
required capabilities and distinguish unsupported-host refusal from a successful
containment qualification. Required Linux/macOS compiler/platform coverage stays
in the usual CI. No compiler build or runtime test is selected for this design
proposal itself.

Update the request register after design acceptance and after Align delivery;
preserve the difference between externally reported IMPLEMENTING, Align-merged,
and consumer-verified. A completed implementation batch runs exactly
`cargo build --release --workspace` under the repository's align-llm rule.
This design-only task does not run a release build, bump a version, tag, publish,
or edit align-llm code. Consumer adoption later rebuilds/pins once for the required
merged wave and runs its own actual feature/integration owners.

## 9. Contract propagation and current completion state

| Capability | Sources to synchronize when its design is accepted |
| --- | --- |
| H1 | Exact digest ledger; `std-design/crypto.md` and `ja/crypto.md`; `draft.md`; `docs/language-spec.md`; `docs/design-notes.md`; corresponding Settled entry; HIR/runtime/interface ledgers where changed. |
| F1/F2 | Exact retained-tree ledger; `std-design/fs.md` and `ja/fs.md`; plans 29/34/36 only where the new contract changes their promises; specification/digest/design notes/Settled; HIR/runtime ledgers. Preserve existing UTF-8 `read_dir` semantics explicitly. |
| O1 | New `std-design/os.md` and Japanese mirror; module catalog/specification/digest/design notes/Settled; owned-record HIR/runtime contract. |
| P1/P2/P3 | Exact verified-process ledger; `std-design/process.md` and `ja/process.md`; fs design only for shared immutable-input ownership; specification/digest/design notes/Settled; native-boundary plan 17 and HIR/runtime/interface ledgers. |
| V1 | Exact Move-slice ledger; specification/digest and array-slice design/mirror where normative text changes; plan 30's deferred boundary; design notes/Settled and validation ledger. No runtime-ABI change unless the final implementation actually needs one. |
| J1 | Only after reopen eligibility: plan 25 first, then core JSON design/mirror, specification/digest/design notes/Settled, descriptors/ABI and independent goldens. |

The present assessment does not amend a Settled entry or ship an API, so it links
those sources instead of making accepted-looking changes to them. The proposal
has a complete request-selection decision and implementation sequence. Detailed
R64 public fields and R65 platform/lifecycle contracts remain explicitly open;
the next work is those bounded design/qualification tasks, not production code.
H1/F1/O1/V1 are candidate interface decisions, not approved capability contracts.

Author consistency pass: each recommended surface maps to the decision ledger;
open signatures/fields are marked unclosed in both ledger and matrix. Allocation,
owned/view lifetime, native error distinctions, observation-versus-authority,
global-state exclusion and source propagation each have an owning row. No
planned acceptance command is reported as executed. No unseen external review
is treated as approval of this proposal. Independent assessment follows this
author pass; it reviews the selected boundaries and remaining gate definitions.

Independent inspection found one P2 in the recommended regular-file admission
order: opening before rejecting an observed special file could cause device-open
effects. The coherent correction preserves plan 29's pre-observation rejection,
post-open descriptor revalidation and explicit concurrent-device-substitution
limit in the filesystem ledger, error order, executable-input boundary and owner
matrix. The reviewer found no other actionable issue in this proposal; no second
full review or implementation verification is claimed.
