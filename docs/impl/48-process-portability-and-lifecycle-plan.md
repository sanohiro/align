# R65: native process capabilities and owned lifetime

**R65 supersession:** [plan 50](50-r65-process-capability-handoff.md) selects the
full request scope, native mechanisms and implementation boundaries, with plan
49 for common operations. Design and independent-review corrections are complete. The earlier
extra namespace/init and unqualified-Mac-extension requirements below are
historical proposals, not decisions left to the implementation model.

Status: **historical platform assessment; superseded by plans 49–50**.
Baseline: Align `4cb14895a06e67f32ee72383b53afe72cd8e5555`.
This records the R65 platform assessment after [plan 46](46-deferred-client-boundaries-plan.md).
The owner requires native Linux and native macOS support and permits different
backend implementations. Existing Python operations provide consumer evidence,
while Align ownership, errors and explicit OS effects determine the design. The owner explicitly asked
for the language-appropriate design. Platform selection is settled, not a
question to return to the owner.
Linux-only qualification is insufficient for the common native workflow. The
owner also rejected administrator registration, dedicated execution users and a
privileged service as prerequisites. That proposal is withdrawn. The portability
contract follows the actual native Python workflows; stronger Linux-specific
requirements must not become prerequisites for ordinary Mac process operations.
The exact replacement contracts are now plans 49–50. Names below remain
historical design notation, not additional available Align APIs. The initial
open cells and acceptance steps below are superseded where those contracts decide
them; implementation acceptance follows the final handoff.

## Design direction

Expose small, composable OS capabilities in `std`; application Align code owns
supervision. The same source-facing operation must have the same meaning on
Linux and macOS even when its backend differs. Start with the consumer's actual
observable requirements, not its Python classes or one OS's syscall vocabulary.

| Principle | Selected design consequence |
| --- | --- |
| One owner per resource | Extend the existing child state. Do not add a separate measured child, capture supervisor or coding-task runner. Borrowed commands configure launch; owned results survive command/input expiry. |
| Facts have concrete types | Preserve exit versus signal as a sum value, resource observations with defined units, and EOF versus pending as distinct outcomes. A numeric PID, pathname, process scan or report boolean cannot manufacture authority or an absence guarantee. |
| Effects are explicit | Capture, file redirection, inherited descriptors, new-session/group ownership, stronger isolation, signal subscriptions and input materialization are selected explicitly. Any extra process in an explicit OS-specific isolation operation must be declared. No administrator registration, dedicated execution user, privileged service or VM prerequisite; no implicit global subreaper or signal installation. |
| One error/optional model | Use ordinary Result for failure and Option for absent/pending observations, with exact meaning per operation. Failed construction publishes no owner; failed release preserves retryable authority. Never fabricate a successful exit or empty domain. |
| Policy is Align code | Deadlines, fair draining, byte caps, incremental hashing, resource comparisons, first-error retention, TERM/KILL grace and workspace cleanup are ordinary loops/data/control flow. Native code marshals and performs OS operations and the minimal ownership mechanism. |
| Native parity is required | Linux and macOS share semantic owner tests. Current native workflows are supported without requiring a new isolation environment. Stronger guarantees have their own two-platform proof before being advertised. |

These principles do not reopen any locked language decision. New OS facilities
belong in `std.process` or the existing typed filesystem boundary, not compiler
special cases named after align-llm. The public ledger below is still a design
candidate: exact lifetime, ABI and platform proof cells must close before code.

## 1. Public-contract ledger and acceptance boundary

This ledger is authoritative for this proposal. `Open` means the implementer
must not choose the missing guarantee, deployment requirement or ABI silently.

