# Test control peer-close handling

## Capability boundary

The existing core.test contract requires normal Ok/Err, early exit/exec/abort,
missing or malformed completion, bounded output and timeout to reach their
specified row outcomes after terminal child cleanup. On macOS every ordinary
child exit instead becomes control read failed (os error 54): a nonblocking Unix
socketpair drains its queued datagrams, then recv returns ECONNRESET when the
peer endpoint is closed. Linux returns WouldBlock at that point. Both the
existing C-driver test route and core.test catalog owner reproduce this with
unchanged baseline compiler main. A native socketpair independently reproduces
the queued-record-then-reset sequence.

Repair this platform EOF classification in the existing runner. Do not change
CLI, language syntax, child codecs, test
scheduling, capture allocation, native descriptor ownership or terminal status
rules. The existing typed Data / Empty / Eof / Error drain contract owns the
behavior; no new public contract is selected.

## Exact internal record

Add one row-local control_eof bool, initially false, and pass its mutable borrow
through drain_control and quiesce_child. The private drain retains its existing
io::Result<()> result and all existing codec-state borrows. On macOS only, raw
ECONNRESET marks control_eof true and ends the drain successfully. It does not
acknowledge launch, synthesize completion, clear the first protocol error or
prove child termination. Every datagram preceding EOF is validated in arrival
order using the existing codecs. WouldBlock remains an open Empty result;
Interrupted retries; every other receive error remains control-read
infrastructure failure with its original raw code.

Once control_eof is true, later drain calls do no receive and both row and
cleanup readiness calls pass -1 for that control endpoint. The owned fd remains
alive until the existing explicit close sequence. EOF alone never permits a
summary or row classification: the unchanged signal, timeout, output,
child-terminal, group cleanup, close, reap and reserved-status rules still run.
No allocation, clock, cache input, environment input or persisted format is
added. Poll exclusion prevents a closed endpoint from becoming a permanent
readiness wakeup while a process remains alive.

## Implementation closure matrix

| Cell | Implementation / regression owner |
| --- | --- |
| Initial state; mutable state borrow; one-way EOF transition | run_row local, drain_control and quiesce_child; native control-peer matrix |
| Empty open peer; queued valid Ok/Err; closed peer before ack or after ack without completion | Existing codecs plus peer matrix crossing open/closed endpoint with each record state; core_test catalog and returned-error/terminal owners |
| Malformed first ack, completion field error, repeated completion, extra/long datagram | Same arrival-ordered matrix retains exact acknowledged/completion/detail outputs with open and closed peers; existing golden codecs |
| Every other OS error remains a receive error | Real non-socket owned descriptor produces ENOTSOCK without changing EOF; no injected production hook |
| No further receive/readiness after EOF | Shared EOF borrow in every drain and row/cleanup poll; repeated-drain native controls, the live exec-peer timeout owner and existing signal/cleanup owners |
| EOF is not child terminal or success | Same protocol matrix; c_driver live exec-peer owner closes fd 3 through CLOEXEC, publishes its pid and evidence, then sleeps. Assert timeout, exact retained output, ESRCH after report and absence of private test stages. Existing exit/exec/abort, output-cap and signal owners retain their outcomes |
| Exactly-once descriptor/child/stage cleanup and captured evidence | Existing quiesce close/reap sequence and cleanup-deadline owner, real CLI core_test and C-driver test route |
| Whole/per-unit/generics, source nulling/Drop | Unchanged semantic producers and owners; native ABI correction below applies to both emission paths |

