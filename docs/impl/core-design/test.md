This directory holds the authoritative per-area design docs for the `core` library, at the same
depth as `../std-design/` (signatures, ownership/effect classification, error policy, pitfalls,
and test anchors).

# core.test — in-language tests

> 🌐 **English** · [Japanese](./ja/test.md)

> **Status:** implemented 2026-08-31. The parser, checked-HIR overlay, MIR/LLVM lowering,
> compiler-private runtime ABI, cache separation, and bounded sequential runner ship as one
> capability.

## Public-contract ledger

This ledger is authoritative for the first implementation. Prose below may explain a row but may
not widen it. A public change updates this table first, then every listed source of truth in one
pass.

| Surface | Exact public record | Owner, artifact/cache identity, and acceptance owner |
| --- | --- | --- |
| Declaration grammar | One private top-level declaration: `test` followed by one ordinary Align string token and one block. `test` remains an identifier and commits to this contextual item only when the item-position two-token lookahead is `test` + string; bare `test {}` and `pub test {}` remain keyword-less type declarations. The lookahead `pub test` + string commits to the rejected visible-test form. After commitment, parameters, type parameters, a written return type, expression-body `=`, attributes, a missing block, and a trailing declaration name reject with test-specific recovery. The declaration creates no callable or name binding. | Lexer keeps `test` as an identifier; AST/parser/formatter own the contextual item. Parser/formatter round-trip, `test`/`pub test` type twins, lookahead/recovery, and every rejected near-shape are one parameterized owner. |
| Name, identity, and catalog | The decoded name is 1..=256 UTF-8 bytes and contains no Unicode C0/C1 control: U+0000..U+001F and U+007F..U+009F are rejected. Duplicate names in one module reject. The canonical public id is `<canonical-module-path>::<decoded-name>`; an entry source uses its declared module path, or `main` only when it omits a module declaration, and the complete id must fit 1..=1,024 UTF-8 bytes. If an entry omits its module declaration while an imported source explicitly declares `module main`, loading rejects before catalog construction with `default entry module 'main' conflicts with imported module 'main'; declare the entry module explicitly`; an explicitly declared entry path follows the ordinary duplicate-module rule. At most 65,535 tests exist in the explicit entry/import closure. Catalog order is the existing dependency-first DFS unit order (direct imports in source order), then source declaration order in each unit. Only that explicit closure is searched; no directory, filename, annotation, or manifest discovery is added. | Sema owns validation and canonical ids; the driver owns the ordered catalog. Declared/default entry paths, exact C0/C1 boundary neighbors, name/id/count limits, duplicate scope, the rejected default-entry/imported-declared-`main` collision, diamond-import order, and ignored unimported files are catalog owners. |
| Body type and control | A test is checked as a compiler-private zero-parameter `fn() -> Result<(), core.Error>`. After its written block completes with Unit, the construct supplies one documented `Ok(())` tail. `?`, `return Err(...)`, `match`, `else`, arenas, Drop, and every ordinary control form retain their existing semantics. A written non-Unit tail or `return` of any other type rejects. Normal Ok/Err and assertion exits run ordinary function cleanup; hard errors, `process.abort`, and successful `process.exec` retain their existing no-unwind behavior. | Sema/HIR/MIR own one flagged private test function and its implicit tail; no public interface entry is emitted. Control-flow, cleanup, Error-variant, and malformed checked-HIR owners cover direct and per-unit lowering. |
| Assertions | With `import core.test`, exactly `test.expect(condition)` and `test.expect_eq(left, right)` are available, only as standalone statements in the lexical test body or its ordinary nested blocks. Because the ordinary parser represents the last expression before `}` as `Block::tail` even on its own terminated line, sema also consumes an exact assertion in that syntactic slot as the block's final statement at root test completion or when the enclosing block/control expression is itself a complete statement. Placement is fixed from the AST parent before child checking; every Value edge rejects, even when its consumer expects Unit. An admitted assertion leaves Unit fallthrough and the test's implicit Ok follows normally. Assertions reject in functions, lambdas, and constants. `expect` requires exact `bool`. `expect_eq` applies the existing `==` type/admission rule, additionally requires that comparison's result to be exact `bool`, and evaluates left then right exactly once; vector/mask comparisons whose ordinary result is `maskN` reject rather than gaining an assertion-only reduction. Success is Unit and allocates nothing. Failure writes one bounded diagnostic containing canonical test id plus the 1-based call line/column, then returns `Err(Error.Invalid)` from the enclosing test; `expect_eq` does not inspect or format operand values. The first failed assertion ends that test. | Parser keeps these as ordinary qualified calls and retains ordinary tail parsing; sema recognizes the imported test-only builtins, records source identity, and normalizes an admitted syntactic assertion tail before checked HIR. Checked HIR admits the discriminator only as a complete `Stmt::Expr`; MIR/runtime own the diagnostic and early Err. Root/nested final-statement twins, every consumed-tail negative family including expected Unit, scalar/string equality, vector/mask rejection, eager-order, first-failure, and cleanup owners are required. |
| Production commands and modes | `check` and `check-per-unit` parse and type-check test declarations. `fmt` formats them. `align-repl` parses the same contextual declaration but rejects an entire submitted entry containing any test item before replacement resolution, session mutation, compilation, or execution with `error: test declarations are not available in align-repl; use 'alignc test <entry.align>'`; prior session state and the next ordinal remain unchanged. Every production source consumer—`build`, `run`, `size`, `emit-mir`, `emit-llvm`, `emit-obj`, `explain-opt`, and `db prepare`—validates the complete checked result but selects only its frozen production prefix; test roots, lifted helpers, test-only generic/type/resource monomorphs, interned types, non-database static descriptors, and capabilities remain in the test overlay. `explain-opt` excludes the overlay from located MIR and optimization remarks. A database Query/command constructor remains legal only as the complete body of an ordinary named top-level descriptor function, so it is formed before the prefix freezes and cannot originate in a test overlay; source use in a test resolves that production descriptor. A handcrafted overlay database descriptor rejects during checked-HIR validation. `db prepare` therefore prepares the existing production descriptor set, has no test flag, and retains ordinary diagnostics from checking tests. `alignc test` consumes that metadata offline; missing/stale `CheckedRequired` evidence fails before artifact creation and never contacts a database. `emit-interface` exports no test name, body, assertion, catalog, descriptor, helper, type suffix, or capability. Commands without an Align source (`cache clear`, `--version`, and `db migrate/status/check/repair`) have no test input. A source with tests still needs an ordinary valid `main` for commands that require one. Prefix selection applies independently to one-shot build/run/size, every initial and changed-source `build --watch` rebuild, whole/per-unit lowering, `dev|release|fast|small|tiny`, target CPU selection, runtime LTO on/off, release/fast ThinLTO, and release/fast PGO instrument/use; the existing option compatibility and artifact semantics do not change. | The selected prefix/combined view is an explicit compiler input. One parameterized accepted-mode product covers every listed verb and the transactional `align-repl` rejection and every admitted watch/profile/target/runtime-LTO/ThinLTO/PGO/jobs/cache-stats state; actual whole/per-unit, watch-rebuild, ThinLTO, PGO-instrument, and PGO-use artifact owners each contain a test declaration. Production/test MIR twins, lifted/monomorph/type-suffix isolation, located-MIR absence, database policy × driver metadata consumption, interface absence, unreachable test-only native libraries, and production executable byte identity close the matrix. |
| Test command and options | `alignc test <entry.align>` checks the explicit closure, builds one host test executable, then runs every catalog entry sequentially. It requires no user `main` and never invokes one automatically. Accepted common options are `--target-cpu`, `--profile`, `--rt-lto`/`--no-rt-lto`, `--cache-stats`, and `-j`/`--jobs`, with their existing spelling, placement, and duplicate rules. Target CPU defaults to `baseline`, accepts `native` or the existing explicit LLVM CPU spelling, and the last occurrence wins. Profile accepts exactly `dev|release|fast|small|tiny`, the last occurrence wins, and test defaults to `dev`. Runtime LTO follows the selected profile default (on for release/fast, off otherwise); the last explicit on/off flag wins, explicit on is valid only for release/fast, and explicit off is valid for every profile. Repeated `--cache-stats` is idempotent and changes diagnostics only. The last positive jobs flag wins; otherwise `ALIGNC_JOBS`, then available parallelism, resolves the build-worker count. Jobs never run catalog rows concurrently and neither jobs nor cache statistics changes artifact bytes. Target/profile/runtime-LTO reach every unit object and the generated harness and enter their cache identities. Timeout/output options affect only the runner and never artifact identity. `--watch`, `--thin-lto`, PGO flags, `--export`, program arguments, and unknown options reject before build. Each test-only option accepts either `--timeout-ns N` or `--timeout-ns=N`, and `--max-output-bytes N` or `--max-output-bytes=N`, anywhere among compiler arguments; each may occur at most once, and no `--` terminator is introduced. Timeout accepts 1..=900,000,000,000 and defaults to 60,000,000,000 per catalog row, including harness launch and cleanup. Output accepts 0..=16,777,216 and defaults to 1,048,576 independently for stdout and stderr. No environment variable changes those two values. After flag removal the remaining argv must be exactly command plus one entry path. Immediately after CLI validation and before source I/O or artifact allocation, the driver snapshots `std::env::current_dir()` once as the suite working directory; failure writes `alignc: test runner working directory failed (os error <signed-i32>)` plus LF to stderr and exits 1 with no artifact. The retained native `PathBuf` is a runner input only and never enters source, object, harness, or cache identity. Zero discovered tests allocates no artifact, writes exactly `alignc: no tests found` plus LF to stderr, writes nothing to stdout, and exits 1. | CLI/driver own parsing and validation before artifact creation. A parameterized Cartesian owner covers every accepted target × profile × resolved runtime-LTO × cache-stats × jobs-source state, each rejected option, and default/explicit identity twins; focused terminal-consumer assertions prove target/profile/LTO reach objects and harness, jobs reaches only build scheduling, cache stats reaches only diagnostics, and timeout/output and the suite working directory reach only runner state. Exact default/limit/rejected-next, conflicting/valueless/repeated option, no-main, user-main-not-run, and zero-test byte, current-directory failure, and snapshot-stability owners are required. |
| Build, entry symbols, and artifact | Test lowering combines the immutable production prefix with its validated test overlay, builds one per-unit test-mode object graph plus one compiler-generated harness, and links them once into a private `ArtifactStage` executable. The generated harness is the sole external literal symbol `main`, with C entry shape `i32 @main()`. Test mode never emits the ordinary source-main wrapper. Each of the four permitted source forms—`() -> i32`, `() -> Unit`, `() -> Result<Unit,Error>`, and `(array<str>) -> Result<Unit,Error>`—instead uses the existing collision-free encoded program symbol; the source spelling `main` is exactly `align_fn$4$6d61696e`, follows ordinary internal/reachability handling, and is retained only if explicitly reached (including a direct same-module test call). Catalog ordinal `n` maps to the exact hidden external root symbol `align_test$` followed by eight lowercase hexadecimal digits for `n` (ordinal 7 is `align_test$00000007`); the reserved family is compiler-owned and never a language export. Overlay helpers retain ordinary encoded private symbols. After the link and before signal-controller acquisition, the command materializes the immutable catalog, cache-statistics report, limits, suite working directory, and executable path needed by the runner, then consumes every compiler/build owner. This explicitly drops the whole/per-unit `PipelinedPackageComplete` (including its private `object_stage`), generated-harness object stage, response/temp files, cache leases, and joined worker/pipeline guards while ordinary Rust Drop still runs. Only the executable `ArtifactStage` crosses into the runner/final guard; no build-stage pathname or descriptor survives to raw `_exit`. The temporary executable is never published at the source stem and is removed after the last direct-child reap and before a suite summary. Every selected test launches that same immutable executable; there is no per-test compilation. | Driver/per-unit codegen/harness generation own the artifact and reserved-symbol collision check. Whole/per-unit semantic parity, all four source-main ABIs with absent/unreferenced/directly-called variants, no duplicate literal `main`, production-prefix identity, overlay-closure inclusion, single-link/same-inode reuse, exact ordinal/symbol dispatch, hidden linkage, and build-stage absence before first spawn, cleanup-before-summary after success/failure are required. |
| Test-callgraph process boundary | Before cache lookup, native-capability collection, object or harness allocation, or any external side effect, `alignc test` walks the validated combined call graph reachable from catalog roots. Reachability includes direct and imported calls, function-value targets, lifted callbacks/destructors, and every concrete generic monomorph in whole-program and per-unit compilation. A reachable `ExprKind::ProcessCommand` rejects with exactly `process.command is not available from test code; run the external process in an owner test`. Sites are considered by catalog order, then dependency-edge and structural HIR order, so the first diagnostic is deterministic and a shared site is emitted once. An unreachable production helper containing `process.command` does not reject and may remain as inert code in frozen-prefix test objects and their ordinary runtime selection, but no catalog root can execute it. The combined checked-HIR validator repeats the exclusion for malformed input and proves the rejected graph has no executable command edge; production commands and ordinary `process.command` behavior are unchanged. `process.spawn`, `process.exec`, `process.exit`, and `process.abort` keep their settled behavior and remain inside the row process-group contract. | Sema records no special test-only process variant; the driver/combined-view validator own the reachability rejection before artifact formation. A parameterized owner crosses direct/nested/imported/generic/function-value/lifted reachability, shared and unreachable production helpers, catalog/source-order precedence, malformed checked HIR, whole/per-unit parity, inert-prefix retention, and unchanged production artifacts. The first capability may widen this boundary only with a separately reviewed portable descendant-containment protocol; it does not ship a partial dynamic supervisor. |
| Signal controller | After artifact workers have joined and before any test child is acquired, the driver acquires the one process-global driver signal lease shared with other long-running modes. A second lease fails before native side effects. Setup first reads the current thread mask and requires SIGHUP, SIGINT, SIGQUIT, and SIGTERM to be unblocked; it requires SIGCHLD to have the default disposition without `SA_NOCLDWAIT` and never changes SIGCHLD. It snapshots the four existing graceful dispositions, blocks those signals, creates one nonblocking close-on-exec self-pipe, installs async-signal-safe handlers without `SA_RESTART` in SIGHUP/SIGINT/SIGQUIT/SIGTERM order, publishes the lease, then restores the original mask. One preallocated lock-free arbitration atomic has the closed states `Idle`, `Writing`, `Selected(signal)`, and `WritingPending(signal)`. On entry a handler saves the interrupted thread's `errno`; every handler exit restores that exact value after arbitration and the self-pipe attempt, including EINTR retry, pipe-full EAGAIN, and already-selected paths. A handler preserves the first signal by changing `Idle -> Selected` or `Writing -> WritingPending`; either signal-bearing state is terminal to later handlers. It then writes one self-pipe byte, retrying EINTR and treating EAGAIN as successful coalescing; it does not allocate, lock, format, close, reap, or touch compiler state. A reporter may start a raw write only by changing `Idle -> Writing`. A handler during that syscall can only make the signal pending; after the syscall the reporter changes `WritingPending -> Selected` or `Writing -> Idle` before advancing any output. Therefore no raw write can begin while `Selected` exists, and a syscall already holding the permit precedes rather than follows selection. Setup failure attempts every installed-handler restoration in reverse order, closes write then read only after all handlers restore, clears the lease, and restores the mask last. An error path that returns to its caller tears down only after every child and stage is gone: it blocks the four signals, restores dispositions in reverse order, closes write then read, clears the lease, and restores the original mask last. Suite finalization instead transfers the valid controller and stage into a non-returning `FinalExitGuard`. A failed final stage removal retains both through the infrastructure diagnostic attempt and direct exit. After successful removal the guard retains the controller through summary writing, blocks the four signals with fixed valid mask arguments, checks the arbitration state once more, and invokes raw `_exit(128 + signal)` for a selected signal or raw `_exit(suite_status)` otherwise; selected SIGHUP/SIGINT/SIGQUIT/SIGTERM therefore produce numeric statuses 129/130/131/143, `WIFEXITED == true`, and `WIFSIGNALED == false` rather than re-raising; a later signal remains pending only until kernel teardown. A terminal `WriteFailureGuard` and any selected graceful signal likewise retain the controller through cleanup and direct exit, so no terminal path restores a prior ignored/custom disposition before its last write. If setup rollback or a returning teardown restoration fails, the pipe/lease remain valid and direct exit lets kernel teardown remove them; no path returns with a handler targeting a closed/reused descriptor. Existing graceful handlers are temporarily replaced, restored only on a genuine returning path, and never chained. | One parameterized signal owner crosses default/ignored/custom prior dispositions, each initially blocked signal, incompatible SIGCHLD forms, second lease, every setup/rollback/returning-teardown failpoint, lock-free-state probe failure, every arbitration transition, errno preservation across pipe-full/EINTR/already-selected delivery, simultaneous signals, final-stage cleanup failure, terminal summary commit, terminal writer failure, exact raw exit/wait-status classification, and a signal before permit, during each raw syscall, after completion, and at every lifecycle state. |
| Parent-to-harness launch ABI | Before each spawn the parent creates one `AF_UNIX` `SOCK_DGRAM` socketpair, marks both endpoints close-on-exec, and marks the parent endpoint nonblocking before spawn; any flag operation failure closes both and is infrastructure failure. The child endpoint remains blocking. Child spawn actions first install the exact suite working directory with `posix_spawn_file_actions_addchdir_np` and map the row endpoints to fd 0/1/2/3 with close-on-exec cleared on each final mapping even when its source already equals the target. Linux then appends `posix_spawn_file_actions_addclosefrom_np(4)` after every duplication. macOS instead enables `POSIX_SPAWN_CLOEXEC_DEFAULT` together with `POSIX_SPAWN_SETPGROUP`; Apple defines every descriptor created by a file action as inherited under that flag, so the four mappings survive while every ambient or row-original descriptor is closed in the child snapshot. Both paths leave exactly `/dev/null` fd 0, capture fds 1/2, control fd 3, and nothing else. Original ambient fd 0..3 cannot survive because all four slots are replaced. A supported Linux release host must provide the addchdir and addclosefrom spawn extensions; a supported macOS release host must provide addchdir and the close-on-exec-default spawn attribute. An unavailable extension or configuration failure rejects before spawn (`working directory` for cwd setup, `descriptor mapping` for remap, Linux close-from, or macOS close-on-exec-default setup). Argv contains only argv[0] equal to the private stage path, the inherited environment is unchanged, and cwd equals the one invocation snapshot for every row even if another embedding thread later changes the parent's cwd. Every parent receive is nonblocking, uses one fixed 21-byte capacity that distinguishes either accepted record from a long datagram, and drains until `EAGAIN` or `EWOULDBLOCK`; there is no blocking control receive in AwaitAck, Running, or Quiescing. No ordinal or control input travels through argv or environment. The parent samples the monotonic row start immediately before spawn and the timeout includes time spent inside spawn. The spawn setup requests `setpgid(0, 0)` before exec, and the parent must prove `getpgid(child_pid) == child_pid` before sending any launch datagram. If spawn succeeds but group establishment cannot be proved, the harness remains blocked before user code; the parent sends SIGKILL only to the retained direct PID, never to an unverified negative PGID, drains/closes the row descriptors, reaps that child, and reports spawn infrastructure failure. ESRCH from that direct-PID kill is accepted only after non-reaping observation proves the same unreaped child terminal. After verified group establishment the parent sends one exact 16-byte launch datagram: bytes 0..7 `41 4c 54 45 53 54 4c 01`; bytes 8..11 selected u32 ordinal little-endian; bytes 12..15 zero. The child runtime receives one datagram with a 17-byte capacity and validates length/magic/version/reserved bytes; the harness validates the linked catalog range, uses the runtime fd helper to mark fd 3 close-on-exec, and uses the runtime acknowledgement encoder/sender to emit exact bytes 0..7 `41 4c 54 45 53 54 41 01`, then the same ordinal and four zero bytes. Only after that acknowledgement may user test code run. Before acknowledgement, deadline expiry, per-stream output excess, descriptor mapping, launch send/read/validation/acknowledgement failure, child exit/signal, an unexpected/repeated datagram, or completion is runner infrastructure failure; the child runs no selected test after invalid launch input. The specialized pre-ack diagnostics are fixed below. | Driver spawn setup/launch encoder and the generated harness/runtime receive/ack path are independent codec owners. Semantic-to-byte and byte-to-semantic goldens pin ordinal 7 as `414c544553544c010700000000000000` and `414c5445535441010700000000000000`; a verification-state matrix covers suite-cwd snapshot/use and addchdir failpoints, endpoint flag/fd-0/1/2/3 remap failpoints including every source-equals-target case, Linux closefrom creation/order, macOS close-on-exec-default configuration, injected ambient fds at 4 and the current soft-limit boundary, concurrent parent cwd/fd mutation, exact child fd inventory, acknowledgement followed by an idle child/no completion, `EAGAIN`/`EWOULDBLOCK`, short/exact/long datagrams through the 21-byte parent capacity, magic/version, reserved, ordinal range, order, repetition, group setup/proof failure, deadline/output before versus after acknowledgement, and every syscall/acquisition edge. |
| Capture transport | After allocating the two fixed stores, the parent opens the `/dev/null` stdin source and creates stdout/stderr pipes with every original endpoint close-on-exec, then makes both parent read endpoints nonblocking before spawn. Child spawn actions map only `/dev/null` to fd 0 and the child write endpoints to fd 1/2 with close-on-exec cleared on those mappings, including every source-equals-target case, and close every unused original stdio endpoint. The parent closes its `/dev/null` and child write-end copies after spawn and never performs a blocking capture read. On each selected readiness it drains that stream directly into its store/probe until `EAGAIN`, `EWOULDBLOCK`, EOF, the rejected next byte, or a hard error, then returns to the event loop; a short write by a still-live child cannot stop control, the other stream, status, signals, or the deadline. Any open/pipe/flag/remap failure before successful spawn closes every acquired endpoint, releases both stores, and is infrastructure failure with no user code. | The runner owns stdio/capture flags and drain state. Parameterized owners cover stdout/stderr independently, short-write-then-idle children, simultaneous pressure, `EAGAIN`/`EWOULDBLOCK`, EOF, exact/rejected-next bounds, source fd 0/1/2 remaps, every open/pipe/flag/remap failpoint, and deadline/signal progress after a partial drain. |
| Process isolation and completion record | Each acknowledged test runs in its verified new process group. Only normal return from the selected test sends one exact 20-byte completion datagram on fd 3: bytes 0..7 `41 4c 54 45 53 54 00 01`; byte 8 outcome (`0` Ok, `1` Err); byte 9 Error tag (`255` for Ok, otherwise `0=NotFound`, `1=Invalid`, `2=Denied`, `3=Timeout`, `4=Code`); bytes 10..11 zero; bytes 12..15 signed i32 Code payload little-endian (zero unless tag 4); bytes 16..19 selected u32 ordinal little-endian. The harness then returns 0 for Ok or 1 for Err; process termination is the sole ordinary close owner for fd 3. After acknowledgement, datagrams are consumed in arrival order: the first must be the sole completion and is validated field-by-field; the first malformed field freezes that detail. A later exact-length datagram whose bytes 0..7 are the completion magic/version freezes `repetition`, while any acknowledgement or other unexpected control datagram freezes `order`; the earliest sequence/field error wins. No completion after the terminal barrier is the `length` detail. Ok passes only with one completion Ok plus exit 0; Err is a test failure only with one completion Err plus exit 1. Every other completion/exit/signal product fails closed, so `process.exit(0)`, `abort`, `exec`, and a crash cannot impersonate success. | Generated harness and the exact compiler-private runtime ABI below own child receive/fd-state/encoding/send; the parent driver owns an independent launch encoder and acknowledgement/completion decoders plus datagram cardinality. Independently checked semantic-to-byte and byte-to-semantic golden vectors cover both directions, arrival-order combinations, and every malformed product. |
| Child control runtime ABI | Test artifacts alone may declare four compiler-private unkeyed symbols: `i32 @align_rt_test_launch_recv_v1(i32 fd, ptr out_ordinal)`, `i32 @align_rt_test_fd_cloexec_v1(i32 fd)`, `i32 @align_rt_test_ack_v1(i32 fd, i32 ordinal)`, and `i32 @align_rt_test_report_v1(i32 fd, i8 outcome, i8 error_tag, i32 code, i32 ordinal)`, each `nounwind`. Rust receives the ordinal fields as `u32`, outcome/tag as `u8`, code/fd as `i32`, and the first output as `*mut u32`. Launch receive requires a non-null four-byte-aligned output, zeros it before I/O, receives once with a fixed 17-byte capacity while retrying EINTR, validates exact length/magic/version/reserved bytes, and publishes the little-endian ordinal only on success; catalog range remains harness-owned. The fd helper adds close-on-exec without changing other flags. Ack/report validate their closed semantic products, encode the exact stack-resident 16/20-byte records, and perform one datagram send with EINTR retry; a short send is `EIO`. All four return zero on success or a positive raw OS code, using `EINVAL` for invalid ABI input and `EPROTO` for malformed launch bytes. They allocate nothing, retain no pointer or descriptor, never close fd 3, and change no process-global state. The harness maps launch receive/range failure to reserved exit 120, close-on-exec failure to 121, acknowledgement send failure to 122, and completion send failure to 123; before acknowledgement 120/121/122 become `launch`/`descriptor flags`/`control write` infrastructure respectively, while post-ack 123 becomes `control write`, all with OS code zero because the failed child channel cannot transport its native code. Reserved-status interpretation is phase-specific; the same status produced by user code after acknowledgement remains an ordinary missing-record exit except for 123. | `align_codegen_llvm`, `align_runtime`, and the runtime ABI ledger own declarations/definitions atomically; none receives a language-callable `RuntimeKey`. ABI semantic/byte goldens, null/alignment, every invalid tag/product, EINTR/short/error send, malformed receive, fd retention/CLOEXEC/exec, reserved-status phase, symbol collision, base-export parity, and whole/per-unit link owners are required. |
| Time, output, and child cleanup | Before creating child pipes or spawning, the parent fallibly allocates one fixed raw-byte backing store of exactly the selected bound for each stream; zero allocates none. It never geometrically grows, replaces, or duplicates those stores after spawn. Reads fill the remaining range directly and use one fixed one-byte probe per full stream to detect the rejected next byte, so retained capture payload is exactly twice the selected bound plus two probe bytes and fixed pipe/control state, excluding allocator metadata/rounding, with no old/new-allocation transient. Allocation failure frees any first store and is runner infrastructure failure before user code. The deadline runs from the pre-spawn sample through acknowledgement, user execution, target signalling, group quiescence, descriptor drain, and direct-child reap. Exact output fit succeeds; the first extra byte before acknowledgement is infrastructure failure and after acknowledgement is a test failure. Every poll wake drains all currently queued control datagrams nonblockingly through `EAGAIN`/`EWOULDBLOCK`. Once `waitid(..., WNOWAIT)` observes the leader terminal, the runner drains control through that boundary again before classifying any completion as missing; because normal completion send precedes leader exit, this terminal-observation barrier cannot lose a queued fast-test record. After every successful spawn with verified group establishment and for every terminal path—pre-ack failure, Ok, Err, nonzero exit, signal, timeout, output excess, malformed/missing completion, infrastructure error, or interrupt—the leader PID stays unreaped while the runner signals the pinned process group first and that direct PID second. This direct target is mandatory even if the leader moved out of the verified group after acknowledgement. The unverified-group failure above uses only direct-PID cleanup because no group target is trusted and no user code has run. Ordinary/test/infrastructure paths send SIGKILL to both targets in that order. A graceful SIGHUP/SIGINT/SIGQUIT/SIGTERM first forwards that signal to both, permits exactly 250 ms, then sends SIGKILL to both. A release host without non-reaping observation rejects before the first spawn. Group ESRCH is accepted after either a same-child terminal observation or a successful subsequent direct-PID signal; direct-PID ESRCH is accepted only after non-reaping observation proves that same unreaped child terminal. Other target errors are infrastructure failures, but every later signal/observe/drain/close/reap step is still attempted in fixed order. The parent then completes the terminal control barrier, drains accepted stream prefixes, closes descriptors, and reaps only the direct child with EINTR retry. In-group descendants are signalled through the pinned group but not reaped; descendants that escaped it remain outside the contract. The quiesced result retains both capture stores after descriptors close and direct-child reap; it releases them only after complete failure-block reporting or silent pass discard. Any cleanup failure stops the suite after best effort; if an individual failure reason was already selected, its retained bounded block precedes the infrastructure diagnostic. The first graceful signal outranks simultaneous test/infrastructure outcomes, emits no new diagnostic or summary after cleanup, and invokes raw `_exit(128 + signal)`—129/130/131/143 respectively; an observing parent sees a normal numeric exit (`WIFEXITED`, never `WIFSIGNALED`) because the runner never restores/re-raises the selected signal. Only a fully reported/discarded and released ordinary test result permits the next catalog row. | The dedicated driver test-runner component alone owns signal snapshots, allocation, descriptors, deadline, pinned PGID/direct PID, non-reaping observation, quiesced evidence, and stage cleanup. A Cartesian state/event owner crosses pre/post-ack, idle-child nonblocking drain, completion before/after first drain and terminal observation, leader retained in/moved out of its group, descendants absent/present/escaped, every result class, four graceful signals, deadline/output at every boundary, each acquisition/cleanup failpoint, evidence retention through reporting, and no-next-row-before-report/release. |
| Reporting and exit | Passing child stdout/stderr is never emitted. A fully passing suite writes exactly `test result: ok. <N> passed; 0 failed` plus LF to runner stdout; explicit `--cache-stats` retains its existing additional cache diagnostics on stderr without exposing child output. For each failure, in catalog order, runner stdout receives `FAIL <canonical-id>` plus LF, one fixed `reason: ...` line, then nonempty captured stdout and stderr under fixed `--- stdout ---` / `--- stderr ---` headers without decoding or rewriting their bytes; a missing final LF is followed by one LF before the next runner line. The reporter consumes the quiesced row, writes directly from its still-owned stores, then releases them. Runner stdout then receives `test result: FAILED. <P> passed; <F> failed` plus LF. After a complete all-pass or failed summary, the controller-owning `FinalExitGuard` performs the terminal signal commit and invokes raw `_exit(0)` or `_exit(1)`. Timeout, output-limit, signal/exit/record mismatch, returned Error, and malformed-record details use the closed formats below; assertion location is the bounded child-stderr line. Successful child output remains suppressed even when another test fails. Zero tests use the exact diagnostic above. Pre-ack timeout/output use the exact infrastructure lines below. Every runner-owned stdout/stderr write uses the arbitration-aware no-SIGPIPE writer fixed below. A stdout report/summary write failure retains any written prefix and any incomplete row evidence in the terminal guard, removes the stage, retains any live signal controller, attempts the exact `report write` infrastructure line on stderr, emits no further summary bytes, and exits 1 directly. A stderr diagnostic write failure retains its written prefix and any current row evidence in the terminal guard, removes the stage, retains any live controller, makes no recursive diagnostic attempt, and exits 1 directly. Other build/compiler/catalog diagnostics retain their compiler-owned formats. A graceful interrupt emits no new bytes after its signal reaches `Selected`; a raw syscall that already held `Writing` may leave its complete or partial prefix before that selection. | CLI reporter owns deterministic bytes, terse-success behavior, quiesced-row consumption, sink failure, and terminal commit and exact numeric exit semantics. Golden stdout/stderr owners cover zero tests, all-success with/without cache stats, mixed result, arbitrary bytes, missing LF, assertion, every termination/control reason, every pre/post-ack product, signal before write permit/during syscall/after commit and the final block/recheck boundary, stdout/stderr partial/EINTR/EPIPE/zero-write failure, terminal evidence/controller retention, `WIFEXITED`/`WIFSIGNALED` classification, and infrastructure before/after a selected result. |
| Ownership, allocation, and effects | A test declaration owns no runtime value. The frozen production prefix and appended test overlay are compiler-owned immutable data; the test harness catalog and static assertion locations live only in the overlay. Test-mode object/harness allocation is driver-owned and every build-stage owner is consumed before signal-controller acquisition. One compiler-owned suite `PathBuf` snapshots cwd before artifact work and is borrowed by every spawn action; it allocates no per-row path. A live row owns two exact pre-spawn capture stores plus child/descriptors; quiescence consumes it into an immutable row that owns the selected outcome and both stores but no child or descriptor. Reporting consumes that row, writes framing and stored ranges directly without copying, and releases both stores only after the complete failure block; passing consumes and releases them without output. A terminal write failure instead transfers any incomplete row and stores into the non-returning guard, which retains them through direct process exit. Test functions and assertions are Impure and never exported. The bounded process-signal lease/self-pipe plus its lock-free output-arbitration atomic is the only new process-global state and remains valid through the non-returning final commit; per-write SIGPIPE suppression changes only the runner thread mask, restores it after a complete write, and retains the blocked mask only on the terminal write-failure path through direct process exit. No test registry, reflection table, hidden source scan, persistent test history, retry, concurrency, shuffle, filter, fixture lifecycle, snapshot, coverage, benchmark, or network policy is introduced. | Sema overlay formation, checked-HIR prefix/suffix validation, codegen data ownership, build-stage consumption, suite-cwd ownership, signal-lease/arbitration RAII, live-to-quiesced typestate, consuming reporter, terminal write/final-exit guards, and runner RAII are owners. Allocation accounting and failpoints pin exact requested live/transient capture bytes and prove no release-before-last-write; production-prefix and capability-link twins prove exclusion/inclusion. |
| Cache and sources of truth | Test mode is a distinct versioned cache domain. Its unit key includes the canonical span-erased production semantic/codegen identity, complete local overlay suffixes, ordered test catalog, bodies, assertion locations, permitted non-database test static descriptors, mode version, target/profile/codegen inputs, imported interface hashes, and runtime ABI fingerprint. Its harness key includes the complete ordered canonical-id/symbol catalog, sole-entry/source-main mapping, four child-runtime symbol/ABI records, and launch, acknowledgement, completion, and terminal-commit protocol versions. The production HIR projection starts with domain `align-production-codegen-v1` and exhaustively visits Program tables/functions/statements/expressions in stored order. It encodes every non-`Span` field with existing canonical scalar/sequence encoders. Immediately after each expression it also encodes the exact tri-state `absent | arena | individual` lookup result that MIR would obtain from that function's current span-keyed `drop_individual_exprs`; the validator rejects a side-table key that matches no traversed expression, and raw map iteration/order never enters the key. The ordered production static-descriptor projection encodes unit, item, descriptor id, visibility, consumer, driver, source tag plus `File.path_literal` or `Inline.decoded_sql`, params/row types, complete reachable contracts, and static options; it omits only constructor/common/native option spans and the source variant's path/literal span. The lowering memo key consumes the HIR projection plus its existing visibility/toggle inputs. Production codegen/artifact identity consumes the HIR and descriptor projections before its existing mode/target/profile and resolved-artifact inputs. Neither consumes raw `Debug`, compiler source-file paths, source spans, diagnostics, located metadata, raw side-table iteration, nor the test overlay. Public interface identity keeps its existing public-surface encoder over the production prefix and excludes the overlay without acquiring private-body or descriptor inputs. The existing complete-source frontend lookup may miss after a test-only edit; current production HIR/descriptor spans and located output may change when offsets shift, while the semantic ownership stream, descriptor projection, MIR codegen graph, object key, link inputs, and executable bytes remain unchanged. Update this file first, then `draft.md`, `docs/language-spec.md`, `docs/design-notes.md`, `docs/open-questions.md`, `docs/impl/02-frontend.md`, `docs/impl/07-roadmap.md`, `docs/impl/10-cache-first-optimization.md`, `docs/impl/16-test-policy.md`, `docs/impl/19-hir-validation-ledger.md`, `docs/impl/20-runtime-abi-ledger.md`, `docs/impl/21-build-perf-plan.md`, `docs/impl/22-repl-plan.md`, `docs/impl/pkg-design/db.md`, and the synchronized Japanese mirrors. | The exhaustive HIR/descriptor projection matches, canonical span-erased prefix/overlay key encoders, runtime ABI registry, and interface/implementation hashes own invalidation. A new HIR field/variant or semantic side table is a compile-time projection update. Owners independently mutate every ownership fact and descriptor semantic field, reject orphan ownership keys and database consumers in the overlay, vary every descriptor-only span, and change earlier test width to shift later production spans; they pin changed source/located metadata beside identical semantic/descriptor projections, public interfaces, object keys, link inputs, and executables, plus changed overlay objects/harness where applicable. |
| Cache projection tags | The domain is the literal UTF-8 bytes `align-production-codegen-v1` under the existing domain encoder. Each structurally visited expression appends exactly one u8 ownership tag: `00` absent, `01` arena (`false`), or `02` individual (`true`). Each descriptor source appends exactly one u8 tag: `00` File followed by the canonical `Option<str>` path-literal encoding, or `01` Inline followed by the canonical decoded-string encoding. All other scalar, option, sequence, enum, string, type, and contract fields use their existing fixed canonical encoders; unknown tags and unencodable lengths reject before cache lookup or publication. | Independently implemented semantic-to-byte and byte-to-semantic fragment goldens pin all five new tags; complete projection owners pin field order, sequence lengths, malformed tags/lengths, and a changed-field/unchanged-diagnostic-span product. |