| Surface | Required semantics and exact candidate | Ownership/allocation/error rule | Provider, identity and prerequisite | Acceptance owner |
| --- | --- | --- | --- | --- |
| Common native process capability | The exact candidate is [plan 49](49-native-process-contract.md): live capture/file output, termination observation distinct from wait4 reap, typed resource result, finite readiness, pinned process-group operations and explicit signal observation. | Existing child/command/result owners, plus a separately mergeable signal lease. No keeper/helper process or privileged deployment. The exact ledger owns validation, lifetimes, ABI and acceptance. | Linux and macOS native backends. A process group covers its members, not descendants that leave it. | `m11_process_live` and signal owners in plan 49. |
| Immutable streaming input | Explicit writer construction, chunk writes, consuming seal; sealed owner offers bounded reader access and has no mutation API. **Exact declarations open.** | Move writer and sealed owner. OS-enforced write/grow/shrink refusal must survive other descriptors. No implicit file-sized snapshot in launch. Failed seal cannot publish a usable sealed owner. | Linux memfd candidate; no equivalent macOS backend established. Input format and native representation await backend qualification on the required platforms. | `m11_process_verified::sealed_input` |
| Admitted executable | Distinct executable owner for a finalized native image; hash and launch the same immutable object. **Exact declarations open.** | Explicit image materialization, no path re-resolution after admission, no script-magic fallback. Native loader/runtime closure is a separate deployment promise. | Linux executable memfd + fd-selected exec candidate. macOS executable identity and immutable storage proof open. | `m11_process_verified::verified_exec_replacement` |
| Running-image observation | OS-derived running main-image observation, never a caller-supplied filename. **Exact result/reader declarations open.** | Independently retained read authority; does not attest all loaded code, libraries or source provenance. Failure returns Error, not a guessed pathname. | Linux `/proc/self/exe` candidate; macOS needs a bound OS observation, not just `_NSGetExecutablePath` followed by reopen. | `m11_process_verified::current_image` |
| Descriptor inheritance | Typed sealed-input / namespace / executable authority plus explicit child slot. **Exact builder methods and slot domain open.** | Borrow sources through launch; independently owned child descriptors. Validate the entire map before effects, preserve parent offsets/flags, handle cycles, close every unlisted descriptor. | Shared remapper for live/isolated launch. Linux/macOS transport must be qualified separately; no raw-fd-to-authority cast. | `m11_process_verified::descriptor_allowlist` |
| Isolated launch | One launch owns the entire lifetime domain. Root status, stdout/stderr EOF, and complete absence are separate observations. **Exact launch/owner declarations open.** | No caller-global subreaper lease. Explicit stop and fallible retryable release retain authority on error/pending. Drop never certifies consumer success or deletes workspace. | Linux owned PID namespace candidate (§4). An equivalent macOS guarantee is unestablished; do not expose it as a portable promise or require a privileged deployment. This stronger requirement remains open separately from native workflow parity (§2). | `m11_process_containment::lifecycle_matrix` |
| Member observation | Cursor/handle admission binds individual processes to the owned domain before metadata/signalling. **Exact fields and declarations open.** | No raw PID signalling. Per-process observations, unavailable values explicit; no atomic total or hard resource ceiling inferred from scans. | Stable OS identity and namespace/domain membership are required together. Field units, counter width/overflow, scan work and handle expiry must be fixed before code. | `m11_process_containment::member_matrix` |

The proposal adds no native product supervisor, aggregate resource-limit policy,
first-error selector, result report, workspace deletion, implicit external tool,
VM manager or generic event framework. Application Align code owns those choices.
Existing synchronous capture retains its timeout, byte/UTF-8 and output-ownership
guarantees. Its terminal-status accessor is deliberately unified with child wait
below; old lossy status spelling is migrated rather than preserved as an alias.

## 2. Native support is required; start from the Python consumer

The user clarified that align-llm supports Mac and is replacing its Python
implementation with Align. Native Linux and native macOS are both required.
Different native implementations are permitted. Linux VM execution is not the
selected solution, and an unsupported-on-macOS stub does not close native support.
Do not turn an unqualified Linux-specific feature into a gate on the entire
portable process library or on the existing native Mac workload.

Read-only source inspection at consumer revision
`c9062c5fb51e137d6b78815c199c7935a732f7b6` establishes the following baseline.
It is inspection evidence, not a claim that consumer tests were run. Consumer
code and tests are unchanged. Paths below are relative to `../align-llm`.

