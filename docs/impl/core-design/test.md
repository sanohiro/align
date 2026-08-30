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
| Declaration grammar | One private top-level declaration: `test` followed by one ordinary Align string token and one block. `test` is contextual only in this item shape. `pub`, parameters, type parameters, a written return type, expression-body `=`, attributes, and a trailing declaration name are rejected. The declaration creates no callable or name binding. | Lexer keeps `test` as an identifier; AST/parser/formatter own the contextual item. Parser/formatter round-trip, recovery, and every rejected near-shape are one parameterized owner. |
| Name, identity, and catalog | The decoded name is 1..=256 UTF-8 bytes and contains no U+0000..U+001F or U+007F. Duplicate names in one module reject. The canonical public id is `<canonical-module-path>::<decoded-name>`; the entry module path is `main`, and the complete id must fit 1..=1,024 UTF-8 bytes. At most 65,535 tests exist in the explicit entry/import closure. Catalog order is the existing dependency-first DFS unit order (direct imports in source order), then source declaration order in each unit. Only that explicit closure is searched; no directory, filename, annotation, or manifest discovery is added. | Sema owns validation and canonical ids; the driver owns the ordered catalog. Exact-name/control/id/count limits, duplicate scope, diamond-import order, and ignored unimported files are catalog owners. |
| Body type and control | A test is checked as a compiler-private zero-parameter `fn() -> Result<(), core.Error>`. After its written block completes with Unit, the construct supplies one documented `Ok(())` tail. `?`, `return Err(...)`, `match`, `else`, arenas, Drop, and every ordinary control form retain their existing semantics. A written non-Unit tail or `return` of any other type rejects. Normal Ok/Err and assertion exits run ordinary function cleanup; hard errors, `process.abort`, and successful `process.exec` retain their existing no-unwind behavior. | Sema/HIR/MIR own one flagged private test function and its implicit tail; no public interface entry is emitted. Control-flow, cleanup, Error-variant, and malformed checked-HIR owners cover direct and per-unit lowering. |
| Assertions | With `import core.test`, exactly `test.expect(condition)` and `test.expect_eq(left, right)` are available, only as standalone statements in the lexical test body or its ordinary nested blocks; they reject in functions, lambdas, constants, and every operand/tail position. `expect` requires exact `bool`. `expect_eq` applies the existing `==` type/admission rule, additionally requires that comparison's result to be exact `bool`, and evaluates left then right exactly once; vector/mask comparisons whose ordinary result is `maskN` reject rather than gaining an assertion-only reduction. Success is Unit and allocates nothing. Failure writes one bounded diagnostic containing canonical test id plus the 1-based call line/column, then returns `Err(Error.Invalid)` from the enclosing test; `expect_eq` does not inspect or format operand values. The first failed assertion ends that test. | Parser keeps these as ordinary qualified calls; sema recognizes the imported test-only builtins and records source identity; MIR/runtime own the diagnostic and early Err. Positive/negative lexical-context, scalar/string equality, vector/mask rejection, eager-order, first-failure, and cleanup owners are required. |
| Production commands | `check` and `check-per-unit` parse and type-check test declarations. `fmt` formats them. Every production source consumer—`build`, `run`, `size`, `emit-mir`, `emit-llvm`, `emit-obj`, `explain-opt`, and `db prepare`—type-checks tests but does not lower, link, report on, or make reachable test bodies or the harness. `explain-opt` excludes tests from located MIR and optimization remarks. `db prepare` excludes every test-body query from `Checked.static_descriptors` and native preparation, while retaining ordinary diagnostics from checking the test. Test-only capabilities cannot affect a production/native link. `emit-interface` exports no test name, body, assertion, catalog, descriptor, or capability. Commands without an Align source (`cache clear`, `--version`, and `db migrate/status/check/repair`) have no test input. A source with tests still needs an ordinary valid `main` for commands that require one. | The mode is an explicit compiler input. A parameterized source-command owner covers every listed verb; production/test MIR twins, located-MIR absence, database-descriptor absence, interface absence, unreachable test-only native libraries, and production executable byte identity close the command matrix. |
| Test command and options | `alignc test <entry.align>` checks the explicit closure, builds one host test executable, then runs every catalog entry sequentially. It requires no user `main` and never invokes one. Accepted common options are `--target-cpu`, `--profile`, `--rt-lto`/`--no-rt-lto`, `--cache-stats`, and `-j`/`--jobs`, retaining their existing spelling, placement, and duplicate semantics; test defaults to profile `dev`. `--watch`, `--thin-lto`, PGO flags, `--export`, program arguments, and unknown options reject before build. Each test-only option accepts either `--timeout-ns N` or `--timeout-ns=N`, and `--max-output-bytes N` or `--max-output-bytes=N`, anywhere among compiler arguments; each may occur at most once, and no `--` terminator is introduced. Timeout accepts 1..=900,000,000,000 and defaults to 60,000,000,000 per test. Output accepts 0..=16,777,216 and defaults to 1,048,576 independently for stdout and stderr. No environment variable changes these values. After flag removal the remaining argv must be exactly command plus one entry path. Zero discovered tests is an error and runs nothing. | CLI/driver own parsing and validation before artifact creation. Exact default/limit/rejected-next, conflicting/valueless/repeated option, no-main, user-main-not-run, and zero-test owners are required. |
| Build and artifact | One per-unit test-mode object graph plus one compiler-generated harness is linked once into a private `ArtifactStage` executable. Each test function has a compiler-private hidden external symbol so the harness can dispatch by catalog ordinal; it is not a language export. The temporary executable is never published at the source stem and is removed after the last direct-child reap and before a suite summary. Every selected test launches that same immutable executable; there is no per-test compilation. | Driver/per-unit codegen/harness generation own the artifact. Whole/per-unit semantic parity, single-link/same-inode reuse, exact ordinal dispatch, hidden linkage, and cleanup-before-summary after success/failure are required. |
| Parent-to-harness launch ABI | Before each spawn the parent creates one `AF_UNIX` `SOCK_DGRAM` socketpair. The child endpoint is the only deliberately inherited non-stdio descriptor and is mapped to fd 3; stdin is `/dev/null`, stdout/stderr are the capture pipes, argv contains only argv[0] equal to the private stage path, and the inherited environment is unchanged. No ordinal or control input travels through argv or environment. The parent sends one exact 16-byte launch datagram: bytes 0..7 `41 4c 54 45 53 54 4c 01`; bytes 8..11 selected u32 ordinal little-endian; bytes 12..15 zero. The harness reads one datagram with a 17-byte receive capacity, rejects a short/long datagram, wrong magic/version, nonzero reserved field, or ordinal outside the linked catalog, marks fd 3 close-on-exec, and sends one exact 16-byte acknowledgement: bytes 0..7 `41 4c 54 45 53 54 41 01`, then the same ordinal and four zero bytes. Only after that acknowledgement may user test code run. Descriptor mapping, launch send/read/validation/acknowledgement failure, child exit or signal before a valid acknowledgement, an unexpected/repeated datagram, or completion before acknowledgement is runner infrastructure failure; the child runs no selected test after invalid launch input. | Driver spawn setup and generated harness own separate launch/ack codecs. Semantic-to-byte and byte-to-semantic goldens pin ordinal 7 as `414c544553544c010700000000000000` and `414c5445535441010700000000000000`; a malformed matrix covers length, magic/version, reserved, ordinal range, order, repetition, and every syscall/acquisition edge. |
| Process isolation and completion record | Each acknowledged test runs in its new process group. Only normal return from the selected test sends one exact 20-byte completion datagram on fd 3: bytes 0..7 `41 4c 54 45 53 54 00 01`; byte 8 outcome (`0` Ok, `1` Err); byte 9 Error tag (`255` for Ok, otherwise `0=NotFound`, `1=Invalid`, `2=Denied`, `3=Timeout`, `4=Code`); bytes 10..11 zero; bytes 12..15 signed i32 Code payload little-endian (zero unless tag 4); bytes 16..19 selected u32 ordinal little-endian. The descriptor then closes. Short, long, reserved-nonzero, unknown outcome/tag, inconsistent tag/code, repeated, out-of-order, or wrong-ordinal datagrams fail the test after a valid acknowledgement. Ok passes only with completion Ok plus exit 0; Err is a test failure only with completion Err plus exit 1. Every other completion/exit/signal product fails closed, so `process.exit(0)`, `abort`, `exec`, and a crash cannot impersonate success. | Generated harness and a minimal runtime reporting ABI own the completion record; the parent driver owns exact decoding. Independently checked semantic-to-byte and byte-to-semantic golden vectors cover both directions and every malformed product. |
| Time, output, and child cleanup | Before creating child pipes or spawning, the parent fallibly allocates one fixed raw-byte backing store of exactly the selected bound for each stream; zero allocates none. It never geometrically grows, replaces, or duplicates those stores after spawn. Reads fill the remaining range directly and use one fixed one-byte probe per full stream to detect the rejected next byte, so retained capture payload is exactly twice the selected bound plus two probe bytes and fixed pipe/control state, excluding allocator metadata/rounding, with no old/new-allocation transient. Allocation failure frees any first store and is runner infrastructure failure before user code. The parent drains both streams concurrently. Exact fit succeeds; the first extra byte fails the test. Timeout or output excess sends SIGKILL to the complete child process group, drains accepted prefixes, and directly reaps only the selected child. Descendants remaining in that group are signalled, not reaped by this runner. Every kill/close/drain/wait/reap/control cleanup step is attempted in the fixed order even after an earlier error. Any acquisition or cleanup failure is infrastructure failure, overrides permission to continue, and stops the suite after best-effort group signalling, direct-child reap, descriptor closure, buffer release, and stage removal; if an individual failure reason was already selected, its bounded failure block is emitted before the infrastructure diagnostic. A caught SIGINT/SIGTERM follows the same cleanup then exits with the conventional interrupted status. Only an individual Err, assertion, nonzero exit, signal, timeout, output excess, or malformed completion after successful cleanup permits the next catalog row. | Driver process lifecycle owns allocation, descriptors, deadline, process group, signal forwarding, and stage cleanup. Barrier/failpoint owners cover exact requested allocation and failure at each store, every acquisition/cleanup edge, both pipe orders, exact/rejected-next bytes, timeout, descendants, interrupted wait/read, evidence preservation, and restoration order. |
| Reporting and exit | Passing child stdout/stderr is never emitted. A fully passing suite writes exactly `test result: ok. <N> passed; 0 failed` plus LF to runner stdout and exits 0. For each failure, in catalog order, runner stdout receives `FAIL <canonical-id>` plus LF, one fixed `reason: ...` line, then nonempty captured stdout and stderr under fixed `--- stdout ---` / `--- stderr ---` headers without decoding or rewriting their bytes; a missing final LF is followed by one LF before the next runner line. Runner stdout then receives `test result: FAILED. <P> passed; <F> failed` plus LF and the runner exits 1. Timeout, output-limit, signal/exit/record mismatch, returned Error, and malformed-record details use the closed formats below; assertion location is the bounded child-stderr line. Successful child output remains suppressed even when another test fails. Build/catalog/infrastructure errors use the closed stderr `alignc:` format below, exit 1, and never print a complete-suite summary. | CLI reporter owns deterministic bytes and terse-success behavior. Golden stdout/stderr owners cover all-success, mixed result, arbitrary bytes, missing LF, assertion, every termination reason, and infrastructure-before/after-first-test products. |
| Ownership, allocation, and effects | A test declaration owns no runtime value. The test harness catalog and static assertion locations are compiler-owned immutable data. Test-mode object/harness allocation is driver-owned. The parent owns the two exact pre-spawn capture stores, never copies captured bytes for reporting, writes framing and stored ranges directly, and releases both stores after the row. Test functions and assertions are Impure and never exported. No process-global registration, reflection table, hidden source scan, persistent test history, retry, concurrency, shuffle, filter, fixture lifecycle, snapshot, coverage, benchmark, or network policy is introduced. | Sema effect inference, codegen data ownership, and driver RAII are owners. Allocation accounting and failpoints pin exact requested live/transient capture bytes; capability-link twins prove production exclusion and test inclusion. |
| Cache and sources of truth | Test mode is a distinct versioned cache domain. Its unit key includes the complete local ordered test catalog, bodies, assertion locations, mode version, target/profile/codegen inputs, imported interface hashes, and runtime ABI fingerprint. Its harness key includes the complete ordered canonical-id/symbol catalog plus launch, acknowledgement, and completion protocol versions. Production interface and codegen keys exclude test catalog/body semantics; the existing source-keyed frontend lookup may miss after a test-only edit, but the resulting production MIR/object key and executable bytes remain unchanged. Update this file first, then `draft.md`, `docs/language-spec.md`, `docs/design-notes.md`, `docs/open-questions.md`, `docs/impl/02-frontend.md`, `docs/impl/07-roadmap.md`, `docs/impl/19-hir-validation-ledger.md`, the runtime ABI ledger when its row lands, and the synchronized Japanese mirror. | Canonical key encoders and interface/implementation hashes own invalidation. Tests mutate each named input independently and pin unchanged production artifacts plus changed test objects/harness where applicable. |

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

