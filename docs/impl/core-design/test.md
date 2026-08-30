This directory holds the authoritative per-area design docs for the `core` library, at the same
depth as `../std-design/` (signatures, ownership/effect classification, error policy, pitfalls,
and test anchors).

# core.test — in-language tests

> 🌐 **English** · [Japanese](./ja/test.md)

> **Status:** proposed design completed 2026-08-30; implementation pending. No parser, compiler,
> runner, or assertion surface is shipped by this document alone.

## Public-contract ledger

This ledger is authoritative for the first implementation. Prose below may explain a row but may
not widen it. A public change updates this table first, then every listed source of truth in one
pass.

| Surface | Exact public record | Owner, artifact/cache identity, and acceptance owner |
| --- | --- | --- |
| Declaration grammar | One private top-level declaration: `test` followed by one ordinary Align string token and one block. `test` remains an identifier and commits to this contextual item only when the item-position two-token lookahead is `test` + string; bare `test {}` and `pub test {}` remain keyword-less type declarations. The lookahead `pub test` + string commits to the rejected visible-test form. After commitment, parameters, type parameters, a written return type, expression-body `=`, attributes, a missing block, and a trailing declaration name reject with test-specific recovery. The declaration creates no callable or name binding. | Lexer keeps `test` as an identifier; AST/parser/formatter own the contextual item. Parser/formatter round-trip, `test`/`pub test` type twins, lookahead/recovery, and every rejected near-shape are one parameterized owner. |
| Name, identity, and catalog | The decoded name is 1..=256 UTF-8 bytes and contains no Unicode C0/C1 control: U+0000..U+001F and U+007F..U+009F are rejected. Duplicate names in one module reject. The canonical public id is `<canonical-module-path>::<decoded-name>`; an entry source uses its declared module path, or `main` only when it omits a module declaration, and the complete id must fit 1..=1,024 UTF-8 bytes. At most 65,535 tests exist in the explicit entry/import closure. Catalog order is the existing dependency-first DFS unit order (direct imports in source order), then source declaration order in each unit. Only that explicit closure is searched; no directory, filename, annotation, or manifest discovery is added. | Sema owns validation and canonical ids; the driver owns the ordered catalog. Declared/default entry paths, exact C0/C1 boundary neighbors, name/id/count limits, duplicate scope, an entry/imported `main` twin, diamond-import order, and ignored unimported files are catalog owners. |
| Body type and control | A test is checked as a compiler-private zero-parameter `fn() -> Result<(), core.Error>`. After its written block completes with Unit, the construct supplies one documented `Ok(())` tail. `?`, `return Err(...)`, `match`, `else`, arenas, Drop, and every ordinary control form retain their existing semantics. A written non-Unit tail or `return` of any other type rejects. Normal Ok/Err and assertion exits run ordinary function cleanup; hard errors, `process.abort`, and successful `process.exec` retain their existing no-unwind behavior. | Sema/HIR/MIR own one flagged private test function and its implicit tail; no public interface entry is emitted. Control-flow, cleanup, Error-variant, and malformed checked-HIR owners cover direct and per-unit lowering. |
| Assertions | With `import core.test`, exactly `test.expect(condition)` and `test.expect_eq(left, right)` are available, only as standalone statements in the lexical test body or its ordinary nested blocks; they reject in functions, lambdas, constants, and every operand/tail position. `expect` requires exact `bool`. `expect_eq` applies the existing `==` type/admission rule, additionally requires that comparison's result to be exact `bool`, and evaluates left then right exactly once; vector/mask comparisons whose ordinary result is `maskN` reject rather than gaining an assertion-only reduction. Success is Unit and allocates nothing. Failure writes one bounded diagnostic containing canonical test id plus the 1-based call line/column, then returns `Err(Error.Invalid)` from the enclosing test; `expect_eq` does not inspect or format operand values. The first failed assertion ends that test. | Parser keeps these as ordinary qualified calls; sema recognizes the imported test-only builtins and records source identity; checked HIR admits their discriminator only as a complete `Stmt::Expr` in the root or an ordinary nested block; MIR/runtime own the diagnostic and early Err. Positive/negative lexical-context and HIR-placement, scalar/string equality, vector/mask rejection, eager-order, first-failure, and cleanup owners are required. |
| Production commands | `check` and `check-per-unit` parse and type-check test declarations. `fmt` formats them. Every production source consumer—`build`, `run`, `size`, `emit-mir`, `emit-llvm`, `emit-obj`, `explain-opt`, and `db prepare`—validates the complete checked result but selects only its frozen production prefix; test roots, lifted helpers, test-only generic/type/resource monomorphs, interned types, static descriptors, and capabilities remain in the test overlay. `explain-opt` excludes the overlay from located MIR and optimization remarks. `db prepare` excludes every overlay query from production static descriptors and native preparation, while retaining ordinary diagnostics from checking the test. `emit-interface` exports no test name, body, assertion, catalog, descriptor, helper, type suffix, or capability. Commands without an Align source (`cache clear`, `--version`, and `db migrate/status/check/repair`) have no test input. A source with tests still needs an ordinary valid `main` for commands that require one. | The selected prefix/combined view is an explicit compiler input. A parameterized source-command owner covers every listed verb; production/test MIR twins, lifted/monomorph/type-suffix isolation, located-MIR absence, database-descriptor absence, interface absence, unreachable test-only native libraries, and production executable byte identity close the command matrix. |
| Test command and options | `alignc test <entry.align>` checks the explicit closure, builds one host test executable, then runs every catalog entry sequentially. It requires no user `main` and never invokes one. Accepted common options are `--target-cpu`, `--profile`, `--rt-lto`/`--no-rt-lto`, `--cache-stats`, and `-j`/`--jobs`, retaining their existing spelling, placement, and duplicate semantics; test defaults to profile `dev`. `--watch`, `--thin-lto`, PGO flags, `--export`, program arguments, and unknown options reject before build. Each test-only option accepts either `--timeout-ns N` or `--timeout-ns=N`, and `--max-output-bytes N` or `--max-output-bytes=N`, anywhere among compiler arguments; each may occur at most once, and no `--` terminator is introduced. Timeout accepts 1..=900,000,000,000 and defaults to 60,000,000,000 per catalog row, including harness launch and cleanup. Output accepts 0..=16,777,216 and defaults to 1,048,576 independently for stdout and stderr. No environment variable changes these values. After flag removal the remaining argv must be exactly command plus one entry path. Zero discovered tests allocates no artifact, writes exactly `alignc: no tests found` plus LF to stderr, writes nothing to stdout, and exits 1. | CLI/driver own parsing and validation before artifact creation. Exact default/limit/rejected-next, conflicting/valueless/repeated option, no-main, user-main-not-run, and zero-test byte owners are required. |
| Build and artifact | Test lowering combines the immutable production prefix with its validated test overlay, builds one per-unit test-mode object graph plus one compiler-generated harness, and links them once into a private `ArtifactStage` executable. Each catalog root has a compiler-private hidden external symbol so the harness can dispatch by ordinal; neither roots nor overlay helpers are language exports. The temporary executable is never published at the source stem and is removed after the last direct-child reap and before a suite summary. Every selected test launches that same immutable executable; there is no per-test compilation. | Driver/per-unit codegen/harness generation own the artifact. Whole/per-unit semantic parity, production-prefix identity, overlay-closure inclusion, single-link/same-inode reuse, exact ordinal dispatch, hidden linkage, and cleanup-before-summary after success/failure are required. |
| Signal controller | After artifact workers have joined and before any test child is acquired, the driver acquires the one process-global driver signal lease shared with other long-running modes. A second lease fails before native side effects. Setup first reads the current thread mask and requires SIGHUP, SIGINT, SIGQUIT, and SIGTERM to be unblocked; it requires SIGCHLD to have the default disposition without `SA_NOCLDWAIT` and never changes SIGCHLD. It snapshots the four existing graceful dispositions, blocks those signals, creates one nonblocking close-on-exec self-pipe, installs async-signal-safe handlers in SIGHUP/SIGINT/SIGQUIT/SIGTERM order, publishes the lease, then restores the original mask. A handler records only the first signal in one preallocated atomic and writes one byte, retrying EINTR and treating EAGAIN as successful coalescing; it does not allocate, lock, format, close, reap, or touch compiler state. Setup failure attempts every installed-handler restoration in reverse order, closes write then read only after all handlers restore, clears the lease, and restores the mask last. An error path that returns to its caller tears down only after every child and stage is gone: it blocks the four signals, restores dispositions in reverse order, closes write then read, clears the lease, and restores the original mask last. Suite finalization instead transfers the valid controller and stage into a non-returning `FinalExitGuard`. A failed final stage removal retains both through the infrastructure diagnostic attempt and direct exit. After successful removal the guard retains the controller through summary writing, blocks the four signals with fixed valid mask arguments, checks the first-signal atomic once more, and directly exits with the selected signal or suite status; a later signal remains pending only until kernel teardown. A terminal `WriteFailureGuard` and any selected graceful signal likewise retain the controller through cleanup and direct exit, so no terminal path restores a prior ignored/custom disposition before its last write. If setup rollback or a returning teardown restoration fails, the pipe/lease remain valid and direct exit lets kernel teardown remove them; no path returns with a handler targeting a closed/reused descriptor. Existing graceful handlers are temporarily replaced, restored only on a genuine returning path, and never chained. | One parameterized signal owner crosses default/ignored/custom prior dispositions, each initially blocked signal, incompatible SIGCHLD forms, second lease, every setup/rollback/returning-teardown failpoint, pipe-full/EINTR delivery, simultaneous signals, final-stage cleanup failure, terminal summary commit, terminal writer failure, and a signal at every lifecycle state and write boundary. |
| Parent-to-harness launch ABI | Before each spawn the parent creates one `AF_UNIX` `SOCK_DGRAM` socketpair. The child endpoint is the only deliberately inherited non-stdio descriptor and is mapped to fd 3; stdin is `/dev/null`, stdout/stderr are the capture pipes, argv contains only argv[0] equal to the private stage path, and the inherited environment is unchanged. No ordinal or control input travels through argv or environment. The parent samples the monotonic row start immediately before spawn and the timeout includes time spent inside spawn. The spawn setup requests `setpgid(0, 0)` before exec, and the parent must prove `getpgid(child_pid) == child_pid` before sending any launch datagram. If spawn succeeds but group establishment cannot be proved, the harness remains blocked before user code; the parent sends SIGKILL only to the retained direct PID, never to an unverified negative PGID, drains/closes the row descriptors, reaps that child, and reports spawn infrastructure failure. ESRCH from that direct-PID kill is accepted only after non-reaping observation proves the same unreaped child terminal. After verified group establishment the parent sends one exact 16-byte launch datagram: bytes 0..7 `41 4c 54 45 53 54 4c 01`; bytes 8..11 selected u32 ordinal little-endian; bytes 12..15 zero. The harness reads one datagram with a 17-byte receive capacity, rejects a short/long datagram, wrong magic/version, nonzero reserved field, or ordinal outside the linked catalog, marks fd 3 close-on-exec, and sends one exact 16-byte acknowledgement: bytes 0..7 `41 4c 54 45 53 54 41 01`, then the same ordinal and four zero bytes. Only after that acknowledgement may user test code run. Before acknowledgement, deadline expiry, per-stream output excess, descriptor mapping, launch send/read/validation/acknowledgement failure, child exit/signal, an unexpected/repeated datagram, or completion is runner infrastructure failure; the child runs no selected test after invalid launch input. The specialized pre-ack diagnostics are fixed below. | Driver spawn setup and generated harness own separate launch/ack codecs. Semantic-to-byte and byte-to-semantic goldens pin ordinal 7 as `414c544553544c010700000000000000` and `414c5445535441010700000000000000`; a verification-state matrix covers length, magic/version, reserved, ordinal range, order, repetition, group setup/proof failure, deadline/output before versus after acknowledgement, and every syscall/acquisition edge. |
| Process isolation and completion record | Each acknowledged test runs in its verified new process group. Only normal return from the selected test sends one exact 20-byte completion datagram on fd 3: bytes 0..7 `41 4c 54 45 53 54 00 01`; byte 8 outcome (`0` Ok, `1` Err); byte 9 Error tag (`255` for Ok, otherwise `0=NotFound`, `1=Invalid`, `2=Denied`, `3=Timeout`, `4=Code`); bytes 10..11 zero; bytes 12..15 signed i32 Code payload little-endian (zero unless tag 4); bytes 16..19 selected u32 ordinal little-endian. The descriptor then closes. After acknowledgement, datagrams are consumed in arrival order: the first must be the sole completion and is validated field-by-field; the first malformed field freezes that detail. A later exact-length datagram whose bytes 0..7 are the completion magic/version freezes `repetition`, while any acknowledgement or other unexpected control datagram freezes `order`; the earliest sequence/field error wins. No completion after the terminal barrier is the `length` detail. Ok passes only with one completion Ok plus exit 0; Err is a test failure only with one completion Err plus exit 1. Every other completion/exit/signal product fails closed, so `process.exit(0)`, `abort`, `exec`, and a crash cannot impersonate success. | Generated harness and a minimal runtime reporting ABI own the completion record; the parent driver owns exact decoding and datagram cardinality. Independently checked semantic-to-byte and byte-to-semantic golden vectors cover both directions, arrival-order combinations, and every malformed product. |
| Time, output, and child cleanup | Before creating child pipes or spawning, the parent fallibly allocates one fixed raw-byte backing store of exactly the selected bound for each stream; zero allocates none. It never geometrically grows, replaces, or duplicates those stores after spawn. Reads fill the remaining range directly and use one fixed one-byte probe per full stream to detect the rejected next byte, so retained capture payload is exactly twice the selected bound plus two probe bytes and fixed pipe/control state, excluding allocator metadata/rounding, with no old/new-allocation transient. Allocation failure frees any first store and is runner infrastructure failure before user code. The deadline runs from the pre-spawn sample through acknowledgement, user execution, group quiescence, descriptor drain, and direct-child reap. Exact output fit succeeds; the first extra byte before acknowledgement is infrastructure failure and after acknowledgement is a test failure. Every poll wake drains all currently queued control datagrams through EAGAIN. Once `waitid(..., WNOWAIT)` observes the leader terminal, the runner drains control through EAGAIN again before classifying any completion as missing; because normal completion send precedes leader exit, this terminal-observation barrier cannot lose a queued fast-test record. After every successful spawn with verified group establishment and for every terminal path—pre-ack failure, Ok, Err, nonzero exit, signal, timeout, output excess, malformed/missing completion, infrastructure error, or interrupt—the leader PID stays unreaped until the complete pinned process group has been signalled. The unverified-group failure above instead uses direct-PID cleanup because no group target is trusted and no user code has run. Ordinary/test/infrastructure paths send SIGKILL. A graceful SIGHUP/SIGINT/SIGQUIT/SIGTERM first forwards that signal, permits exactly 250 ms, then sends SIGKILL. A release host without non-reaping observation rejects before the first spawn. ESRCH from group kill is success only after non-reaping observation proved the leader terminal; otherwise it is infrastructure failure. The parent then completes the terminal control barrier, drains accepted stream prefixes, closes descriptors, and reaps only the direct child with EINTR retry. Descendants are signalled, not reaped by this runner. Every kill/observe/drain/close/reap step is attempted in fixed order even after an earlier error. The quiesced result retains both capture stores after descriptors close and direct-child reap; it releases them only after complete failure-block reporting or silent pass discard. Any cleanup failure stops the suite after best effort; if an individual failure reason was already selected, its retained bounded block precedes the infrastructure diagnostic. The first graceful signal outranks simultaneous test/infrastructure outcomes, emits no new diagnostic or summary after cleanup, and exits 129/130/131/143 respectively. Only a fully reported/discarded and released ordinary test result permits the next catalog row. | The dedicated driver test-runner component alone owns signal snapshots, allocation, descriptors, deadline, pinned PGID, non-reaping observation, quiesced evidence, and stage cleanup. A Cartesian state/event owner crosses pre/post-ack, completion before/after first EAGAIN and terminal observation, leader running/terminal, descendants absent/present, every result class, four graceful signals, deadline/output at every boundary, each acquisition/cleanup failpoint, evidence retention through reporting, and no-next-row-before-report/release. |
| Reporting and exit | Passing child stdout/stderr is never emitted. A fully passing suite writes exactly `test result: ok. <N> passed; 0 failed` plus LF to runner stdout; explicit `--cache-stats` retains its existing additional cache diagnostics on stderr without exposing child output. For each failure, in catalog order, runner stdout receives `FAIL <canonical-id>` plus LF, one fixed `reason: ...` line, then nonempty captured stdout and stderr under fixed `--- stdout ---` / `--- stderr ---` headers without decoding or rewriting their bytes; a missing final LF is followed by one LF before the next runner line. The reporter consumes the quiesced row, writes directly from its still-owned stores, then releases them. Runner stdout then receives `test result: FAILED. <P> passed; <F> failed` plus LF. After a complete all-pass or failed summary, the controller-owning `FinalExitGuard` performs the terminal signal commit and directly exits 0 or 1. Timeout, output-limit, signal/exit/record mismatch, returned Error, and malformed-record details use the closed formats below; assertion location is the bounded child-stderr line. Successful child output remains suppressed even when another test fails. Zero tests use the exact diagnostic above. Pre-ack timeout/output use the exact infrastructure lines below. Every runner-owned stdout/stderr write uses the no-SIGPIPE fallible writer fixed below. A stdout report/summary write failure retains any written prefix and any incomplete row evidence in the terminal guard, removes the stage, retains any live signal controller, attempts the exact `report write` infrastructure line on stderr, emits no further summary bytes, and exits 1 directly. A stderr diagnostic write failure retains its written prefix and any current row evidence in the terminal guard, removes the stage, retains any live controller, makes no recursive diagnostic attempt, and exits 1 directly. Other build/compiler/catalog diagnostics retain their compiler-owned formats. A graceful interrupt emits no new bytes after selection; complete or partial report/summary bytes already written remain. | CLI reporter owns deterministic bytes, terse-success behavior, quiesced-row consumption, sink failure, and terminal commit. Golden stdout/stderr owners cover zero tests, all-success with/without cache stats, mixed result, arbitrary bytes, missing LF, assertion, every termination/control reason, every pre/post-ack product, graceful interruption during every write boundary and the final block/recheck boundary, stdout/stderr partial/EINTR/EPIPE/zero-write failure, terminal evidence/controller retention, and infrastructure before/after a selected result. |
| Ownership, allocation, and effects | A test declaration owns no runtime value. The frozen production prefix and appended test overlay are compiler-owned immutable data; the test harness catalog and static assertion locations live only in the overlay. Test-mode object/harness allocation is driver-owned. A live row owns two exact pre-spawn capture stores plus child/descriptors; quiescence consumes it into an immutable row that owns the selected outcome and both stores but no child or descriptor. Reporting consumes that row, writes framing and stored ranges directly without copying, and releases both stores only after the complete failure block; passing consumes and releases them without output. A terminal write failure instead transfers any incomplete row and stores into the non-returning guard, which retains them through direct process exit. Test functions and assertions are Impure and never exported. The bounded process-signal lease/self-pipe is the only new process-global state and remains valid through the non-returning final commit; per-write SIGPIPE suppression changes only the runner thread mask, restores it after a complete write, and retains the blocked mask only on the terminal write-failure path through direct process exit. No test registry, reflection table, hidden source scan, persistent test history, retry, concurrency, shuffle, filter, fixture lifecycle, snapshot, coverage, benchmark, or network policy is introduced. | Sema overlay formation, checked-HIR prefix/suffix validation, codegen data ownership, signal-lease RAII, live-to-quiesced typestate, consuming reporter, terminal write/final-exit guards, and runner RAII are owners. Allocation accounting and failpoints pin exact requested live/transient capture bytes and prove no release-before-last-write; production-prefix and capability-link twins prove exclusion/inclusion. |
| Cache and sources of truth | Test mode is a distinct versioned cache domain. Its unit key includes the frozen production-prefix identity, complete local overlay suffixes, ordered test catalog, bodies, assertion locations, test static descriptors, mode version, target/profile/codegen inputs, imported interface hashes, and runtime ABI fingerprint. Its harness key includes the complete ordered canonical-id/symbol catalog plus launch, acknowledgement, completion, and terminal-commit protocol versions. Production interface and codegen keys encode only the frozen production prefix; the existing source-keyed frontend lookup may miss after a test-only edit, but the resulting production HIR tables, descriptors, MIR, object key, link inputs, and executable bytes remain unchanged. Update this file first, then `draft.md`, `docs/language-spec.md`, `docs/design-notes.md`, `docs/open-questions.md`, `docs/impl/02-frontend.md`, `docs/impl/07-roadmap.md`, `docs/impl/19-hir-validation-ledger.md`, the runtime ABI ledger when its row lands, and the synchronized Japanese mirror. | Canonical prefix/overlay key encoders and interface/implementation hashes own invalidation. Tests mutate each named input independently and pin byte-identical production prefixes/artifacts plus changed overlay objects/harness where applicable. |

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