| Existing Python operation / owner | Observed Linux/macOS behavior | Align design obligation |
| --- | --- | --- |
| `scripts/gpu_qualifier_process.py::_MeasuredProcess.poll` | Explicitly accepts `linux` and `darwin`; uses `wait4(pid, WNOHANG)`, keeps exit/signal distinction, and scales ru_maxrss by 1024 on Linux and 1 on Darwin. | Cache status and usage together at the one reap. Return a typed result with distinct signal information and memory measured in bytes. No `/proc` prerequisite. |
| `scripts/gpu_qualifier_process.py::run_owned_command`, `_drain` | Popen with closed environment, `/dev/null` input, two capture pipes, `start_new_session=True`, `close_fds=True`; selectors plus nonblocking read; monotonic deadline still checks a live process after EOF. | Common launch/read/poll/status primitives. Keep bounded log retention, full-stream hashing, fairness and timeout policy in Align application code. |
| `scripts/gpu_qualifier_process.py::_group_member_count`, `_terminate_group` | Uses `ps -axo pid=,pgid=` for group observations and killpg for TERM/KILL on both platforms. | Owned-group signalling and individual observation; replace tool-output parsing with a native boundary where practical. Preserve the group-scoped meaning; do not assert that this Python code already contains every setsid descendant. |
| `scripts/gpu_qualifier_process.py::ToolchainExecutable`, `_file_sha256` | Checks file metadata/digest and rechecks a host-tool alias before path-based Popen. It does not use an immutable executable memfd on macOS. | Preserve explicit path/argv[0] and declared tool verification. Immutable admitted execution is a separate stronger capability, not a prerequisite silently imposed on current native tool invocation. |
| `scripts/signal_subprocess.py::SignalSubprocessOwner`, `signal_subprocess_owner` | Installs selected handlers explicitly, remembers pending cancellation across launch, forwards to the active group, restores previous handlers, and lets the surrounding finalizer own cleanup. | Explicit signal-state lease/observation and group operation; application Align code implements cancellation precedence, grace periods and finalization. |
| `scripts/gpu_final_process.py::run_logged` | Sends stdout/stderr to caller-opened files, waits with a deadline, and signals its group during failure/finalization. | Explicit typed output redirection/inheritance must cover existing file writers as well as sealed inputs; no extra in-memory whole-log capture. |
| `scripts/prompt-fixed-adapter.py::ImmutableInput` and measurement/repair adapters | Uses Linux memfd sealing and `/proc/self/fd`. `run-prompt-repair-adapter-smoke` explicitly skips sealed-input/containment launch rows off Linux. | Retain the Linux guarantee. Design a native Mac counterpart as additional coverage; do not invent evidence that the existing Python Mac route already supplies this guarantee. |
| `eval/runners/run-coding-task.py::require_process_containment` | Explicitly rejects when Linux subreaper setup is unavailable; descendant discovery is `/proc`-based. `run-prompt-evaluate-smoke` likewise skips the strict descendant boundary off Linux. | Existing Linux-specific containment and the required new Mac counterpart need a separate qualification owner. This is not evidence that ordinary native Mac process execution is impossible. |

This baseline corrects two earlier mistakes: choosing Linux namespaces before
mapping the consumer's native operations, and omitting already-used wait4 usage,
exit cause, group control, signal observation and file output from the common API.
It also prevents a different mistake: describing a group-only Python path as an
already-proven arbitrary-descendant containment implementation.

### Required platform matrix

| Capability | Native Linux | Native macOS | Completion rule |
| --- | --- | --- | --- |
| Launch, env/cwd, caller-buffer reads, readiness, exact wait cause and wait4 memory | Required native implementation. | Required native implementation. | The same semantic owner executes on both hosts; Python-language differences are not blockers. |
| New session/group authority, group observation, explicit signals and file output | Required native implementation. | Required native implementation. | Preserve current native workflow and close identity/reap/handler lifetime defects without copying unsafe races. |
| Immutable admitted input/image and full isolated descendant lifetime | Existing Linux-specific consumer guarantee plus plan 46 qualification. | Required native extension; exact backend proof still open. | A separate proof must establish the stronger guarantee; ordinary Mac migration can progress independently, but R65 as a whole remains open. |

The inspected macOS primitives bound the stronger extension under the selected
unprivileged deployment:

- [Apple wait4](https://developer.apple.com/library/archive/documentation/System/Conceptual/ManPages_iPhoneOS/man2/wait4.2.html)
  provides nonblocking child status and resource observation. Native readiness
  can use [kqueue](https://developer.apple.com/library/archive/documentation/System/Conceptual/ManPages_iPhoneOS/man2/kqueue.2.html)
  or finite poll with the same public observation contract.
- `launchd`'s same-process-group cleanup and XNU's unsupported NOTE_TRACK do not
  establish arbitrary descendant containment. These limits concern the strong
  extension, not availability of Popen-equivalent primitives.
  [launchd source](https://github.com/apple-oss-distributions/launchd/blob/main/man/launchd.plist.5),
  [XNU event definitions](https://github.com/apple-oss-distributions/xnu/blob/main/bsd/sys/event.h).
- Privileged coalition control, administrator-installed helpers and dedicated
  execution identities are outside the owner-approved deployment. The presence
  of a kernel interface does not make its privileged administration an acceptable
  prerequisite of a language's common process library.
  [XNU coalition implementation](https://github.com/apple-oss-distributions/xnu/blob/main/bsd/kern/sys_coalition.c).

The implementation decision belongs to Align. Do not ask the user again to choose
native Mac versus a Linux VM, and do not report native Mac support as complete
before its owners execute on macOS.

## 3. Common live-process contract to carry into the exact ledger

### Rust reference boundary

The owner asked how Rust handles this boundary. Inspection of Rust 1.98.1's
documentation and versioned Unix source gives the following comparison. This is
reference evidence, not a change to Align's Rust toolchain requirement or an
accepted dependency selection.

| Concern | Rust contract / implementation | Consequence for this proposal |
| --- | --- | --- |
| Ordinary launch, I/O and status | `Command`, `Stdio` and `Child` separate configuration, stream ownership and direct-child status. `Stdio` accepts files; `try_wait` returns `Result<Option<ExitStatus>>` with cached completion. | Reuse this separation. Keep Align's own explicit stream bindings, finite observation and error model. [Child](https://doc.rust-lang.org/std/process/struct.Child.html), [Stdio](https://doc.rust-lang.org/std/process/struct.Stdio.html). |
| Exit cause | `ExitStatus::code()` is absent for signal termination; Unix `ExitStatusExt::signal()` supplies the signal. | Preserve both facts in Align's concrete termination sum. A shell-style numeric code is a caller conversion. [Unix exit status](https://doc.rust-lang.org/std/os/unix/process/trait.ExitStatusExt.html). |
| Native launch and cost | The versioned Unix implementation tries eligible `posix_spawn` paths on Linux and Apple targets, with fork/exec fallback under configuration/platform constraints. | Evaluate native spawn fast paths before adding bootstrap work to ordinary launch. Shared public meaning permits different native paths; speed still requires measurement. [Rust Unix source](https://github.com/rust-lang/rust/blob/1.98.1/library/std/src/sys/process/unix/unix.rs). |
| Unix and OS-specific mechanisms | Stable Unix `process_group` sets PGID. The inspected `setsid`, Unix group-signal and Linux pidfd APIs are experimental; pidfd is not a portable standard contract. | OS-specific implementation is normal. An integer group operation does not establish R65's retained post-exit group authority or full-domain absence. [CommandExt](https://doc.rust-lang.org/std/os/unix/process/trait.CommandExt.html), [ChildExt](https://doc.rust-lang.org/std/os/unix/process/trait.ChildExt.html), [Linux pidfd](https://doc.rust-lang.org/std/os/linux/process/trait.CommandExt.html). |
| Wait resource usage | `Child`'s public wait result contains exit status, not wait4 usage. The ordinary Unix source consumes status with waitpid; the Linux pidfd path has its own wait implementation. | Wrapping `std::process::Child::wait` cannot supply R65's one-reap status-plus-usage record. Select one native wait owner; never combine an independent wait4 reaper with Rust Child wait state. |
| Drop and cleanup | Standard `Child` has no process-cleanup Drop; applications must arrange waiting. Its handle does not certify descendant termination. | Retain Align's existing explicit blocking direct-child Drop contract. Strong isolated release remains a separately proved semantic owner. Rust memory ownership alone is not process-tree lifetime ownership. |
| Asynchronous orchestration | Tokio provides an asynchronous process API and optional `kill_on_drop`; its documented background reaping is best effort with no timing guarantee. | It is evidence for composition, not a substitute for complete release. R65 does not require an implicit async runtime or transfer application supervision to Rust. [Tokio process](https://docs.rs/tokio/1.53.1/tokio/process/index.html). |

The resulting implementation position is to reuse established Rust/native
facilities where their contract fits, without adding two owners for the same
process or inheriting a weaker cleanup promise. Align's existing process runtime
already calls native fork/exec/wait directly; it is not currently a wrapper around
Rust `Child`. A Rust implementation can expose the required wait4, descriptor and
platform mechanisms through Align's typed boundary without exposing Rust traits
or arbitrary post-fork callbacks to application code. Neither the inspected Rust
standard API nor Tokio's cleanup contract supplies verified immutable execution
or arbitrary-descendant absence on both platforms. Their omission is a scope
comparison, not evidence that Align must omit those requested capabilities.

### Exact common contract

[Plan 49](49-native-process-contract.md) now owns the common public ledger,
state machine, validation, native mechanisms, ABI and implementation matrix.
It supersedes §1/§3's earlier candidate spellings and keeper investigation.
Use `status()` for WNOWAIT termination observation and retain the unreaped
session leader while signalling its group; use wait/try_wait for the one wait4
reap and resource result. No additional group keeper process is selected.

Safe Align operations own the direct-child reap. Foreign FFI must not reap that
child or change SIGCHLD to auto-reap it, just as it must not close an owned fd.
This is an explicit interop obligation, not an impossible promise to prevent all
concurrent native violations with one constructor check.

The exact ledger distinguishes successful publication of a child from proof
that its target executed an instruction. Reported setup/exec errors return Err;
pre-exec external termination can be observed as an ordinary Signaled child.
This removes the earlier unimplementable requirement to infer a successful exec
from CLOEXEC-pipe EOF. Source-selected paths, literal argv and inherited/default
native environment remain ordinary process inputs, not immutable authority.

Applications own finite cancellation budgets, fair streaming, bounded retained
logs, whole-stream hashing, first-error selection and cleanup decisions. The
composition example borrows the child so a failed observation loop returns its
caller the authority needed to perform cleanup. An ordinary child Drop closes
its capture reads and blocks to reap only that direct child.

## 4. Linux qualification advances and remaining mechanism decisions

### Completion witness, including another waiter

The preferred domain is still one authenticated PID namespace init per launch.
Keep a process pidfd acquired atomically with creation; never request PIDFD_THREAD
or reacquire identity from an unprotected reported PID.

An important refinement to plan 46 is a candidate alternative to caller-global
SIGCHLD/reaper exclusion. A retained init pidfd can observe `POLLHUP` after the
task has been detached even when SIGCHLD auto-reaping or another waiter consumed
its status. In the inspected Linux v6.18 ordering, namespace teardown precedes
init exit notification; task detachment subsequently produces pidfd HUP.
`ECHILD` alone remains insufficient. Root command status must be independently
preserved by the private init/control channel; a lost outer wait result cannot
be converted to successful workload status.

Sources: [pidfd poll](https://github.com/torvalds/linux/blob/v6.18/fs/pidfs.c),
[namespace teardown](https://github.com/torvalds/linux/blob/v6.18/kernel/pid_namespace.c),
[exit/reap ordering](https://github.com/torvalds/linux/blob/v6.18/kernel/exit.c).
This is a source-derived candidate proof, not yet the accepted release API.
It must cover any bootstrap participant as well as the actual init.

A disposable local probe on Linux `6.18.33.2-microsoft-standard-WSL2`, x86_64,
used `clone(CLONE_NEWUSER | CLONE_NEWPID | CLONE_PIDFD | SIGCHLD)` and a
double-setsid orphan. Default SIGCHLD, SIG_IGN before creation, SIG_IGN after
creation and explicit wait consumption each produced init pidfd flags 17
(`POLLIN | POLLHUP`); the retained descendant pidfd also reached HUP and its proc
entry was absent. Probe: `/tmp/align-r65-reap-witness.py`. The wait-consumption
case simulates already-consumed status; it does **not** test concurrent waiters
or a SIGCHLD handler race. SA_NOCLDWAIT, creating-thread death, multithreaded
bootstrap and production namespace credentials remain untested here.

### Prepared user namespace and bubblewrap compatibility

Joining a supplied user namespace affects the privilege/topology design before
PID namespace creation. A multithreaded caller cannot simply perform `setns`
in place. Joining may require a separate single-threaded bootstrap process; that
process is another owned participant with its own failure and completion proof.

`setns` forbids reentering the current user namespace. Bubblewrap 0.12.0 performs
`setns` for `--userns FD`; therefore an Align bootstrap that already joined that
same namespace is not compatible with blindly preserving those arguments.
An outer fresh user namespace can also make an existing prepared namespace an
inaccessible sibling/ancestor. This is a real topology constraint, not a naming
issue or permission to discard the supplied authority.
[setns contract](https://man7.org/linux/man-pages/man2/setns.2.html),
[bubblewrap source](https://github.com/containers/bubblewrap/blob/v0.12.0/bubblewrap.c).

Q0 must select and execute one explicit arrangement: permitted namespace ancestry,
UID/GID mappings, retained authority and the exact external tool invocation.
The constructor must reject an incompatible arrangement before workload exec.
No ambient root privilege, cgroup delegation, installed helper or implicit
namespace creation policy may substitute for that deployment record.

### Parent death and bootstrap identity

PDEATHSIG refers to the creating **thread**, is not inherited across fork, can
be cleared by credential changes, and has a setup race if the parent already
died. An init whose parent is outside its PID namespace cannot use a guessed
`getppid` value as complete authentication. Select the retained parent identity,
acknowledgement order, post-credential arming and failure-at-each-boundary cleanup
together. Merely adding PR_SET_PDEATHSIG(SIGKILL) is not acceptance.
[PDEATHSIG contract](https://man7.org/linux/man-pages/man2/PR_SET_PDEATHSIG.2const.html).

Prefer an audited syscall-only bootstrap incorporated in the runtime over a
new installed helper artifact **if** the multithreaded and namespace proof works.
If a helper is required, its producer, architecture, content digest, discovery,
distribution, cache identity and authenticated protocol become explicit ledger
fields before implementation. No re-exec of an arbitrary user binary with a
magic environment variable or private CLI mode is permitted.

## 5. Implementation closure matrix and capability boundaries

One exact ledger per selected capability must close the following cells before
code. Existing invariant-level owners may cover multiple cells; these are test
obligations, not a demand for one new fixture per row.

| Axis | Implementation closure and required owner |
| --- | --- |
| Formation and type storage | Exact imported types/signatures, allowed carriers, mutability, purity and Send rules; malformed checked-HIR rejects before native effects. `m11_process_live::formation`, verified/containment equivalents. |
| Construction / partial acquisition | Every pipe/fd/owner/namespace/bootstrap/exec transition, deterministic multi-invalid precedence, no leaked child on Err. Runtime injected acquisition failures plus an independent external child observer. |
| Move / source nulling / replacement / Drop / return | Existing child and new authority owners; independent post-launch lifetime; exact one-time cleanup on partial failure and discarded Results. Whole/per-unit ownership owner with omitted-Drop negative control. |
| Control flow | if/match/else/?/map_err, branch/loop joins and early exits with live pipes/authorities; generic/imported helpers. Reuse the resource-carrier sweep where it actually detects the new resource class. |
| Native ranges and layouts | Null/alignment/length/overflow/alias checks before state advance; concrete C/LLVM field offsets and inactive payload rules; exact symbol/key inventory. No pointer-shaped authority accepted as an unrelated handle kind. |
| Interfaces / cache / artifacts | Complete nominal record/carrier definitions, semantic method identities, compiler/runtime/helper fingerprints, whole/per-unit/cold/hit parity. Protocol version and golden byte vectors before any transport is implemented. |
| Live I/O and time | Dual-stream pressure, explicit wait before drain/after EOF/after cached completion, one-byte buffer, zero buffer, EOF-live timeout, cached readiness and interest selection, EINTR/overflow, nonzero/failed exec, exit(143) versus SIGTERM, wait4 usage/units/cache, abandoned pipes, foreign waiter. Linux **and macOS** owner runs. |
| Native workflow parity | New-session group control after root completion with identity retention; group observations are not descendant certificates; pending signal during launch, overlapping signal leases/restoration, typed output files/shared offset/buffer flush. `m11_process_live` native group/signal/redirection owners on both OSes. |
| Sealing and exec binding | Writable aliases/mappings, growth/shrink/seal failure, replaced pathname and same-inode mutation, independent read offsets, current-image mismatch, scripts and loader constraints, descriptor-map cycles/leaks. |
| Domain membership and observation | No raw PID authority, PID reuse, inaccessible metadata, namespace ancestry, nested setsid/namespaces, fork churn, zombie leader with live thread; exact unavailable fields and scan limits. |
| Stop / release / abrupt death | No partial-success certificate, stop idempotence, pending/error retains owner, all bootstrap/init identities, two-owner isolation, supervisor/creator-thread death at each handshake, SIGCHLD/foreign waiter matrix, no workspace deletion. |
| Supported deployment | Local qualification script matches any added CI job. Record kernel/architecture/credentials/tool identity and actual sandbox command. A refusal test or Docker/VM compiler run is not native macOS containment evidence. |

The fewest useful capability boundaries are:

1. **Native Python process-workflow replacement**: live child I/O, exact status
   and usage, group control, explicit cancellation observation and file output.
   Useful independently on Linux/macOS. Close the reaper/group/signal/output
   ledger and syntax-checkable composition example before coding. Use the mapped
   GPU qualifier as the concrete consumer, with its policy expressed in Align.
2. **Verified input/executable authority and inheritance**: an independent failure
   domain whose stable consumer is verified launch. Ship the writer, sealing,
   reader, executable admission and launch/remap consumer together; no dormant
   authority-only chain. Required native platform proofs must be closed first.
3. **Complete isolated lifecycle**: domain admission, bootstrap, member identity,
   stop, absence and release/Drop in one capability. Do not split constructor
   from teardown to meet a line target. It does not close R65 while its
   Linux deployment proof and the exact supported-platform contract remain open.
   Do not label common Mac process support unavailable because this stronger
   extension is unqualified there.

Each is likely to cross compiler/runtime ownership boundaries. A capability over
roughly 1,000 handwritten lines needs the specific boundary rationale in its
exact ledger before implementation. Latency/throughput improvements are not
promised here; resource owners must nevertheless prove no hidden full-input or
full-output materialization. Benchmark only an explicit performance promise.

## 6. Authoring and next work

The author-side pass separated portable observations, immutable authority and
complete lifetime guarantees. It checked every claimed source-derived conclusion
against the references above and marked incomplete public fields explicitly.
This closes the architecture and required-native-platform direction, not the
complete implementation handoff. Plan 49 supplies the exact common candidate.
Its author/review gate and the stronger extension's immutable-image profile, transport and full release proof
remain separate handoff requirements. Do not give those decisions to an
implementation model as guesses.

One independent inspection of this proposal found one P2: the initial sketch
specified Drop's pipe closure but left explicit blocking wait under pipe pressure
unstated. The ledger, live-operation rules and closure matrix now require an
initial nonblocking status check and reject a pending captured wait before both
EOFs. The author also made an interrupted read return pending, keeping policy-loop
progress visible. The reviewer found no additional actionable issue in the
macOS boundary or candidate authenticated init-pidfd HUP reasoning. This is
proposal-level review, not acceptance of the unclosed backend or exact contract.

The subsequent owner clarification fixes both native platforms. Read-only Python
inspection produced §2's mapping and the missing common-mechanism rows. The
previous proposal review predates that correction and does not review the new
status/usage or signal/group/output requirements. Complete and review plan 49
from this mapping first, with an Align policy-loop example reproducing its
observable outcomes. Continue backend qualification for the strict extension;
do not infer blanket Linux-only support. The owner has explicitly rejected a
privileged-service deployment; it is no longer a backend candidate.
Review the completed exact ledger and capability matrix once before code.

At exact public-contract acceptance, propagate the one decision to draft.md,
docs/language-spec.md, docs/design-notes.md, Settled in docs/open-questions.md,
std-design/process.md and its Japanese mirror, HIR/runtime/interface/cache
ledgers, examples, plan 40 and the request register. Do not modify those shipped
contracts now merely to restate an unresolved proposal. Consumer changes and
adoption remain external; only the sibling request register may be updated here.
