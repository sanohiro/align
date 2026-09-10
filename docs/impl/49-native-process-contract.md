# Native process observation and control

Status: **IMPLEMENTING — common child capability and explicit signal subscription**.
Baseline: `4cb14895a06e67f32ee72383b53afe72cd8e5555`.
This ledger owns the common Linux/macOS capability selected from R65. It refines
[plan 48](48-process-portability-and-lifecycle-plan.md). No administrator account, registered service, privileged helper, VM,
background reaper or extra keeper process is part of this capability.

## Boundary

The owner rejected a privileged-service deployment on macOS. Native workflow
parity means the process operations that the mapped Python consumer performs on
both hosts. It does not turn Linux namespaces or memfd seals into macOS kernel
facilities. The verified-input and arbitrary-descendant requirements are supplied by the
explicit capabilities in [plan 50](50-r65-process-capability-handoff.md). This
common capability does not claim those stronger guarantees on its own.

The decisive separation is **observation versus reaping**. Observe direct-child
termination with `waitid(WEXITED | WNOHANG | WNOWAIT)`, retain the unreaped session
leader while signalling its group, then obtain the final status and usage with
one `wait4`. This permits post-exit group control without adding a keeper or
losing resource usage to a premature reap. A process group remains group-scoped;
descendants that leave it are not covered by a group operation.

The existing direct-child Drop contract remains: close owned capture reads, then
wait for the direct child. Drop may block, discards status, and makes no claim
about descendants. Applications explicitly signal groups and choose cleanup
deadlines, drain fairness, byte retention, hashes and failure precedence.

## Authoritative public ledger

All declarations require `import std.process`. Types in this ledger are qualified
nominal builtins, with no bare aliases. Method receivers are bound owned locals
or explicit borrowed parameters; existing place/borrow rules apply. A stateful
method requires an exclusive receiver. Shared borrowing is admitted only for a
receiver-readonly operation; OS effects may still make that operation Impure, as
with existing filesystem metadata methods.
Every argument is evaluated once, in source order, before its operation.
The two read destinations use the existing out/writable-place admission, including
its no-alias reservation against the receiver and every other argument. Require
writable backing storage; `mut` on a view header cannot make string bytes, shared
borrows, readonly mmap or another readonly origin writable. Preserve this
provenance through slices, helpers and whole/per-unit interfaces. Sema and HIR
validate it before producing a native call; MIR preserves the checked mutation
and no-alias effects. No new mutability model is introduced.