## Contract rationale

The test block is a language declaration because its body needs the ordinary checker, ownership
model, and error propagation. It is not a second function syntax exposed to normal code: it has no
parameters, callable name, visibility, or public interface entry. The implicit successful tail is
the only wrapping performed by the construct. A recoverable failure remains the existing
`Result<(), Error>` path, while a hard error stays process-fatal and is isolated by the runner.

`core.test` is an explicit import because assertions are library capability, not punctuation.
Their early-Err behavior is limited to standalone calls in a test block, making the control edge
visible at the assertion site and preventing an assertion value from being ignored or transported
through an ordinary function. Helper functions keep the one existing pattern: return `Result` and
let the test apply `?`.

The entry/import closure is the only discovery root. Filesystem conventions such as a `tests/`
directory, filename suffixes, annotations, and manifests would add hidden inputs to compilation and
cache identity. Dependency-first unit order already exists and is deterministic, so the runner
uses it instead of inventing a second order.

An entry without a module declaration receives the default `main` identity only when no imported
source explicitly declares `module main`. That collision rejects before catalog construction with
`default entry module 'main' conflicts with imported module 'main'; declare the entry module
explicitly`. This makes module identity unique before test-name, source-ordinal, or catalog-order
validation. An explicitly declared entry path follows the ordinary duplicate-module rule.

