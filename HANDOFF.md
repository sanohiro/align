# Session handoff

Current continuity note for a fresh Claude Code or Codex session. Keep this file
about the present state, the next decision, and operational facts. The former
per-PR journal is preserved in
[`docs/archive/HANDOFF-2026-07-25.md`](docs/archive/HANDOFF-2026-07-25.md).

_Last updated: 2026-08-10._ The C-B borrow/ownership capability is complete
through L2e, F-A native resources is complete through L3, and F-B explicit
region materialization is complete through L4 and L6. Direct, captured,
imported, and function-value returns preserve exact owner provenance;
recursively Move returns carry a path-selected cleanup bit; shared/exclusive
parameters preserve caller ownership, replacement, generation invalidation,
and whole/per-unit ABI parity; package-defined native resources have nominal
identity, checked refs/views, producer-owned cleanup thunks, and exactly-once
Drop; named regions support explicit recursive cloning plus chunked `RegionPlain`
array construction without a hidden heap vector; and the library-boundary
prerequisites are complete through L7. `pkg.db` Q1/D1 now provides typed static
Query/command descriptors, exact versioned artifacts, producer-owned ordinal
bind/decode/metadata plans, whole/per-unit cache identity, and fail-closed fake
driver execution. Q1 owns no native database resource. Q2 now closes scalar
execution across SQLite and PostgreSQL (D2 + D4). Q3 now closes deterministic
checked-metadata regeneration and offline consumption across both drivers
(D3 + D5), including exact schema identities, fail-closed native description,
atomic publication, and required PostgreSQL CI coverage. Q5a/D11 migration
lifecycle tooling and Q5b1's producer-owned static Query metadata consumer have
also shipped; Q5b2 completes D12 native catalog inspection and EXPLAIN. The
schema tooling/inspection path is complete. Q4a closes D6/D7 with reusable
prepared statements, shared connection/transaction execution, and exact
failure-safe native cleanup. Q4b closes D8/D9 with one-pass typed rows,
generation-bound borrowed views, complete bind/decode parity, scoped deadline
enforcement, cancellation drain/recovery, and failure-safe stream cleanup. The
Q6 compound product closure now completes D10 with ordinary Query-local Pure
shapers for transaction/master and User + Groups outputs, exact one-execution
stream ownership, one-parent and adjacent segmented shaping, visible region
allocation/copies, and whole/per-unit mutable-retention parity. The next product
work is the first independently useful A1/D13 throughput rail: common bounded
batch/SoA delivery.

Out-of-gate suite status (suites outside the bounded CI gate; a nightly
full-suite workflow now runs them daily so this class cannot rot silently):

- The 2026-08-09 `per_unit_surface` CLI/library parity failure was fixed on
  `main` by #731 (gate-on-use in `install_static_descriptor_data`); a unit
  regression owner inside the bounded gate now pins the empty-descriptor path.
- The stale `drop_value` and `call double` MIR-spelling assertions in
  `buffer_donate` and `m4` are updated to the current cleanup and
  call-qualification spellings.

Known remaining pre-existing hang (found 2026-08-10, reproduced twice
including a build without this branch's validator change, both at CPU 0 for
40+ minutes): `fuzz_differential::result_question_chain_computes_the_oracle_value`
never completes. Its generators are integer-only Result chains, untouched by
the owned-string validator work; triage the hang (likely a generated binary
blocking) before relying on the nightly full-suite signal, which it will
otherwise consume up to the job timeout (the nightly workflow skips it by name
until then).

The fixed-array follow-up from the validator-alignment review is closed. The
suspected owned `array<T>` element is source-reachable: sema admitted it even
though fixed arrays have no per-element null/drop lowering for that Move
shape. Sema now rejects the complete scalar-Move element class, including owned
`string`, because fixed arrays have no per-element scalar Drop path. The HIR
validator uses the same recursive resource/ref exclusion as the producer.
Focused owners preserve Copy values and in-place Move structs while rejecting
owned strings, owned arrays, and resource-bearing near-misses before MIR
construction.