The change touches the runner, its existing native owner module and the c_driver leaf owner. The live exec-peer owner reuses c_driver Process (one child deadline and process-group/direct kill/reap guard) and ArtifactStage (exclusive acquisition with immediate RAII). Its explicit C-driver fixture avoids ambient PATH selection. Run
those owners plus the existing core_test target on Linux and macOS. Prove the
new closed-peer owner fails with the old drain, then restore the candidate.
Use the existing C-driver test-route control on macOS as an additional consumer
witness; its unrelated Linux PGO/no-PATH fixture failures are outside this
boundary. The independent pre-code finding that EOF was not crossed with a still-live child is closed by that bounded exec-peer owner. Its two-second row budget accommodates native first-launch admission on both platforms while retaining the sixty-second outer process guard. The existing macOS SIGPIPE writer probe also fails without the EOF change; it belongs to the independent writer boundary and remains unchanged. No new test binary or benchmark is needed: this is correctness, with
no new latency or resource-size promise.

Complete the author matrix-to-diff pass and one fresh independent full-diff
review before the exact-head local preflight and publication.

## Native unsigned-byte ABI closure

The peer-close repair exposes a second existing failure: a normal macOS test
child sends its acknowledgement but rejects its Ok completion before send.
The harness passes tag 255 as an i8 without the zeroext C ABI attribute.
Clang 22 emits zeroext on both declarations and calls on Apple ARM64 and x86_64;
it emits neither on Linux AArch64. Apple ARM64 requires caller extension for
arguments narrower than 32 bits. The native Rust u8 callee may therefore inspect
unextended register bits. The same fixed registry contains five sibling symbols
with u8 arguments. Repair this existing native C contract as one capability;
changing the test runtime signature or special-casing tag 255 would leave the
shared defect open.

Rename private NativeType::I8 to U8 to record its actual unsigned Rust meaning.
The only U8 rows are BufferFilled (parameter 1), BufferAppendFilled (2),
CommandNewSession (1), FsDirectoryAccess (1,2,3), FsDirectoryAccessAt (3,4,5),
and TestReportV1 (1,2). There is no narrow native return or signed-byte row.
The canonical registry supplies zeroext at these exact declaration and call
positions for x86_64 and Apple arm64/aarch64; Linux AArch64 supplies no extension
attribute. The module's already-resolved target triple selects the rule, with
no new ambient input. Neither memory-effect attributes nor Align-owned scalar
boundary conventions are copied to native call sites.

Use one registry apply_call_attributes helper at the BufferNew,
BufferAppendFilled, ProcessLive and FsTree producers, generic DirectCall::Runtime (the current producer validator admits no U8 row there),
compatible fixed-native extern calls, and the private harness completion call.
Existing target-aware declaration application uses the same selector. All admitted unsigned-byte source externs are keyed rows and reuse these declarations. All four private test-control symbols remain forbidden source externs; TestReportV1 is harness-only. Keep the
six unsigned rows outside rt-lto's measured string-primitive subset; ABI facts
are not shed as optimization promises. Add no instruction sweep or runtime
allocation. Existing core.test and native C boundaries and semantic wire formats
remain unchanged. Its private fingerprint advances to v3, records U8 distinctly
and serializes the selector's result for all four supported Linux/macOS
architecture triples, together with the declaration-and-call application rule.
The existing target identity continues to bind the concrete artifact target.

| ABI closure cell | Implementation / exact owner |
| --- | --- |
| Six Rust u8 signatures, all declaration and call argument widths/ordinals | Canonical U8 registry; parameterized registry target/row owner checks both declaration and call attributes, no signed or return facts |
| Four supported platform rules; no ambient target selection | Resolved module triple; Clang 22 C evidence for macOS ARM64 and Linux x86_64/AArch64, registry owner includes macOS x86_64 |
| Dedicated MIR producers, generic direct native and compatible extern calls | One registry helper at each producer; codegen IR owner covers all five keyed rows and extern reuse; existing whole/per-unit native owners remain applicable |
| Private harness Ok and all Err tags | Harness report call helper; existing core_test catalog, returned-error, whole/per-unit, malformed/options owners on macOS/Linux |
| Artifact/cache identity and stale harness exclusion | Registry v3 fingerprint records U8, four-target policy and declaration/call scope; exact fingerprint owner plus existing driver harness identity owner |
| rt-lto/optimization attributes; source ownership, Drop and replacement | Six rows not guarded; existing rt-lto registry owner and declaration golden. No source ownership or runtime body change |
| Authoritative prose and mechanical inventory | Runtime ABI ledger and checked-in declaration golden updated together; no language/library public signature changes or mirrors needed |