| Surface | Exact contract, defaults and errors |
| --- | --- |
| `process.termination` | Copy sum, variants in order `Exited(i64)`, `Signaled(i64)`. Native Exited values are 0..255, Signaled values are positive OS signal numbers. No stopped/continued events are returned. User construction is ordinary sum construction and grants no process authority. |
| `process.wait_result` | Copy natural record, fields in order `termination: process.termination`, `max_rss_bytes: Option<i64>`. Native wait4 maximum-RSS observation in bytes, not a process-tree sum or memory limit. Some is nonnegative and representable; absent/unrepresentable usage is None. |
| `process.readiness` | Copy natural record, fields in order `stdout: bool`, `stderr: bool`, `status: bool`. Used as requested interests and the returned ready subset. |
| `process.signal` | Copy sum with payload-free variants in order `Hangup`, `Interrupt`, `Quit`, `Terminate`, `Kill`. These name native HUP, INT, QUIT, TERM, KILL respectively. |
| `process.signal_set` | Copy natural record, fields in order `hangup: bool`, `interrupt: bool`, `quit: bool`, `terminate: bool`. Every field is explicit; no implicit subscription. |
| `process.signal_number(value: process.signal) -> i64` | Pure native mapping. Provides numbers for existing `ch.kill(i64)` and observed signal comparison. Kill is sendable but not subscribable. |
| `c.new_session(enabled: bool) -> ()` | Impure in-place command setter, default false. True creates a new session whose leader and initial PGID are the direct child; no parent session change. Latest value wins. |
| `c.stdout_to(destination: writer) -> Result<(), Error>` | Impure call-scoped borrow of an existing writer. Require a generic writable regular-file fd and an empty user-space buffer; otherwise Invalid. Caller uses `flush()` explicitly before binding. Duplicate its fd with CLOEXEC, then replace the old binding atomically. The command owns the duplicate; no source lifetime is retained. Failure preserves the old binding and source. |
| `c.stderr_to(destination: writer) -> Result<(), Error>` | Same semantics for stderr. Binding both streams to the same writer is allowed and shares its open-file offset; relative write ordering is unspecified. Neither call opens/truncates a pathname or flushes a writer. |
| `c.start() -> Result<child, Error>` | Impure borrowed/re-runnable launch. Stdin is `/dev/null`; each output is a capture pipe unless explicitly bound to a file. Reject positive timeout or any configured capture bound before launch. Live policy belongs in the application. The returned existing Move child independently owns native process state and its capture reads. |
| `ch.id() -> i64` | Pure Copy read of the original positive PID, also after reap. This is an observation, never a raw-PID signalling capability. |
| `ch.status() -> Result<Option<process.termination>, Error>` | Impure nonblocking termination observation; None pending, Some terminal. Does not reap, drain or release group authority. Repeated terminal observations return the same cache. After a successful wait it returns that wait's termination. |
| `ch.try_wait() -> Result<Option<process.wait_result>, Error>` | Impure nonblocking reap; None pending, Some final result. One wait4 captures status and usage. Success releases group signalling authority and caches the complete Copy result. Repeats return the cache; no second native reap. |
| `ch.wait() -> Result<process.wait_result, Error>` | Replace the existing i64 result outright. First check cached/nonblocking completion. If pending and any captured stream has not returned EOF, Invalid without closing/draining it. Otherwise block in the shared wait4 path. Newly observed completion can return while buffered pipe data remains readable. |
| `ch.read_stdout(out destination: slice<u8>) -> Result<Option<i64>, Error>` | Impure one-attempt nonblocking read. Some(n>0) data, Some(0) EOF, None pending or EINTR. Reject zero capacity or absent capture. Write only the n-byte prefix; leave the rest unchanged. No retained buffer or allocation. |
| `ch.read_stderr(out destination: slice<u8>) -> Result<Option<i64>, Error>` | Same contract with independent stderr/EOF state. |
| `ch.poll(interest: process.readiness, wait_ns: i64) -> Result<process.readiness, Error>` | Impure finite readiness wait. Negative budget, empty interest or requested absent capture is Invalid. Zero polls immediately. Unrequested flags are false. Cached EOF/termination immediately satisfies a requested flag. Status readiness does not reap. All false means the budget elapsed with no requested event observed. |
| `ch.kill(signal: i64) -> Result<(), Error>` | Retain existing source signature and direct-child meaning. Validate native signal domain without narrowing first; zero remains the POSIX probe. Reject reaped/lost ownership before signalling. No signal to a recycled PID. |
| `ch.kill_group(signal: i64) -> Result<(), Error>` | Impure group operation, valid only for a child created as a new session and still unreaped, including after status returned Some. Native signal validation matches kill. Signal the original pinned PGID; ESRCH is a native error, not a descendant-absence certificate. Successful wait or loss of reap ownership permanently disables this operation. |
| `ch.group_members(max_scan: i64) -> Result<array<i64>, Error>` | Impure explicit process-table observation, valid under the same unreaped new-session condition. max_scan must be 1..536870910. Count every candidate PID examined, including vanished candidates. Exceeding the budget is Invalid with no partial array. Return observed matching PIDs in ascending order with duplicates removed, including the leader if observed. Empty is an observation, not a stable absence guarantee. |
| `out.status() -> process.wait_result` | Pure accessor on both run_output and run_bytes; replace code() outright. The record shares the wait4 result contract. Output views keep their existing ownership and byte/text semantics. |
| `process.signals(selection: process.signal_set) -> Result<process.signal_subscription, Error>` | Impure explicit process-global lease for selected termination signals. Empty selection or another active subscription is Invalid. Allocate the owner and validate all inputs before installing handlers. No SIGCHLD changes. Construction failure publishes no owner and restores every installed disposition. |
| `subscription.next() -> Result<Option<process.signal>, Error>` | Impure nonblocking observation. Repeated instances of a signal coalesce. Return/remove one pending signal in signal_set field order; None means none observed. No arrival-order/count guarantee. Closed subscription is Invalid. |
| `subscription.close() -> Result<(), Error>` | Impure non-consuming, retryable restoration of previous dispositions. Success marks closed and releases the global lease; repeating close succeeds. Failure retains the owner/reservation and the exact remaining restoration work. Unread pending observations are discarded on successful close. |

All native failures use the existing errno-to-Error mapping; invalid contract
inputs use Invalid. No synthetic successful child with exit 127 is published on
cwd/exec failure. Apply that pre-release correction to process.spawn, command.run,
run_bytes and start through one launch implementation. A program that actually
exits 127 remains a successful launch with Exited(127).

Synchronous run/run_bytes reject explicit output-file bindings before launch:
their existing contract promises both captured streams. They accept new_session;
the existing timeout/cap requirement to create a group is retained even when the
new-session setter is false. Only an actual new-session child grants the live
kill_group/group_members capability. No captured-versus-redirected empty-result
convention is invented.

## State, validation and native mechanisms

Child state is `Running -> ObservedTerminal -> Reaped`, with `OwnershipLost` on
uncached ECHILD. Running can also go directly to Reaped. Termination observation
does not imply EOF. EOF does not imply termination. ObservedTerminal retains PID
and PGID identity; Reaped retains only cached observations and owned pipe reads.
No reaped/lost state may make a native integer PID/PGID signal call.

The common native ownership precondition is exclusive reaping of each owned
child. Safe Align exposes no wildcard waiter or SIGCHLD disposition setter.
Reject initial SIGCHLD=SIG_IGN or SA_NOCLDWAIT before launch. Foreign code must not
reap an Align-owned PID or install automatic reaping while it is owned, just as
it must not close an Align-owned fd. This is a native-interop lifetime obligation,
not a constructor-only check advertised as enforcement against arbitrary FFI.
Detected ECHILD disables further integer signalling and preserves an Error;
there is no claim of safety against a concurrent native violation before detection.