## Checking and lowering

Every compiler command parses tests. Semantic checking installs a test context only while checking
the declaration body. It resolves normal same-module private items and imported public items, and
it permits the two `core.test` assertions only when that import is present. The context does not
flow through a called function or into a lambda. A nested ordinary block, `if`, `match`, loop,
arena, or unsafe block remains part of the same test body.

The parser's ordinary block rule remains global: a last expression before `}` is an AST tail even
when a newline or semicolon produced `End`. Sema assigns placement structurally from each AST parent
before checking children. In test context only, it reclassifies an exact imported assertion in that
syntactic slot at root completion or when the enclosing block/control expression is a complete
statement. It rejects the same tail on every Value edge, including one whose expected type is Unit.
Checked HIR therefore keeps one statement-only assertion form without a
parser-only builtin shape or an assertion value path.

Checked formation completes and freezes every ordinary-source function, generated helper,
monomorph, interned/nominal type, analysis fact, native capability, and static descriptor before it
checks a test body. Test roots and every artifact created only while checking them append to a
separate overlay whose indices use the production tables as immutable prefixes. An identical
monomorph already present in production is reused; test checking cannot upgrade, reorder, or mutate
that prefix. Production lowering validates both partitions and consumes only the prefix. Test
lowering combines prefix and overlay, emits each catalog root with the exact
`Result<Unit, Error>` ABI, and emits assertion locations as immutable data. The complete overlay
artifact graph—functions, all type-table suffixes, and test static descriptors—must equal the
closure reachable from catalog roots through every checked-HIR reference or generation edge,
including direct calls, function values, callback/destructor descriptors, lifted targets,
nominal/interned type members, and transitive function/type/resource monomorph demands. A malformed
partition, catalog back-reference, name, ordinal, body result, assertion context, or implicit-tail
shape rejects before MIR construction. No validator infers test status from generated symbol
spelling.