The fresh independent pre-code review of the expanded matrix is clean, with the private-test-control extern exclusion made explicit. Run the matrix-to-diff pass and one full candidate review.
The original EOF and this ABI defect are both required to restore ordinary
macOS core.test execution, so one coherent capability avoids an unusable dormant
intermediate consumer. No benchmark claim is made.

## Darwin terminal-group signalling closure

A native setsid/sleep/waitid(WNOWAIT) reproduction establishes the next existing
consumer defect: Darwin kill(-pgid, 0/SIGKILL) returns EPERM when only the exited,
unreaped group leader remains. Direct signalling of that pid succeeds. Reaping
that same leader makes kill(-pgid, 0) return ESRCH. Linux does not use EPERM for
this state. Do not classify EPERM as absence, and do not ignore it merely because
the direct child is terminal.

Keep both graceful and forced signal dispatch records until the existing
terminal reap and bounded process-group-empty observation have finished.
Introduce group_empty, initially false, set true only after successful leader
reap and wait_process_group_empty returns Ok for the verified group. The private
signal classifier may suppress a group-only EPERM on macOS only when terminal
and group_empty are both proven. Direct-target EPERM, other group errors,
nonterminal children, failed reap, failed/expired group observation and Linux
EPERM keep their existing errors. No second deadline, scan, signal, allocation,
process ownership or pid reuse policy is added. ChildGuard's emergency kill
path has no empty-group proof and therefore retains conservative errors.

Deferred error selection preserves existing precedence: the first terminal-observation loop wait/poll error, graceful kill error, forced kill error, then the first later poll/read/close/reap/group error. Retain terminal-observation and later cleanup errors separately until selection.
Do not clear a stored generic kill error after cleanup, since that could erase
a direct-target failure. Classification consumes the original target-specific
records with the exact terminal/empty proof.

| Group closure cell | Implementation / owner |
| --- | --- |
| Original graceful/forced target provenance and precedence | Saved SignalDispatch values; final selection preserves terminal-loop error, graceful kill, forced kill, then later cleanup precedence |
| Only proven macOS group EPERM is suppressed | Parameterized private classifier owner crosses terminal/empty proof, group/direct Sent/Missing/EPERM/other errors and verified/direct dispatch |
| Failed reap, timeout or surviving group never supplies empty proof | group_empty is set only in the existing successful reap/empty branch; same proof matrix and core_test descendant timeout owner |
| Real exited leader, live exec peer, signal and timeout cleanup | Existing core_test catalog/terminal/signal/descendant owners, C-driver normal test route and new live exec-peer owner on both platforms |
| Emergency guard, ownership, source nulling, allocations and all semantic compilation layers | No new ownership strategy; emergency guard passes false, existing deadline/cleanup native owners remain applicable |

This expands only the platform terminal-cleanup boundary already required by the
consumer. Independent pre-code review found the earlier terminal-loop error precedence; the matrix now preserves that ordering explicitly before implementation. Preserve the separate baseline SIGPIPE writer-probe issue.

## Child observation after capture EOF

The repaired macOS catalog produces correct outcomes but samples show a full
row-budget poll after all three channels have drained: fd exclusion removes the
last readiness notification while the child is exiting. The last nonblocking
waitid may legitimately precede its terminal state. Linux can encounter the same
race after both capture pipes close even though its datagram endpoint reports
WouldBlock. Do not infer child termination from any EOF.