Native method validation order is receiver pointer/alignment and state, each
input range in declaration order, output scratch range/alignment and all forbidden
overlaps, semantic argument values, then OS observation/effects. Validate before
initializing scratch if initialization would overwrite an input or receiver.
Once ranges are valid, zero the complete scratch including dormant fields on
all success/error paths. Null buffer with positive length, negative length and
pointer-plus-length overflow reject before forming a Rust slice. Native callers
still owe valid allocations; arbitrary dangling pointers are not probed.

Launch order is command state and text/argv validation, incompatible settings,
signal/reap admission, source descriptor validity, native allocation and descriptor
setup, spawn/exec handshake, then owner publication. Text is UTF-8 with embedded
NUL rejected; executable and cwd are nonempty. Preserve existing explicit argv[0],
environment override ordering and PATH/cwd semantics. No shell interpretation.
Failed spawn closes all acquired descriptors; failed child setup is killed/reaped
before returning the original error. Runtime heap OOM retains the settled fatal
policy. Allocation precedes child publication, so no fallible allocation follows
publication without an armed native cleanup guard.

An Ok launch publishes an owned child, not proof that the target executed an
instruction. A child killed during startup may therefore be returned with
Signaled status. This is distinct from a reported setup/exec error, which is Err.
Do not invent an exec-success witness: EOF on a CLOEXEC error pipe can also follow
pre-exec death. A partial/malformed error record is Invalid; a complete error
record wins over subsequent child death. Ordinary program exit 127 is never used
to guess a setup error.

The executed image inherits only fd 0/1/2 in this common capability. File bindings duplicate
the selected open-file description, preserving parent descriptor flags/offset
without seeking. Subsequent independent parent writes share the native offset;
buffered writes become visible when the parent explicitly flushes. Prepare
private source duplicates above fd 2 before remapping, including when the parent
has closed one or more standard descriptors. Stage low-numbered acquisitions
above fd 2 and close only those temporary low slots, preserving every existing
parent standard descriptor. Child pipe write ends stay
blocking; only the parent's capture read ends become nonblocking.

The selected native launch paths are:

- Linux: marshal environment, argv, PATH candidates and fixed failure storage in
  the parent. Fork with a private descriptor table, remap standard streams, then
  use close_range with CLOEXEC on all descriptors >= 3. On ENOSYS/EINVAL use a
  syscall-only /proc/self/fd scan in the single-threaded child with fixed stack
  storage; close every numeric descriptor >= 3 except the scan fd and error
  writer, then close the scan fd. Malformed records or an unavailable fallback
  fail launch. No parent fd snapshot or RLIMIT-based close loop is sufficient:
  either can miss inherited descriptors. No allocator, environment mutation,
  formatting, user callback or lock-taking library routine runs after fork.
- macOS: untimed launch uses posix_spawn with POSIX_SPAWN_CLOEXEC_DEFAULT,
  explicit standard-fd file actions, child-only cwd action and
  POSIX_SPAWN_SETSID when requested. Timed synchronous launch first forks the
  owned workload process. Its child applies the pre-marshalled cwd, setsid and
  standard-fd dup2 operations exactly once, before trying executable candidates.
  It then uses POSIX_SPAWN_SETEXEC | POSIX_SPAWN_CLOEXEC_DEFAULT with a separate
  parent-built action list containing only inherit actions for fd 0/1/2 and the
  executed-image signal policy. No cwd, setsid, open or source-fd dup2 action is
  repeated by SETEXEC. Its exec copy may discard CLOEXEC sources before file
  actions, so those sources must already have been remapped in the forked child.
  Ordinary posix_spawn's non-exec copy retains them for its dup2 actions.
  SETEXEC replaces the owned PID; it creates no keeper or additional workload.
  The parent retains signalling authority while setup is pending. The error
  writer remains CLOEXEC in the forked child, available after a returned failure
  and absent from the committed image. Kernel exec closes every unlisted fd,
  including ones another parent thread opened during preparation. All attribute
  and file-action allocation occurs in the parent. The audited SETEXEC call
  marshals a stack descriptor and enters the kernel, with no child-side builder,
  PATH-searching spawnp, environment mutation or application callback.
  Setup failure is terminal; only executable-candidate lookup failures enter
  the existing PATH retry policy.

The fork paths' private CLOEXEC error pipe transports exactly eight bytes: ASCII
`ALPE`, followed by a positive errno as little-endian u32. For example ENOENT=2
is `[65,76,80,69,2,0,0,0]`; a decoder golden checks this independently. Read into a fixed nine-byte buffer until EOF or a ninth byte. Only EOF after
exactly eight bytes validates an error frame. EOF with zero bytes publishes the
owned child; lengths 1..7 or at least 9, unknown magic, zero or out-of-i32 errno
are Invalid. Thus `[65,76,80,69,2,0,0]` and
`[65,76,80,69,2,0,0,0,0]` reject, independently of pipe chunk boundaries. Writes retry EINTR/short progress without
allocation. A write failure cannot manufacture an exec error at the parent;
terminate the child and allow the parent to observe its actual termination.
Direct macOS spawn reports setup/exec failure through its native result; the
fork/SETEXEC path writes that returned errno into the same error frame. Both paths
prepare cleanup authority before publishing a child, and the owner injects
termination at each startup boundary.