Before any test cache lookup, native-capability collection, or artifact allocation, the combined
view walks that reachable closure from catalog roots in catalog order, then dependency-edge and
structural HIR order. Direct/imported calls, function-value targets, lifted callbacks/destructors,
and concrete generic monomorphs are all edges. A reachable `ExprKind::ProcessCommand` rejects with
`process.command is not available from test code; run the external process in an owner test`; a
shared site is diagnosed once at its first root. An unreachable production helper remains in the
frozen prefix and may remain as inert code in test objects with its ordinary runtime selection, but
it has no path from the harness or a catalog root and therefore cannot execute.
The checked-HIR validator repeats the exclusion for a handcrafted combined view. Production
validation and ordinary `process.command` behavior do not change.

Database descriptors are the deliberate exception to overlay static formation. The package contract
admits `db.query`/command constructors only as complete bodies of named top-level descriptor
functions, and those ordinary declarations close in the production prefix before any test body is
checked. Tests may call them but cannot construct another database descriptor. The combined
validator rejects a database consumer in `test_static_descriptors`; `alignc test` reads the same
offline metadata as production compilation, including the same `CheckedRequired` failure, and never
opens a database. Consequently `alignc db prepare` has no test-mode option and no hidden overlay
discovery path.

Test functions are Impure. This is conservative and keeps them out of `par_map`, task transfer, and
generic effect promises even when one body happens to contain only arithmetic. The declaration and
overlay are not part of an interface summary. Imported units' test roots and suffixes are compiled
only because the driver explicitly selected the combined test view for those units.

## Runner model

The driver links one immutable test executable. Its generated `i32 @main()` is the sole literal entry
symbol. Test-mode codegen maps every permitted source `main` shape to the existing encoded
`align_fn$4$6d61696e` identity, emits no ordinary main wrapper, and gives it no automatic root; a
same-module test may still call it explicitly as an ordinary function. The generated entry receives
one launch datagram on the fixed compiler-private fd 3, validates the selected catalog ordinal,
acknowledges that exact ordinal, dispatches exactly one `align_test$<ordinal-as-eight-lowercase-hex>`
root, sends the completion datagram after that function returns, and exits. Therefore a test-only
program need not declare a user main. The parent launches that same executable once per catalog row,
sequentially.