## Checking and lowering

Every compiler command parses tests. Semantic checking installs a test context only while checking
the declaration body. It resolves normal same-module private items and imported public items, and
it permits the two `core.test` assertions only when that import is present. The context does not
flow through a called function or into a lambda. A nested ordinary block, `if`, `match`, loop,
arena, or unsafe block remains part of the same test body.

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

Test functions are Impure. This is conservative and keeps them out of `par_map`, task transfer, and
generic effect promises even when one body happens to contain only arithmetic. The declaration and
overlay are not part of an interface summary. Imported units' test roots and suffixes are compiled
only because the driver explicitly selected the combined test view for those units.

## Runner model

The driver links one immutable test executable. Its generated entry receives one launch datagram on
the fixed compiler-private fd 3, validates the selected catalog ordinal, acknowledges that exact
ordinal, dispatches exactly one test function, sends the completion datagram after that function
returns, and exits. It does not call the user's `main`; therefore a test-only program need not
declare one. The parent launches that same executable once per catalog row, sequentially.

The fd 3 datagram socket is separate from stdout and stderr and carries no application input. The
parent passes no compiler-private argv or environment value: the child observes only its stage path
as argv[0], the inherited environment, `/dev/null` stdin, and captured stdout/stderr. The harness
sets fd 3 close-on-exec before acknowledging launch, so a successful `process.exec` cannot inherit
the authority to report completion. A valid acknowledgement distinguishes harness setup from user
execution; only a normal test return is the safe completion producer. Missing completion bytes make
even exit zero fail, closing the `process.exit(0)` bypass. Unsafe code can violate the descriptor
boundary, as it can every safe compiler invariant; malformed or self-inconsistent datagrams still
fail closed.

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