The checked record carries the canonical id, source ordinal, body, and one test discriminator. A
production lowering validates that record and deliberately omits its function. Test lowering emits
the private function with the exact `Result<Unit, Error>` ABI and emits assertion locations as
immutable data. A malformed discriminator, name, ordinal, body result, assertion context, or
implicit-tail shape rejects before MIR construction. The validator must not infer test status from
the generated symbol spelling.

Test functions are Impure. This is conservative and keeps them out of `par_map`, task transfer, and
generic effect promises even when one body happens to contain only arithmetic. The declaration is
not part of an interface summary. Imported units' test functions are compiled only because the
driver explicitly selected test mode for those units.

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

During a child run, one parent event loop owns stdout, stderr, the control socket, process status, and
the monotonic deadline. At one wake it drains ready stdout before stderr, then control bytes, then
observes process status, then the deadline. The first observable stdout excess wins over stderr
excess; either excess wins over a simultaneously observable completion or timeout. A complete
record/status product wins over a deadline observed in that same wake. This order is deterministic
for every multi-ready product and prevents retry or scheduler-dependent error rewriting.

After termination the parent applies this result order only if every required cleanup operation
succeeds:

1. a previously selected output or timeout result;
2. completion-record length, magic/version, outcome, tag, reserved, conditional code, then ordinal;
3. record/process-status correlation;
4. returned Error or Ok.