The fd 3 datagram socket is separate from stdout and stderr and carries no application input. The
parent passes no compiler-private argv or environment value: the child observes only its stage path
as argv[0], the inherited environment, `/dev/null` stdin, and captured stdout/stderr. The harness
sets fd 3 close-on-exec before acknowledging launch, so a successful `process.exec` cannot inherit
the authority to report completion. The parent's endpoint is nonblocking before spawn, and every
parent receive returns to the poll/deadline loop at `EAGAIN`/`EWOULDBLOCK`; the child endpoint alone
uses blocking receive while awaiting the single launch record. The child mapping explicitly clears
close-on-exec on fd 3, including the source-is-3 case, before the harness sets it again after launch.
The parent's fixed 21-byte receive capacity distinguishes both valid record sizes from any long
datagram. A valid acknowledgement distinguishes harness setup from user execution; only a normal
test return is the safe completion producer. Missing completion bytes make
even exit zero fail, closing the `process.exit(0)` bypass. Unsafe code can violate the descriptor
boundary, as it can every safe compiler invariant; malformed or self-inconsistent datagrams still
fail closed.

The parent stdout and stderr read endpoints are likewise nonblocking before spawn. Readiness drains
only the currently queued bytes through `EAGAIN`/`EWOULDBLOCK` and returns to poll; it never waits in
`read` for a live child to produce another byte. The child mappings alone clear close-on-exec on fd
1/2, including source-equals-target cases. A parameterized short-write-then-idle owner proves that
each capture stream still permits control, status, signals, and the row deadline to advance.

The v1 launch/acknowledgement golden vectors are:

| Semantic value | Exact 16 bytes, hexadecimal |
| --- | --- |
| Launch ordinal 7 | `414c544553544c010700000000000000` |
| Acknowledge ordinal 7 | `414c5445535441010700000000000000` |

The v1 completion-record golden vectors are:

| Semantic value | Exact 20 bytes, hexadecimal |
| --- | --- |
| Ok, ordinal 7 | `414c54455354000100ff00000000000007000000` |
| Err(Error.Invalid), ordinal 7 | `414c544553540001010100000000000007000000` |
| Err(Error.Code(-9)), ordinal 7 | `414c54455354000101040000f7ffffff07000000` |

The parent encoder/decoders and child runtime codecs are separate implementations checked against
these vectors. The child boundary consists only of `align_rt_test_launch_recv_v1`,
`align_rt_test_fd_cloexec_v1`, `align_rt_test_ack_v1`, and `align_rt_test_report_v1` with the exact
ledger signatures above and in `docs/impl/20-runtime-abi-ledger.md`. A decoder reads the fixed
envelope in byte order, validates reserved and conditional fields, then compares the selected
ordinal and process status. It never transmutes an untrusted byte array into a native record.

## Assertions

`test.expect` evaluates its boolean once. `test.expect_eq` evaluates the left operand, then the
right operand, then performs the same comparison an ordinary source `==` would perform only when
that comparison returns exact `bool`. An ordinary vector or mask equality returns a mask and is
therefore rejected; there is no implicit all-lanes reduction, assertion-only equality family, or
debug formatter for aggregates. On failure the child writes exactly one line in this form to
stderr:

```text
assertion failed: <canonical-id>:<line>:<column>: expected true
assertion failed: <canonical-id>:<line>:<column>: expected equality
```

The canonical-id limit makes the line bounded. Line and column are decimal, one-based source
positions of the qualified assertion call. Operand source text and values are not retained,
formatted, reflected, or allocated. The assertion then follows the ordinary Err cleanup edge with
`Error.Invalid`; the runner's completion record independently proves that the test returned.

## Resource and outcome precedence

CLI input is validated left-to-right in this order before reading or compiling the entry: command
shape, common option spelling/value, test timeout, output bound, forbidden option combinations,
then entry path. Repeating either test-specific option is an error rather than last-wins. After
that validation and before source I/O, cache lookup, or artifact allocation, the driver snapshots
the suite working directory once. Failure has the exact `working directory` infrastructure
diagnostic, creates no artifact, and outranks every later source, catalog, cache, build, or runner
failure. A bad
module-identity pair rejects before catalog construction. A bad catalog is reported after checking;
the reachable-`ProcessCommand` walk follows catalog validation. Both complete before cache lookup,
native-capability collection, or artifact allocation.

The accepted test-command option product has these terminal consumers:

| Dimension | Admitted states | Sole terminal effect |
| --- | --- | --- |
| target CPU | default `baseline`; `native`; existing explicit LLVM CPU spelling; last occurrence | target machine for every unit and harness object; versioned object/harness key |
| profile | default/explicit `dev`; explicit `release`, `fast`, `small`, `tiny`; last occurrence | optimization pipeline for every unit and harness object; versioned object/harness key |
| runtime LTO | profile default; explicit off for every profile; explicit on only for release/fast; last on/off occurrence | one test-artifact runtime-bitcode decision; versioned unit/link key |
| cache statistics | absent/present; repetition idempotent | existing cache diagnostics only; no artifact or execution-order change |
| build jobs | last positive flag; otherwise positive `ALIGNC_JOBS`; otherwise available parallelism or 1 | object-build worker bound only; no catalog concurrency or artifact-identity change |
| timeout/output | exact defaults or one explicit in-range value each | per-row runner deadline and two capture stores only; no compiler/cache input |

The parameterized owner enumerates the Cartesian product after the stated profile/runtime-LTO
constraint, compares default and explicitly equivalent states, and observes each terminal consumer.
It does not infer coverage from successful CLI parsing alone.

One dedicated test-runner component owns the complete state machine; no CLI branch, harness codec,
or reporter may independently wait, signal, reap, or advance the catalog. Its states are:

| State | Ordered observable events | Required transition and invariant |
| --- | --- | --- |
| Ready/acquire | first graceful signal; capture allocation; stdout/stderr pipe; control socket; close-on-exec/nonblocking flags; suite-cwd/fd-close spawn actions; pre-spawn clock sample; spawn; process-group proof | The runner owns only the immutable executable stage, catalog, limits, suite cwd, and cache report; every build-stage owner has already completed normal Drop. A signal removes the stage and raw-exits with numeric status `128 + signal`. Any pipe/socket/flag/cwd/remap/close-from failure closes every acquired descriptor, releases both stores, and is infrastructure with no child. The completed spawn plan installs the snapshotted cwd, replaces fd 0/1/2/3, then closes fd 4 and above. Successful spawn plus `getpgid(child_pid) == child_pid` proof retains the leader PID and enters AwaitAck. Failed proof sends SIGKILL only to that PID, drains/closes the row, reaps it, and stops with spawn infrastructure failure without sending launch or running user code. |
| AwaitAck | first graceful signal; nonblocking control drain through EAGAIN/EWOULDBLOCK; nonblocking stdout; nonblocking stderr; non-reaping leader status; terminal control barrier; deadline | Control drains before streams and always returns to the event loop when no datagram is queued. Each ready capture stream drains only through `EAGAIN`/`EWOULDBLOCK`, EOF, excess, or hard error, so a short write from a live child returns to the event loop. A valid acknowledgement enters Running immediately, and the same drain continues under Running rules so queued acknowledgement + completion is consumed in one wake. An acknowledgement followed by an otherwise idle child cannot block stream/status/deadline processing. A pre-ack malformed/order error, deadline, or first excess byte is launch infrastructure. Leader terminal status is recorded without reaping or declaring completion missing; it enters Quiescing, whose post-terminal drain is the required barrier. |
| Running | first graceful signal; nonblocking stdout; nonblocking stderr; nonblocking control drain through EAGAIN/EWOULDBLOCK; non-reaping leader status; terminal control barrier; deadline | Stdout excess wins over stderr; either wins over completion/timeout. Each ready stream and the control socket drain only their queued data through `EAGAIN`/`EWOULDBLOCK`, EOF, excess, or hard error and return to poll. Every queued datagram is consumed in arrival order, so acknowledgement/completion coalescing and repetition are recorded before status classification. Completion without status remains pending. Empty queues immediately return to poll. Terminal status enters Quiescing without reaping or selecting a missing-record reason. After its barrier, a complete valid completion/status product observed before the same wake's deadline wins over that deadline; otherwise the closed precedence below applies. |
| Quiescing | selected graceful signal or mandatory signal; pinned-group then direct-PID targets; non-reaping terminal observation; final nonblocking control drain; remaining stdout/stderr; deadline; descriptor close; direct-child reap; cleanup errors | The pinned group and then the still-unreaped direct PID are both signalled before reap; the second target closes a leader that moved out of its verified group. The runner obtains or retains terminal `WNOWAIT` status, drains control again through EAGAIN/EWOULDBLOCK, drains accepted stdout before stderr, closes every row descriptor, then reaps only the direct child. The second control drain is mandatory even if the first drain returned empty immediately before terminal observation. A pre-ack deadline remains launch infrastructure; a post-ack deadline remains test timeout. Graceful signal remains highest precedence. Otherwise cleanup failure becomes infrastructure while preserving the selected outcome and stores. Successful cleanup produces one immutable quiesced row with no child/descriptor and with both stores still owned. |
| Reporting | first graceful signal; pass discard or failure-block write progress; report-write failure; store release | A pass emits no child bytes. A failure writes framing and retained ranges directly from the quiesced row. Complete reporting or pass discard releases both stores and enters Between rows. A selected graceful signal releases the stores, removes the stage, retains the controller, and raw-exits with no later syscall; one syscall that already held `Writing` may leave only its complete/partial prefix. Report failure transfers the row and stores into the non-returning writer guard, removes the stage while retaining the controller, attempts the stderr infrastructure line, and exits 1 directly without advancing, releasing incomplete evidence, or ordinary controller teardown. |
| Between rows | first graceful signal; next-row acquisition | There is no live/waitable direct child, open row descriptor, or capture store. Only a reported/discarded ordinary result advances the catalog. Infrastructure stops; graceful signal removes the stage and exits. |
| Finalize | first graceful signal; guard-owned stage removal; controller-owned summary write; terminal graceful-mask block and arbitration recheck; direct exit | `FinalExitGuard` owns the stage and controller before removal. Removal precedes the summary; failure retains both through the `stage cleanup` infrastructure-line attempt and direct exit 1 with no summary. After successful removal the valid controller stays installed and the no-SIGPIPE writer acquires one output permit for each raw syscall. A signal selected before the first permit emits none; a signal made pending during one syscall is selected after its complete/partial prefix and prevents another permit. After a complete summary, the guard blocks the four signals with the platform-probed fixed mask operation, rechecks the arbitration state, and invokes raw `_exit(128 + signal)` if selected or raw `_exit(suite_status)` otherwise; the former is a numeric `WIFEXITED` status, never `WIFSIGNALED`. Signals arriving after that block remain pending until kernel teardown. There is no ordinary controller teardown or return after summary publication. |

After successful Quiescing the ordinary result order is selected output/timeout, the first
arrival-ordered control sequence or record error, record/status correlation, then returned Error or
Ok. Within the first completion candidate the field order is length, magic/version, outcome, tag,
reserved bytes, conditional code, then ordinal; after one valid candidate, a later exact-length
datagram with the completion magic/version is `repetition` and any other datagram is `order`. Zero
completion candidates after the terminal barrier is `length`. This state-specific order closes
every multi-ready product without using scheduler timing as a retry or error-rewriting input.