Encoding and decoding are separate implementations checked against these vectors. A decoder reads
the fixed envelope in byte order, validates reserved and conditional fields, then compares the
selected ordinal and process status. It never transmutes an untrusted byte array into a native
record.

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
then entry path. Repeating either test-specific option is an error rather than last-wins. A bad
catalog is reported after checking and before artifact allocation.

One dedicated test-runner component owns the complete state machine; no CLI branch, harness codec,
or reporter may independently wait, signal, reap, or advance the catalog. Its states are:

| State | Ordered observable events | Required transition and invariant |
| --- | --- | --- |
| Ready/acquire | first graceful signal; capture allocation; stdout/stderr pipe; control socket; pre-spawn clock sample; spawn; process-group proof | A signal removes the stage and exits conventionally. Failure before spawn is infrastructure failure with no child. Successful spawn plus `getpgid(child_pid) == child_pid` proof retains the leader PID and enters AwaitAck. Failed proof sends SIGKILL only to that PID, drains/closes the row, reaps it, and stops with spawn infrastructure failure without sending launch or running user code. |
| AwaitAck | first graceful signal; control drain through EAGAIN; stdout; stderr; non-reaping leader status; terminal control barrier; deadline | Control drains before streams. A valid acknowledgement enters Running immediately, and the same drain continues under Running rules so queued acknowledgement + completion is consumed in one wake. A pre-ack malformed/order error, deadline, or first excess byte is launch infrastructure. Leader terminal status is recorded without reaping or declaring completion missing; it enters Quiescing, whose post-terminal drain is the required barrier. |
| Running | first graceful signal; stdout; stderr; control drain through EAGAIN; non-reaping leader status; terminal control barrier; deadline | Stdout excess wins over stderr; either wins over completion/timeout. Every queued datagram is consumed in arrival order, so acknowledgement/completion coalescing and repetition are recorded before status classification. Completion without status remains pending. Terminal status enters Quiescing without reaping or selecting a missing-record reason. After its barrier, a complete valid completion/status product observed before the same wake's deadline wins over that deadline; otherwise the closed precedence below applies. |
| Quiescing | selected graceful signal or mandatory SIGKILL; non-reaping terminal observation; final control drain through EAGAIN; remaining stdout/stderr; deadline; descriptor close; direct-child reap; cleanup errors | The pinned group is signalled before direct-child reap. The runner obtains or retains terminal `WNOWAIT` status, drains control again through EAGAIN, drains accepted stdout before stderr, closes every row descriptor, then reaps only the direct child. The second control drain is mandatory even if the first drain returned EAGAIN immediately before terminal observation. A pre-ack deadline remains launch infrastructure; a post-ack deadline remains test timeout. Graceful signal remains highest precedence. Otherwise cleanup failure becomes infrastructure while preserving the selected outcome and stores. Successful cleanup produces one immutable quiesced row with no child/descriptor and with both stores still owned. |
| Reporting | first graceful signal; pass discard or failure-block write progress; report-write failure; store release | A pass emits no child bytes. A failure writes framing and retained ranges directly from the quiesced row. Complete reporting or pass discard releases both stores and enters Between rows. A graceful signal releases the stores, removes the stage, retains the controller, and exits directly with no new line. Report failure transfers the row and stores into the non-returning writer guard, removes the stage while retaining the controller, attempts the stderr infrastructure line, and exits 1 directly without advancing, releasing incomplete evidence, or ordinary controller teardown. |
| Between rows | first graceful signal; next-row acquisition | There is no live/waitable direct child, open row descriptor, or capture store. Only a reported/discarded ordinary result advances the catalog. Infrastructure stops; graceful signal removes the stage and exits. |
| Finalize | first graceful signal; guard-owned stage removal; controller-owned summary write; terminal graceful-mask block and atomic recheck; direct exit | `FinalExitGuard` owns the stage and controller before removal. Removal precedes the summary; failure retains both through the `stage cleanup` infrastructure-line attempt and direct exit 1 with no summary. After successful removal the valid controller stays installed and the no-SIGPIPE writer checks its atomic at every summary boundary. A selected signal before the first byte emits none; selection during a write retains the complete/partial prefix and emits no more. After a complete summary, the guard blocks the four signals with the platform-probed fixed mask operation, rechecks the atomic, and directly exits with the signal status if selected or the suite status otherwise. Signals arriving after that block remain pending until kernel teardown. There is no ordinary controller teardown or return after summary publication. |

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
is one of `stage create`, `stage cleanup`, `capture allocation`, `signal handler`, `pipe`, `control
socket`, `control write`, `control read`, `launch`, `spawn`, `stdout read`, `stderr read`, `close`,
`wait`, `kill`, `reap`, `report write`, or `diagnostic write`; the numeric value is the raw OS code,
or zero for allocation, validation, zero-byte write, or a platform failure that exposes no code. It
emits no final suite summary, even after earlier test outcomes; any already emitted or
just-selected failure-block prefix remains. A `diagnostic write` failure cannot describe itself on
the failed sink: its partial stderr prefix and any incomplete row evidence are retained, no
recursive line is attempted, stage cleanup still runs, any live controller remains valid, and
direct exit is 1. Compiler diagnostics and link errors retain their existing formats and also emit
no suite summary. Artifact-stage removal occurs before the all-pass or failed-suite summary, so
cleanup failure cannot follow a published complete summary.

