# Session handoff

Current continuity note for a fresh Claude Code or Codex session. Keep this file
about the present state, the next decision, and operational facts. The former
per-PR journals are preserved in
[`docs/archive/HANDOFF-2026-08-13.md`](docs/archive/HANDOFF-2026-08-13.md) and
[`docs/archive/HANDOFF-2026-07-25.md`](docs/archive/HANDOFF-2026-07-25.md);
neither is a source of current status.

_Last updated: 2026-08-15._ Align main is at `956d943d`. align-llm Request 5's bounded HTTP response
body is shipped, and Request 7's evidence design (#813), benchmark inputs (#815/#821), installed
manifest (#816), canonical JSON (#817), typed report schema (#818), SSHSIG framing (#820), strict
controller CLI (#822), installed-manifest/profile binding (#823), raw Git object identity codec
(#825), pinned Git batch response codec (#828), pinned Git process boundary (#830), and two-sided
Git revision/tree binding (#831) are merged.
The align-repl user guide and release artifacts are shipped in #826/#829. The final requested work
is PR #827 on
`agent/request7-prepared-benchmarks`: both protected JSON benchmarks gain the reviewed two-phase
`prepare native` / direct `native` interface, canonical artifact sealing, checked integer inner
medians, descriptor-bound Linux evidence execution, and inode-aware cleanup. The integrated review
of `845f0b90` against `863c20ba` found two valid P1 handoff races: retained-root replacement and
same-inode writes after hashing. The consolidated repair carries the captured root device/inode into
the launcher, verifies the manifest below that retained descriptor, and executes/preloads Linux
bytes only after copying them into fully write-sealed anonymous memfds; macOS remains native ARM
development qualification, not accepted evidence. Its focused owners are
`scripts/test-benchmark-input.sh`, `scripts/test-benchmark-evidence-manifest.sh`, and
`scripts/test-benchmark-evidence-statistics.sh`.

The final integration review of `0dbbd709` against `c47e57c7` found three valid remaining handoff
issues. The consolidated closure requires the trusted caller to retain prepare's manifest SHA-256
and supply it to every native invocation, keeps accepted Linux preparation writes below a retained
private-child descriptor, carries that descriptor into the launcher, and recursively cleans only
the retained tree before an identity-checked non-recursive public-path removal.

The later exact-base review of `d81466f5` against `6adfa13d` found that path-based manifest creation
could not consume the Linux proc-fd root, intermediate symlinks could still redirect trusted copies,
and even non-recursive public-path removal retained a check/rmdir race. The final repair uses one
descriptor-relative prepared-tree helper for copy/config/prune/manifest/cleanup, retains the
`artifacts` descriptor before untrusted work, and leaves a directory-only owned tree for trusted
outer cleanup after candidate teardown instead of removing its public path in-script.

The exact-base review of `3052c9ab` against `5a6ae64b` found three valid final integration gaps:
direct execution inherited ambient loader/timing state, nested cleanup repeated the directory-entry
race, and the top-level benchmark workflow still described one-phase execution. The consolidated
repair constructs the ledger's fixed execution environment, never removes a directory entry before
candidate teardown, verifies directory-only failure residue, and documents digest-bound two-phase
native use in `bench/README.md`.

The repair review of `d7235794` found two narrow documentation/owner issues: the empty-`HOME` shell
assertion used the wrong parameter expansion, and the decode README still advertised the now-forbidden
ambient profiling switch. The final repair asserts defined-and-empty `HOME` directly and marks the
old profile numbers as historical, with no profiling switch on the sealed evidence interface.

Latest durable verification after the exact-base review repair passed the serial owner aggregate:
`scripts/test-benchmark-evidence-manifest.sh`, `scripts/test-benchmark-evidence-bootstrap.sh`,
`scripts/test-benchmark-evidence-cli.sh`, `scripts/test-benchmark-evidence-git-objects.sh`,
`scripts/test-benchmark-evidence-git-batch.sh`, `scripts/test-benchmark-input.sh`, and
`scripts/test-benchmark-evidence-statistics.sh`. Native Apple ARM64 also passed with
`CARGO_BUILD_JOBS=1`: each of
`bench/json_decode/run.sh prepare native` plus `run.sh native` and
`bench/json_soa/run.sh prepare native` plus `run.sh native` completed serially from an empty external
work directory, with the prepare-time digest supplied to direct execution and an injected ambient
sentinel excluded by the fixed launcher environment. These are
correctness/boundary owners, not accepted x86_64 performance evidence. At this PR's merge, no work
is active by user request. The next eligible capability is installed-source replacement protection
and protected-input matching, followed by the controller/profile/image/adversarial owners;
native x86_64 host qualification and the Request 7 language implementation remain later gates. No
emulation is accepted in any evidence lane.

The C-B borrow/ownership capability is complete
through L2e, F-A native resources is complete through L3, and F-B explicit
region materialization is complete through L4 and L6. Direct, captured,
imported, and function-value returns preserve exact owner provenance;
recursively Move returns carry a path-selected cleanup bit; shared/exclusive
parameters preserve caller ownership, replacement, generation invalidation,
and whole/per-unit ABI parity; package-defined native resources have nominal
identity, checked refs/views, producer-owned cleanup thunks, and exactly-once
Drop; named regions support explicit recursive cloning plus chunked `RegionPlain`
array construction without a hidden heap vector; and the library-boundary
prerequisites are complete through L7. The `pkg.db` roadmap is complete through
Q1–Q6, D13's common batch/SoA rail, its PostgreSQL lease/status/delivery/format
and pool rails, and D14's dual-driver dynamic SQL and proved SQLite scalar
callbacks. The final cross-rail audit is complete: arena-owned owner patterns,
the Q5b1 connection-v2 reset, partial SQLite fixture interposition, and
PostgreSQL streamed-versus-buffered fixture isolation were closed. VC1 is also complete: the
existing SQL-native path is proved against pinned pgvector 0.8.6, checked extension identity and
the no-extension failure control are owned, SQLite vec1 0.7 has an isolated no-loader disposition,
and the English/Japanese guide records the wider RDB boundary. All fourteen `pkg.db` owner suites
now run against the ordinary PostgreSQL and isolated pgvector services in the local and required CI
gate. The
wave-by-wave record moved to the 2026-08-13 archive.
The exact contracts and implementation matrices are in
`docs/impl/pkg-design/db.md` §23. PostgreSQL COPY remains deferred by that
document's §25 until a concrete measured consumer selects its public operation.
VC1 added no public API: existing driver-pinned SQL, Text/Bytes casts, checked metadata, metadata,
and EXPLAIN are proved against PostgreSQL/pgvector; SQLite vec1 and the wider RDB portability
boundary are recorded without adding an extension loader or common vector/search abstraction. Any
later direct native-vector mapping remains a separate consumer-driven, driver-qualified design.

align-llm Request 4 is complete on `main`: #798 settled client-side HTTP/1.1 chunked response
framing and #800 shipped the shared strict incremental decoder across plaintext/TLS, `http.parse`,
pooling, and all existing client calls without a public API or ABI change. The sibling request
register records the merged surface and the required align-llm batch release build is green.

The align-llm compatibility break found by the v0.4.0-successor release
compiler is closed on `main` by #786: Sema admitted both `str.clone()` and
`string.clone()` while the MIR body validator accepted only the borrowed
receiver, and the newly admitted fresh and control-flow receivers additionally
required MIR's borrow-owned lowering path. Both stages now route through
`align_sema::str_clone_body_ty`; the exact contract is
`docs/impl/19-hir-validation-ledger.md` "Producer-delegation closure matrix".
Pinning align-llm to the merged commit is align-llm-side work.

align-llm Request 6 adoption is closed by align-llm PR #84. Request 8's Align-owned heap
declared-record `array_builder` contract was accepted in #799 and shipped in #801 at
`029e27465d79e24cd36d374aae41dca0ec7e6979`. The sibling request register records the merged
surface and the required batch release build is green. Request 10's recursive heap-tree-record
extension is shipped from the accepted
`docs/impl/17-library-boundary-prerequisites.md` §7.6 ledger: nested Options, string/Copy/record
arrays, the exact C6 field graph, existing tagged-wrapper closure, whole/per-unit identity, and
stack/boxed recursive cleanup share the existing builder/runtime ABI. Reopen §7.6 only for a public
or ownership-strategy change after design review.

align-llm Requests 11 and 12 are implemented. Request 12 shipped in #807: the bounded canonical JSON
path shares the existing formatter, binds the complete encode plan back to the source schema, and
validates the fallible MIR envelope before LLVM construction. Request 11 adds
`max_capture_bytes` as the per-stream command-local bound;
`run_bytes` is the arbitrary-byte terminal, and the shared runtime owns exact bounded allocation,
allocation-free post-fork drain, deadline-aware post-EOF wait, deterministic error precedence,
process-group signalling, and direct-child reap. Whole/per-unit interface identity, cache
edit/revert, checked-HIR inventory, fatal allocation and syscall failpoints, and the 64/256 KiB
resource measurement are owned by the process suite and `bench/process_capture`. Do not add a
second JSON formatter, estimator pass, dynamic JSON value, unbounded-encode-then-discard path, or
post-capture length check.

Out-of-gate suites (everything outside `scripts/test-pr.sh`) are guarded by the
nightly full-suite workflow, which builds once, runs every compiled test binary
concurrently through `scripts/run-suite-binaries.sh`, and diffs the observed
failures against `scripts/known-failures.txt` in both directions. That manifest —
not this file — is the live baseline: a new failure is named, and a line whose
test starts passing is red until the fixing change deletes it. The 2026-08-13
baseline (#782, run 31669771705) is 16 strict entries in four triage classes
recorded in the manifest's own triage comment, and running
`scripts/run-suite-binaries.sh` with no arguments reproduces the same judgement
locally. The job budget is 30 minutes and exceeding it is the signal, not
something to raise.

Known remaining pre-existing hang (found 2026-08-10, reproduced twice
including a build without the owned-string validator change, both at CPU 0 for
40+ minutes): `fuzz_differential::result_question_chain_computes_the_oracle_value`
never completes. Its generators are integer-only Result chains, untouched by
the owned-string validator work; triage the hang (likely a generated binary
blocking) before relying on the nightly full-suite signal, which it will
otherwise consume up to the job timeout (the test now carries `#[ignore]` so
`cargo test` excludes it by default until then).

The nightly's own findings are closed: the Gate-3 rejected-operand sentinel
class (#745), the open-world callback rule that had broken `apps/web` since #672
and the silent-empty-MIR break it hid (#742), the validator's private copy of
the mangling scheme (#744), and fixed struct-array slicing for the router
(#743). What remains red is exactly `scripts/known-failures.txt`; its triage
comment carries the open questions, including the two `m5` owners that expect
rejections the compiler no longer emits.

The silent-empty-MIR class — a body validator re-deriving a fact the producer
owns — reached eight occurrences and is closed at its root by #774: the fallible
`lower_program_checked` boundary names the refusing validator and function
instead of publishing an empty program, and every production path uses it. The
complete matrix is `docs/impl/19-hir-validation-ledger.md`
"Producer-delegation closure matrix".

The completed prerequisite waves and current product boundary are:

```text
C-A canonical callable closure  complete through c3
C-B borrow/ownership closure    complete through L2e
F-A native resources            complete through L3
F-B region materialization      complete through L4 + L6
F-C static artifacts             complete through L5
F-D package integration          complete through L7
Q1 static Query vertical         complete through D1
Q2 dual-driver scalar parity     complete through D2 + D4
Q3 checked/offline parity        complete through D3 + D5
Q4a reusable execution           complete through D6 + D7
Q4b streaming resilience         complete through D8 + D9
Q5 schema tooling/inspection     complete through D11 + D12
Q6 compound product closure      complete through D10
A1 common batch/SoA rail         complete through #740 / D13
A1 PostgreSQL lease prerequisite complete / catalog + EXPLAIN overlap closure / D13
A1 PostgreSQL status prerequisite complete / package/tool fail-close + migration COPY / D13
A1 PostgreSQL direct delivery    complete / SingleRow + PortalBatch / D13
A1 PostgreSQL prepared parity    complete / streamed delivery + stmt v3 resolver / D13
A1 PostgreSQL formats            complete / binary parameters/results + protocol budgets / D13
A1 explicit fixed pool           complete / SQLite + PostgreSQL non-waiting ownership / D13
deferred: A1 PostgreSQL COPY     requires a concrete measured consumer / D13
A2 dynamic SQL rail              complete / dual-driver value + execute + rows / D14
A2 proved callbacks              complete / SQLite scalar functions + final cross-rail audit / D14
VC1 vector compatibility         complete / pgvector proof + vec1/RDB boundary / no public API
```

The exact cell contracts and owner matrices remain in
`docs/impl/17-library-boundary-prerequisites.md`.

The user-directed release checkpoint is the end of the complete committed
`pkg.db` roadmap through D14. At that point, perform the formal versioned Align
release workflow—not only a release build—including the workspace version and
lockfile bump, matching release notes, `chore(release): Align vX.Y.Z` on `main`,
and the matching tag and push.

Completion terms are fixed across the roadmap. The first public `pkg.db`
release is L1a–L7 plus D1–D12. The complete committed `pkg.db` roadmap also
includes D13 batch/SoA/native breadth and D14 dynamic SQL/proved callbacks.

## Start here

1. Read `CLAUDE.md` for repository rules, sources of truth, and the required
   review flow. `AGENTS.md` is the Codex compatibility link to the same file.
2. Read the design or audit directly governing the requested work.
3. Use the archive only when historical implementation detail is material.

Do not rely on Claude's per-machine memory or a previous conversation. Durable
facts must live in this repository.

## Current baseline

- **Release:** v0.5.0 is the release checkpoint for the complete committed
  `pkg.db` roadmap through D14. `RELEASE_NOTES_0.5.0.md` is the release record.
  It includes the post-v0.4.0 macOS `clearenv(3)` portability fix from #636;
  macOS CI is green and Linux behavior is unchanged.
- **Compiler roadmap:** M0-M15, the LLVM 19-to-22 checkpoint, separate
  compilation, the default-on per-unit object cache, the in-process compilation
  memo and the persistent per-unit frontend cache, parallel codegen, ThinLTO,
  instrumented PGO, and `ld.lld` linking on ELF are complete. The roadmap
  retains the implementation evidence; it is not the live backlog. The dedicated
  build-performance track is `docs/impl/21-build-perf-plan.md`.
- **pkg.web:** F0-F3 and W1-W7 are complete. The current contract is
  `docs/impl/pkg-design/web.md`; `docs/impl/15-pkg-web-plan.md` is the completed
  execution record. The framework is general-purpose REST infrastructure, not
  an LLM-gateway-specific subset.
- **align-llm requests:** Request 4 is merged through Align design #798 and implementation #800;
  Request 6 is closed after Align design #703, implementation #704, and align-llm adoption #84;
  Request 8 is merged through Align design #799 and implementation #801; Request 10's §7.6
  implementation is merged; Requests 11 and 12 are implemented, and Request 5's bounded-HTTP
  implementation is merged at `5aa5b23a`. Request 7's benchmark-evidence design is active, and its
  benchmark-input, manifest, canonical JSON, typed report, SSHSIG, CLI, and bootstrap-binding
  prerequisites are merged; Requests 9 and 13–15 remain proposed in
  `../align-llm/docs/align-requests.md`; Request 9 remains the later C7 blocker after Request 7.

Consumer-gated deferrals that remain intentional:

- Fully escaping function values wait for a consumer and a settled heap-owned
  environment/drop model.
- `std.process` bounded/binary capture is complete for align-llm Request 11; client adoption still
  requires pinning the merged implementation and running the request's focused real-client gate.
- Top-level `array<str> := json.decode(...)` waits for a result representation
  that carries the input region. Struct fields of `array<str>` already ship;
  see `docs/impl/core-design/json.md`.
- The first pkg.web consumer application remains a separate, owner-scheduled
  task.

## Build and test notes

Use the repository wrapper on macOS, WSL2, Ubuntu, or Debian. It resolves LLVM
22 and the macOS keg-only library paths; explicit environment overrides remain
available for nonstandard installations.

```bash
scripts/cargo.sh build --workspace
scripts/test-pr.sh
scripts/cargo.sh clippy --workspace --lib --bins -- -D warnings
```

Operational rules:

- After modifying `align_runtime`, run a plain workspace build before driver
  tests or `alignc run`; user programs link the runtime static archive.
- Do not edit runtime sources while a workspace test run is in progress; that
  can produce a stale-archive cascade in driver tests.
- Do not pipe test output through a command that hides the original exit code.
- Use `ALIGNC_CACHE=off` when a test specifically requires a cold build.
- `align_driver` memoizes repeated identical compilations inside one process
  (whole-program sema, per-unit frontend, HIR lowering, object bytes;
  `docs/impl/10-cache-first-optimization.md` §6.6). It is content-keyed and
  unobservable, and it cut the `pkg.db` owner suites' compile CPU 2.3x. A test
  that must observe a genuinely cold in-process compile calls
  `align_driver::memo::set_enabled(false)` / `memo::clear()`.
- `alignc build`/`run`/`size` additionally reuse per-unit FRONTEND results across
  processes (`docs/impl/10-cache-first-optimization.md` §6.7): an unchanged unit
  skips sema and lowering entirely, keyed on its source plus its dependencies'
  interface hashes and import closures, in the same `ALIGNC_CACHE` root. A hit
  carries no MIR, so a unit whose object also misses is rehydrated and verified
  against its entry. Descriptor-owning units, located walks, and `--thin-lto`
  are excluded; `ALIGNC_CACHE=off` disables it with everything else.
- Linux links through `ld.lld` when the matched LLVM install provides one;
  `ALIGNC_LINKER=lld|system` forces the choice and every CI job sets it
  explicitly so a missing linker is red rather than silently slow
  (`docs/impl/21-build-perf-plan.md` item 2). Mach-O never selects lld.
- `scripts/run-suite-binaries.sh` is the local equivalent of the nightly
  full-suite job, judged against `scripts/known-failures.txt`;
  `ALIGN_GATE_JOBS` sets the concurrency both it and `scripts/test-pr.sh` read.
- Network, TLS, filesystem, and fd tests may need an unrestricted local
  environment rather than a sandbox.

## Durable records

```text
Language semantics and surface       draft.md
Current decisions and open items     docs/open-questions.md
Milestone implementation evidence    docs/impl/07-roadmap.md
Current pkg.web contract             docs/impl/pkg-design/web.md
Current pkg.db contract              docs/impl/pkg-design/db.md
Native library boundary prerequisites docs/impl/17-library-boundary-prerequisites.md
pkg.db feasibility review            docs/impl/18-pkg-db-review.md
Cache architecture and parity resolution    docs/impl/10-cache-first-optimization.md
Test execution policy                docs/impl/16-test-policy.md
Closure/memory/I/O/SIMD audit        docs/impl/12-pipeline-closure-memory-io-simd-audit.md
Allocation and short-input audit     docs/impl/13-string-array-allocation-short-input-audit.md
Source-correctness fixes             docs/impl/source-correctness-fixes-2026-07-13.md
Checked-HIR validation ledger        docs/impl/19-hir-validation-ledger.md
Runtime ABI ledger                   docs/impl/20-runtime-abi-ledger.md
Build performance track              docs/impl/21-build-perf-plan.md
Out-of-gate failure baseline         scripts/known-failures.txt
Historical session journal           docs/archive/HANDOFF-2026-08-13.md
Earlier session journal              docs/archive/HANDOFF-2026-07-25.md
```

## Maintaining this handoff

- Update the state paragraphs and the current baseline in place.
- Do not append a full PR narrative. Put durable design facts in the relevant
  spec/audit, and rely on the PR and Git history for implementation chronology.
- When historical context is still worth retaining, add a dated archive rather
  than growing the live handoff indefinitely.
- Keep release and review procedures in `CLAUDE.md`; link to them instead of
  duplicating them here.
- Keep this file readable at the start of a session, roughly 200 lines. When it
  outgrows that, move the historical portion into a dated `docs/archive/` file
  in one pass rather than trimming paragraph by paragraph.