The two selected-size capture stores are allocated fallibly before pipe, socket, or child
acquisition. Reads fill them in place; after a store reaches its exact bound, one fixed byte detects
overflow. No capture store grows or is replaced after spawn, and reporting writes stored ranges
directly instead of copying them. Thus the requested live and transient capture payload is exactly
two selected bounds plus two probe bytes and fixed state; allocator metadata and size-class rounding
are outside that selected-byte promise. Failure of either allocation frees both stores and reports
infrastructure failure before any child exists.

Kill, descriptor close/drain, wait, reap, and control cleanup run in a fixed best-effort sequence.
Any failed acquisition or cleanup operation is infrastructure failure and forbids the next test,
even when output excess, timeout, or another test failure was selected first. In that product the
runner first preserves the already selected bounded failure block, then emits the infrastructure
diagnostic, removes the stage, emits no suite summary, and stops.

## Terse output policy

The runner retains bounded output from acquisition through quiescence and outcome consumption but
emits none of it after success. This is the same failure-only evidence policy used by repository
validation: large passing suites have
constant-size output, while a failure retains the local evidence needed to diagnose it. Output is
kept as bytes. On failure the runner writes its own ASCII framing and replays each nonempty stream
unchanged; it does not perform UTF-8 replacement, terminal escaping, line splitting, or truncation
below the selected cap.

The output limit is a correctness result, not silent truncation. The first byte beyond a stream's
inclusive bound fails and terminates the test. A zero bound therefore permits only empty output,
including assertion diagnostics. Successful output from one test remains suppressed even if a
later test fails; this prevents one failure from expanding the whole suite log.

Implementation owner commands and the CI phases that exercise this capability use the repository's
existing `scripts/run-quiet.sh`/bounded-binary wrappers from `docs/impl/16-test-policy.md`: a
successful phase emits only its phase/aggregate summary, while a failing or interrupted phase
replays the complete captured diagnostic log for the failing unit. `ALIGN_QUIET_VERBOSE=1` is the
explicit investigation escape hatch. The wrapper changes no selected test, concurrency, timeout,
or verdict. This keeps compiler-test and CI success logs proportional to phases rather than to the
thousands of passing cases, while preserving full failure evidence.

The `reason:` line uses exactly one of these formats:

```text
reason: returned Error.NotFound
reason: returned Error.Invalid
reason: returned Error.Denied
reason: returned Error.Timeout
reason: returned Error.Code(<signed-i32>)
reason: timed out after <timeout-ns> ns
reason: stdout exceeded <max-output-bytes> bytes
reason: stderr exceeded <max-output-bytes> bytes
reason: exited with status <signed-i32>; completion record: <record-detail>
reason: terminated by signal <positive-i32>; completion record: <record-detail>
reason: completion record <Ok|Err> mismatched exit status <signed-i32|signal positive-i32>
```

`<record-detail>` is exactly one of `order`, `repetition`, `length`, `magic/version`, `outcome`,
`error tag`, `reserved bytes`, `error code`, or `ordinal`. The first arrival-ordered protocol error
wins; fields within its candidate use the validation order above. Decimal numbers have no leading
plus sign or zero padding. An output excess detected in both streams selects stdout by the
event-loop order. A valid Err record plus exit 1 selects its returned-Error line, including an
assertion's `Error.Invalid`; the assertion location remains in the replayed stderr bytes.

A runner infrastructure abort writes exactly
`alignc: test runner <operation> failed (os error <signed-i32>)` plus LF to stderr, where operation
is one of `stage create`, `stage cleanup`, `capture allocation`, `signal handler`, `stdin open`,
`pipe`, `control socket`, `descriptor flags`, `working directory`, `descriptor mapping`, `clock`, `control write`,
`control read`, `launch`, `spawn`, `process group`, `stdout read`, `stderr read`, `poll`, `close`,
`wait`, `kill`, `reap`, `report write`, or `diagnostic write`; the numeric value is the raw OS code,
or zero for allocation, validation, a child-side control failure whose channel cannot report its
code, zero-byte write, or a platform failure that exposes no code. The mapping is closed:

| Fallible runner operation | Exact diagnostic operation |
| --- | --- |
| private directory/executable-stage acquisition; final unlink/directory removal | `stage create`; `stage cleanup` |
| either fixed capture-store allocation | `capture allocation` |
| signal lease/mask/disposition/self-pipe install, rollback, or returning teardown | `signal handler` |
| `/dev/null` acquisition for child stdin | `stdin open` |
| stdout or stderr capture-pipe creation | `pipe` |
| fd-3 socketpair creation | `control socket` |
| close-on-exec, nonblocking, or other descriptor-flag operation | `descriptor flags` |
| suite `current_dir` snapshot, native-path conversion, or cwd spawn-action construction | `working directory` |
| fd 0/1/2/3 remap, Linux fd-4-and-above close-from action, or macOS close-on-exec-default attribute configuration | `descriptor mapping` |
| monotonic sample or timed graceful-cleanup clock operation | `clock` |
| parent launch send; child acknowledgement/completion send reported by reserved status | `control write` |
| parent acknowledgement/completion receive/drain | `control read` |
| launch/ack codec, reserved bytes, linked ordinal, or phase validation | `launch` |
| process creation and execution of the completed cwd/remap/close-from action plan | `spawn` |
| `setpgid` request or parent `getpgid` proof | `process group` |
| captured stream drain | `stdout read`; `stderr read` |
| readiness wait over controller/control/capture descriptors | `poll` |
| explicit descriptor close outside process termination | `close` |
| non-reaping terminal observation | `wait` |
| pinned-group or direct-PID signal | `kill` |
| direct-child consuming wait | `reap` |
| failure block or suite summary sink | `report write` |
| infrastructure stderr sink | `diagnostic write` |

No fallible syscall, allocation, validation, or cleanup site may synthesize another operation word.
Compiler, linker, checked-metadata, and catalog diagnostics retain their existing non-runner
formats. An infrastructure abort emits no final suite summary, even after earlier test outcomes;
any already emitted or
just-selected failure-block prefix remains. A `diagnostic write` failure cannot describe itself on
the failed sink: its partial stderr prefix and any incomplete row evidence are retained, no
recursive line is attempted, stage cleanup still runs, any live controller remains valid, and
direct exit is 1. Compiler diagnostics and link errors also emit
no suite summary. Artifact-stage removal occurs before the all-pass or failed-suite summary, so
cleanup failure cannot follow a published complete summary.

The reporter's private `write_no_sigpipe(fd, bytes)` primitive never allocates or buffers. On the
runner thread it snapshots the original signal mask, blocks SIGPIPE, then loops raw `write` over the
remaining range. Before each syscall it must acquire the controller's sole output permit by changing
`Idle -> Writing`; observing `Selected` returns the terminal signal path without calling `write`.
The handler changes `Writing -> WritingPending(signal)` rather than selecting while bytes may still
be emitted. After every positive, zero, error, or EINTR result, the writer resolves the state before
the caller can advance: pending becomes `Selected`, otherwise `Writing -> Idle`. It retries EINTR
only after returning to Idle and acquiring a new permit. Thus a signal that wins before a permit
emits no byte, while a signal delivered during one permitted raw syscall selects only after that
syscall's complete or partial prefix and no later syscall can begin. A complete range with no
selected signal restores the original SIGPIPE mask before success. Zero or any other error returns a
terminal `WriteFailureGuard` with its raw code and keeps SIGPIPE blocked; there is no ordinary-return
path from that guard. The guard absorbs and retains any incomplete row evidence; its caller retains
the partial prefix, removes the stage but deliberately retains any live signal controller, attempts
the diagnostic with SIGPIPE still blocked and the same permit protocol, then exits the process
directly so kernel teardown discards the evidence, controller, and any generated or pre-existing
pending SIGPIPE. Fixed valid mask arguments make writer restoration and the `FinalExitGuard`
graceful-mask block infallible on the release hosts; the platform owner also proves the arbitration
atomic lock-free before artifact allocation. A selected graceful signal outranks report, diagnostic,
or suite status. Parameterized sink owners cross every arbitration transition, original
blocked/unblocked and pre-pending SIGPIPE, full/short/zero writes, EPIPE/ENOSPC, EINTR with and without
a graceful signal, signal before permit/during syscall/after commit, stdout report/summary, stderr
diagnostic, pre/post-final-block signal, terminal controller retention, both guard non-return paths,
and diagnostic failure without recursion.

The three pre-ack resource failures instead write exactly one of these specialized infrastructure
lines to stderr and discard captured setup bytes:

```text
alignc: test runner launch timed out after <timeout-ns> ns
alignc: test runner launch stdout exceeded <max-output-bytes> bytes
alignc: test runner launch stderr exceeded <max-output-bytes> bytes
```

They exit 1, print nothing to stdout, run full group/direct-child/stage cleanup, and emit no suite
summary. Decimal formatting follows the result-line rule above.

## Cache and artifact identity

Production and test compilation share parsing and semantic rules, but sema closes and freezes the
production prefix before it forms the test overlay. A test-only edit can invalidate the existing
complete-source frontend lookup. The checked prefix retains current spans and located metadata, so
an earlier test edit may shift them. Production lowering/object keys instead encode a canonical
span-erased projection of the complete semantic HIR plus a semantic production-descriptor
projection; diagnostic spans, located metadata, and the overlay cannot enter either projection.
The exhaustive HIR walk emits each expression's `absent | arena | individual` ownership lookup
result in structural order instead of serializing or omitting the span-keyed map. The descriptor
walk emits every codegen-relevant field, including semantic file-path literals and decoded inline
SQL, while omitting only its diagnostic locations. These projections, the MIR codegen graph, object
key, link inputs, and executable must therefore remain byte-identical. The versioned encoder
replaces the current total-`Debug` lowering fingerprint rather than filtering rendered text; its
exact cache-plan transition is `docs/impl/10-cache-first-optimization.md` §6.6. Test-mode keys
encode that production identity plus every overlay suffix, local
body, ordinal, canonical id, assertion location, static descriptor, and mode version. The harness
key covers the complete ordered catalog, the sole-entry/source-main/root symbol mapping, the four
child runtime ABI records, all three control protocols, and terminal-commit version. An import-order
change may therefore change the harness order without changing a public module interface.
The invocation's native suite cwd is runner state only: it is snapshotted once and used by every
child, but never enters source, HIR, object, harness, link, or cache identity.

The executable stays below an `ArtifactStage` retained by the runner through the final child reap.
No source-adjacent binary, catalog file, history, snapshot, or machine-readable public test artifact
is produced.

## Implementation closure matrix