Timeout and output excess kill the complete child process group with SIGKILL, drain the already
accepted prefixes, and reap the direct child. They never publish a completion success. Descendants
are signalled while they remain in the group but cannot be reaped by this parent. A runner interrupt
signals the group and reaps the direct child before removing the private artifact stage.

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

The runner retains bounded output while a test is live but emits none of it after success. This is
the same failure-only evidence policy used by repository validation: large passing suites have
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

`<record-detail>` is exactly one of `length`, `magic/version`, `outcome`, `error tag`,
`reserved bytes`, `error code`, or `ordinal`, selected in record-validation order. Decimal numbers
have no leading plus sign or zero padding. An output excess detected in both streams selects stdout
by the event-loop order. A valid Err record plus exit 1 selects its returned-Error line, including
an assertion's `Error.Invalid`; the assertion location remains in the replayed stderr bytes.

A runner infrastructure abort writes exactly
`alignc: test runner <operation> failed (os error <signed-i32>)` plus LF to stderr, where operation
is one of `stage create`, `stage cleanup`, `capture allocation`, `signal handler`, `pipe`, `control
socket`, `control write`, `control read`, `launch`, `spawn`, `stdout read`, `stderr read`, `close`,
`wait`, `kill`, or `reap`; the numeric value is the raw OS code, or zero for allocation, validation,
or a platform failure that exposes no code. It emits no final suite summary, even after earlier test
outcomes; any already emitted or just-selected failure block remains. Compiler diagnostics and link
errors retain their existing formats and also emit no suite summary. Artifact-stage removal occurs
before the all-pass or failed-suite summary, so cleanup failure cannot follow a published complete
summary.