Fixed on this branch (was present on `main` since the 62f48771 checked-HIR
validation activation): the body validator contradicted sema when reading a
`string` element field from a Move-struct array as a borrowed `str` view, so
`emit-mir` silently produced an **empty program** (check ok, no `_main`, link
failure). Sema also admitted owned-`string` scalar fixed arrays despite their
missing element Drop. The validator now mirrors the closed producer contract,
the three `owned_structs_arrays` owners are green again, and a checked unit
whose functions vanish at the MIR boundary is now a loud internal error in the
shared CLI walk instead of a silent empty binary; the library lowering entry
points keep their tested fail-closed empty-program contract for hand-constructed
HIR.

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
next: A1 common batch/SoA rail   D13
```

The exact cell contracts and owner matrices remain in
`docs/impl/17-library-boundary-prerequisites.md`. Line/test counts no longer
trigger automatic splits. Each wave gets one stable-candidate full-diff review
plus explicit final finding closure and one bounded final gate. It is not
reviewed again after PR opening unless the fix crosses the high-risk triggers.
F-A/F-B/F-C may proceed concurrently; F-B deliberately keeps the named-region
producer with its first useful materialization consumer instead of landing L4
as another dormant seam. D0 probes may run in parallel.

After the prerequisite gate, the initial database product is delivered in seven
waves rather than twelve serial D-label PRs:

```text
Q1 static Query vertical          D1
Q2 dual-driver scalar parity      D2 + D4
Q3 checked/offline parity         D3 + D5
Q4a reusable execution            D6 + D7
Q4b streaming resilience         D8 + D9
Q5 schema tooling/inspection      D11 || D12
Q6 compound product closure       D10
```

Q3 and Q4a start in parallel after Q2. Q5a/Q5b follow Q3; they are the only
default two-PR wave because schema mutation and read-only inspection are
independently usable failure domains. Q4b follows Q4a, and Q6 follows Q4b. The
initial release waits for Q5a/Q5b and Q6.
The database therefore has two parallel critical paths after Q2: runtime
`Q4a -> Q4b -> Q6`, and tooling `Q3 -> {Q5a,Q5b}`. Checked metadata does not
delay prepared/transaction implementation.
D13 and D14 then run as two additive release trains whose independently useful
driver rails may proceed in parallel; their internal acceptance labels do not
serialize unrelated native surfaces.

The user-directed release checkpoint is the end of the complete committed
`pkg.db` roadmap through D14. At that point, perform the formal versioned Align
release workflow—not only a release build—including the workspace version and
lockfile bump, matching release notes, `chore(release): Align vX.Y.Z` on `main`,
and the matching tag and push.

Every eight hours of active implementation should leave a compiling,
owner-test-backed source checkpoint. Every twenty-four hours should leave a
whole capability PR-ready, or one independently usable rail when the plan
explicitly permits parallel rails. Missing that checkpoint triggers a time-cost
audit and a consumer-boundary re-cut, not another documentation/review loop or
an automatic smaller dormant PR. Operational PR/SHA/review narration belongs
in Git/GitHub and is not extended here after every checkpoint.

Completion terms are fixed across the roadmap. The first public `pkg.db`
release is L1a–L7 plus D1–D12. The complete committed `pkg.db` roadmap also
includes D13 batch/SoA/native breadth and D14 dynamic SQL/proved callbacks. A
2026-08-05 source audit aligned this dependency plan across
`docs/impl/07-roadmap.md`, `docs/impl/17-library-boundary-prerequisites.md`,
`docs/impl/18-pkg-db-review.md`, `docs/impl/19-hir-validation-ledger.md`,
`docs/impl/20-runtime-abi-ledger.md`, and both `pkg-design/db.md` language
versions.
The 2026-07-27 F1–F95 design review remains the incorporated review of record;
the source audit did not pretend to be a fresh line-by-line independent review
of the complete design contract.

## Historical detail

Everything below this heading is a historical record, not current workflow
instruction. Old line targets, hard review bounds, rerun requirements, branch
names, and PR sequences are superseded by the baseline above and `CLAUDE.md`.

The merged c1 checkpoint made `align_mir::RuntimeKey` supply the exact 281 semantic keys and
alphabetical `ALL`; the backend-private 286-row typed ABI registry is the sole fixed-native
declaration/type/attribute/rt-LTO authority. Extern compatibility rejects before function
declaration construction, exact keyed and unkeyed externs reuse one handle, same-spelled program
claimants keep legacy LLVM uniquification while attributes and dedicated calls stay on the typed
native handle, and every dedicated allocation/drop/cleanup/native consumer is keyed. Only the
explicitly deferred generated-trampoline and generic direct-call mixed-map seams remain string
indexed through c3. The two main-wrapper calls use typed unkeyed handles. The fresh exact-diff
adversarial review found one P2 guarded rt-LTO physical-name collision and two P3 machine-gate
gaps; the bounded follow-up retargets incoming guarded bodies to captured typed physical names,
adds the guarded program-collision owner, checks all 286 declarations against a checked-in golden,
and adds a compiled default/feature export script with compile-time probe-signature pins. The
required bounded follow-up review confirmed those fixes and found one further P2 in the same class:
a parseable rt-LTO artifact could omit a guarded body and leave its declaration un-curated. The
follow-up now preflights all four guarded rows for presence, exact type, and a body, with loud
all-row re-curation/fallback owners for missing, declaration-only, and wrong-type artifacts. The
fresh `cc9da75` final-SHA review then found three valid P2 closure gaps and one stale-state P3:
linkage/calling convention were not preflighted and a declaration-only post-link handle could still
continue; `ArgsBuild` was incorrectly claimed as a source-compatible extern despite the settled
view-return rejection; and the machine gates compared all names but not all 286 compiled Rust
signatures or every return/parameter ordinal. Commit `3b6041f` requires external+C guarded
definitions before link and a body-bearing external+C typed handle after link, adds seven exact
malformed artifact cells plus a post-link error owner, records `ArgsBuild` as wrapper-only, compares
all 286 compiled Rust LLVM signatures independently, and runs the production extern predicate over
all 286 returns and every parameter ordinal. It also covers the five attribute classes, all eight
ordinary probe spellings, and whole/per-unit-shaped declaration parity. The next exact-diff review
found that the five unkeyed identities still kept symbol/type facts outside the 281 keyed rows,
that the `ArgsBuild` unreachability owner omitted the closest source-valid aggregate, and that this
handoff was stale. Pushed review head `d240a57` closes the class with one 286-identity `RuntimeAbiId` row
iterator, makes the keyed-only iterator explicit for declaration order and typed MIR lookup, and
passes a source-valid `layout(C) { u64, i64 }` return through x86-64 SysV extern ABI formation to
prove it mismatches the native `{ptr, i64}` view. (`raw` itself is not a valid `layout(C)` field.)
Post-open host review found two P2s: the export audit overwrote the ordinary runtime archive with
the last feature build, and the `FilterStrContains` dedicated consumer still used a method-style
string lookup. The independent review found three P3s: the private unkeyed/id enum representation
drifted from the concrete c1 design, three old comments overclaimed pre-c1 byte identity, and this
handoff still described the unpushed pre-PR state. The current HEAD closes all five findings.

| Post-open finding | Closure | Focused evidence |
|---|---|---|
| P2 export-audit archive contamination | Build every feature case under one temporary audit target; never write the caller/default target. Restore the already-contaminated local archive with a default build. | Exact base/alloc/par/task/all export sets are 286/290/290/286/294; the ordinary archive remains byte-identical across the audit. |
| P2 method-style fixed-runtime lookup | Route `FilterStrContains` through `self.runtime(RuntimeKey::StrContains)` and normalize whitespace in the source inventory before rejecting literal `self.funcs.get("...")` seams. | `runtime_abi` owners 14/14 and the string-filter stable-compaction owner 1/1. |
| P3 private enum representation drift | Give `UnkeyedRuntimeKey` exact `repr(u8)` discriminants 0–4 and restore ordered derives on it and `RuntimeAbiId`; compile-pin the `Ord` bound and discriminants. | Registry completeness/uniqueness and exact representation assertions pass in the 14-owner group. |
| P3 stale byte-identity comments | State only the current no-probe/no-link and PGO separation semantics; acknowledge canonical c1 declaration order. | Author comment-to-ledger inspection against the c1 whole/per-unit/cache row. |
| P3 stale operational handoff | Record pushed base/head branches, draft PR #702, post-open findings, closure, and current gates. | Current branch/PR/check state inspected before the follow-up. |

Focused c1 owners pass 14/14; `align_codegen_llvm` passes 70/70 with 5 intentional PGO ignores;
`align_mir` passes 95/95; the runtime source/registry owner passes; compiled
base Rust signatures match 286/286 and base, `alloc-count`, `par-map-probe`, task-only, and
all-feature runtime export sets are exactly 286/290/290/286/294; and all nine rt-LTO integration
owners pass. Changed-crate all-target Clippy
passes with `-D warnings` through the current HEAD. The ordinary `scripts/test-pr.sh` invocation completed the workspace
build and codegen owners, then its combined driver binary remained at 0% CPU in `_dyld_start` and
was stopped as INCOMPLETE; the exact driver binary and every not-yet-run library/integration target
then passed without source changes, so the gate was closed by checkpointed continuation rather than
a broad rerun. Workspace all-target Clippy passes with `-D warnings` (14m00s) on `cc9da75`; both
finding-closure affected-crate Clippy runs pass. The post-open closure additionally passes the
14 focused runtime-ABI owners, the one changed string-filter execution owner, the isolated export
audit with an unchanged ordinary-archive SHA-256, and `align_codegen_llvm` library Clippy with
`-D warnings`. PR #702's final head passed Linux x86_64, Linux ARM64, macOS Apple Silicon,
Pre-PR attestation, and Post-open review before merging as `ca26c68e`. The rebased c2a1 candidate
stack now owns only the public effect-free `FunctionTypeDef`, private semantic error/node identities,
and closed primitive/scalar/root field encoder; its
implementation closure matrix is in the plan of record. Its first independent boundary review found the summary/mode, semantic-versus-byte,
private-error, count, reachable-only, fixed-point-topology, and split-trigger gaps; those findings
were addressed. The revised review correctly rejected the first review's am-h ownership expansion
and found cross-node charge-point plus tuple/anonymous/nominal-equivalence ambiguities; those are
resolved in the matrix. The final review found only private error-order/mapping gaps; those are
resolved, and the bounded finding-closure check returned CLEAN. The first combined c2a source
checkpoint then measured 407 source lines before tests, exceeding the recorded 360-source trigger;
with the projected 380 test lines it would also have exceeded the 750-total trigger. Coding stopped
before fixing its two compile diagnostics. The first split review
then measured ordinary formatted size at 777 combined lines (about 663/115), rejected the untyped
seam and duplicate DFS/raw-byte authorities, found later-sibling precedence/complexity gaps, and
found stale project-truth paragraphs below. The rejected three-way plan still bundled the existing
372-line formatted comparator with graph validation, misstated its worst-case pair complexity and
fixed-point refinement cost, and left the source-size estimate implausible. The plan now splits
c2a1 field codec, c2a2a typed source-shape comparator extraction, c2a2b observation/complexity closure, c2a3's sole `ValidatedGraph`
traversal, and c2a4 equivalence/canonical bytes, with explicit compiler-only complexity owners. A
fresh independent four-way split review found only variable-length signature-sort complexity and
measured-versus-projected wording gaps. Both were closed in the ledger, and the bounded follow-up
returned CLEAN. C2a1 now implements only the reviewed boundary: public effect-free
`FunctionTypeDef`, the behavior-neutral relocation of the existing five-variant HIR `Node`
identity, and a private transactional primitive/scalar/root field codec. The final new module is
338 production lines plus 260 test lines, below the recorded 400/260 caps. Complete-byte owners pin
all 57 root, 34 scalar, and six primitive cases; width owners cross all 256 `u8` values, lane owners
cross every admitted value and invalid boundaries, and success-prefix/error-rollback owners pin the
caller-buffer contract. On the pre-restack dedicated macOS target the first focused owner passed
6/6, the then-current full `align_mir` suite passed 101/101, and all-target `align_mir` Clippy
passed with `-D warnings`. The
ordinary-target focused launch compiled but stopped INCOMPLETE after sampling confirmed CPU 0 in
`_dyld_start`; no failed test was observed. The fresh implementation preflight found incomplete
payload/invalid-domain coverage, extra `Node` ordering derives, unspecified error-time buffer
mutation, and this stale status. The coherent fix removes the derives, makes every compound helper
transactional, expands the exact owners, and passes the final focused 6/6 plus Clippy. After
restacking onto C1 and PR #706, the refreshed exact-pair focused owner passes 6/6, the full MIR
owner passes 108/108, and all-target MIR Clippy passes; this directly covers the behavior-neutral
`Node` relocation against the current validator. The bounded finding-closure review verified all
four pre-restack fixes and returned CLEAN. The
ordinary `scripts/test-pr.sh` run completed its workspace build in 2m32s and the zero-test
`align_ast` owner, then the `align_codegen_llvm` binary remained at 0% CPU in `_dyld_start`; the
sample is preserved and that invocation is INCOMPLETE. Checkpointed continuation on the dedicated
target passed every then-unrun library owner (`align_codegen_llvm` 70/70 with five intentional
ignores, `align_driver` 28/28, `align_mir` 101/101, `align_sema` 206/206, and all remaining listed crates),
both interface integration owners (4/4 and 34/34), the formatter example owner (1/1), and all three
driver M0 owners. The executable M0 owner required first building `libalign_runtime.a` in that
dedicated target; its rerun passed without a source change. Workspace all-target Clippy passes with
`-D warnings` in 11m04s. No public `pkg.db` surface exists yet.

Prior am-b4 operational record (historical): `main` includes the shipped wave through #688 plus the merged am-p
placement-validation PR #690 (`39f9c7d`), am-n nominal/link PR #691 (`755cb9c`), am-h
declarations/headers PR #692 (`f7ebcb4`), am-b1 dormant body validation PR #694
(`b4b2d19`), am-b2a storage/vector/array body validation PR #695 (`96b16cc`), bounded task-wait
replay PR #699 (`9e2d615`), checked-HIR body-fact replay merge #700 (`1296b97`), and the later
am-b4 activation checkpoint `22e5ba3`. At that historical checkpoint the working slice was the
am-b4 MIR activation vertical on `agent/pkg-db-am-b4-activate`: the shared
four-entrypoint gate now runs body-core validation before sema fact replay. Replay uses explicit
numeric enter/exit occurrence frames for the 259-record checked-HIR ceiling, with no Span or native
pointer identity, and iterative teardown for successful, rejected, and panic-after-analysis paths.
The body envelope also validates declaration-free `print`/`hash64`/`hash128` calls and fresh local
`FnTy` cells by callable shape. The current follow-up closes nested callable/tagged-payload
matching, source-reachable loop-break/diverges agreement, and arena/task-group region crossing;
the body owner now has explicit tests for those cases. The pre-follow-up focused body owner passed
42/42, alongside
malformed-body, replay, identity, header, process/HTTP, root-completion, and task-wait owners with
the configured LLVM environment.
The full depth matrix reached successful compilation but its test
binary remained in macOS `_dyld_start` at 0% CPU before being stopped; that launch is preserved as
INCOMPLETE rather than CLEAN. The author-side closure matrix is preserved in
`.git/am-b4-activation-author-matrix.log`, and no public `pkg.db` surface is present yet.
`cargo build --workspace --locked` and the LLVM-configured full workspace/all-targets Clippy run
pass with `-D warnings`. The ordinary `scripts/test-pr.sh` reached the codegen test binary, but
both the normal and `DYLD_SHARED_REGION=private` runs remained in `_dyld_start` with 0% CPU before
listing tests; the same loader stall also affected the combined `align_driver` library owner.
Those attempts are preserved as INCOMPLETE in `.git/am-b4-test-pr-dyld-incomplete.log`; the latest
focused body/replay startup stalls are additionally recorded in
`.git/am-b4-followup-body-incomplete.log`. No missing test verdict is treated as CLEAN. The earlier
codegen owner itself passed 56/56 with 5 ignored after the validator fixes.
Review operation rule recorded after the 2026-08-03 incident: one exact `HEAD`/base review gets one
live invocation. A timeout, invocation bound, missing verdict, or killed process is INCOMPLETE, never
CLEAN. Preserve its log, elapsed time, process state, and last completed area; continue only with
the unfinished or changed slice. A longer user-approved duration extends that invocation only and
does not authorize polling, restart chains, or a duplicate broad review. This is also a standing
guardrail in `CLAUDE.md`/`AGENTS.md`.
The current follow-up tree has passed the MIR library owner (94/94), nested tagged-payload driver
owner (8/8), copy-collection owner (6/6), replay-clone owners (4/4), and targeted `align_sema`/
`align_mir` Clippy with `-D warnings`; its final exact-base review is pending after the fix
checkpoint is committed. The driver owners required one explicit `align_runtime` build in the
dedicated target directory before linking; the reruns passed without source changes.
The latest independent preflight then found one P1 in the revised break matrix: a forged
same-region `Break.accepted = false` could suppress a real loop exit. The closure matrix is reopened;
the current working tree requires exact bidirectional target/arena/task acceptance and adds
same-region-forged plus targetless/nested-region owner cells. The pre-fix MIR owner passed 94/94;
the post-fix ordinary target launch remained at 0% CPU in `_dyld_start` and was stopped as
INCOMPLETE, while the dedicated `/tmp/align-am-b4-break-owner` target passed the three exact break
tests and the complete MIR library owner 94/94. A fresh independent review of the reopened
`Break.accepted` slice returned CLEAN after checking sema production, every downstream consumer,
both region dimensions, fail-closed lowering, docs, and owner coverage. The complete exact-base
review then returned CLEAN, and the repository gate, workspace all-target Clippy, and final MIR
owner passed under the dedicated target. The branch is now pushed as the stacked base for PR #702;
it has no standalone PR.
The one host-native review invocation for the follow-up range
`62f487713561693cd93228928145010507680fcf..87b45b4` also produced no verdict because repeated
Xcode/git temporary-cache failures left the host review stalled; it was stopped and preserved as
INCOMPLETE in `.git/am-b4-followup-host-review.log` and
`.git/am-b4-activation-review-incomplete.log`. The same range will not be reviewed again unless it
changes or the user explicitly requests a new review.
The later continuation used `/Applications/Xcode.app/Contents/Developer/usr/bin/git` to avoid the
read-only Xcode shim cache failure and progressed through the remaining `hir_depth.rs`/`lib.rs`
scope, but reached its single 600-second bound without a verdict. It is preserved in
`.git/am-b4-followup-path-fixed.log` and remains INCOMPLETE; no pre-PR stamp or PR was created.
The subsequent fresh independent review of `origin/main...8bb574f` also reached its 120-second
bound without a verdict; it is recorded as INCOMPLETE in `.git/am-b4-activation-review-incomplete.log`.
A later fresh independent review of the complete `origin/main...88bdfa3` diff found two valid
follow-up issues: strict production body validation did not replay lexical local visibility and
definite initialization, and `match` result joins still compared fresh `FnTy` ids nominally. The
current follow-up adds the strict iterative scope pass, structural join matching, and one owner for
each; the ledger now records both closure rules. No review verdict is inferred from the earlier
incomplete host invocations.
The narrow `align_sema` test binary was then launched directly with a dedicated `TMPDIR`, fixed
`DEVELOPER_DIR`, and the prior dyld setting; it still produced no listing within 20 seconds and is
recorded as INCOMPLETE in `.git/am-b4-followup-body-incomplete.log`.
Historical Request 6 record: as of 2026-08-04, the current branch was `agent/align-llm-request6-implementation-v2`; the
implementation code head is `aa5bb7d`, with the expected-seed, return-spelling, and
diagnostic-order repairs implemented. The current branch also contains this continuity-only
handoff commit; its exact branch-head SHA is bound by the final review and preflight records rather
than self-referenced here. The earlier
implementation commit `43211ec` is followed by consolidated review repair `2688681` and the
baseline-compatible identity-harness repair `50912db`. The branch is based on implementation
checkpoint `8b55352` plus the reviewed Align Request 6 design head `712317b`, merged in Align at
`0ab7a30d6e7bfda56d4c8145b4672306634b9fea`. The implementation is complete, but PR #704 is not
yet merged. The PR branch contains the pushed implementation through `aa5bb7d` plus the
continuity-only Handoff update. Focused verification and the identity probe pass at the
implementation head. Fresh final review, preflight attestation, hosted CI, and merge are pending.
No `.align-revision` adoption pin or align-llm `make ci` verification exists yet.

## Active Request 6 implementation checkpoint

- Complete: reason-valued scanner envelope validation with Span-first precedence; generic return-context inference and failed-call non-publication; Copy composite runtime and allocation fixtures; imported whole/per-unit coverage; cache rejection/revert coverage; identity owner test and comparison script; explicit rejection of partially substituted generic composites before argument checking.
- Complete: the independent review finding at `2858d63` is closed by `1ac708d` and `5663c23`. Inferred scanner-typed locals now retain initializer spelling, generic-call results propagate spelling through their scanner arguments, and lambda captures inherit the repaired local map; no HIR field was added.
- Complete: the final-review P2s at `2f0c7b5` are closed by design commit `a8719cf` and implementation commit `c241beb`. Expected-return seeding now validates concrete leaves before arguments, and generic call expressions carry producer-owned annotated return spelling even without a scanner argument. The repair adds m5 and cache no-publication owners; no HIR field was added.
- Verification at `aa5bb7d`: generic m5 owners pass 11/11, imported-module owners pass 4/4, and the cache generic no-publication owner passes 1/1. `cargo +1.96.1 test -p align_sema --lib function_return_completeness_matrix -- --nocapture`, `cargo +1.96.1 check -p align_sema -p align_mir -p align_driver --tests --locked`, the matching clippy command, `scripts/test-pr.sh`, and `git diff --check` pass. The full m5 aggregate is 193/198; the same five known pre-existing link/diagnostic failures remain outside Request 6. `scripts/compare-json-scan-identity.sh HEAD` passes with interface, MIR, raw LLVM, object bytes, and non-build-id CodegenKey fields matching the fixed baseline while compiler-isolated identity fields differ as required.
- Review: the independent review found three valid P1 generic-inference findings and five P2 coverage/state findings; all were consolidated in `2688681`, with the identity-harness compatibility repair in `50912db`. A later independent final pass found one additional P1: a partially substituted `Result<T, U>` expected argument could reach constructor checking without a sound partial-type contract. The closure matrix was reopened; `b5b3f4d` narrows the contract and `7b88b97` rejects the state before checking or publishing the argument. The redesigned independent pass at `2858d63` found the alias/call-result spelling gap; `1ac708d` updated the contract and `5663c23` closed its first root-cause class. Final reviews at `2f0c7b5` found the concrete expected-seed and no-scanner-argument return-spelling gaps; `a8719cf` updated the contract and `c241beb` closes both with exact owners. Final HOST and INDEPENDENT reviews at `f467ddb` and `e6bf75f` found only stale Handoff continuity metadata; this final continuity-only update removes the obsolete Handoff action and distinguishes the implementation head from the externally SHA-bound branch head. Fresh final review evidence must bind the final pushed repair head.
- Next, in order: obtain fresh SHA-bound clean review evidence for the final continuity-only branch head, run preflight and hosted CI, merge, refresh `.align-revision`, and run `make ci` as the real-client adoption gate. This is the terminal PR for the current user goal; do not start another PR or roadmap item after it.
- Constraints: baseline identity is Align `576e57307fe4ef34e74566f5e389a2f0e2a04acd`; implementation must not consume hypothetical Align APIs; Request 7 remains blocked until this implementation is merged and verified by align-llm.
#667 adds the canonical recursive Drop plan and sound `Option<string>` fields;
#668 admits one direct recursively Move payload per tagged arm; #669 admits multiple Move payloads;
#670 completes nested tagged payload representation and the exact pkg.db L1b acceptance shape.
#672 carries L2a parameter modes and explicit empty return-provenance facts through AST, HIR, MIR,
interfaces, caches, and separate compilation without enabling the borrow ABI.
#673 infers named direct/imported parameter-root provenance; #674 refines product projections; #675
closes MIR continuation after every terminating eager child; #677 validates the global HIR type
domain; #678 fixes and closes the am-r public-contract ledger and implementation order.
#653 adds stable compaction for callable primitive-scalar `where` stages before
`par_map`; #654 adds a measure-first task-group record probe without changing
production behavior. The width/stride probe now covers scalar fused and
materializing maps without changing production behavior. #657 adds a runtime-only
aggregate-like stride probe without changing production behavior. #658 widens the
compiler-generated AoS range path to field projection and `where(.field)` stages;
#660 extends the same range path to `chunks` source slices and direct integer
chunk transform-reduce. #662 records a bounded, measure-only probe for the retained
`chunks` header allocation. #664 admits the already-recognised invariant
`str.contains` filter to the stable range path; SoA, richer string-search, and
unsupported layouts remain sequential._

## Start here

1. Read `CLAUDE.md` for repository rules, sources of truth, and the required
   review flow. `AGENTS.md` is the Codex compatibility link to the same file.
2. Read the design or audit directly governing the requested work.
3. Use the archive only when historical implementation detail is material.

Do not rely on Claude's per-machine memory or a previous conversation. Durable
facts must live in this repository.

## Current baseline

- **Release:** v0.4.0 was tagged at `88ee798` and published for the completed
  align-llm R1/R2/R3 batch. `RELEASE_NOTES_0.4.0.md` is the release record.
- **Post-release portability:** v0.4.0's macOS artifact failed to link because
  `clearenv(3)` is unavailable on macOS/BSD. #636 fixed this on `main` with a
  platform-specific portable implementation; macOS CI is green and Linux
  behavior is unchanged. The v0.4.0 tag predates the fix, so the next versioned
  release must be cut from current `main`.
- **Compiler roadmap:** M0-M15, the LLVM 19-to-22 checkpoint, separate
  compilation, the default-on per-unit object cache, parallel codegen,
  ThinLTO, and instrumented PGO are complete. The roadmap retains the
  implementation evidence; it is not the live backlog.
- **pkg.web:** F0-F3 and W1-W7 are complete. The current contract is
  `docs/impl/pkg-design/web.md`; `docs/impl/15-pkg-web-plan.md` is the completed
  execution record. The framework is general-purpose REST infrastructure, not
  an LLM-gateway-specific subset.
- **align-llm requests:** Requests 4–14 remain proposed in
  `../align-llm/docs/align-requests.md`; Request 6 is accepted with its reviewed design merged in
  Align PR #703. Its implementation/adoption is the active work on the branch named above. No
  release pin or align-llm adoption verification exists yet. Request 9 remains the later C7
  blocker.

## Latest shipped wave

```text
#630  std.process captured output + cwd
#631  process timeout + Error.Timeout
#632  process env / env_clear
#633  std.net connect/read/write timeout substrate
#634  std.http client/request I/O timeouts, including TLS and pooled reuse
#635  core.json array<str> struct-field decode
#636  portable env_clear for macOS/BSD
#637  unified Claude/Codex guidance + compact handoff
#638  Copy-struct array materialization
#639  Unit-call values + aggregate call-ownership hardening
#640  cold/cache build-result parity via complete structural MIR identity
#641  remove redundant JSON schema summaries from MIR
#642  whole-range par_map kernels with direct vectorizable element loops
#643  Copy-capturing par_map range contexts + integration-test hardening
#644  par_map range-threshold retune from measured crossover data
#646  separate bounded and full test suites
#647  direct integer par_map reduction fusion and ABI documentation
#648  batched parallel pool job publication
#649  focused test selection policy
#650  low-lock task-group claims and completion
#651  fuse primitive scalar `par_map` map stages
#652  bounded body and byte-aware `par_map` grain hints
#653  stable callable scalar `where` compaction
#654  measure-first task-group record layout probe
#655  relevance/cost test-selection policy
#656  scalar par_map width and output-stride measure-first probe
#657  runtime-only aggregate-like par_map stride measure-first probe
#658  compiler-generated AoS projection/field-filter par_map range stages
#659  bounded pre-PR attestation, review watchdog, and focused verification policy
#660  chunk-source range kernels and direct integer chunk transform-reduce
#662  measure-only chunks header allocation probe
#664  parallelize compiler-recognised invariant string filters
#667  recursive Drop plans and Option<string> owned fields
#668  direct recursive Move tagged payloads
#669  multiple Move payload partial construction
#670  nested tagged payload representation and exact pkg.db L1b shape
#672  L2a parameter modes and return-provenance representation
#673  named/direct/imported parameter-root inference
#674  product return-provenance refinement
#675  eager MIR continuation closure
#685  path-complete task-wait dominance
#686  native output-buffer local/mutability validation
#688  lexical extern invocation permission and non-escaping extern-call closure (am-u)
#694  dormant am-b1 body-core validator and owner matrix
#695  dormant am-b2a storage/vector/array validator and owner matrix
#699  bounded task-wait replay identity and stack closure
#700  checked-HIR sema body-fact replay and imported-provenance presence closure
```

#639 fixes Unit-call values across direct, indirect, pipeline, and per-unit
lowering. Its final review also hardened call ownership: temporary aggregate
arguments keep per-member cleanup until the call is reached, and arena-owned
Move values cannot transfer to a callee. Bound aggregates require one uniform
owned allocation mode so their single path-local cleanup bit remains exact.
The same transfer rule covers `Result.map_err`; fused pipeline functions reject
Move source and result elements until explicit per-iteration cleanup exists.
The review follow-up carries `map_err` and one-owner struct runtime provenance
through their result slots, guards partial direct struct and fixed-array initialization, and
rejects mutation when aggregate ownership is path-dependent. `map_err` also
retains its receiver during mapper evaluation and tracks mapper-capture borrows.
Move values leaving a `task_group` forward the tail local's cleanup bit and
clear that inner source before return or call transfer.
Early exits join open task groups before dropping captured frame or arena storage.
The task-group runtime region is reserved for spawned environments and result slots:
ordinary owned values inside the block retain individual cleanup, and general arena-only
allocation still requires an explicit nested `arena {}`.
Both `reduce` and materializing `scan` require a Copy accumulator until MIR has
explicit per-iteration transfer and error-path cleanup for Move values.

#640 replaced the function-only implementation hash with the complete structural
per-unit MIR program consumed by codegen. Type tables, declarations, linkage,
alignment, and located metadata now participate automatically, so a warm cache
hit cannot skip a cold codegen failure caused by an omitted backend input.

#641 then removed the recursive JSON schema strings that had existed only to
perturb the former incomplete implementation hash. JSON MIR nodes now retain
their target ids; the structural program hash owns cache identity.

#642 replaced the per-element indirect `par_map` callback with one generated
typed kernel per claimed range. The element loop and direct Pure body call now
share an LLVM function, allowing cheap arithmetic bodies to inline and
vectorize while preserving the existing worker-pool scheduler and ordered
output.

#643 carried direct-source Copy captures through a synchronous immutable context
so they use the same range kernel; filtered and unsupported aggregate forms remain
sequential. It also hardened malformed-expression cleanup, the macOS `fcntl` ABI,
and the web integration-test startup retry.

The #644 threshold probe moved the caller-only `PAR_MIN_CHUNK` floor from 32768
to 65536 on native Apple Silicon. It uses alternating paired timings and median
ratios for cheap and heavy bodies; the value remains host- and body-sensitive, so
rerun `bench/par_map/run.sh threshold` before another retune.

#651 carries the first primitive-scalar length-preserving `map` stages into the
same ordered range kernel. #652 adds a compiler-generated 1/2/4 body hint and
input/output byte-aware floor. An aggressive reduction of the common `i64` floor
was measured at the boundary and regressed pool/caller by about 7%, so the first
model keeps that floor conservative until width and aggregate measurements justify
a broader retune.

#653 adds callable primitive-scalar `where` stages to the same ordered
range pipeline. It counts survivors per range, prefixes byte offsets, then scatters
in source order into an exact-sized owned array. Pure map and predicate stages are
rerun by the two kernels.

#658 extends that path to direct and dynamic AoS struct inputs, compiler-generated
field projection and `where(.field)` stages, and ABI allocation-size validation for
padded rows and JSON struct-array descriptors. #660 extends the typed range
kernel to direct and materializing `chunks` maps and to direct integer chunk
transform-reduce; #664 admits the compiler-recognised invariant `str.contains`
filter to the same stable count/scatter path. Move aggregates, SoA, richer
string-search, and unsupported layouts remain on their safe existing paths.

#659 establishes the operational rule that self-review and an adversarial
preflight happen before a draft PR, review processes have a bounded watchdog,
and related review fixes are batched instead of committed one finding at a time;
see `CLAUDE.md` and `docs/impl/16-test-policy.md`.

## Delivery retrospective: 2026-07-27 through 2026-07-30

The recent pkg.db prerequisite wave delivered nontrivial compiler foundations
but did not deliver a user-visible database package or SQLite/Postgres driver.
The merged capability sequence is recursive Drop and `Option<string>` fields
(#667), direct and multiple Move sum payloads (#668-#669), nested payload
representation (#670), parameter-mode and empty return-provenance
representation (#672), named return provenance (#673), and product return
provenance (#674). PR #675 closes eager MIR continuation lowering, but remains
compiler infrastructure rather than a usable pkg.db surface.

GitHub's earliest-commit-to-merge wall-clock proxy shows the throughput failure
clearly:

```text
PR    observable wall time    production / test / docs changed lines
#666  8h17m                   0 / 0 / 10,746
#667  7h13m                   1,736 / 1,397 / 158
#668  15m                     808 / 447 / 155
#669  24m                     68 / 313 / 82
#670  47m                     3,889 / 496 / 156
#671  14m                     0 / 0 / 169
#672  8h42m                   4,468 / 2,068 / 72
#673  23h27m                  7,380 / 3,057 / 549
#674  2h20m                   1,150 / 620 / 113
#675  4h14m                   4,595 / 295 / 258
```

These wall times exclude work before the first commit and may include idle
review or CI waits. Changed-line counts also overstate implementation progress
when formatting moves dominate, as in #675. They are evidence of delivery cost,
not productive coding hours.

The failure was operational: cross-cutting compiler slices were too broad,
contract and matrix authoring expanded before a small executable checkpoint,
ordinary findings caused repeated full-diff review, broad gates were repeated
after narrow changes, and document or formatting churn was allowed to resemble
implementation progress. The fast #668-#671 sequence proves that narrow,
mergeable slices do not have the same failure mode.

The current correction is in `CLAUDE.md`: capability-sized delivery, one
complete review pass, one coherent all-findings fix, no ordinary re-review,
narrow owner checks after the fix, and no repeated broad gate on an unchanged
tree. Line counts are inventory, not a delivery target.

### PR #679 delivery retrospective

PR #679 was squash-merged as `b194703`. Its earliest commit-to-merge proxy was
16h12m across 58 commits; the public PR was open for 3h31m. The atomic am-d
vertical changed 11 files by +14,242/-5,033 because every already-active
checked-HIR body and type-DAG consumer had to become stack-safe together; the
plan of record documents why splitting that consumer closure would have merged
an accepted graph that could still overflow in a later phase.

The avoidable delay was proof closure, not the iterative implementation alone.
The initial deep fixtures did not fully reproduce sema-owned allocation facts,
exact Result/Error identities, owned endpoints, or proportional control-flow
evidence. That caused the high-risk repeated-review exception, including P1
findings and one matrix reopen. The corrected closure matrix and exact owner
tests now make those producer-validity facts author-side obligations. The final
local gate also spent about 35 minutes on macOS test-binary startup and locked
workspace all-target Clippy; that was slow but continuously progressing and is
not a reason to narrow required verification.

One separate operational error was generalizable: a normal `gh pr edit` after
preflight deleted hidden markers and failed CI. The simplified workflow now
uses only `scripts/open-pr.sh`: ordinary prose is finished before opening, and
the same command's `--update PR_NUMBER` mode refreshes markers after a required
later push. Separate preflight-update and post-review-recording tools were
removed.
No additional rule was added for the implementation findings because the
existing cross-cutting closure-matrix gate already states their durable fix.

### PR #681 delivery retrospective

PR #681 was squash-merged as `f3bb666`. The predecessor-merge-to-merge proxy was
2h22m; the public PR was open for 8m14s. The independently mergeable am-e vertical
changed 11 files by +729/-31 and closed the source producer, malformed-MIR preflight,
whole/per-unit LLVM ABI, optimized ABI, ThinLTO parity, and exact exit behavior together.
It stayed below the 1,000-line split-proof threshold.

The avoidable delay was acceptance-matrix translation. The first implementation had
the correct producer and backend strategy, but its tests sampled return categories,
build paths, and malformed builtin `Error` fields where the ledger promised every
category and an exact Cartesian product. Preflight therefore added the complete
source return/parameter matrix, raw/optimized whole/per-unit signatures, direct-i32
and argv ThinLTO cases, and one-field-at-a-time argv/Error mutations. The durable rule
is now in `CLAUDE.md`: before coding an exact boundary slice, translate every
`every`/`exact` acceptance phrase and every named build path into an explicit owner-test
closure checklist; representative samples do not close such a contract.

Required verification was slow but not stalled: locked all-target Clippy took 13m42s
before the final lint fix and 13m45s in the HEAD-bound preflight. Test binaries also
reported seconds of measured work after longer tool-output waits. Those commands kept
making progress and are not a reason to weaken the gate. The post-open host review
completed cleanly but put its verdict marker inline; its preserved result was normalized
instead of rerunning the full review. No extra rule is needed for that formatting-only
tool failure because the existing bounded-review continuation rule already applies.

### PR #683 delivery retrospective

PR #683 was squash-merged as `c304512`. The predecessor-merge-to-merge proxy was
3h48m14s; the public PR was open for 53m30s. The independently mergeable am-f vertical
changed 12 files by +884/-89 and closed source completion, HIR publication, MIR/LLVM
termination, whole/per-unit code generation, execution, and object-cache parity together.
It stayed below the 1,000-line split-proof threshold.

The avoidable delay was an incomplete control-flow/type-inference Cartesian matrix. The
initial implementation treated completion mainly as a body-tail property, while the exact
contract also depended on whether a discriminator falls through, whether each alternative
produces a value or completes directly, whether its expected type is already known or inferred
later, arm source order, and whether a subtree already emitted an error. The broad owner gate
first exposed an eager-termination compatibility gap; post-open review then found unreachable
alternatives receiving an outer expectation and mixed branches typing differently by source
order. The coherent fix delayed only the necessary branch reconciliation, and the focused
closure found and removed one resulting diagnostic cascade on malformed children.

The durable rule is now in `CLAUDE.md`: before coding a control-flow/type-inference slice,
cross reachability, completion kind, expected-type availability, source-order permutations,
and subtree validity in the owner matrix. Runtime joins use only fallthrough alternatives;
discriminator-unreachable alternatives are structurally checked without an enclosing expectation;
reachable eager-diverging typed wrappers receive required late reconciliation without contributing
a runtime value. Delayed constraints retain the same diagnostic guard as immediate ones.
Required verification was slow but progressing: the final locked all-target Clippy took
11m56s, and several test binaries had their known long startup delay. Those timings do not
justify weakening the gate.

### PR #685 delivery retrospective

PR #685 was squash-merged as `257c704`. Its earliest-commit-to-merge proxy was 5h17m;
the public PR was open for 4h18m. The implementation changed 4 files by +1,042/-18,
including the complete compiler-only task-wait proof checker and its owner tests. The
vertical remained intentionally atomic because splitting proof state, control joins, or
TaskGet diagnostics could authorize an uninitialized task result or reject an outer proof.

The avoidable delay was review execution and checkpoint churn, not the final compiler fix.
The first host review spent its bound reading the large repository context and repeatedly
hit macOS `xcodebuild` cache errors. A later host pass found a real P1: join tokens were
keyed by incoming generation/epoch, so loop headers could allocate a new token on every
state change and miss the required fixed point. The fix made join generation/epoch tokens
stable by syntax site, then replaced an invalid owner fixture that tried to move a Task
directly out of a conditional expression with a legal branch/loop reassignment twin.

Durable rules: a review that has no verdict is never a clean result; inspect process state,
log growth, and the last completed action, then stop at the hard bound and rerun against a
small explicit complete patch. For control-flow token interning, keys that define a join
identity are syntax-site-only; incoming state may key Spawn/Wait/Err events but must not
create fresh identities for the same loop or branch join. Finally, every owner fixture must
be legal under the language's own Move/arena rules before it can test compiler-only proof
transport.

### PR #688 delivery retrospective

The am-u implementation interval exceeded the two-hour checkpoint target because the callback and
lambda lexical-depth matrix had to be expanded after independent review, while macOS test startup
and compilation runs intermittently produced no harness output. The implementation was narrowed to
one sema producer gate plus the existing resolver consumers; direct execution of the built owner
binary was used to distinguish a completed test from a stalled Cargo wrapper. The durable rule is
to preserve the stalled transcript, inspect the actual child process, and continue with the smallest
complete owner target instead of restarting a broad suite. The matrix now includes exact one-error
precedence, rejected-HIR absence, branch/loop/early-exit lambda depth, and qualified whole/per-unit
parity.

### am-p delivery checkpoint (2026-08-01)

The am-p checkpoint exceeded the two-hour implementation target because its body-independent
placement matrix spans fields, payloads, tuples, tagged values, function headers, imported headers,
and extern boundaries, while the existing sema predicates had to be compared exactly (including
nested-array and generic `ResponseBuilder` behavior). The ordinary PR gate also spent several
minutes in macOS `_dyld_start` before each integration binary produced output, although it kept
progressing and finished clean. The durable rule is to freeze each producer predicate from the
owning sema function before writing the validator, keep graph validity separate from placement
validity, and run focused owner tests before one monitored full PR gate. When a binary sits in
`_dyld_start`, inspect the child and preserve the transcript rather than restarting the broad gate;
if the next checkpoint still cannot be mergeable by two hours, split at the next independently
correct placement family and record the boundary here.

The am-p checkpoint was merged as PR #690. The required broad host preflight at
`90a30fa` reached its 900-second bound without a verdict, and the fresh independent adversarial
review found six valid closure issues: nested `array<string>` field placement was over-rejected,
generic `ResponseBuilder` producer validation disagreed between sema and am-p, body-only handle
types were admitted in declaration headers, an abstract `box<Param>` could pass the placement
predicate, shared tagged DAGs lacked completed-node memoization, and the owner negatives did not
prove graph-valid placement rejection. The owning ledger and closure matrix were updated first;
the working tree now closes each issue with producer-aligned predicates, graph-valid owner twins,
and a memoized shared-DAG test. The first post-open host continuation also found that the inline
struct alignment traversal had not shared the completed-node memo; the follow-up adds that memo and
routes the deep shared-DAG fixture through a struct field. Existing deep body fixtures now use a
source-nameable declaration return while retaining their body-produced command/HTTP owner expression.

The old host and independent transcripts are not attestations for the corrected tree. A later
independent post-open pass found that the ledger incorrectly called graph-valid
`Ty::DynArray(Scalar::DynArray(_))` producer-valid even though `scalar_to_prim` rejects nested owned
arrays; the correction rejects that shape and adds its owner negative. The final independent pass
also confirmed a real ownership gap: sema admitted `array<file>`/`slice<file>` declaration types,
but the generic dynamic-array Drop path frees only the buffer and leaks each File handle. The
current correction closes that producer/validator/codegen boundary by rejecting File collection
elements in sema and am-p, updating the ledger, and adding sema plus MIR negatives. It still needs
the full pre-PR gate and a fresh post-open review on the resulting SHA. That fresh preflight then
found one valid P1: `inline_structs_unaligned` did not traverse enum payloads, so an
`align(N)` struct embedded in an enum could pass placement although enum storage is inline and the
LLVM type cannot carry the custom member alignment. The current follow-up adds enum/DAG traversal,
checks direct enum payloads, adds graph-valid enum-field/payload negatives, and updates the ledger;
the follow-up passed focused MIR/sema/Clippy checks. The fresh preflight on that SHA then found one
valid P1: tuple placement admits deep-owned `array<string>` and `array<Move-struct>` elements while
the tuple Drop path freed only each outer buffer. The current follow-up routes every owned tuple
element through the existing recursive pointer-based destructor and adds a codegen owner test plus
ledger/type-doc coverage. The subsequent full independent preflight reported one P1 claiming that
`Option/Result<array<T>>` fields were rejected; exact source inspection and the named positive MIR
fixture show this is a false positive—the direct-only `array<string>` guard does not apply inside
the payload. The adjudication is recorded in `.git/align-independent-61e482c-pre.log`. The final
pre-PR gate passed on `71951e1`; its host-native post-open review reached the 900-second bound
after inspecting the validator, tuple Drop, and sema through `resolve_type`/`box`, so the transcript
is preserved. The narrow continuation fixed stale tuple-contract comments in `hir.rs` and the
LLVM tuple-layout setup, then a fresh independent preflight on `4664039` found two P2 closure gaps:
the `array<Move-struct>` tuple Drop path lacked a dedicated owner assertion, and those comments
still described primitive-only tuples. The follow-up adds both comments and a codegen test that
asserts element iteration plus both recursive element and outer-buffer frees; the focused owner
test passed before merge. Those pre-merge review records remain historical evidence; the landed
am-p surface is now the base for am-n.
Rebuilds and test execution on this macOS host may need `DYLD_SHARED_REGION=private` because some
Cargo-spawned Rust binaries pause in `_dyld_start`; inspect the child and preserve the transcript
rather than restarting the broad gate. At the historical am-p checkpoint, the next implementation
slice was am-n nominal/link on `feat/am-n-nominal-link`; am-n and am-h have since shipped.

The corrected am-p change is above the repository's 1,000-line split threshold (currently about
1,212 changed lines against `main`). It cannot be split safely: validator activation, the producer
contract fixes, graph-valid negative fixtures, four lowering-entrypoint parity, and the owning
ledger are one atomic boundary; an intermediate split would either publish an unvalidated entrypoint
or leave the reviewed producer/validator matrix without its required owner evidence.

### am-n shipped checkpoint (2026-08-02)

The am-n implementation shipped in PR #691 (`755cb9c`), after am-p shipped in PR #690
(`39f9c7d`). MIR now validates struct/enum
identity text, combined internal-name collisions, ASCII member names, repeated source-name
structural equality, tuple interning uniqueness, alignment, enum field bases, and link-library
names before all four HIR-to-MIR entrypoints copy HIR. Source-shape comparison uses an explicit
worklist plus bidirectional node correspondence, so header-mediated recursion and deep equal-shape
DAGs are stack-bounded and preserve sharing. The owner matrix is in
`validate_hir_tests.rs`: malformed metadata is graph/placement-valid but reaches canonical-empty
MIR through every entrypoint; equal source-shape and function-effect-origin twins are accepted;
the 4,096-node deep twin, later mismatch, and a shared-node correspondence conflict are covered.
The first independent preflight reached its 15-minute bound while inspecting the source-shape
memoization and downstream ABI consumers without returning a verdict; it was stopped with that
last useful checkpoint preserved. The author follow-up then added the shared-correspondence
regression and changed cache hits to restart once without memoization when the current comparison
has sibling or ancestor mappings, retaining the linear deep-duplicate path. A fresh independent
preflight after that fix returned `CLEAN` after inspecting the complete six-file diff, all current
type/scalar variants, malformed-HIR paths, all four entrypoints, and the downstream ABI consumers.

The focused source-shape owners and the full `align_mir` library suite pass after the cache
follow-up (44/44); `cargo build --workspace --locked` and locked all-target Clippy also pass.
`align_driver` per-unit codegen passed 9/9 before the cache follow-up, but its latest rerun reached
the test binary and then paused in macOS `_dyld_start`; `scripts/test-pr.sh` hit the same dyld
startup blocker in its first test binary and was stopped after sampling confirmed the stall. The
required benchmark row is implemented, but this host cannot run the release harness because
`llvm-config-22` is absent. The current diff is about 940 changed lines:
the validator, four-entrypoint owner matrix, shared-graph regression, and benchmark are one atomic
am-n boundary, so splitting it would leave an activated identity gate without its complete owner
evidence. The author self-review, pre-PR stamp, CI, post-open host/independent review records,
and final attestation all bind to the merged am-n SHA. The branch advanced through the completed
am-h slice recorded below.

### am-h delivery checkpoint (2026-08-02)

PR #692 completed am-h declarations and headers and was squash-merged as `f7ebcb4`. It activates
body-independent validation for
externs, imported functions, stored functions, `main`, locals, parameter modes, return-borrow and
return-region summaries, and structural drop-set records. Imported interface facts gain a
normalized `FnEffect`: a present compatibility-map entry is copied as-is, while an absent entry
becomes `Impure`. HIR retains that fact for validation and MIR strips it after validation so the
existing six-field imported declaration and its structural Debug bytes, interface bytes,
`impl_hash`, and cache identity remain unchanged. `FnOrigin` replaces the overloaded
`lifted_capture_count`/`exportable` pair and is derived before per-unit export decisions.

The public contract and owner evidence are recorded in the am-h ledger and implementation
closure matrix in `docs/impl/17-library-boundary-prerequisites.md`; the universal malformed-record
rules are in `docs/impl/19-hir-validation-ledger.md`. A fresh independent adversarial review of
the completed matrix returned `CLEAN`; its record is
`.git/align-am-h-matrix-review-2026-08-02.log`. The intended owner tests are
`malformed_hir_declaration_header_metadata_fails_closed`,
`main_header_abi_matrix_is_exhaustive`,
`valid_hir_declaration_header_preflight_is_mir_identity`, imported-effect normalization twins,
deep signature/summary twins, and the `mir-header-validation` benchmark row. The first fresh
independent am-h preflight found one valid P2 in the parameter-id mutation fixture; the baseline
now contains a valid non-parameter local so that mutation reaches the header validator. A second
fresh review found two P2s: capture-count rollback now truncates nested lifted functions, and the
matrix now covers duplicate declaration names, FnTy mode/summary fields, root order/range/
borrowability, drop-set ranges, and the full main/Error ABI matrix. The author self-review, matrix
review, pre-PR attestation, bounded host continuation, fresh independent post-open review, and CI
all completed on the committed PR head; no incomplete review attempt was treated as CLEAN. The
broad host review reached its 900-second bound without a verdict and remains recorded as
incomplete, while the final bounded continuation returned CLEAN.
The merged am-h Rust delta is 1,445 changed hand-written lines under `crates/`, above the
750–950 estimate and the roughly 1,000-line throughput target. The count includes the common
validator, its all-entrypoint mutation matrix, the expanded exact header/ABI matrix, the
producer/effect migration and every consumer, the distinct MIR imported record, and the
codec/producer owners. It cannot be split safely: a producer-only slice would need a temporary
old/new HIR representation, a validator-only slice would leave the overloaded origin pair, and a
MIR-only slice would publish an unvalidated effect; the reviewed am-h matrix therefore keeps this
as one atomic vertical boundary.

The standalone benchmark now checks and runs with the repository LLVM 22 path. Its release
`provenance` run reported `mir-header-validation 0.730 ms/valid+malformed-lower` for 257
functions, alongside the unchanged global-type, nominal/link, continuation, and interface rows.
The revised header owner test compiled but its macOS test binary again paused in dyld startup
(CPU 0, no output after sampling) and was stopped; this host limitation is recorded with the
successful compile and benchmark evidence rather than treated as a test pass.

Historical am-b1 checkpoint: PR #694 completed the dormant am-b1 body-core validator: statements, ordinary expressions,
calls, aggregates, tagged values, and structured control are checked through an explicit
child-first worklist without general public activation. PR #695 adds the dormant am-b2a
storage/vector/array records. The next slice is am-b2b1 for pipeline and array-view records;
am-b2b2 then adds templates, JSON, groups, and dictionary records. Request 6's scanner Copy
predicate is the one narrow pre-lowering safety exception and must be routed through the active
`hir_program_is_valid` gate; am-b3, am-b4, and am-c typed callable namespaces follow.

### am-b2b1 working checkpoint (2026-08-02)

The current `feat/am-b2b-pipeline-validator` branch extends the same dormant body worklist through
pipeline stages, fused terminals, array views/chunks, and `ArrayToSoa`; b2b2 records remain
fail-closed. The closure matrix is in `docs/impl/17-library-boundary-prerequisites.md`, and the
ledger split is in `docs/impl/19-hir-validation-ledger.md`. Owner tests are
`hir_body_validator_pipeline_stage_records`, `hir_body_validator_pipeline_terminals`,
`hir_body_validator_pipeline_array_views`, `hir_body_validator_pipeline_control_flow`,
`hir_body_validator_pipeline_deferred_b2b2`,
`hir_body_validator_pipeline_deferred_facts_are_not_consumed`, and
`deep_hir_body_pipeline_type_dag_is_stack_bounded`.

The test target compiled with `cargo check -p align_mir --tests --locked`, and focused Clippy passed
with `cargo clippy -p align_mir --all-targets --locked -- -D warnings`. The focused runtime test
invocation compiled but the macOS ARM test process paused in `_dyld_start` at 0% CPU for about one
minute and was stopped; it is recorded in `.git/align-am-b2b1-test-2026-08-02.log` and is not a
test pass or a CLEAN verdict. Do not rerun the same broad invocation solely because it stopped;
continue with the static checks and one bounded final review after the source/test diff is stable.
The current `crates/` delta is about 2,057 hand-written lines, above the ordinary 500-line target
and the 1,000-line split threshold. This checkpoint keeps it atomic because the range shares one
`derive_expression` dispatch, one child-ordering helper, and one source/stage/terminal type
thread; splitting those into temporary validator paths would leave an incomplete contract or a
second implementation path. The closure matrix and owner tests cover the whole closed range, and
no second broad review should be started merely because the diff is large.

### am-b2b2 working checkpoint (2026-08-02)

The branch now implements the next dormant body range through `ArrayDictEncode`. The shared
worklist validates exact template object/`PopComma` state, separate JSON Decode/Encode descriptor
walks, JSON document result contracts, scanner `Result<scalar, Error>` terminals, and all four
single-group source shapes plus the AoS-string fused multi-group shape. The authoritative closure
matrix is in `docs/impl/17-library-boundary-prerequisites.md`; the exact rows are in
`docs/impl/19-hir-validation-ledger.md`.

Owner coverage is `hir_body_validator_pipeline_template_json_group`,
`hir_body_validator_pipeline_template_json_group_control_flow`,
`deep_hir_body_pipeline_b2b2_type_dag_is_stack_bounded`, and the existing
`hir_body_validator_pipeline_deferred_b2b2` / deferred-facts owners. `cargo test -p align_mir
--tests --no-run --locked` and `cargo clippy -p align_mir --tests --locked -- -D warnings` pass.
The first ordinary gate reached the `align_mir` unit binary and ran 74 tests: 72 passed and two
positive fixtures were correctly rejected by the body-independent placement gate because they
published body-only `DynSliceArray`/`DictEncoded` types in function returns. Those fixtures now
evaluate the records as statements and return `Unit`. The follow-up focused owner invocation then
remained at 0% CPU in macOS dyld startup before printing a test count and was stopped; the host
attempts are recorded in `.git/align-am-b2b2-test-2026-08-02.log` and
`.git/align-am-b2b2-prepr-2026-08-02.log`, and are not runtime passes. Do not rerun the same host
execution solely because it stopped; retain the compile/Clippy evidence and continue with the
final static diff, commit, and one bounded review cycle. After the fixture corrections, the
targeted `hir_body_validator_pipeline_template_json_group` owner ran successfully (`1 passed`);
its `JsonValue` variant field bases are now the producer's cumulative `1,2,3,4,5` ordinals.

Against `origin/main`, the current range is 3,801 added and 73 removed hand-written Rust lines
(2,070/54 in `validate_hir.rs` and 1,731/19 in its owner tests). It exceeds the ordinary 500-line
target because this range shares one `derive_expression` dispatch, one child-ordering helper, one
descriptor worklist, and one pipeline source/result thread; splitting it into temporary validator
paths would leave an incomplete public discriminator contract. This exact count is recorded before
the PR gate.

#660's final verification records 48/48 `align_driver` `par_map` tests,
including 65,537-element worker-range tests for both materializing chunks and
direct chunk reduction, a chunk filter, a cross-worker i8 wrapping fold, and
backend-independent MIR-shape assertions. Codegen also has malformed chunk
`ParMap`, `ParMapReduce`, and filter-stage tests; the full bounded preflight,
workspace build, `scripts/test-pr.sh`, and locked all-target Clippy passed.
Review attempts are part of the record: a broad host review produced no verdict
within the 15-minute bound, and a narrower input attempt was stopped after its
process/log showed no useful progress. A compact diff review then found missing
worker-range materialization, filter-type, wrapping, backend-independent, and
malformed-MIR coverage; those findings were fixed and the final adversarial
review was CLEAN. At elapsed review checkpoints, inspect the process, log growth,
and last completed action first. Meaningful review work may continue until the
hard 15-minute watchdog; otherwise record the elapsed time and last action,
then narrow or rerun the review rather than treating elapsed time as a verdict.

After #660 opened, the final pushed SHA's broad host-native review also reached
the 900-second watchdog without a verdict; its last completed inspection was
the `align_rt_chunks`/`ParPool::submit_many` runtime path. The required narrower
host rerun and independent adversarial diff review found no actionable issues.
The final SHA's preflight attestation, Linux x86_64/ARM64 and macOS CI, and
post-open review status passed; #660 was squash-merged as `6153847`.

#662's measure-only probe was squash-merged as `f8ef4c7`. Its broad host review
did meaningful inspection for about five minutes but the wrapper emitted no
machine-readable verdict, so elapsed time was not treated as a clean result.
The review state and last useful inspection were checked, then a bounded narrow
host rerun and a fresh independent adversarial review both returned CLEAN. The
two SHA-bound logs were recorded before the `Post-open review` status was set;
the initial attestation failure was only because that post-open marker was not
yet present, and the rerun passed after the marker was recorded. This is the
operational rule to retain: when a review interval elapses, inspect process
state, log growth, and the last completed action first; allow meaningful work
to continue while the hard 15-minute bound has not been reached. At that bound,
stop the review, record the elapsed time and last completed action, and then
narrow or rerun it; never infer a verdict from elapsed time.

#664's invariant string-filter slice was squash-merged as `f775889`. The recognised
`where(fn s { s.contains(NEEDLE) })` form now lowers to a compiler-generated
`FilterStrContains` stage and calls the existing `str_contains` ABI in both stable
count/scatter passes. The focused `align_driver` `par_map` suite passed 49/49;
the new driver test pins survivor order, MIR shape, both kernel names, and the
ABI call, while the malformed-MIR codegen test pins a `CodegenError` for a
callable-bearing string stage. Workspace build, `scripts/test-pr.sh`, and
workspace all-target Clippy passed; required Linux x86_64/ARM64 and macOS CI
also passed.

The review record is intentionally explicit. The broad preflight reached the
15-minute watchdog without a verdict, and bounded commit-scoped follow-ups also
ended without a verdict after meaningful capture/ABI inspection; the final
narrow independent preflight returned CLEAN. After opening #664, the broad
host-native review reached its 900-second watchdog after inspecting the sema
parallelizability boundary; a narrower host attempt also reached its bound after
checking the LLVM branch, `AlignStr` ABI, HIR recognition, and stage matches. A
final narrow host check returned no findings, and a fresh independent post-open
review returned CLEAN. These are not elapsed-time verdicts: at each checkpoint
the process/log/last action was inspected, meaningful work was allowed to
continue, and only the hard bound caused a rerun or narrowing.

The task-group record probe in `bench/task_group/` compares the shipped split env/result/error
allocations with one-record tight and cache-line-separated padded controls over the same registration ABI.
The recorded native Apple Silicon command was `LIBRARY_PATH=/opt/homebrew/lib:/opt/homebrew/opt/openssl@3/lib:/opt/homebrew/opt/llvm/lib TRIALS=30 REPS=7 bench/task_group/run.sh`;
the harness reported eight runtime workers and covers the one-task caller-only path plus
worker-derived scheduler/batch boundaries. Packed-tight is primarily allocation-call/record-shape
evidence because the split bump-arena allocations may already be physically adjacent; the padded
control measures total padding/alignment and cache-footprint cost rather than isolated false sharing.
Error-slot rows use successful task bodies; the separate smoke exercises the actual error return.
The run improved large groups but did not meet the median 10% gate across small and error-slot
groups;
repeatability still requires multiple fresh invocations. The harness drains late pool helpers between
repetitions outside the clock. Padding was materially slower on large groups. Zero-task groups are
rejected because they contain no record to measure. The production
task-group ABI and allocation shape remain unchanged until that gate is met.

The last recorded full workspace run before #636 was 2748 passed / 0 failed,
with clippy clean. #636 then passed focused Linux runtime/process tests, clippy,
and the macOS release-build CI path. A local `cargo build --release --workspace`
was rerun after #636. #637-#644 passed their focused and PR CI gates.

## Next work

C-B closes direct, captured, imported, and function-value return provenance together with
path-selected Move-return cleanup and shared/exclusive borrow consumers through L2e. The next
independent implementation waves are F-A native resources (L3), F-B region materialization
(L4 + L6), and F-C static artifacts (L5). Each reopens its owning closure matrix before coding;
F-D package integration begins only after F-A/F-B and the complete F-C prerequisite gate.

The query-centered `pkg.db` design and its general library-boundary prerequisites are specified in
`docs/impl/pkg-design/db.md` and `docs/impl/17-library-boundary-prerequisites.md`; the feasibility
review and revised delivery gates are `docs/impl/18-pkg-db-review.md` (#666). L1a is complete: the
canonical recursive Drop plan and sound `Option<string>` struct fields are implemented with
tag-tested Drop, move/null handling, focused analysis coverage, and an alloc-count benchmark.
Partial Move-leaf replacement fails closed without an exact drop-old lowering, and fixed-array
field reads admit only Copy leaves or a borrowed `string` view; replace the whole struct/element for
larger Move leaves. L1b is complete through the authoritative §8.3 closure matrix: L1b-a (#668)
admits one direct existing-`Scalar` Move payload per tagged arm; L1b-b (#669) adds multiple Move
payload partial construction and uniform ownership; L1b-c (#670) adds tagged-in-tagged type
representation plus the exact `Result<Option<Output>, DbError>` acceptance. Dynamic
path-selected return cleanup bits remain L2; L1b accepts only values proven free-standing by the
current ABI rules. L2a is complete in #672. L2b-a1 named/direct/imported parameter-root inference
with conservative aggregate and indirect unions is complete in #673. Its hand-written diff exceeds
1,000 lines because producer inference, consumer import
validation, whole/per-unit serialization parity, source-order reachability, and their owner tests
form one compatibility boundary: splitting them would either publish unvalidated provenance or
consume provenance whose producer semantics are not yet closed. The closure matrix therefore
keeps this as the smallest independently correct vertical slice. The final local provenance
preflight also carries checker-owned break acceptance into HIR so region-rejected recovery syntax
cannot manufacture an effect, escape, ownership, provenance, or MIR loop-exit edge. The final local
review then reopened the closure matrix for termination inside an accepted break payload:
effect inference now stops every eager/source-order boundary action after termination, and MIR
emits an outer result/cleanup edge only from a reachable continuation. Copy `str`, owned `string`,
mixed control-flow, process termination, and malformed-HIR owners cover the correction. The final
independent review found one further pipeline timing gap, so the matrix was reopened before more
code: pipeline sources now form first, stage Copy captures snapshot once at their written
positions, explicit terminal arguments follow, terminal captures snapshot last, and callback
actions begin only after every operand falls through. Sequential, parallel, zip, and JSON-scanner
lowering reuse the preheader operands; an owned temporary source is hidden-owner guarded so a
later terminating operand still cleans it exactly once. MoveCheck retains captured-view owner
dependencies across `init`, EffectScan separates formation from action, and structural
EscapeCheck/finalization still visit dead HIR without joining reachable state. Focused sema/MIR,
lambda, borrow-liveness, map_into, zip, and JSON scan-reduce owners are clean. The refreshed local
provenance benchmark on Apple Silicon reports 1.916 ms/check and 22,848 interface bytes for
summary inference, plus 1.878 ms/import for semantic import validation. The final owner review
also closed source snapshots across later terminal arguments: direct, zip, scanner, and
borrow-preserving `if`/`match` sources retain selected owner roots until action; a Move-`else`
success payload and a value-producing loop instead transfer and null their old container/source.
Terminating return and outer-break paths remove the analysis snapshot from current and saved loop
states.
L2b-a2-s is complete in #674, L2b-a2-ac in #675, L2b-a2-am-g-t in #677, and the am-r design gate in
#678. Am-g-t validates only
the global type domain, concrete roots, references, template reachability, and inline cycles before
direct handcrafted-HIR lowering. The am-r ledger's first
fresh adversarial review was not clean. The revised draft addresses those findings: all 239
expression and every helper
discriminator have exact rows in `docs/impl/19-hir-validation-ledger.md`; native ABI,
generated-identity, corrected source/header origin facts, and the dependent PR order are recorded.
The pre-PR author consistency pass and adversarial preflight were clean. Post-open review of PR
#678 reopened the am-w closure matrix: a failed drained Wait could be hidden by a later empty
successful Wait, and seven adjacent ledger contradictions also required one coherent follow-up.
The first revised-HEAD review then found five closure details: depth preflight had to precede every
body pass, differing control states needed an exact Task-proof remap, task-error mirrors retained
the obsolete i32-code ABI, two extern cross-class collision rows were absent, and optional probe
names were over-reserved. The next independent pass found one final closure omission: a differing
am-w join allocated fresh generation and proof-epoch tokens without explicitly assigning them as
the active state. That assignment and its post-join Wait/TaskGet owner evidence are now authored;
the following host continuation then reopened the matrix for a missing loop-header/backedge fixed
point and a conflicting Wait-coverage sentence. The ledger now uses stable syntax-site tokens,
computes loop headers to a monotone fixed point before joining breaks, and keeps established
completion across a later unhandled no-task Wait. The final review, repository gate, and merge
completed in #678. PR #679 completes am-d with exact producer-valid depth owners and
stack-safe checked-HIR/type-DAG consumers. PR #681 completes am-e by restricting
the entry producer to Unit, signed i32, or exact `Result<(), Error>`, preserving exact whole/per-unit
and ThinLTO C entry behavior, and rejecting malformed MIR entry ABI before LLVM construction.
PR #683 completes am-f by rejecting bare non-Unit returns and reachable fallthrough
before HIR publication/MIR lowering while preserving typed and proven non-fallthrough paths. Its
focused sema, MIR, LLVM, and whole/per-unit driver matrices pass, including raw/optimized IR,
execution, and cold/hot object-cache parity. The reviewed order is am-w
outcome-sensitive task-wait dominance (#685), am-v native output-buffer local/mutability (#686),
#688 completes am-u lexical extern invocation, #690 am-p placement, #691 am-n nominal/link, and
#692 am-h declarations/headers, #694 am-b1, #695 am-b2a, #696 am-b2b, commit `af5e17a` am-b3,
#699 task-wait replay, #700 body-fact replay, merged am-b4 activation PR #706, merged #702 am-c1,
merged #705 am-c2a1, and merged #707 am-c2a2a. C-A subsequently closes the canonical callable
vertical through c3. The remaining L2 work is C-B: return-provenance closure through b, then cleanup
and borrow closure through L2e. Am-c follows am-b4 because it consumes body-validated callable facts.
The former thirty-two-L2b/thirty-six-L2 PR schedule is retired; the acceptance cells remain.
The final author pass found one additional hidden dependency before review convergence: imported
effect bits previously arrived only through the sema call's out-of-band map and did not survive in
checked HIR, so a later handcrafted-HIR preflight could not replay parallel purity independently.
Am-h now owns one normalized imported `FnEffect` header fact (an absent compatibility-map entry
normalizes to fail-closed `Impure`), and am-b4 replays body/call-graph effect inference, every
concrete function-value/projection/join cell, and parallel eligibility before am-c consumes callable
facts. Am-h converts that validation-only fact to a new effect-free MIR imported-declaration record
with the same six existing fields and structural Debug bytes before codegen, so MIR `impl_hash`,
interface bytes/hash, and cache behavior stay unchanged. This is an internal checked-HIR transport
change only; the interface already carries the same effect fact.
The closing review also found five semantic producer holes that must land before header/body
validation. Am-e restricts no-arg `main` to Unit, exact i32, or
`Result<Unit,builtin Error>`: other returns were accepted by sema but emitted an invalid external
C `main` ABI. Am-f rejects bare return and reachable absent tails in non-Unit functions; before #683
both reached MIR/LLVM as `ret void` under a value-returning signature. Am-w replaces task
wait-state traversal order with generation- and Wait-id-sensitive successful-wait
dominance across `if`, direct `match wait()`, direct `wait() else`, every reachable loop break,
early exits, nested groups, Spawn reset, infallible Wait, and direct fallible `wait()?`; its
fallible-Wait Result carries compiler-only group provenance through bare locals, copies,
reassignment, `map_err`, and value-producing control joins. Every earlier Wait for the same drained
task generation must also resolve Ok: a later empty Wait cannot hide an unresolved or failed first
result. Err invalidates every Task/Wait proof it covered. Every Spawn advances the current
generation and stales prior Wait proofs; with an unresolved Wait it also invalidates covered Tasks,
while after success one later Wait reauthorizes old and new handles. The settled stored-result form that exhaustively handles every
covering Wait Result before reading a task remains valid. Calls, returns,
captures, imported values, and aggregate reconstruction do not synthesize provenance, and every
Spawn stales all WaitProof aliases for that group. Each Move Task handle separately carries its originating
group through transparent local/control transfer. Nested entry preserves outer facts; inner Wait
cannot authorize an outer Task, while handling an outer Wait Result inside the inner group updates
the outer fact. Exit clears proofs naming the inner group, including proof on its Result block
value, but preserves outer proofs; handling that cleared Result outside cannot authorize an outer
Task. Current primitive `TaskGet` is non-consuming and repeatable, and its rejection diagnostic
uses the originating group's fallibility rather than the innermost group. The current
match/loop gaps can otherwise admit `TaskGet` from an uninitialized result slot. The final
post-review closure assigns stable tokens per syntax site, computes each loop header from entry and
reachable fallthrough to a fixed point before joining breaks, and preserves an established
completion across a later unhandled no-task Wait. This prevents an earlier iteration's unresolved
or failed Wait from disappearing while keeping initialized slots readable. The next independent
pass found the same distinction missing in join remapping: a TaskProof now survives an unresolved
Wait only when that predecessor already completed its current generation; otherwise coverage
clears the proof. The TaskGet discriminator and straight-line/branch/loop owner rows carry the same
rule. Checkpoint `7247fc6` contains the first compiling vertical implementation. The am-w implementation is intentionally one vertical: splitting group state, proof
transport, joins, or TaskGet diagnostics would leave an intermediate compiler that can authorize
an uninitialized task slot or reject a valid outer proof. If the implementation exceeds the
500-line target, the split-proof exception is justified by this single safety invariant; the
formation, control, ownership, and whole/per-unit rows must land together. Am-v (#686) required a
bound `mut Buffer` local at ReaderRead, ReaderReadLine, FilePread, UdpRecvFrom, and CryptoRandom;
those five producer paths now reject equal-typed temporaries and immutable buffers even though the
runtime writes through them.

The 2026-08-03 am-w continuation is the stable-identity/replay-safety correction. It replaces the
Span-keyed recursive task-wait replay with one checked-HIR preorder `NodeId` map, explicit block/
statement/expression/branch/loop continuation work items, a checked-HIR-derived work bound, and
an explicit loop-header fixed point that fails closed on exhaustion. The current source diff is
about 1,800 changed hand-written lines, above the 1,000-line split-proof threshold. It remains one
vertical because the three changes are one safety invariant: a NodeId-only intermediate leaves
recursive replay exposed to stack overflow, a worklist-only intermediate still aliases duplicate
Span sites, and a fixed-point-only intermediate can replay the wrong identity/order. Splitting any
one would require a temporary parallel proof path or would leave the accepted task-group contract
without its owner matrix. `cargo check -p align_sema` and `cargo test -p align_sema --lib --no-run`
pass on the current tree; the prior narrow test invocations reached the macOS `_dyld_start`
startup stall with 0% CPU and no test output, so they remain INCOMPLETE rather than CLEAN and are
not being rerun unchanged.

The fresh bounded adversarial review then returned INCOMPLETE (never CLEAN) with no P0/P1 and
these P2/P3 findings: the old “over 64” test did not exercise a >64 convergence; two stale owner
names were absent from the tree; duplicate-Span evidence covered only Wait; the replay bound was
only a stack-height check; and the matrix overstated O(1) invalidation and byte identity. The
coherent follow-up renamed the guard owner to its actual depth-derived contract, removed the stale
names in favor of the real driver/sema owners, added group/Spawn/Err/join/loop duplicate-Span
coverage plus empty-body budget coverage, bounded total work by checked-HIR record count, and
defined semantic state equality and live-entry invalidation costs. No second broad review is
started for these non-public-contract P2/P3 fixes; the owner compile/static gates are rerun once.
The final pre-PR adversarial pass then found one unsatisfiable owner assertion (the duplicate-span
fixture had six Spawn sites, not eight), a loop matrix sentence that still implied more-than-64
convergence, and Span-keyed TaskGet diagnostic deduplication. The first follow-up keyed diagnostic
suppression by TaskGet `NodeId` while retaining its source span for the emitted location, corrected
the owner to six sites, and removed the more-than-64 claim. Its targeted closure then found that the
loop fixture still had no header-state change. The final fixture adds a loop-body Spawn before the
branch/break join, so the header state changes and reaches the stable tokenized join; the owner now
has eight structural Spawn sites. No unrelated broad review or test rerun is started.
The post-open PR #699 review cycle returned INCOMPLETE rather than CLEAN: the host-native reviewer
stalled after roughly ten minutes while inspecting the large diff and its bounded log was preserved;
the independent reviewer found P2 token exhaustion aliasing, a matrix overclaim that treated
dispatcher steps as a total CPU bound, and weak direct owner evidence for loop/header convergence.
The coherent follow-up changes token allocation to checked `Option<Token>` with analyzer fail-closed
propagation, narrows the bound wording to dispatcher steps plus explicit input/live-entry costs,
adds a token-exhaustion owner, and directly observes changed incoming loop Spawn tokens and each
duplicate loop-header join site. Per the review policy this is one finding-closure follow-up; no
second broad review is requested for these non-public-contract fixes.

The prior am-b4 sema-only checkpoint on top of merged am-w #699 (`9e2d615`) was:
`checked_hir_body_facts_are_valid` clones checked HIR, resets return/Drop/assignment/effect facts
through the bounded event walk, replays task-wait, return-provenance, MoveCheck, EscapeCheck, and
effect solving, then compares the published facts without mutating the caller or creating a new
function-type topology. The MIR shared gate was not activated in that checkpoint; the current
activation adds its four-entrypoint empty-result and identity owners. The focused replay owners
`checked_hir_body_fact_replay_rejects_stale_producer_facts` and
`checked_hir_body_fact_replay_covers_cleanup_and_function_effects` pass; the closure matrix is in
`docs/impl/17-library-boundary-prerequisites.md`.

The post-open review of PR #700 then found stale am-w owner placeholders, a premature MIR-gate
statement, combined rather than per-field stale-fact mutations, missing rejected-input immutability
checks, and one P1 producer/replay mismatch for omitted imported return provenance. The follow-up
replaces the owner names, qualifies the gate statement, adds the HIR-only
`return_provenance_known` presence bit so compatibility omissions retain the producer fallback,
adds absent/explicit-`None`/`Roots` × effect-seed replay coverage, isolates each negative fact, and
checks rejected-input immutability. The predicate's direct contract remains
prevalidated-HIR plus depth/panic containment; the current MIR activation adds the structural
metadata rejection and canonical-empty boundary. Focused replay, declaration-header, check, and
Clippy gates pass;
the original broad host wrapper's human no-action text lacked its required machine marker and stays
recorded as INCOMPLETE rather than CLEAN.

The current follow-up is above the 1,000-line split threshold because it replaces the replay's
pointer-sensitive reconstruction with numeric occurrence frames, adds exhaustive child-edge
teardown, and closes the deep/duplicate-span/Cell/FnTy owner rows in one safety invariant. Splitting
the frame protocol from teardown would leave either an accepted path with recursive Drop or a
temporary parallel replay path. This boundary proof is recorded in the am-b4 matrix; am-c, L3
resources, and public `pkg.db` remain separate.

The LLVM 22 toolchain is available at `/opt/homebrew/opt/llvm`, and focused task-group tests pass
with `LLVM_CONFIG`/`LIBRARY_PATH` set. The ordinary `scripts/test-pr.sh` gate remains INCOMPLETE
because the `align_codegen_llvm` and `align_driver` unit-test binaries hang in macOS dyld startup
before listing tests; this is an environment/toolchain execution blocker, not a compiler test
failure, and the transcripts are preserved in `.git/am-b4-test-pr-dyld-incomplete.log`. Am-u rejects
extern declarations as first-class function values and requires direct or non-escaping named
pipeline/reducer/sort invocation to be lexically
inside `unsafe`; current HIR function values cannot carry visible unsafe-call permission. These are
independently correct producer fixes, not malformed-HIR heuristics.
The same review found a separate stack-safety closure. The constructor/parser audit fixes 259 as a
proved conservative checked-HIR producer ceiling; it does not claim a syntactic program reaches the
minimum possible ceiling. This avoids relying on apparent maximum-depth shapes that the shared
parser recursion counter or template-hole grammar cannot construct. Am-d accepts handcrafted depth
259, rejects depth 260 before semantic consumption, and converts producer replay to explicit
enter/exit worklists. All four MIR-lowering paths use a heterogeneous worklist for strict eager
spines, immediate parent-owned continuations for multi-child ownership boundaries, and small
out-of-line structured-control and specialized-operation frames bounded by the fixed 259 ceiling.
The outer dispatcher reaches the existing file, reader, array-builder, process-command, path,
regex, and HTTP helpers without retaining the giant recursive frame; strict string wrappers stay
on the eager worklist. Operation-specific MIR assertions cover mixed eager/call, string trim,
producer-valid path/regex chains whose individually owned temporary strings end in an owned return,
self-buffering reader plus file/command/HTTP producers whose every Move-valued node carries its
individual-allocation fact, a producer-valid `Result<str, Error>` `StrBytes`/`BytesAsStr`/`Try` cycle,
template spines cloned from their hidden views into an owned return, file create, array-builder push,
process-command construction, HTTP request construction, block/statement sentinel reachability,
proportional `if` and binary/wildcard `match` CFG evidence, independently counted `else` and
short-circuit branches, loop, and arena/task-group boundary owners on the 2 MiB test stack. Result
construction and inspection instructions are checked against the exact Ok/Error payload and bool or
unwrap destination type, and the bytes/string owner checks the exact builtin Error declaration.
Reachability replay propagates strict-child divergence
through transparent stage/template records before visiting later siblings and preserves the
non-fallthrough status of `process.exit`/`process.abort`. The final raw/optimized
LLVM owner uses the same accepted record boundary, and the deep owned-leaf codegen owner emits the
actual recursive Drop on that same stack.
Am-g-t intentionally
admits every finite header-mediated type DAG with no depth cap, so
am-d also establishes one common iterative type traversal. Am-p placement, am-n shape comparison,
am-h signature/summary validation, am-b1–b4 body type relations, and am-c canonical codecs each
inherit deep valid and deep malformed-sibling owners. Am-c preserves depth-first first-visit bytes
with explicit work items. Am-d first inventories and closes every already-active recursive type
consumer from HIR through LLVM: Drop/Move, borrow/region/escape/ownership predicates, MIR
type/layout conversion, and LLVM struct body/layout. Deep acyclic-inline and header-mediated graphs
run through stored headers, parameters, locals, returns, aggregate fields, Drop, borrow/region, and
raw/optimized LLVM roots on the 2 MiB test stack. Am-d may exceed 1,000 hand-written lines because
splitting any current body/type consumer or the common type visitor would merge an accepted graph
that can still overflow a later consumer; the atomic vertical and its reason are recorded in the
plan.
L2b-a2-af then adds validated
fixed-array formation and exact/dynamic element and element-field
selection/replacement on that completed substrate. L2b-a2-ar closes retained storage across
non-fixed index/range, `ArrayChunks`, and `HttpRespHeader` actions. L2b-a2-ap separately closes
pipeline `Project`/`WhereField` under an explicit stage/terminal state machine. L2b-a2-t completes
user-sum/`Option`/`Result`, `match`, `else`, `?`, and `map_err`. The public interface remains the L2a
parameter-index summary, so a single aggregate actual deliberately remains conservative. Unknown
extern and indirect targets retain the all-compatible-input fallback.
The ac implementation uses one required-child continuation protocol across every eager MIR parent:
a terminating child may not feed a typed operation, lower a later sibling, append an action, build
a helper CFG, allocate, or transfer cleanup. Direct tail delegation is permitted only when the
caller performs no later work, and its first non-tail parent must apply the guard. The owner matrix
requires every recursive child-lowering entrypoint (`lower_expr`, borrow/block wrappers, consumed
argument lowering, and delegating helpers) to be classified, plus representative
fixed/non-fixed/index/native/call/aggregate/control/pipeline no-action tests and whole/per-unit
parity. Normal driver input is semantically checked HIR; exhaustive handcrafted-HIR action
metadata validation is the following am boundary.
Block reachability is an O(1) `BuilderCtx` bit maintained by `new_block`/`terminate`, not a CFG scan
after every child. Pre-child synthetic-owner, cleanup-bit, explicit-region setup, and infallible
checked-HIR type/layout fact derivation remain permitted when the terminating edge owns any
required cleanup. Fallible compiler-table/path lookup and every parent action wait until all
required children fall through; no post-child transfer/action may use non-fallthrough as success.
The L2b-a2-ac implementation is complete on its branch: the caller-local required-child protocol
covers expression, statement, call, aggregate, native, index/range, and pipeline lowering; all
value-carrying joins reject an unterminated zero-predecessor block. Missing indirect-function
signatures and invalid element-field paths terminate without a parent action as narrow defense in
depth, not as an exhaustive malformed-HIR contract. The author-side matrix-to-diff pass classifies
every recursive lowering entrypoint as an immediate required child, explicit predecessor-backed
control continuation, or side-effect-free tail delegation. The focused whole/per-unit MIR and
codegen owners pass. Positive MIR/codegen owners also preserve origin-compatible indirect and
`map_err` callback signatures and the settled owned-`string` field to borrowed-`str` read. The
final revised adversarial preflight is clean. The post-open independent review found that dynamic
and SoA element-field lowering still emitted `SliceLen` before its narrow malformed-path check;
the follow-up moves that check before the first length/field action and adds fixed/dynamic/SoA
negative owners. The refreshed Apple Silicon provenance benchmark
records 3.136 ms/check and
22,848 bytes for summary inference, 1.835 ms/import, and
`mir-continuation-lowering` at 1.105 ms/lower over 2,561 basic blocks. The passing ac depth owner is
the end-to-end `within_limit_chain_compiles_and_runs` test; the broader `expr_depth` binary
currently reproduces the same sema-time `deep_within_limit_expression_is_accepted` stack overflow
on unchanged `origin/main` under local Rust 1.96.1 and is recorded as a baseline failure rather than
a clean ac gate. The ac debug `lower_expr` frame is 75,808 bytes versus main's 77,168 bytes, so the
slice improves rather than consumes MIR-lowering headroom.
The hand-written ac diff is roughly 1,700 lines. It cannot split safely by expression family:
landing any subset would retain the same reachable placeholder operand while knowingly leaving
another eager parent able to append after termination. Reachability state, the caller-local guard,
every recursive parent family, and their whole/per-unit tests are therefore one compatibility
boundary and the smallest independently correct vertical. A fresh adversarial preflight found
that the original matrix also promised fail-closed checks for every handcrafted-HIR action family
while the implementation only hardened the two lookups touched by ac. The matrix was reopened:
the broad contract was first split as L2b-a2-am-g/am-b. A second revised-matrix review found that
action-only validation still omitted callable symbol uniqueness and universal
`Expr.ty`/operator consistency. Implementation then proved the global half itself too broad: the
combined type plus nominal/link checkpoint measured 1,535 changed hand-written lines. Type-domain
validation has no atomic dependency on nominal/link validation, so am-g-t was the only
authorized next slice at that checkpoint. It returns a canonical all-empty MIR program only for an invalid global type
domain and leaves every placement, nominal, namespace, declaration/header, and body predicate
unchanged.

The broader remainder was deliberately reopened rather than patched again. Reviews found that a
body-free validator must not claim body-derived Drop facts; inline-layout cycles must be separated
from valid header-mediated nominal recursion; generic-template roots must remain producer-compatible;
and current HIR cannot distinguish source functions from generic monomorphs. Am-h therefore replaces
the ambiguous `lifted_capture_count`/`exportable` pair with one required `FnOrigin` record for source,
monomorph, and lifted functions; exportability is derived from source entry/visibility flags.
Post-open review then
found two more missing invariants: graph-valid types still need exact per-position producer
admissibility, and callable validation must cover logical runtime lookup keys plus body-generated
`$fnval`, `$clos`, task-trampoline, and parallel-kernel identities. It also found that rejecting
source-accepted exact compiler/runtime spellings as malformed HIR would be a hidden semantic
change. The completed am-r ledger preserves those spellings by separating typed program, runtime, and
generated call registries; proposes injective compiler-owned identities for non-exported Align and
private generated helpers; inventories the 277 existing runtime lookup keys, all 239 `ExprKind`
variants, and every helper discriminator. The revised native ledger promotes the four previously
codegen-selected AEAD symbols to typed keys, for 281 keyed records. Five always-built unkeyed
runtime records make the fixed compiler registry 286 entries. Four `alloc-count` and four distinct
`par-map-probe` exports extend only the verification-time maximum export table to 294; their names
remain ordinary program/extern/export spellings, and probe-feature runtime fixtures never link user
artifacts. Runtime feature selection changes no compiler input, source acceptance, or cache
identity. `task-group-probe` adds no unmangled export. Exact LLVM
types/attributes and verification presence policy live in
`docs/impl/20-runtime-abi-ledger.md`. The completed ledger
also records every placement predicate and gives every body discriminator an envelope/child/type/
ownership row in `docs/impl/19-hir-validation-ledger.md`; any body failure returns the same
canonical all-empty program as a global failure. Concrete MIR call-target types, structural
generated-identity bytes, and semantic/byte goldens are now recorded in the am-r ledger. Body
construction shipped as three dormant exhaustive validator PRs and one atomic activation PR, so no
partial malformed-HIR claim was exposed. Historically, #678 fixed the then-current
twenty-three/twenty-seven counts, which later expanded to thirty-two/thirty-six. Both counts are
historical; remaining work now uses capability waves.

Am-g-t's type-domain implementation is preserved separately. The split applies the existing
review-size and closure-matrix rules; it does not justify a new process rule.
The final L2b-a2-s vertical is approximately 1,900 lines because its adversarial review required
malformed constructor/read/write fail-closed validation, common eager-child source-order snapshots,
snapshot-generation invalidation, checked-expression identity, action-boundary validation, and
discriminating deferred-array/liveness owners in the same PR; separating any of them would publish
an under-approximating or dangling fact.
Its final local provenance benchmark reports 3.147 ms/check, 22,848 interface bytes, and
1.844 ms/import on Apple Silicon.
Do not begin a safe SQLite/PostgreSQL driver or add database-named compiler variants before L1a–L7
are complete. The completed L2 cells run through c3. The remaining cells close in the C-B
direct/captured return-provenance, cleanup, and shared/mutable-borrow capability wave. These are
acceptance cells, not a thirty-six-PR schedule. After L2, L3 resources, L4 regions, and L5 static Query/command artifacts may proceed
concurrently; L6 follows L4 and L7 closes the integrated generic surface. No safe driver begins
before the complete prerequisite gate. L2 includes contextual
parameter parsing, all-peer mutable-borrow alias checking, drop-old replacement, target-relative
capture provenance, and the dynamic Move-return cleanup ABI. L3 includes a producer-owned linkable
Drop thunk and root-only raw transfer. L5 permits exactly one whole-body static constructor per
uniquely named descriptor item and uses tagged file/inline source identity plus exact per-driver
checked-metadata Missing/Present manifest state. D3's migration-backed SQLite prepare uses the canonical
validated numeric catalog. Query and command share the L5/D1 statement artifact and generated
binder. Checked metadata is per permitted driver. Option API ownership is D1/D2/D4/D6/D7/D12, with
D9 completing deadline enforcement/native cancellation cleanup without a v1 public cancel
resource. PostgreSQL Text `bytea` uses exact hex while raw bytes are Binary-only; cancellation must
resynchronize or close before connection reuse; and Query-less validation uses no fabricated Query
identity. `resource.borrow` is public safe ownership-only access while raw forms stay
declaring-subtree privileged. `db.exec_result` is exactly the Copy affected-row record. Every D11
live command has explicit entry/catalog/driver/target inputs. D12 uses exact `SchemaRef`/`TableRef`
and complete flat Column/Key/Index/Query records, with exact detail/discriminator projection,
Unknown-state and Summary→Parameter→Column ordering, ordinal/digest semantics, and declaration-order
U+0000 rejection precedence for identifier components. Duplicate constraint names use a canonical
`key_ordinal`; its complete key signature includes action/deferral/validation fields. The L5/D1
artifact has an exact top-level/nested canonical codec, checked-in Query/command byte+digest
goldens, structural reachable-definition Params/Row fingerprints, every binder/decoder ABI version,
and a producer-owned QueryMeta plan. D12 owns the materializer thunk ABI/code and descriptor-header
version with its first consumer. D2 owns SQLite's single active-execution lease. D3/D5 use
the exact derived metadata path, fail-closed canonical JSON/identity codec, explicit schema ID for
mutable targets, and independent goldens; D3/D11 share the versioned migration/schema-identity
codec. D0 records the actual
engine/version origin/nullability evidence; D3/D5 own fail-closed driver matrices, ambiguous evidence
remains `Unknown`, and runtime NULL guards remain mandatory. The first public database release gate is
driver-relevant D1–D12: D4 merge/release requires non-skippable provisioned PostgreSQL CI; D11 uses
the exact atomic-default/dirty-forbidden migration contract; and D12 returns flat metadata/plan
records into an explicit region. D13/D14 remain committed additive work. Native feasibility probes
may run independently but create no public API.

The capture-context, threshold, test-policy, direct integer transform-reduce, queue-publication,
focused-verification, low-lock task-group, staged-map, and body/byte-aware grain slices are
shipped in #643, #644, #646, #647, #648, #649, #650, #651, and #652. #653 shipped stable
callable scalar `where` compaction, and #654 shipped the task-group record probe. The probe
measured packed-tight and padded controls without changing production behavior; the cross-size
gate was not met. The scalar width/stride measure-first probe covers `i8`, `i32`, and `i64`
fused/materializing maps around the runtime floor; #657 also records runtime-only aggregate-like
16/32/64/128-byte record strides with full-output checksums, without changing production behavior.
Other-host aggregate cost sweeps and any broader width/aggregate retune remain
separate follow-ups. The merged #660 chunk-source
range path intentionally retains the `chunks` header allocation. The measure-only
`bench/par_map/run.sh chunks` probe on Linux x86_64 used symmetric validation in both
timed arms and showed 1.249x–1.336x versus the allocation-free cursor control across
two final invocations; an earlier one-sided-validation result was rejected as a timing
artifact. This earns an end-to-end no-header design measurement, but not a production
allocation-removal change by itself. Review of the harness also found that a failed ABI
assertion could leak the runtime-owned header buffer; the follow-up uses an RAII cleanup
guard, explicit count bounds, checked pointer arithmetic, and reran the probe plus
probe-feature Clippy clean. The no-header design remains consumer-gated, and no
new compiler widening is justified until the cross-host/aggregate measurements
earn a concrete consumer.

Consumer-gated deferrals that remain intentional:

- Fully escaping function values wait for a consumer and a settled heap-owned
  environment/drop model.
- `std.process` binary capture (`run_bytes`) waits for a binary-output consumer;
  see `docs/impl/std-design/process.md`.
- Top-level `array<str> := json.decode(...)` waits for a result representation
  that carries the input region. Struct fields of `array<str>` already ship;
  see `docs/impl/core-design/json.md`.
- The first pkg.web consumer application remains a separate, owner-scheduled
  task.

The integration-test execution-policy review is implemented on `main` in #646 and its
focused-test-selection clarification shipped in #649.
`scripts/test-pr.sh` is the bounded ordinary gate: workspace build,
deterministic non-runtime library tests, and the M0 compile/link/run smoke.
CI no longer runs the full workspace corpus or the pkg.web performance gate on
each PR. Deep driver regressions, differential fuzz, runtime network/filesystem,
and performance suites remain explicit change-specific checks. The former
`scripts/test-full.sh` all-or-nothing workspace wrapper is retired; versioned
release preparation runs named affected owner and package smoke targets.
`docs/impl/16-test-policy.md` records the audit,
commands, suite-growth rule, and the relevance/cost rule: every add-on target
must name the changed boundary, plausible failure, non-duplicate information,
and reason its cost is justified. Meaningful expensive checks remain allowed;
unrelated or duplicative suites do not become mandatory by proximity.

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
Historical session journal           docs/archive/HANDOFF-2026-07-25.md
```