| Axis | Required implementation closure | Acceptance owner |
| --- | --- | --- |
| Syntax and formatting | `test` + string contextual lookahead, `test {}` / `pub test {}` type twins, rejected `pub test` + string, attribute/signature near-shapes, newline/braces, recovery, depth cap, format idempotence | parameterized lexer/parser/AST/formatter owner |
| Catalog and modules | independent HIR module/name/id correlation, exact external catalog-root/function back-reference, every assertion id equal to its enclosing catalog root, declared/default entry path, rejected default-entry/imported-declared-`main` collision and ordinary explicit-module duplicate, name/id/count bounds, duplicates, same name across modules, dependency-first diamond order, unimported exclusion, private/public access | whole/per-unit catalog golden, malformed-HIR identity/back-reference matrix, and sema owner |
| Body and control | implicit Ok, explicit Err, `?`, `match`, `else`, assertion early exit, every cleanup-bearing control join, malformed HIR | sema + checked-HIR + MIR control/Drop owner |
| Assertion surface | import rule, lexical contexts, statement-only checked HIR, root/nested statement-placement final syntactic tail normalization, every Value-edge rejection including expected Unit, bool equality families, vector/mask non-Bool rejection, left-to-right once, complete qualified-call span retained independently of a nested/multiline condition, line/column, first failure | parameterized parser-shape/sema-context owner plus MIR/runtime diagnostic golden |
| Checked artifact partition | production fixed point frozen before tests; catalog root/back-reference; overlay closure equality; test lifted and generic helper; shared production/test monomorph reuse; suffixes for every nominal/interned type class; permitted non-database test static descriptor and capability; database-consumer overlay rejection; no prefix mutation/reference to suffix | malformed prefix/overlay HIR matrix, whole/per-unit span-erased production-semantic twin, generated-closure, database policy/driver, and descriptor/capability owners |
| Production isolation and mode product | `align-repl` rejects a test-bearing entry transactionally; every source command validates the full result but selects the frozen prefix; located MIR/remarks omit overlay; all database descriptors are prefix-owned; no overlay capability/link/export/interface/cache influence; ordinary main unchanged | REPL rollback/ordinal owner plus parameterized command × accepted watch/profile/target/runtime-LTO/ThinLTO/PGO/jobs/cache-stats matrix; actual watch-rebuild/ThinLTO/PGO artifacts; database policy × driver offline metadata; production/test view, current-span/located-metadata, and byte-identical semantic/MIR-codegen/object/link/executable owners |
| Test options and artifact | complete accepted option product reaches its named terminal consumer; suite cwd snapshots after CLI validation and remains runner-only; no-main test program; sole harness `main`; all four source-main ABIs encoded without wrapper; source main absent/unreferenced/directly called; one link; one immutable inode; every whole/per-unit/harness build-stage owner completes normal Drop before runner acquisition; exact `align_test$<8hex>` ordinal dispatch and hidden symbols | CLI terminal-consumer Cartesian owner plus cwd failure/stability, build-stage absence-before-spawn, driver artifact, reserved-symbol collision, and whole/per-unit semantic/execution owners |
| Test-callgraph process boundary | catalog-root reachability across direct/imported/function-value/lifted/generic and recursively embedded implicit resource-Drop-hook edges; deterministic first site; direct and nested `ProcessCommand` rejection; shared site de-duplication; unreachable production helper acceptance; malformed-HIR rejection; no-test/count/process validation before publication-lock acquisition, production static-input resolution, or static-artifact formation; inert frozen-prefix command-code/runtime-selection retention; production parity | whole/per-unit reachability matrix including direct/nested resource Drop and the no-test/process × missing production-static-input precedence product, checked-HIR negative, inert-prefix twin, and unchanged-production artifact owner |
| Launch protocol | one snapshotted suite cwd installed by `posix_spawn_file_actions_addchdir_np`; fd 0/1/2/3 remapping including source-equals-target; trailing Linux `posix_spawn_file_actions_addclosefrom_np(4)` or macOS `POSIX_SPAWN_CLOEXEC_DEFAULT`; exact child descriptor inventory with no ambient fd; parent capture/control nonblocking before spawn; argv/environment/stdin shape; concurrent parent cwd/fd mutation; pre-exec group setup/proof; launch/ack codecs and goldens; Ack/short-output idle-child return to poll; malformed/order/ordinal products; pre/post-ack exit distinction | driver/harness protocol matrix and cwd/acquisition/flag/remap/platform-close failpoints |
| Child control runtime ABI | exact four symbols/signatures; output-pointer initialization/alignment; borrowed descriptor/pointer ownership; no allocation/close/retain/global state; launch receive/CLOEXEC/ack/report success and every error; reserved exits 120..123 and phase mapping | LLVM declaration/Rust export parity, independent codec goldens, registry/collision/attribute owner, and whole/per-unit link matrix |
| Completion protocol | independent runtime encoder/driver decoder, three semantic goldens, every malformed field, order/repetition/cardinality, wrong ordinal, Ack+completion coalescing, completion before/after EAGAIN and terminal observation, exit/record Cartesian product, exit/exec/abort/crash bypass | runtime/driver protocol matrix |
| Signal controller | shared lease, prior mask/dispositions, SIGCHLD compatibility, setup/rollback/returning teardown, lock-free `Idle/Writing/Selected/WritingPending` arbitration, exact interrupted-thread `errno` preservation on every handler path, controller retained through summary, terminal mask/recheck/raw `_exit`, exact `WIFEXITED`/not-`WIFSIGNALED`, HUP/INT/QUIT/TERM ordering, first-signal coalescing, signal before permit/during syscall/after commit and at every runner state | parameterized process-global signal, handler-errno, writer-arbitration, final-commit, and wait-status owner |
| Runner state machine | Ready/AwaitAck/Running/Quiescing/Reporting/Between/Finalize x every event, one pre-spawn terminal row deadline threaded unchanged through signalling, quiescence, drain, and reap with no cleanup reset, one work cutoff derived inside that same total budget to reserve cleanup time, pre/post-ack deadline/output, nonblocking drain-empty/terminal/drain barrier, all-outcome pinned-group then direct-PID signalling before reap, quiesced evidence through report/release, controller-owned summary through terminal commit | deterministic Cartesian state/event, exact short-deadline cleanup bound, typestate, nonblocking/barrier, and final-commit owner |
| Child lifecycle | exact preallocation/failures, `/dev/null`, cwd snapshot/action, spawn/pipe/control acquisition/flag/remap/platform-close/clock/poll failures, exact fd 0..3 inventory with ambient fd 4/soft-limit probes, every closed diagnostic-operation mapping, all build-stage owners absent before first spawn, unverified-group direct-PID cleanup, nonblocking stdout/stderr/control drains through empty, short-output idle child, exact/rejected-next bounds, leader retained in/moved out of verified group, descendants absent/present/escaped for every terminal result, WNOWAIT pin, group-then-direct signalling before reap, terminal control barrier, cleanup-error override with evidence preservation, interrupted wait/read | deterministic allocation/I/O-liveness/failpoint, closed diagnostic-operation, build-owner, and process-tree owners |
| Reporting | exact zero-test and pre-ack infrastructure bytes, all pass one line with/without cache stats, mixed failure, returned Error variant, assertion, exit/signal/timeout/output/order/repetition/malformed reason, four graceful signals with numeric raw-exit status, raw bytes/missing LF, capture lifetime through last byte or terminal exit, SIGPIPE-safe full/short/zero/EINTR/error writes, `Idle/Writing/Pending/Selected` permit transitions, partial-prefix-before-selection, terminal evidence/controller retention, pre/post-final-block signal, nonrecursive diagnostic failure, and quiet-wrapper success/failure/interrupt behavior | exact CLI stdout/stderr, consuming-row, sink-failpoint, writer-arbitration, final-exit/wait-status, and repository `run-quiet` integration owners |
| Cache identity | independent mutations of every span-erased production and overlay/harness key field; per-expression `absent | arena | individual` ownership facts; orphan span-key rejection; every semantic descriptor field and diagnostic-only descriptor span; earlier variable-width test edit causing frontend/located miss and changed production/descriptor spans but identical semantic/descriptor projections and object hit; test descriptor/capability inclusion | cache hit/miss, ownership-stream/descriptor projection, prefix/suffix, and canonical-key owner |

## Capability boundary and deferrals

The implementation closure matrix is re-opened around the implicit-effect, identity, and deadline
axis: recursive resource Drop hooks participate in test-callgraph process reachability, assertion
identity is correlated with the external catalog root while its complete call span survives HIR,
and one pre-spawn terminal row deadline, with an internal work cutoff that reserves cleanup inside
the same total budget, reaches every cleanup phase without reset. This extends the earlier
test-callgraph and terminal-exit closure around reachable process construction, default/imported
module identity, `align-repl` item handling, signal/write arbitration, and exact numeric exit
observation. It retains the preceding artifact-formation-to-execution mode and ABI closure and the earlier
statement-placement, terminal-target, capture/control I/O-liveness, and semantic side-table
closures. The final launch/terminal-owner pass also closes build-stage discharge before raw exit,
suite-cwd installation, ambient-descriptor exclusion, and handler-`errno` preservation. The accepted code
has two strict top-level proof domains: compiler formation owns syntax through the hidden harness
and exact control codecs; one dedicated driver test-runner
component consumes only an immutable artifact, ordered catalog, validated limits, snapshotted suite
cwd, and immutable cache report after every compiler/build owner has completed normal Drop, and
exclusively owns signal state, spawn/poll, deadline, process groups, capture, reap, and reporting.
Neither side may reproduce the other's validation or lifecycle transition.

Compiler formation itself has a typed seam: `CheckedProgram.production` reaches its complete
ordinary-source fixed point and freezes before `TestOverlay` formation. The overlay may read and
reuse prefix identities but owns every newly generated test root, lifted/monomorph function,
nominal/interned type suffix, static descriptor, and capability consequence. Production consumers
can obtain only the prefix view; test consumers obtain the validated combined view. Prefix/suffix
bounds and catalog reachability make it impossible to omit a test helper by checking only the root
tag or to perturb production ids while discarding a body later. A database Query/command descriptor
cannot enter the overlay because its source constructor is restricted to an ordinary named
top-level descriptor function formed in the prefix; the validator repeats that consumer exclusion,
and tests reuse ordinary offline checked metadata.

The same combined-view reachability walk rejects every test-reachable `ProcessCommand` before
cache, capability, or artifact work. It does not try to supervise a command subtree dynamically:
direct, imported, generic, function-value, and lifted routes are one closed static product, while an
unreachable production helper may remain inert in the frozen-prefix object and every production
command remains unchanged. `align-repl` closes
its own exhaustive item match by rejecting any submitted entry containing `Item::Test` before
replacement resolution or session mutation. These are explicit first-capability boundaries, not
runtime special cases hidden inside `std.process`.