The reporter's private `write_no_sigpipe(fd, bytes)` primitive never allocates or buffers. On the
runner thread it snapshots the original signal mask, blocks SIGPIPE, then loops raw `write` over the
remaining range. Every positive short or complete write checks the graceful-signal atomic before
the caller may advance; EINTR checks it first and otherwise retries. A complete write with no
selected graceful signal restores the original mask before success. Zero or any other error
returns a terminal `WriteFailureGuard` with its raw code and keeps SIGPIPE blocked; there is no
ordinary-return path from that guard. The guard absorbs and retains any incomplete row evidence;
its caller retains the partial prefix, removes the stage but deliberately retains any live signal
controller, attempts the diagnostic with SIGPIPE still blocked, then exits the process directly so
kernel teardown discards the evidence, controller, and any generated or pre-existing pending
SIGPIPE. Fixed valid mask arguments make writer restoration and the `FinalExitGuard` graceful-mask
block infallible on the release hosts; the platform owner probes that premise before artifact
allocation. A graceful signal observed at any write boundary or at the final blocked recheck
outranks report, diagnostic, or suite status. Parameterized sink owners cross original blocked/unblocked and
pre-pending SIGPIPE, full/short/zero writes, EPIPE/ENOSPC, EINTR with and without a graceful signal,
every byte boundary, stdout report/summary, stderr diagnostic, pre/post-final-block signal,
terminal controller retention, both guard non-return paths, and diagnostic failure without
recursion.

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
source-keyed frontend lookup; after checking, it must still reproduce byte-identical production HIR
tables, descriptors, MIR, object key, link inputs, and executable. Test-mode keys encode that prefix
identity plus every overlay suffix, local body, ordinal, canonical id, assertion location, static
descriptor, and mode version. The harness key covers the complete ordered catalog, symbol mapping,
all three control protocols, and terminal-commit version. An import-order change may therefore
change the harness order without changing a public module interface.