Synchronous run/run_bytes start their existing monotonic timeout immediately
when the parent obtains the forked PID, before reading the exec-error channel.
Poll that channel nonblocking with the remaining budget. At each checkpoint,
an expired deadline wins over a newly readable error frame or EOF; an already
selected earlier error is preserved during cleanup. Timeout closes capture and
control reads, signals only the owned PID/group whose formation is confirmed,
and kills/reaps the direct child before returning Timeout. No blocking handshake
read or wait precedes the deadline check. A stopped pre-exec child and an
executable lookup that stalls must therefore reach the same cleanup path as a
running timeout. Untimed start retains no launch-duration promise. Neither path
promises a bound on kernel scheduling or uninterruptible termination/reaping.

Before fork, save and block all blockable signals in the calling thread; restore
its exact prior mask in the parent on success and every failure. Linux uses an
audited raw fork-equivalent syscall with SIGCHLD and no shared VM/fd table, so
pthread_atfork callbacks are not invoked. Its child uses only native syscalls,
resets every caught disposition to default while signals remain blocked,
preserves ignored dispositions, and restores the original thread mask immediately
before exec. Inspect the child's inherited actions, not a stale parent snapshot.
No parent signal-subscription handler may run inside this bootstrap.

macOS's timed fork uses libSystem fork and its system-maintained fork machinery;
foreign application pthread_atfork callbacks that perform unsafe work violate
this native-interop precondition. Safe Align exposes no callback registration.
After fork, use only the setup-once syscalls and audited SETEXEC path. Spawn attributes restore
the saved mask and reset caught dispositions before the new image runs, retaining
ignored dispositions. Direct native spawn uses the same executed-image signal
policy. The runtime's signal installation and launch admission share a short
reservation while constructing the native snapshot; that reservation never
covers the child's execution or the parent's handshake wait.