Reuse the existing single Instant::now() read for the timeout check. When both
capture EOF flags are true, cap the next row poll at min(existing work deadline,
now + 10 ms), then repeat the existing terminal observation. Other rows keep the
existing poll deadline. No new deadline, clock read, event handler, fd, allocation
or public timeout guarantee is introduced; the existing cleanup loop already
uses the same bounded observation interval. The native deadline owner remains
unchanged. Extend the C-driver exec-peer owner with a shell that closes stdout
and stderr, stays alive for 100 ms and exits: its terminal failure must be
reported as the actual exit, never as a timeout. Retain the long live-peer case
and require both cases to be reaped with all private stages absent. This closes
the deferred terminal-observation race without re-enabling a permanently ready
EOF endpoint. The author matrix-to-diff pass covers this polling cell.

The two existing core_test execution/descendant timeout fixtures also use the
same two-second row budget on every platform. Their former 50/500 ms budgets
can expire during first native launch on macOS (the 50 ms failure reproduces
when run alone), so they did not deterministically reach the execution or
descendant boundary they claim to own. Expected exact timeout text follows the
new fixture value; production CLI limits and row/cleanup deadlines do not
change. The overall runner budget remains thirty minutes, with no increase.

## Qualification and author closure

The author matrix-to-diff pass is complete. Every EOF, unsigned ABI and group
classification cell maps to the producer/helper and the parameterized or existing
owner named above. Linux native codecs/runner, six-row ABI inventory/golden,
core_test and both C-driver exec-peer cases pass. macOS native runner owners,
all nine core_test owners and both exec-peer cases pass; the original EOF arm
omission reproduces ECONNRESET, and native send tracing distinguishes the former
pre-send unsigned-byte rejection from the now-successful completion datagram.
The untouched SIGPIPE writer probe has an independently reproduced macOS baseline
failure; the independent owner correction below closes that deferred test boundary.
No new library API, GPU control or latency benchmark promise is introduced.

The capability diff is approximately one thousand hand-written changed lines,
including its owning matrix and exhaustive regression inventories. One boundary
restores an actually usable macOS test consumer and closes the unsigned-byte
native sibling class. EOF suppression, terminal wakeup and group cleanup share
one row state and the same execution owners; separating their producer/consumer
chain would duplicate that proof and leave intermediate consumers unusable.

The final local owner gate exposed the same zombie-group class in the post-reap
probe: a descendant's other parent may not have reaped it yet. On macOS,
wait_process_group_empty now treats raw EPERM as a still-present group and
retries within its unchanged deadline; only ESRCH proves absence. If EPERM
persists at expiry it retains that exact error. Other errors and Linux behavior
are unchanged. The existing cleanup_waits_honor_deadlines owner now creates a
private group, keeps its killed direct child unreaped, proves the group cannot
be reported absent or fail early, then reaps and proves absence. Its immediate
ChildGuard also protects fixture failure cleanup. The real descendant-timeout
owner closes the delayed other-parent consumer. This is the same local cleanup
finding class and does not change the terminal/empty-group proof strategy.

## Isolated SIGPIPE writer owner

The deferred failure is a test-topology defect. Rust startup installs SIG_IGN
for SIGPIPE, and the real CLI already returns numeric 1 for closed stdout,
stderr and both sinks. The isolated probe deliberately installs SIG_DFL.
Libtest nevertheless runs a main thread beside its single test thread. Darwin's
pipe EPIPE uses process-directed psignal, so blocking only the test thread lets
the signal terminate the unblocked libtest main thread. Darwin sigpending also
observes only the calling thread's pending set; it need not expose that generated
process-directed signal. A prior thread-directed raise remains observable.