The executable stays below an `ArtifactStage` retained by the runner through the final child reap.
No source-adjacent binary, catalog file, history, snapshot, or machine-readable public test artifact
is produced.

## Implementation closure matrix

| Axis | Required implementation closure | Acceptance owner |
| --- | --- | --- |
| Syntax and formatting | `test` + string contextual lookahead, `test {}` / `pub test {}` type twins, rejected `pub test` + string, attribute/signature near-shapes, newline/braces, recovery, depth cap, format idempotence | parameterized lexer/parser/AST/formatter owner |
| Catalog and modules | independent HIR module/name/id correlation, declared/default entry path, entry/imported `main` twin, name/id/count bounds, duplicates, same name across modules, dependency-first diamond order, unimported exclusion, private/public access | whole/per-unit catalog golden, malformed-HIR identity matrix, and sema owner |
| Body and control | implicit Ok, explicit Err, `?`, `match`, `else`, assertion early exit, every cleanup-bearing control join, malformed HIR | sema + checked-HIR + MIR control/Drop owner |
| Assertion surface | import rule, lexical contexts, statement-only placement, bool equality families, vector/mask non-Bool rejection, left-to-right once, line/column, first failure | parameterized positive/negative sema owner plus MIR/runtime diagnostic golden |
| Checked artifact partition | production fixed point frozen before tests; catalog root/back-reference; overlay closure equality; test lifted and generic helper; shared production/test monomorph reuse; suffixes for every nominal/interned type class; test static descriptor and capability; no prefix mutation/reference to suffix | malformed prefix/overlay HIR matrix, whole/per-unit production-byte twin, generated-closure and descriptor/capability owners |
| Production isolation | every source command validates the full result but selects the frozen prefix; located MIR/remarks and `db prepare` descriptors omit overlay; no overlay capability/link/export/interface/cache influence; ordinary main unchanged | parameterized command matrix, production/test view twin, database descriptor owner, and byte-identical HIR/MIR/object/link/executable owner |
| Test artifact | no-main test program, user main not called, one link, one immutable inode, hidden symbols, exact ordinal dispatch | driver artifact and whole/per-unit execution owner |
| Launch protocol | fd 3 datagram mapping, argv/environment/stdin shape, pre-exec group setup/proof, launch/ack codecs and goldens, malformed/order/ordinal products, pre/post-ack exit distinction | driver/harness protocol matrix and acquisition failpoints |
| Completion protocol | two independent codecs, three semantic goldens, every malformed field, order/repetition/cardinality, wrong ordinal, Ack+completion coalescing, completion before/after EAGAIN and terminal observation, exit/record Cartesian product, exit/exec/abort/crash bypass | runtime/driver protocol matrix |
| Signal controller | shared lease, prior mask/dispositions, SIGCHLD compatibility, setup/rollback/returning teardown, controller retained through summary, terminal mask/recheck/direct exit, HUP/INT/QUIT/TERM ordering, first-signal coalescing, signal at every runner state/write boundary | parameterized process-global signal and final-commit owner |
| Runner state machine | Ready/AwaitAck/Running/Quiescing/Reporting/Between/Finalize x every event, pre/post-ack deadline/output, drain-EAGAIN/terminal/drain barrier, all-outcome group signalling before direct reap, quiesced evidence through report/release, controller-owned summary through terminal commit | deterministic Cartesian state/event, typestate, barrier, and final-commit owner |
| Child lifecycle | exact preallocation/failures, spawn/pipe/control acquisition failures, unverified-group direct-PID cleanup, concurrent drains, exact/rejected-next bounds, descendants absent/present for every verified-group terminal result, WNOWAIT pin, group-kill-before-reap, terminal control barrier, cleanup-error override with evidence preservation, interrupted wait/read | deterministic allocation/failpoint and process-tree owner |
| Reporting | exact zero-test and pre-ack infrastructure bytes, all pass one line with/without cache stats, mixed failure, returned Error variant, assertion, exit/signal/timeout/output/order/repetition/malformed reason, four graceful signals, raw bytes/missing LF, capture lifetime through last byte or terminal exit, SIGPIPE-safe full/short/zero/EINTR/error writes, partial-prefix, terminal evidence/controller retention, pre/post-final-block signal, and nonrecursive diagnostic failure | exact CLI stdout/stderr, consuming-row, sink-failpoint, and final-exit owner |
| Cache identity | independent mutations of every production-prefix and overlay/harness key field, production frontend miss with byte-identical prefix/object hit, test descriptor/capability inclusion | cache hit/miss, prefix/suffix, and canonical-key owner |