## Cache and artifact identity

Production and test compilation share parsing and semantic rules but use disjoint codegen domains.
A test-only edit can invalidate the existing source-keyed frontend lookup; after checking, it must
still reproduce the identical production MIR, object key, link inputs, and executable bytes. In
test mode the local body, ordinal, canonical id, assertion locations, and mode version all enter the
owning unit key. The harness key covers the complete ordered catalog, symbol mapping, and all three
control-protocol versions. An import-order change may therefore change the harness order without
changing a public module interface.

The executable stays below an `ArtifactStage` retained by the runner through the final child reap.
No source-adjacent binary, catalog file, history, snapshot, or machine-readable public test artifact
is produced.

## Implementation closure matrix

| Axis | Required implementation closure | Acceptance owner |
| --- | --- | --- |
| Syntax and formatting | contextual item, `pub`/attribute/signature near-shapes, newline/braces, recovery, depth cap, format idempotence | parameterized lexer/parser/AST/formatter owner |
| Catalog and modules | name/id/count bounds, duplicates, same name across modules, dependency-first diamond order, unimported exclusion, private/public access | whole/per-unit catalog golden and sema owner |
| Body and control | implicit Ok, explicit Err, `?`, `match`, `else`, assertion early exit, every cleanup-bearing control join, malformed HIR | sema + checked-HIR + MIR control/Drop owner |
| Assertion surface | import rule, lexical contexts, statement-only placement, bool equality families, vector/mask non-Bool rejection, left-to-right once, line/column, first failure | parameterized positive/negative sema owner plus MIR/runtime diagnostic golden |
| Production isolation | every source command checks but omits tests; located MIR/remarks and `db prepare` static descriptors omit them; no test capability/link/export/interface/cache influence; ordinary main unchanged | parameterized command matrix, production/test twin, database descriptor owner, and byte-identical executable owner |
| Test artifact | no-main test program, user main not called, one link, one immutable inode, hidden symbols, exact ordinal dispatch | driver artifact and whole/per-unit execution owner |
| Launch protocol | fd 3 datagram mapping, argv/environment/stdin shape, launch/ack codecs and goldens, malformed/order/ordinal products, pre/post-ack exit distinction | driver/harness protocol matrix and acquisition failpoints |
| Completion protocol | two independent codecs, three semantic goldens, every malformed field, wrong ordinal, exit/record Cartesian product, exit/exec/abort/crash bypass | runtime/driver protocol matrix |
| Child lifecycle | exact preallocation/failures, spawn/pipe/group/control acquisition failures, concurrent drains, exact/rejected-next bounds, timeout, descendant signalling/direct reap, cleanup-error override with evidence preservation, wait/read interruption, SIGINT/SIGTERM cleanup | deterministic allocation/failpoint and barrier owner |
| Reporting | zero tests, all pass one line, mixed failures, returned Error variants, assertion, exit/signal/timeout/output/malformed reasons, raw bytes and missing LF | exact CLI stdout/stderr golden |
| Cache identity | independent mutations of every unit/harness key field, production frontend miss with object hit, test-only capability inclusion | cache hit/miss and canonical-key owner |

## Capability boundary and deferrals

The accepted implementation is one code capability after this design PR. Parser, semantic test
context, test-mode MIR, hidden per-unit symbols, harness record, and the parent runner form one
strict producer-to-consumer chain; no prefix is useful to an Align program, while splitting it
would duplicate mode/cache/ABI proof. The expected hand-written diff may exceed 1,000 lines for
that reason. The closure matrix and parameterized owners keep the larger boundary lower risk than
shipping dormant intermediate machinery.

The first capability deliberately excludes test filtering/listing, parallel or shuffled execution,
retry, fixtures, setup/teardown hooks, snapshots, coverage, benchmarks, ignored tests, expected
failure, persistent history, hidden file discovery, and an assertion formatting/reflection system.
Each is additive only after a real consumer demonstrates that the sequential error-model core is
insufficient.