Compiler formation also owns two explicit projections. Test-context sema may consume a syntactic
`Block::tail` assertion as a statement only at root completion or statement placement, assigned
from the AST parent before child checking; every Value edge rejects even with expected Unit.
The checked-HIR assertion stays statement-only. Separately, the production codegen/cache identity is
the complete span-erased semantic HIR and descriptor projection. Its structural expression walk
encodes the ownership fact MIR observes through the current span-keyed side table, and its descriptor
walk encodes all semantic fields while discarding diagnostic locations. Current spans remain in the
checked prefix for diagnostics and located output but cannot perturb object identity or hide a
different ownership fact behind the same memo key.

Artifact formation has one exact entry/symbol boundary. The harness alone owns literal `main`;
test-mode codegen suppresses every normal source-main entry policy and maps all allowed source-main
ABIs to `align_fn$4$6d61696e`. Global catalog ordinals map to the reserved hidden
`align_test$<8hex>` family. Target/profile/runtime-LTO inputs reach both unit and harness objects,
while jobs/cache statistics, timeout/output limits, and suite cwd terminate at build scheduling,
diagnostics, and runner state respectively. After link, formation materializes only those immutable
runner inputs and drops the complete whole/per-unit `PipelinedPackageComplete` records, their
private object stages, the generated-harness object stage, response/temp files, cache leases, and
joined worker/pipeline guards. Only the final executable `ArtifactStage` enters the non-returning
runner, so raw exit can skip no build-stage destructor. Production consumers exercise their full admitted one-shot/watch,
whole/per-unit, ThinLTO, and PGO state product against the prefix-only selector.

Child control crosses one compiler/runtime seam containing exactly four unkeyed functions. The
runtime owns launch receive, fd-3 close-on-exec, acknowledgement encoding/send, and completion
encoding/send; the harness owns catalog range/dispatch and reserved-status choice; the driver owns
the independent launch encoder and acknowledgement/completion decoders. No side may duplicate the
other’s codec or infer an ABI from symbol spelling. The ABI rows, codec versions, and reserved exit
mapping enter the harness/runtime cache identity together.

Each spawn uses the one native suite-cwd snapshot and a closed child-descriptor plan. On supported
Linux/macOS hosts, `posix_spawn_file_actions_addchdir_np` installs that cwd before exec and the fd
0/1/2/3 remaps follow. Linux appends `posix_spawn_file_actions_addclosefrom_np(4)` last; macOS sets
`POSIX_SPAWN_CLOEXEC_DEFAULT`, under which its four file-action mappings remain inherited and every
other descriptor is closed. Thus the harness starts with exactly `/dev/null`, stdout capture, stderr
capture, and fd-3 control, and cannot observe an ambient parent fd. The snapshot remains stable if
an embedding thread later changes the parent cwd or descriptor table.

Inside the runner, `LiveRow` owns child, descriptors, stores, protocol state, and deadline. Its
parent capture and control endpoints are nonblocking before spawn, so every drain has a typed
`Data | Empty(EAGAIN/EWOULDBLOCK) | Eof | Error` result and Empty returns to poll. Pinned-group then
direct-PID signalling, non-reaping terminal observation, the required second control drain,
descriptor closure, and direct-child reap consume it into `QuiescedRow`, which owns only immutable
outcome and both capture stores. The reporter is the sole consumer of `QuiescedRow`; complete
failure-block write or silent pass discard consumes and releases the stores before catalog advance.
A terminal writer failure consumes `Reporting` into a non-returning guard that retains any
incomplete row and stores through direct exit. There is no API that releases stores while retaining
a reportable row, returns from that terminal guard, or classifies missing completion before the terminal barrier.
Fake-stage protocol/process-tree and sink-failpoint owners validate this runner without compiling
Align source, while whole/per-unit owners validate the producer against the same codec boundary.

`FinalExitGuard` first consumes the artifact stage and signal controller. A failed stage removal
keeps both owned through the terminal diagnostic/direct-exit path. After successful removal it
retains the controller through the last summary write. Each raw syscall first owns the atomic
`Writing` permit; a signal during it becomes `WritingPending`, is selected after that syscall's
prefix, and prevents every later permit. The guard then blocks the four graceful signals, rechecks
the arbitration state, and invokes raw `_exit(128 + signal)` or `_exit(suite_status)`. It never
re-raises a selected signal, so the external wait status is numeric `WIFEXITED`, not `WIFSIGNALED`.
It has no ordinary teardown/return edge. This terminal
commit prevents restored ignored/custom handlers from changing summary behavior and prevents a
fallible controller cleanup from following a published successful summary.
Every installed handler also saves the interrupted thread's `errno` before arbitration/self-pipe
work and restores it on every exit, so asynchronous delivery cannot corrupt an unrelated syscall's
observable error state.

These domains still land as one public capability after this design PR. Parser-to-harness without a
runner and a runner without compiler-produced private symbols are both dormant, so splitting them
would publish no useful stable consumer and would duplicate mode/cache/ABI integration proof. The
expected hand-written diff may exceed 1,000 lines for that reason. The explicit internal boundary,
state/event matrix, and independent owners reduce integration risk without shipping an unusable
prefix.

The first capability deliberately excludes `process.command` from every test-reachable call graph,
test declarations from `align-repl`, test filtering/listing, parallel or shuffled execution,
retry, fixtures, setup/teardown hooks, snapshots, coverage, benchmarks, ignored tests, expected
failure, persistent history, hidden file discovery, and an assertion formatting/reflection system.
`process.command` may be reconsidered only with a separately reviewed portable descendant-
containment protocol. Each other item is additive only after a real consumer demonstrates that the
sequential error-model core is insufficient.

## Design-review finding closure

| Finding | Ledger-first closure |
| --- | --- |
| P1 final assertions became block values | Test-context sema normalizes an exact syntactic-tail assertion only at root completion or structural statement placement; checked HIR remains statement-only and every Value edge, including expected Unit, rejects. |
| P1 a blocking parent control receive could stop deadlines | Both endpoints become close-on-exec and the parent endpoint nonblocking before spawn; every drain ends at EAGAIN/EWOULDBLOCK, with an Ack-only idle-child owner. |
| P1 group-only signalling could leave the direct child alive | Every verified terminal path signals the pinned group then the unreaped direct PID, including a leader-moved-group owner and target-specific ESRCH rules. |
| P2 raw HIR byte identity included shifting spans | Cache/codegen identity uses the complete span-erased semantic projection; an earlier variable-width test edit must change current spans/located metadata while preserving the object key and artifact bytes. |
| P1 blocking capture drains could stop deadlines | Both parent capture read ends become nonblocking before spawn and every readiness drain ends at empty/EOF/excess/error, with stdout/stderr short-write-then-idle owners. |
| P1 omitting span-keyed semantic side tables could collide | Structural expression order encodes the exact ownership fact MIR observes; orphan keys reject, and semantic descriptor fields have an explicit span-free projection. |
| P2 the cache plan retained the total Debug key | The focused cache plan records the versioned production projection, exact retention charge, transition boundary, and owner cells, and this ledger lists it as a source of truth. |
| P1 the generated harness and a source `main` could both own the executable entry | The harness is the sole literal `main`; all four source-main ABIs use the existing encoded `align_fn$4$6d61696e` identity in test mode, no ordinary wrapper is emitted, and entry/symbol owners cover absent, unreferenced, and direct-call cases. |
| P1 test-only checked database descriptors lacked a preparation path | The accepted package grammar cannot form a Query/command descriptor in a test body: descriptor constructors remain named top-level production declarations. Tests reuse their prefix metadata offline, a database consumer in the overlay rejects, and policy × driver owners cover missing/stale `CheckedRequired` evidence without a new preparation flag. |
| P1 child completion reporting had bytes but no native ABI | Four exact unkeyed runtime declarations fix launch receive, fd close-on-exec, acknowledgement, and completion signatures, validation, ownership, allocation, return/error, descriptor close, reserved-status, registry, and cache rules. |
| P1 production and test option/mode states were not closed | The ledger names the complete admitted production mode product and each test option's sole terminal consumer; parameterized Cartesian selection plus actual watch/ThinLTO/PGO artifacts prevent an overlay from leaking through an untested route. |
| P2 `/dev/null` and event-loop failures had no diagnostic operation | The infrastructure vocabulary now has a closed fallible-site table, including exact `stdin open`, `descriptor flags`, `descriptor mapping`, `clock`, `process group`, and `poll` mappings, with one failpoint owner per row. |
| P1 witness EOF could not prove nested-command cleanup after a supervisor crash | The dynamic witness/sentinel design is removed. Test formation statically rejects every catalog-reachable `ProcessCommand` before cache, capability, or artifact work, and owners cover every direct/imported/indirect/generic route plus unreachable production controls. |
| P1 a supervisor status channel lacked nonblocking target/abort arbitration | No supervisor or status channel ships. The same static exclusion closes the nested-command failure domain structurally instead of adding a second runtime state machine. |
| P1 `align-repl` exhaustively matched `Item` without a test policy | A test-bearing submitted entry rejects transactionally before replacement resolution, session mutation, compilation, or execution with one exact diagnostic; the session and next ordinal remain unchanged. |
| P2 child control prose disagreed on four versus five ABI functions | The discarded containment ABI is absent. The child boundary remains exactly the four launch-receive, fd-CLOEXEC, acknowledgement, and completion functions in this ledger and the runtime ABI inventory. |
| P2 selected signals lacked an observable process-status contract | The final guard uses raw `_exit(128 + signal)`, fixing 129/130/131/143 as numeric `WIFEXITED` statuses and explicitly forbidding re-raise/`WIFSIGNALED`. |
| P2 an implicit entry `main` could alias an imported declared `main` | Loading rejects that pair before catalog construction with one exact diagnostic; explicitly declared entry paths use ordinary duplicate-module validation. |
| P2 raw terminal exit could skip per-unit build-stage cleanup | After link and before signal-controller acquisition, every whole/per-unit/harness build owner completes normal Drop; only the final executable stage enters the runner, and an owner proves object-stage absence before first spawn. |
| P2 the child inherited unspecified ambient descriptors | The supported-host spawn plan replaces fd 0/1/2/3, then uses trailing Linux `posix_spawn_file_actions_addclosefrom_np(4)` or macOS `POSIX_SPAWN_CLOEXEC_DEFAULT`; fd-4/soft-limit probes and an exact child inventory prove no ambient descriptor survives. |
| P1 the macOS plan referenced an unavailable addclosefrom symbol | The platform closure axis is reopened: Linux retains its supported trailing close-from action, while macOS uses its supported close-on-exec-default spawn attribute, whose file-action inheritance rule preserves only the four explicit mappings. |
| P2 child working directory was unspecified | The driver snapshots native cwd once after CLI validation, maps snapshot/action failures to exact `working directory`, installs it with `posix_spawn_file_actions_addchdir_np`, and holds it stable across all rows and concurrent parent cwd changes. |
| P2 signal handlers could corrupt interrupted-thread `errno` | Every handler saves `errno` on entry and restores it on all arbitration/self-pipe exits, with Idle/Writing/Selected plus EINTR and pipe-full owners. |