## Capability boundary and deferrals

The implementation boundary was re-opened around the complete test-generated artifact closure and
terminal finalization. The accepted code has two strict top-level proof domains: compiler formation
owns syntax through the hidden harness and exact control codecs; one dedicated driver test-runner
component consumes only an immutable artifact, ordered catalog, and validated limits and
exclusively owns signal state, spawn/poll, deadline, process groups, capture, reap, and reporting.
Neither side may reproduce the other's validation or lifecycle transition.

Compiler formation itself has a typed seam: `CheckedProgram.production` reaches its complete
ordinary-source fixed point and freezes before `TestOverlay` formation. The overlay may read and
reuse prefix identities but owns every newly generated test root, lifted/monomorph function,
nominal/interned type suffix, static descriptor, and capability consequence. Production consumers
can obtain only the prefix view; test consumers obtain the validated combined view. Prefix/suffix
bounds and catalog reachability make it impossible to omit a test helper by checking only the root
tag or to perturb production ids while discarding a body later.

Inside the runner, `LiveRow` owns child, descriptors, stores, protocol state, and deadline.
Group signalling, non-reaping terminal observation, the required second control drain, descriptor
closure, and direct-child reap consume it into `QuiescedRow`, which owns only immutable outcome and
both capture stores. The reporter is the sole consumer of `QuiescedRow`; complete failure-block
write or silent pass discard consumes and releases the stores before catalog advance. A terminal
writer failure consumes `Reporting` into a non-returning guard that retains any incomplete row and
stores through direct exit. There is no API that releases stores while retaining a reportable row,
returns from that terminal guard, or classifies missing completion before the terminal barrier.
Fake-stage protocol/process-tree and sink-failpoint owners validate this runner without compiling
Align source, while whole/per-unit owners validate the producer against the same codec boundary.

`FinalExitGuard` first consumes the artifact stage and signal controller. A failed stage removal
keeps both owned through the terminal diagnostic/direct-exit path. After successful removal it
retains the controller through the last summary write, blocks the four graceful signals, rechecks
the first-signal atomic, and directly exits. It has no ordinary teardown/return edge. This terminal
commit prevents restored ignored/custom handlers from changing summary behavior and prevents a
fallible controller cleanup from following a published successful summary.

These domains still land as one public capability after this design PR. Parser-to-harness without a
runner and a runner without compiler-produced private symbols are both dormant, so splitting them
would publish no useful stable consumer and would duplicate mode/cache/ABI integration proof. The
expected hand-written diff may exceed 1,000 lines for that reason. The explicit internal boundary,
state/event matrix, and independent owners reduce integration risk without shipping an unusable
prefix.

The first capability deliberately excludes test filtering/listing, parallel or shuffled execution,
retry, fixtures, setup/teardown hooks, snapshots, coverage, benchmarks, ignored tests, expected
failure, persistent history, hidden file discovery, and an assertion formatting/reflection system.
Each is additive only after a real consumer demonstrates that the sequential error-model core is
insufficient.