These mechanisms follow [Linux close_range](https://man7.org/linux/man-pages/man2/close_range.2.html),
[XNU spawn file actions](https://github.com/apple-oss-distributions/xnu/blob/main/bsd/kern/kern_exec.c),
[XNU descriptor copying](https://github.com/apple-oss-distributions/xnu/blob/main/bsd/kern/kern_descrip.c)
and [XNU fork modes](https://github.com/apple-oss-distributions/xnu/blob/main/bsd/kern/kern_fork.c).
Native macOS execution of the selected path remains an implementation acceptance
test; source inspection alone is not a claim that it has run here.

Status uses waitid(P_PID, pid, WEXITED|WNOHANG|WNOWAIT) and zero-initialized siginfo.
Normalize the native cause to Exited/Signaled; interrupted observation returns
None. try_wait likewise returns None on EINTR; wait retries interruptions only
after its blocking-admission check. wait/try_wait use wait4 for the same direct child; Linux KiB multiply by
1024 with checked conversion, Darwin bytes unchanged. The reap result must agree
with cached observed termination, or return an invariant error without inventing
a second result. Capture this result once; no getrusage aggregate delta or second
wait can substitute for the child's usage.

Readiness uses retained native process identity where available: Linux pidfd and
macOS kqueue EVFILT_PROC/NOTE_EXIT alongside captured-pipe readiness. Registration
must handle an already exited child by checking unreaped status. The timeout is
a monotonic start-plus-budget with overflow-safe subtraction and rounded native
chunks; EINTR recomputes remaining budget. No deadline restarts, process-table
scans, output capture or extra processes occur in read/status/poll. Actual
scheduling latency is not a real-time guarantee.

Group enumeration uses Linux procfs numeric entries and their stat PGID, or
Darwin libproc PID enumeration plus
`proc_pidinfo(pid, PROC_PIDT_SHORTBSDINFO, 1, ...)`: the final 1 requests zombie
lookup, and the short BSD flavor supplies PGID without a same-user restriction.
Require the exact native struct byte count, a matching positive PID and a valid
PGID; do not substitute PROC_PIDTBSDINFO. Vanished entries are
skipped; inaccessible/malformed metadata fails, never silently implies absence.
No raw returned PID grants a signal or reap method. The scan/allocation cost is
explicitly bounded by max_scan; exceeding it frees temporary/result storage and
returns Invalid. The application owns any decision based on this non-atomic scan.
On macOS allocate space for max_scan+1 native i32 PIDs and perform one
PROC_ALL_PIDS request; the public maximum keeps its byte length representable by
libproc's signed i32 argument. A full extra slot means Invalid, without a retry
that expands the budget. Ignore zero padding, reject malformed byte counts, and
inspect each positive candidate at most once. Linux stops at the first candidate
over the same budget. Temporary and result storage are O(max_scan); neither route
uses an unbounded sizing-and-retry loop.

## Signal lease closure

There is one process-global subscription reservation. Disjoint subscriptions also
reject while it is held. Reserve it before changing native dispositions; query
all four selected old dispositions before installing in field order. Reject a
selected signal blocked in the calling thread. Do not change any thread's mask.
Delivery to another thread may be observed by next, but is not a wake guarantee
for a concurrently blocked child.poll; the application chooses finite budgets.

Handlers only preserve errno and update process-lifetime lock-free atomic storage.
They allocate nothing, invoke no user callback, write no fd and implement no
forwarding/deadline/first-error policy. A lock-free u64 stores selected mask in
bits 0..3, pending mask in 4..7 and a 56-bit generation in 8..63. All accesses use
sequentially consistent atomics. Generation zero is inactive; successful
reservation increments the generation before installation, never wrapping.
Exhaustion is Invalid before any disposition changes.

A handler captures this word on entry and CASes its pending bit only while that
captured generation and selected bit remain active. On generation/mask change
it returns without publishing. next CASes away the lowest pending selected bit,
preserving all other bits. Enable a selected bit before installing that signal's
action; rollback disables it if installation fails. The reservation prevents a
second constructor from observing a partially installed subscription.

Observation belongs to the lease active when the handler loads its generation,
not the time another process sent the signal. POSIX supplies no send-time lease
identity: a handler delayed until after replacement may observe the new lease.
An invocation that already captured an old generation cannot publish to the new
one. Close may discard in-flight/unread observations, just as it discards pending
bits; the API does not promise an atomic process-wide signal-delivery handover.

On partial construction failure, restore installed actions in reverse order and
release the reservation only after full restoration. Re-deliver the signals
whose pending bits were actually published during construction under their
restored dispositions; ordinary signal termination may then
prevent return. If rollback cannot restore process-global state, fail-stop using
the existing runtime invariant-abort path rather than return an unowned handler.

Explicit close restores installed actions in reverse order, recording each
successful restoration. Failure retains the global reservation and owner; retry
continues only unfinished restoration. Once closed, clear pending state and
release the reservation. Drop performs the same restoration; an unrecoverable
restoration failure aborts. Foreign disposition mutation during the lease is an
interop lifetime violation; detected replacement is Invalid and is not blindly
overwritten. No native handler ever references freed owner memory.

signal_subscription is a non-Send opaque Move owner. Its carrier grammar is the
existing digest grammar: locals, owned/borrowed helper parameters, returns,
natural structs/sums/Option/Result and the existing owned-record array routes.
Direct owner collections, tuples/boxes, out, globals, layout(C), equality/hash,
printing and closure/task/parallel capture reject. Moves null the source, whole
replacement restores the old lease first, and Drop-only/imported paths retain
the runtime entry. All its operations are Impure. Copy records/sums above use
ordinary Copy carrier rules and do not confer authority when constructed by users.

## Native ABI and compiler inventory

Reuse child, command, run_output and run_bytes as opaque pointer owners. Add only
Ty/Scalar ProcessSignalSubscription, nominal spelling process.signal_subscription,
canonical one-byte tags Ty=76 and Scalar=54; interface-12 named encoding remains.
No helper artifact, external protocol or application format is added. Complete
nominal definitions of all Copy records/sums participate in interface/cache
identity; user lookalikes are not native-producer schemas.

The 64-bit natural termination layout is 24 bytes/alignment 8: i32 variant tag at
0, four zero padding bytes, Exited i64 at 8, Signaled i64 at 16. This follows
Align's flattened sum payload layout, not a C union. Inactive native payloads are
zero. wait_result is 40 bytes/alignment 8: termination at 0, max_rss Option u8 tag
at 24, seven zero padding bytes, i64 at 32. Option<termination> is 32 bytes with
tag at 0 and payload at 8; Option<wait_result> is 48 bytes with the same outer
offsets. Option<i64> is 16 bytes; readiness is three u8 booleans (size 3/alignment
1); signal_set is four (size 4/alignment 1); signal is one i32 tag (size/alignment
4). Option<signal> is 8 bytes with tag at 0 and i32 payload at 4.

The keyed native inventory is exact below. ptr outputs point to exclusive
compiler scratch of the stated type. i32 status uses the existing Error mapping;
all void/Pure getter impossible native inputs abort, never produce unchecked data.
No extra LLVM attributes are inferred from a source method's spelling.

| RuntimeKey / symbol suffix after align_rt_ | C/LLVM shape | Result scratch / native effect |
| --- | --- | --- |
| CommandNewSession / command_new_session | void(ptr,i8) | Validate canonical bool; setter. |
| CommandStdoutTo / command_stdout_to | i32(ptr,ptr) | Command, borrowed writer; duplicate binding. |
| CommandStderrTo / command_stderr_to | i32(ptr,ptr) | Same for stderr. |
| CommandStart / command_start | i32(ptr,ptr) | Output pointer to child handle; zero on failure. |
| ChildId / child_id | i64(ptr) | Original positive PID. |
| ChildStatus / child_status | i32(ptr,ptr) | Option<termination>. |
| ChildTryWait / child_try_wait | i32(ptr,ptr) | Option<wait_result>. |
| ChildWait / child_wait | i32(ptr,ptr) | Existing key changes ABI to wait_result scratch. |
| ChildReadStdout / child_read_stdout | i32(ptr,ptr,i64,ptr) | Child, writable bytes, length, Option<i64>. |
| ChildReadStderr / child_read_stderr | i32(ptr,ptr,i64,ptr) | Same for stderr. |
| ChildPoll / child_poll | i32(ptr,i32,i64,ptr) | Input mask bits 0/1/2; readiness scratch. Reject unknown bits. |
| ChildKill / child_kill | i32(ptr,i64) | Existing ABI, native-domain/state validation. |
| ChildKillGroup / child_kill_group | i32(ptr,i64) | Pinned group operation. |
| ChildGroupMembers / child_group_members | i32(ptr,i64,ptr) | Owned array<i64> {ptr,i64}; normal heap provenance. |
| RunOutputStatus / run_output_status | void(ptr,ptr) | wait_result; replaces RunOutputCode/key/export. |
| RunBytesStatus / run_bytes_status | void(ptr,ptr) | wait_result; replaces RunBytesCode/key/export. |
| ProcessSignalNumber / process_signal_number | i64(i32) | Validated source variant to native signal. |
| ProcessSignals / process_signals | i32(i32,ptr) | Input mask bits 0..3; output subscription handle. |
| ProcessSignalNext / process_signal_next | i32(ptr,ptr) | Option<signal>. |
| ProcessSignalClose / process_signal_close | i32(ptr) | Retryable restoration. |
| ProcessSignalFree / process_signal_free | void(ptr) | Null no-op; restore before freeing live shell. |

Represent the new operations in one closed ProcessLiveKind ledger shared by
source checking, HIR validation, MIR validation and native-key selection. Each
row fixes exact receiver/argument/output types and borrow effects; no generic
successful fallback. LLVM lowers checked MIR and owns no lifecycle decisions.
The existing wait/status accessor migrations update every old HIR/MIR producer,
runtime key, exported symbol, golden, import and example in the same capability.

## Application composition

This observation helper borrows every owner. A failure returns control with the
child still owned and unreaped; the caller chooses group stop, grace, reap and
whether to retain its workspace. It never uses an early `?` that drops an owned
live child before the caller can perform that policy. The two digest owners hash
all bytes while the caller supplies fixed scratch storage. Output retention can
be added independently by the caller. Each iteration reads at most one chunk
from each stream and checks time even after both EOFs.

```align
import std.process
import std.crypto
import std.time

fn observe(
    borrow mut ch: child,
    out scratch: slice<u8>,
    borrow mut stdout_hash: crypto.digest,
    borrow mut stderr_hash: crypto.digest,
    budget_ns: i64,
) -> Result<process.termination, Error> {
    if scratch.len() == 0 || budget_ns < 0 { return Err(Error.Invalid) }
    started := time.instant()
    mut stdout_done := false
    mut stderr_done := false
    mut terminal := false
    mut observed: Option<process.termination> := None
    loop {
        elapsed := time.instant() - started
        if elapsed >= budget_ns { return Err(Error.Timeout) }
        if !stdout_done {
            match ch.read_stdout(scratch)? {
                Some(count) => {
                    if count == 0 { stdout_done = true } else { stdout_hash.update(scratch[0..count]) }
                }
                None => {}
            }
        }
        if !stderr_done {
            match ch.read_stderr(scratch)? {
                Some(count) => {
                    if count == 0 { stderr_done = true } else { stderr_hash.update(scratch[0..count]) }
                }
                None => {}
            }
        }
        if !terminal {
            observed = ch.status()?
            terminal = match observed { Some(_) => true, None => false }
        }
        checked_elapsed := time.instant() - started
        if checked_elapsed >= budget_ns { return Err(Error.Timeout) }
        if stdout_done && stderr_done && terminal {
            return match observed {
                Some(value) => Ok(value)
                None => Err(Error.Invalid)
            }
        }
        remaining := budget_ns - checked_elapsed
        pause_ns := if remaining < 10000000 { remaining } else { 10000000 }
        interest := process.readiness {
            stdout: !stdout_done,
            stderr: !stderr_done,
            status: !terminal
        }
        ch.poll(interest, pause_ns)?
    }
}
```

The helper intentionally leaves the final reap to its caller, including on
success: group control may still be needed after the root terminates. It returns
no absence certificate. A complete application may also call subscription.next
once per iteration and forward selected signals explicitly. A bounded grace
policy must not call blocking wait on a still-live process merely because its
pipes reached EOF.

## Closure matrix and implementation handoff gate

Owners below are in align_driver --test m11_process_live unless qualified. One
parameterized owner may close several cells. Both native hosts run the same
semantic cases; refusal on macOS does not satisfy a common-capability owner.

| Axis | Required owner and failure detector |
| --- | --- |
| Type formation, mutability, purity, Send, carriers | formation_and_carriers, including malformed builtin lookalikes and imported signatures without any live operation. |
| Launch and every partial acquisition | launch_matrix; runtime fault injection for allocation/fd/actions/fork/exec boundaries; external observer detects any unreaped child or leaked fd. Actual exit127 differs from exec failure; stopped-before-exec timed launches, zero/short/exact/excess error frames and signal-mask restoration cover both paths. |
| Move-in/out, nulling, Drop, replacement and return | owned_control_flow through structs/sums/Option/Result, helpers and allowed arrays, both compilation modes, positive native lifetime witnesses and omitted-Drop negative controls. |
| if/match/else/?/map_err, joins, loops, early exits | Same owned_control_flow owner; terminating eager arguments never call native code or invalidate a live receiver. |
| Dual output, caller-buffer bounds, EOF, EINTR | pipe_matrix and wait_under_pipe_pressure; simultaneous full pipes, one-byte reads, destination canaries, zero/absent buffer, readonly literal/mmap/shared-borrow origins, aliased destination, forged writable HIR, readable bytes after reap, pending wait rejection. |
| Observation versus reap, status cause and usage | status_matrix: repeated WNOWAIT observation retains group control, wait4 releases it, exit143 versus SIGTERM, Linux/Darwin units, unavailable/overflow usage and a zombie leader with a live worker. |
| Readiness and finite budgets | poll_matrix: cached EOF/status interest removal, absent streams, status after EOF, registration-after-exit, zero/negative/maximum budget, EINTR and no deadline restart. |
| Group operations and PID reuse | group_matrix: new session, root exit then group kill before reap, post-reap rejection, nested setsid explicitly survives group scope, stale PID sentinel, lost-wait state and budgeted enumeration errors and an ordinary-user Darwin scan containing root-owned processes and the retained zombie leader. |
| File output and descriptor inheritance | redirection_matrix: empty/dirty buffers, writable regular file only, shared stdout/stderr offset, source Drop before launch, repeated launches, failed replacement, and unlisted fd leak negative controls on every native spawn path; Darwin timed CLOEXEC pipe/file sources, relative cwd, setsid and a failed first PATH candidate followed by a successful one. |
| Signal-global state | signal_matrix: empty/overlap/blocked input, partial install and restore failures, pending during launch, coalescing/order, generation rollover/exhaustion and old-handler/new-owner race; tests isolate real signals in subprocesses. |
| ABI/range/alias/invalid native products | runtime native_layout_and_ranges plus independent LLVM layout owner, complete scratch zeroing and forbidden overlap checks before state changes. |
| HIR/MIR and type-class propagation | hir_live_schema_rejections / live_mir_gate: mutate each op's receiver/arity/width/output/borrow, false native-producer schema and new enum-tag sweep tripwires. |
| Interface, generic, whole/per-unit and cache | interfaces_and_cache, canonical leaf goldens [76]/[54] with rejected unknown neighbors; complete reachable nominal graph; runtime declaration/export owners. |

One coherent common capability is expected to exceed 1,000 handwritten lines.
Observation/reap/group authority, shared launch failures, wait-result migration
and stream ownership share one proof; splitting those producers from their
consumers would duplicate that proof and leave unsafe intermediate semantics.
Signal subscription is a separately mergeable capability with its own global
state failure domain and an already useful child-loop consumer. It depends only
on the shared Copy signal definitions, not stronger containment.

No latency/throughput improvement is promised. Fixed child observation state and
caller-buffer streaming do promise no retained whole input/output and no extra
processes: an increasing-total-output/fixed-buffer local measurement closes that
resource row, separate from correctness gates. Explicit group enumeration owns
its scan/allocation budget. Do not run unrelated compiler suites for this design.

The author-side ledger-to-prose pass checked the common API rows against the
state/validation/native sections and the matrix. It removed two unsound promises:
CLOEXEC EOF is not an exec-success certificate, and signal generation is bound to
handler observation rather than an unknowable send-time lease. The common
contract does not acquire strict R65 authority through process scans or groups.
The composition example passes the existing alignc formatter's parser; future
API names have not been type checked or executed.

One fresh independent review found two P1 admission/deadline omissions and three
P2 native-detail omissions. This coherent correction pass specifies writable out
admission and readonly/alias negatives, deadline-aware launch including the
Darwin SETEXEC fork path, inherited-signal/fork rules, unprivileged Darwin short
BSD observation with zombie lookup, and EOF-delimited nine-byte error-frame
validation. A focused Darwin source follow-up found that SETEXEC omits CLOEXEC
sources before file actions. Timed launches now remap standard streams and apply
cwd/session setup once in the owned fork child, then use inherit-only SETEXEC;
untimed native spawn retains its distinct valid file actions. The corresponding
descriptor/PATH owner closes this failure class without changing the public API.
[Plan 50](50-r65-process-capability-handoff.md) owns the final capability-boundary
review and implementation handoff. The language specs, rationale, Settled
record, process/fs designs and mirrors, HIR/runtime ledgers, HANDOFF and request
register consume those two exact ledgers. Stronger R65 authority is specified in
plan 50; it does not follow from common process-group operations alone.

### Common capability implementation evidence

The common implementation uses `process_launch`, `process_live`, and
`process_table` runtime modules and one closed `ProcessLiveKind` operation table.
The author extraction of mandatory lines maps native launch/admission/frame/fd
obligations to `process_launch` and its launch, frame, descriptor, partial-acquisition,
closed-standard-stream, cached-readiness and retained-group owners. State/cache,
read range/alias and typed-layout obligations map to `process_live` native owners;
process-table budget/ordering/optional-counter obligations map to `process_table`.
The existing `command_*` runtime owners retain capture budget, timeout and hard-I/O
failure coverage through the same launch and wait4 implementation.

Source ownership and whole/per-unit transport are owned by `m11_process`,
`m11_process_command`, and `m11_process_live`; `live_process_records_and_writable_backing`
and `live_process_mir_contract_matrix` independently reject malformed HIR/MIR.
The interface builtin-inventory owner covers named command/run_output helpers;
runtime ABI declaration and native export owners cover all 423 base rows and
434 maximum optional-feature exports. Signal-global-state and signal-owner
carrier/tag cells belong to the explicitly separate second capability in plan 50;
stronger verified-file/scope cells belong to its third/fourth capabilities.

Local Linux resource measurement used a 256-byte caller buffer with separate
stdout/stderr EOF handling, one explicitly selected head executable, and exact
observed totals of 1,048,576 and 67,108,864 bytes. Fresh-process maximum RSS was
11,864 and 11,868 KiB respectively. This is a local bounded-retention observation,
not a throughput promise or a cross-platform benchmark result. Native macOS code
and test compilation is checked separately; execution remains a macOS owner run.


## Signal capability implementation evidence

`process_signal.rs` owns the permanent generation/selected/pending word and the
retryable disposition-restoration state. Creation and restoration take the
existing process-launch lock before the subscription reservation, preserving
one lock order; handlers take neither lock. `ProcessLiveKind` shares exact
producer signatures among source checking, checked HIR, MIR and LLVM.

The closure matrix above maps signal cells to these concrete owners:

- `m11_process_signals::signal_owner_import_move_close_and_drop` covers imported
  natural records, Result, generic transfer, owned/borrowed helpers, overlap,
  explicit idempotent close and Drop-only release in whole/per-unit executables.
- `native_delivery_and_child_disposition_reset` covers actual parent delivery,
  Option<signal> ABI, and default dispositions in the executed child.
- `signal_owner_rejects_forbidden_carriers_and_shared_mutation` covers direct
  collections, tuple/box/out placement, printing, duplicate moves and shared or
  immutable mutation. The existing recursive native-control classifier owns
  Send, C layout and closure restrictions.
- Runtime `process_signal::tests` covers field-order coalescing, stale captured
  generations, selected-bit cancellation, exhaustion, empty/overlap/blocked
  selection, every partial installation and restoration position, restored
  pending-signal redelivery, retry progress, foreign-action detection and
  malformed/aliased native outputs rejected before state changes.
- `live_process_records_and_writable_backing` and
  `live_process_mir_contract_matrix` include all three signal operations in their
  malformed receiver, arity, output and producer-kind sweeps. Canonical graph
  owners encode and independently decode [76]/[54], rejecting their next unknown
  neighbors. The native export/declaration owner covers the four new entries.

The signal owner allocates one fixed native shell at explicit construction;
next and close allocate no buffers and retain no queue or owner-backed handler
storage. No throughput or delivery-latency promise is introduced.

`scripts/test-process-native.sh` is the local/CI parity owner on Linux and macOS;
it builds the runtime before running native and whole/per-unit process suites.

### Reopened native observation test axis

The capture/readiness owner uses `printf x; exec sleep 30`, so one PID owns the
pipe writer and a shell descendant cannot race the EOF assertion. It observes
pipe readiness separately from process status. The retained-group owner compares
its pre-reap result with an independent native `kill(-pid, 0)` while WNOWAIT pins
the terminal leader; kernel-specific ESRCH or EPERM is encoded through the shared
error model. Reaping then unconditionally revokes the API authority with Invalid.
These cells distinguish native observation, error encoding, pipe lifetime and
owner lifetime on both Linux and macOS without promising a zombie-group result.

Darwin registration/exit transition closure: XNU proc_exit drains process
references before potentially blocking exit cleanup and before publishing wait
status. An EVFILT_PROC ESRCH can therefore precede a WNOWAIT termination result.
The pinned direct-child owner retains a fixed missed-event bit and samples only
that child's WNOWAIT status in at-most-1ms native wait chunks within the original
finite poll budget. It does not wait for exit during launch, reap to manufacture
readiness, restart the deadline, scan processes or add a helper. Other registration
errors retain normal native-error behavior. The parameterized native
registration_exit_window_retains_finite_status_observation owner injects ESRCH
while status is pending, verifies zero/finite timeout and subsequent termination,
and checks an independent permission error is preserved.