## Maintaining this handoff

- Update the current baseline and next-work sections in place.
- Do not append a full PR narrative. Put durable design facts in the relevant
  spec/audit, and rely on the PR and Git history for implementation chronology.
- When historical context is still worth retaining, add a dated archive rather
  than growing the live handoff indefinitely.
- Keep release and review procedures in `CLAUDE.md`; link to them instead of
  duplicating them here.

Two more out-of-gate suites are red on `main` for reasons unrelated to the
nightly's finding, both found while fixing it (2026-08-11):

- `apps_web_router::best_path_route_tree_agrees_with_the_linear_oracle` and
  `route_tree_handles_deep_long_and_empty_tables` build a fixed `array<Route>`
  and slice it (`routes[0..0]`). `check_slice_range` accepts only
  `str`/`array<scalar>`/`slice`, never `StructArray`, and a pre-#739 baseline
  compiler rejects the same shape — so this is older than both recent pkg.web
  changes. Decide whether fixed struct-array slicing is in scope (the router
  needs it) or the tests should build the table differently.
- The stale MIR-spelling class had two more members outside the bounded gate,
  `m4::pipeline_fuses_into_one_loop` and `lambda::lambda_lifts_to_a_called_function`
  (both now fixed), plus `par_map::par_map_rejects_owned_fixed_array_capture`,
  whose expected diagnostic #739 replaced. Suites outside `scripts/test-pr.sh`
  rot silently; the nightly full-suite workflow now runs them daily.

The first nightly full-suite run (2026-08-10) found one real failure, and it
is a long-standing one the bounded gate cannot see:
`apps_web_multipart::the_documented_handler_example_compiles_against_the_real_pkg_web`
fails because `apps/web/pkg/web.align` cannot compile under the L2a
open-world callback rule introduced in #672. The middleware chain calls a
function value out of an array (`pre := hs[i]; pre(c)`, web.align:206) and the
streaming path calls a bound callback (`pump(c, s)`, web.align:345); both are
rejected with "an exportable callback-bearing function cannot route an
open-world callback through an unresolved function-value target". `web.align`
last changed in #631 and the test in #619, so #672 tightened the rule under a
shipped package surface without an owner test in the bounded gate to catch it.
Deciding between relaxing the rule for these shapes and redesigning pkg.web's
middleware dispatch is a public-contract decision for the design gate, not a
local fix; `pkg.web` documentation examples do not compile until then.