The probe owner blocks SIGPIPE only in its exec child, using a validated set and
an async-signal-safe pre_exec mask operation. Libtest threads inherit the block.
The probe proves inheritance before selecting its own original mask, then keeps
the existing default-disposition writer test. Successful writes restore that
mask; EPIPE retains the writer block. Both platforms retain pre-existing pending
SIGPIPE, and Linux additionally requires the newly generated signal to be pending
on the writer thread. No signal is consumed. Parent masks remain unchanged.
The production writer, process dispositions, source contract and runtime ABI do
not change. This private CLI writer is not a general multithreaded SIG_DFL
embedding API; the adversarial probe owns that extra process setup.

| Closure cell | Implementation / regression owner |
| --- | --- |
| Validated native mask; child-only mutation; parent isolation | controlled_write_sigpipe_process_owner checks native setup and parent mask parity; pre_exec contains only pthread_sigmask and nonallocating status construction |
| Inheritance, initial blocked/unblocked, prior pending signal | controlled_write_sigpipe_process_probe asserts inherited blocking before selecting its four cases; SIG_DFL remains installed |
| Success byte and original mask; terminal EPIPE and retained mask | Existing four-case probe, with platform-correct generated-pending assertion and unchanged prior-pending assertion |
| Exclusive fixture ownership and bounded probe cleanup | ArtifactStage plus immediate direct-child guard and one work/cleanup deadline; this libtest probe launches no descendant process |
| Failure-report cleanup before summary; passing-summary cleanup | closed_report_sinks_exit_numerically_and_remove_stages crosses failing and passing source with closed stdout, checking exact surviving diagnostic and numeric exit 1 |
| Stderr diagnostic failure and both sinks closed | Same CLI owner crosses cache diagnostics and failure reporting with closed stderr/both; no summary or recursive diagnostic |
| Native descriptors, artifacts and process groups | Immediate File owners, exclusive outer ArtifactStage/private TMPDIR and file-backed capture; the bounded CLI guard gives SIGTERM cleanup authority to the live runner before pinned-group/direct hard retirement and reap; assert private stages absent before fixture Drop |
| Transitive tool admission and separately grouped row timeout | No external availability subprocess; actual tools run inside the CLI guard. Explicit two-second row limit precedes the fifty-second outer work limit; one sixty-second deadline reserves graceful and forced cleanup. Stalled linker and exec-row controls force the owner timeout, then assert their published PID is absent before fixture removal |
| Types, construction, Move/Drop/replacement/return, branches, generics, interfaces, whole/per-unit, ABI/cache and allocation parity | No semantic or production implementation changes; existing owners remain applicable |

The independent plan review found two P2 omissions: legacy core_test scratch and
unbounded command helpers cannot supply exclusive bounded ownership, and a
passing-only stdout fixture misses pre-summary report-error cleanup. The matrix
above resolves both before implementation. The correction uses existing owner
binaries and makes no performance or new resource-size promise. Qualify the
native probe and CLI owner on macOS and Linux; verify that removing child-mask
setup or terminal stage removal fails the applicable owner before restoring the
candidate. Run the author matrix-to-diff pass and one preflight full-diff review.

Qualification closes the matrix: macOS native runner owners (nine) and all ten
core_test owners pass. Linux native owners (eight) and the new four-case CLI
owner pass; Linux container execution uses an init reaper, required by the
existing descendant-timeout owner, which also passes there. Removing the child
mask, writer mask or terminal stage removal independently fails the intended
owner; byte-identical restoration passes. The production runner prefix remains
identical to main. The author matrix-to-diff pass is complete.

The preflight review found an unbounded legacy cc version probe and an outer
hard-kill path that could orphan a separately grouped test row. The coherent
owner correction removes the external availability probe entirely and keeps
actual tool work inside the bounded CLI. Before forced group/direct retirement,
the owner gives a live runner up to three seconds within its original deadline
to forward SIGTERM and discharge its row. Explicit row/outer deadline ordering
and stalled-linker/exec-row controls close the transitive process cell. These
controls force the outer timeout while the published process is live, exercise
Drop, and require ESRCH before fixture removal; the row also requires stage
absence. This is a local test-lifecycle correction, with no production change.
