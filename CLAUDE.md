# Repository Agent Instructions

This file is the canonical repository guidance for both Claude Code and Codex.
`AGENTS.md` is a compatibility symlink for Codex. Shared guidance must be edited
here, not copied between tool-specific files.

Project skills are canonical under `.claude/skills/`. Matching entries under
`.agents/skills/` are Codex compatibility symlinks. Keep tool-specific
permissions, sandbox settings, hooks, and plugin manifests in their native
configuration files.

## What this repository is

Align is the design specification and implementation of an AOT-compiled,
data-oriented programming language.

Two kinds of work coexist:

- **Docs** (`draft.md`, `docs/`): the language and implementation design.
  Correctness means internal consistency across the documentation set.
- **Code** (`crates/`): the Rust workspace implementing `alignc`. Code must
  implement the specification, not redefine it.

## Sources of truth

Read the narrowest relevant source before changing a design or implementation:

- `HANDOFF.md` — living project status and immediate next work. Trust it over
  historical status summaries.
- `draft.md` — authoritative and most complete language specification.
- `docs/language-spec.md` — condensed digest of `draft.md`; keep both aligned.
- `docs/design-notes.md` — rationale behind design decisions.
- `docs/history.md` — decision chronology and rejected alternatives.
- `docs/non-goals.md` — deliberately excluded features.
- `docs/open-questions.md` — **Settled**, **Open**, and **Future** decisions.
  Settled items are locked; read them before proposing language changes.
- `docs/impl/00-overview.md` through `07-roadmap.md` — compiler strategy,
  pipeline, and milestone truth.
- `docs/impl/09-explain-opt.md` through `21-build-perf-plan.md` — focused
  implementation plans, ledgers, and audits (two numbers carry two documents
  each, so read by filename). Read the relevant one before modifying its
  optimization, safety, ABI, or validation path.
  `17-library-boundary-prerequisites.md` owns the native-library boundary,
  `19-hir-validation-ledger.md` the checked-HIR record contract,
  `20-runtime-abi-ledger.md` the runtime ABI inventory, and
  `21-build-perf-plan.md` the build-performance track, which deliberately
  consumes no milestone.
- `docs/impl/std-design/`, `core-design/`, and `pkg-design/` — shipped and
  planned library surfaces. English files are authoritative; `ja/` mirrors
  must not drift.
- `docs/archive/` — dated historical handoffs. Never a source of current
  status.

For `pkg.web`, `docs/impl/15-pkg-web-plan.md` is the plan of record and
`docs/impl/pkg-design/web.md` is the design source of truth.

## Core design invariants

Every extension must preserve these load-bearing principles:

- **Four-way alignment:** serve Human + AI + Compiler + Hardware together.
- **One way to do things:** one error model, optional model, ownership model,
  and parallel model.
- **Nothing hidden:** allocation, copies, errors, side effects, parallelism,
  and `unsafe` stay visible in source.
- **Compiler-friendly by restriction:** infer contiguous memory, no-alias,
  non-null, and region properties without visible lifetime syntax.
- **Data-oriented core:** normal array and slice pipelines must lower well to
  SIMD, cache-friendly execution, and future accelerators.
- **AI-friendliness is a constraint, not a feature:** avoid macros, complex
  generics, multiple paradigms, and lifetime annotations.

## Locked language decisions

Do not re-litigate these. Full rationale is in `docs/open-questions.md`:

- The compiler is Rust. LLVM lowering always goes through backend-agnostic MIR;
  semantics belong in MIR, and MIR-to-LLVM is pure lowering.
- Syntax is Go-style: newlines terminate statements, braces delimit blocks,
  indentation is insignificant, and leading `.` or binary operators continue a
  line.
- The language is expression-oriented. `if`, `match`, `else`-unwrap, `arena`,
  and blocks produce values; `fn f() -> T = expr` is the single-expression
  function form.
- Type declarations are keyword-less structs or sum types, disambiguated by
  their contents.
- Integer overflow is defined two's-complement wrap. Checked, saturating, and
  wrapping operations are explicit; invalid integer division is a hard error.
- Ownership is a property of the type. Values are Copy or Move; arenas and
  explicit heap allocation are visible; lifetimes are inferred and never
  written.
- Purity is inferred. Parallel closures must be Pure.
- The formatter normalizes meaningless variation but does not force one-line
  versus multi-line layout.
- Sequential control uses one `loop` expression with value-carrying `break`.
  There is no `for`, `while`, `continue`, labels, or guaranteed TCO.
- Printing supports primitives only; strings are single-line with the settled
  escape set; `==` supports scalars and strings only; shadowing is forbidden;
  floats follow IEEE 754 and never abort; `Ord(str)` is byte-lexicographic; and
  `else` on `Result` deliberately discards the error while `?` propagates and
  `match` inspects it.

Re-examining any of these goes through the friction-ledger protocol in
`docs/impl/23-friction-ledger.md`, which requires recorded mechanical workarounds in real
programs rather than argument, and fixes the only shape a widening may take.

## Editing conventions

- Core code, comments, identifiers, diagnostics, the authoritative
  specification, and internal implementation docs are English.
- Repository-facing GitHub text is English: commit messages, PR titles and
  descriptions, review comments and replies, release notes, and GitHub Release
  titles and descriptions.
- End-user guides and library design specs may be bilingual:
  `docs/guide/`, `docs/little-aligner/`, and the `std-design`, `core-design`,
  and `pkg-design` trees keep an English original plus a synchronized `ja/`
  mirror. Update English first.
- Match the house style: terse declarative prose, fenced `align` examples, and
  fenced `text` blocks for concept lists.
- Keep library layering strict: `core` for language intrinsics, `std` for OS
  boundaries, and `pkg` for frameworks and ecosystem packages.
- When changing a design decision, update `draft.md`,
  `docs/language-spec.md`, `docs/design-notes.md`, the relevant implementation
  documents, and the Settled section of `docs/open-questions.md`.
- Align is pre-release. Change APIs and behavior outright: no deprecated
  aliases, compatibility shims, legacy syntax, or parallel old/new paths.
- Ship the ideal unified design or defer it. Do not land compromise
  implementations that add magic special cases or violate an invariant.

## Documentation proportionality

Documentation work must be proportional to a changed public contract. An
implementation of an already-settled design does not reopen or restate that
design merely because code moved, tests grew, a PR number changed, or an
internal checkpoint landed.

- Update specifications, design notes, ledgers, mirrors, and examples only
  when their normative promise actually changes.
- Record implementation status once at a capability or milestone boundary.
  `HANDOFF.md` is not a per-commit or per-review journal; archive historical
  detail instead of extending the live handoff.
- Operational metadata such as the current branch, draft PR number, pushed
  SHA, review tool wording, or CI state is not a code-review finding and does
  not block an otherwise complete implementation. Inspect Git/GitHub for that
  live state.
- A small non-normative documentation-only change needs `git diff --check` and
  at most one directly relevant consistency/render check. It needs no compiler
  build, code owner test, adversarial code review, or broad documentation
  review.
- Use `scripts/pre-pr.sh --docs-only` for such a PR. Its SHA-bound attestation
  does not require preflight or post-open code-review evidence; the PR wrappers
  mark the required status context as docs-only exempt. A broad normative
  design change still follows the design review gate below.
- A code PR may omit documentation changes when it implements the existing
  contract without changing user-visible behavior. Finish any required
  normative prose before the final-SHA attestation; do not mutate status prose
  afterward and rerun code gates merely to narrate the PR lifecycle.

## Large design authoring gate

This gate applies only when authoring or changing a broad public contract. It
does not apply again to an implementation PR that follows an already-reviewed
ledger without changing that contract.

Before writing a broad cross-cutting design, create one public-contract ledger
in its design or audit document. Keep that ledger authoritative while drafting.
For every public surface, record the exact type or signature, inputs and
defaults, errors, ownership and lifetime, allocation, compiler/runtime/package
owner, artifact and cache identity, prerequisite milestone, acceptance test,
any benchmark required by an explicit performance/resource promise, and every
source-of-truth or language mirror that must agree.

Complete one author-side ledger-to-prose consistency pass before requesting an
independent review:

- every normative prose promise appears in the exact public record, and every
  public field has specified semantics;
- the Cartesian product of every detail level, discriminator, verification
  state, and option state has an exhaustive field-presence, row-order,
  ordinal, and unavailable-value rule;
- every argument and result has a concrete type, ownership, lifetime, and
  allocation rule;
- every text/view input crossing a native or wire boundary has explicit
  encoding, embedded-NUL, validation-error, and pre-side-effect semantics;
- every multi-invalid input has a deterministic validation order and error
  precedence;
- every CLI and build input is explicit, deterministic, and free of ambient
  configuration unless the contract names that configuration;
- every canonical persisted or exchanged format fixes all scalar widths and
  tags, every nested record and sequence order, malformed-input rejection, and
  independently checked semantic-to-byte and byte-to-semantic golden vectors;
- every type/cache fingerprint states whether it is nominal or structural; a
  structural contract includes the complete reachable definition graph;
- every promised runtime inspection field names the producer-owned table or
  thunk that supplies it without reflection or artifact/source I/O;
- every operation that changes connection-global or process-global native
  state defines overlap exclusion, failed-second-operation behavior, and
  exhaustion/error/Drop restoration order;
- every normative code example is syntax-checked, and declarations are shown
  separately from positional call expressions;
- no milestone consumes a decision or capability scheduled for a later
  milestone;
- `draft.md`, `docs/language-spec.md`, implementation plans, package designs,
  and required language mirrors agree; and
- acceptance tests cover each ledger invariant; a local benchmark covers only
  a ledger performance/resource promise and is not a correctness gate.

Do not use independent review as the primary completion loop for a design.
When a finding changes a public surface, update the ledger first and propagate
that one decision through all affected documents in one pass.

## Cross-cutting implementation gate

Do not use repeated full-diff review as the implementation discovery loop.
Before changing ownership, cleanup, FFI, ABI, an IR variant, or three or more
compiler layers, write one implementation closure matrix in the owning plan or
audit. At minimum, enumerate:

- type formation and validation, construction, move-in, move-out, source
  nulling, Drop, replacement, and return;
- every affected control path, including `if`, `match`, `else`, `?`,
  `map_err`, branch joins, loop joins, early exits, and malformed input;
- generic monomorphization, interface serialization, whole-program and
  per-unit compilation, runtime ownership provenance, and allocation parity;
- the exact owner tests that close each applicable cell, plus a benchmark only
  for a cell that makes an explicit performance/resource claim.

When the matrix introduces or changes a public contract or safety strategy,
get one fresh independent adversarial review of it and the proposed capability
boundaries before implementation. When implementation follows an already-
reviewed ledger without changing strategy, perform the author-side matrix pass
and fold boundary checking into the one preflight review instead of commissioning
a separate plan review. Resolve plan findings first. Use the fewest
independently correct, mergeable capability PRs. A boundary must isolate a
distinct failure domain or leave an actually useful stable consumer; do not
split a strict dormant producer-to-consumer chain merely to meet a line target.
If a proposed PR is expected to exceed roughly 1,000 changed hand-written
lines, record why the larger capability boundary produces less duplicated
proof and lower integration risk before coding. The threshold requires an
explanation, not an automatic split.

One parameterized or invariant-level owner may close many matrix cells. Reuse
existing regression coverage when it would fail for the changed defect; a
matrix row does not require a new fixture or command merely to obtain a
one-to-one paper trail.

Before requesting code review, perform one author-side matrix-to-diff pass.
Every applicable matrix cell must point to implementation and a regression
test, or be explicitly deferred by the plan of record. When a reviewer finds a
bug, audit the entire diff for the same root-cause class and fix the class in
one pass rather than patching only the reported line.

The second review of a revised diff should normally converge. If it finds a
new P1 or an equivalent soundness/correctness issue, stop the local patch loop:
re-open the closure matrix, identify the missed invariant, and redesign the
implementation boundary before continuing. A redesign may combine a dormant
producer/consumer chain, remove duplicated proof, or split genuinely distinct
failure domains; smaller is not the default answer.

## Build and verification

The workspace runs end to end from lexer through executable generation.
Use the checks appropriate to the change:

```text
scripts/cargo.sh build --workspace
scripts/test-pr.sh
scripts/cargo.sh clippy --workspace --lib --bins
```

Use `scripts/cargo.sh` for local Cargo work. It resolves LLVM 22 on Apple
Silicon/Intel Homebrew and Debian/Ubuntu/WSL2 layouts, validates the major
version, and supplies keg-only macOS library paths (including libpq). Run
workspace Clippy under `CARGO_TARGET_DIR=target/clippy` (as `pre-pr.sh` does):
clippy and build/test record incompatible fingerprints, so sharing one target
dir forces a near-full rebuild in both directions on every alternation. It respects explicit
`LLVM_CONFIG`, `LLVM_SYS_221_PREFIX`, and `LIBRARY_PATH` overrides. Repository
shell scripts require Bash and must remain compatible with the macOS-provided
Bash 3.2 and current Debian/Ubuntu Bash; do not invoke them through `sh`.
Every Linux CI job sets `ALIGNC_LINKER=lld` and the macOS legs set `system`, so
a missing `ld.lld` is a red build rather than a silent fallback to the slow
system linker; locally an unset value fails open to `ld.lld` when the matched
LLVM install has one (`docs/impl/21-build-perf-plan.md` item 2).

Run the narrow regression target that owns the changed behavior. There is no
mandatory full-workspace test command: deep driver, fuzz, resource, stress,
and integration targets run only when they own the changed boundary.
Benchmarks are separate local measurements run only for the changed
performance path or an explicit performance/resource claim. See
`docs/impl/16-test-policy.md` for selection and growth rules.

**Thirty minutes is the hard test budget.** A run that needs longer is
worthless as a detector, so the nightly suite job carries `timeout-minutes: 30`
and the runner caps each individual binary at 15 minutes. Exceeding the budget
*is* the red signal, not a number to raise: cut test cost or raise concurrency
with `ALIGN_GATE_JOBS`, the shared knob `scripts/test-binaries-lib.sh` reads for
both the bounded gate and the full suite. Do not add a suite whose cost only
fits by extending the budget.

**The nightly full suite is the out-of-gate detector, not a second gate.**
`scripts/test-pr.sh` is bounded by design, so every suite outside it can rot on
`main` unnoticed — four such failures accumulated before 2026-08-10.
`.github/workflows/nightly.yml` builds once and runs every compiled test binary
through `scripts/run-suite-binaries.sh`, diffing the observed failures against
`scripts/known-failures.txt` in **both** directions: a new failure is named, and
a manifest line whose test starts passing stays red until the change that fixed
it deletes the line. `scripts/run-suite-binaries.sh` reproduces that judgement
locally. A red nightly is triaged against the manifest; it does not block an
unrelated PR, and it never substitutes for running a change's owner target
locally before pushing.

**CI is the final guard, never the discovery loop.** In any implementation
flow — human or agent-driven — do not push a change to find out whether
required verification passes; behavior verification happens locally before the
push, using Docker for service- or platform-dependent suites. Concretely: a
diff touching `apps/db` or the `pkg_db_*` driver tests must pass
`scripts/db-verify-local.sh` (a CI-parity disposable PostgreSQL container
running the same required suites) before it is pushed, and when a new
required CI job is added, the matching local script ships in the same PR. An
explicit investigation phase (reproducing a CI-only failure, gathering
platform data) may use CI runs as an instrument; ordinary implementation may
not.

**Independent review and local gates may run concurrently.** The one fresh
full-diff review is inspection-only (no builds or tests), so on a committed
candidate it can run in parallel with the owner tests, the bounded gate, and
`scripts/db-verify-local.sh` without interference. Start both on the same
candidate SHA instead of serializing them; apply the finding fixes and any
gate failures in the one coherent fix commit afterward.

**The preflight stamp binds the exact pushed HEAD.** Run `scripts/pre-pr.sh`
on the final commit immediately before pushing; any later commit, amend, or
rebase invalidates the stamp and requires a rerun. Push and open/update the PR
as one uninterrupted step — three recent Preflight CI failures were stamps
belonging to a different SHA, each costing a full round-trip.

**Every base binding is the merge base, never the base branch tip.**
`scripts/pre-pr.sh`, `scripts/review-bounded.sh`, `scripts/new-review-log.sh`,
`scripts/open-pr.sh`, and the CI attestation checker all resolve the base as
`git merge-base HEAD origin/main` — the basis of the `origin/main...HEAD` diff
they already review and classify. Another PR landing on main therefore leaves
an open PR's review log, stamp, and body attestation valid; a change to the
branch itself (a rebase, an amend, or merging main in) invalidates them. So
does main coming to contain this branch's commits: the merge base then advances
to HEAD, the attested range is empty, and both `scripts/open-pr.sh` and the CI
checker refuse it rather than attest nothing. The checker derives that base from
the checked-out base branch itself, never from an argument its (PR-supplied)
workflow passes in.

**Recurred finding classes are closed by machinery, not prose.** When
`.claude/skills/align-self-review/FINDINGS.md` reaches its two-event threshold
the class becomes an explicit checklist question; at three events it must get a
compile-time tripwire, lint, structural assertion, or parameterized owner where
feasible (for example, `align_sema`'s `variant_sweep_tripwire` turns a missed
Gate-1 enum sweep into a build failure, and `scripts/lint-ratchet.sh` pins the
panic-source and lossy-cast counts so a new Gate-2/Gate-3 violation fails the
gate immediately while the legacy count only ratchets down). Adding another
checklist sentence for an already-recurred class is not closure.

Consult `HANDOFF.md` and the roadmap for the current Rust and LLVM versions,
milestone gates, and specialized verification bundles.

## align-llm requests and releases

`../align-llm/docs/align-requests.md` is the request register for the external
align-llm client. For align-llm-driven work:

1. Read the register before starting.
2. Implement the request in Align's normal design-first discipline.
3. Update the same register with Align's status, shipped surface, ownership
   model, limits, and PR numbers. Leave that separate repository's edit
   uncommitted unless asked.
4. When a request batch is complete, run exactly:

```text
cargo build --release --workspace
```

A **release build** only produces optimized local artifacts. A versioned
**release** is different and happens only when the user explicitly asks to
release: bump `Cargo.toml` and `Cargo.lock`, write matching release notes,
commit `chore(release): Align vX.Y.Z` on `main`, then tag and push `vX.Y.Z`.
Never infer the publish flow from “build” or “release build.” Versioned
release artifacts build with `--profile dist` (thin LTO, one codegen unit)
plus two-phase PGO over the examples/`pkg.db` corpus — both wired in
`release.yml`; ordinary `--release` builds stay on the untuned default so
routine batch builds never pay the LTO/PGO cost. The `alignc` binary itself
uses mimalloc in every profile (a measured ~30% frontend win).

## Long-running work and progress monitoring

Elapsed time is not a stopping criterion by itself. For any long-running command,
review, test, or investigation:

- inspect actual progress at least once per minute while the tool is running;
- check process state, new log output, the latest completed phase, and whether
  the work is still producing new relevant results rather than repeating itself;
- keep useful work running even when it takes longer than expected;
- stop or redirect only after evidence of a stall, repeated analysis, scope
  drift, or an actual tool failure;
- preserve logs, findings, completed phases, and other checkpoints before
  stopping; resume from the first unfinished area instead of restarting the
  whole task;
- report the current phase and evidence of progress during extended work, not
  only elapsed time.

An automation timeout ends that invocation only. It does not invalidate useful
work already produced, imply a clean result, or justify rerunning the same broad
scope from the beginning. Narrow a continuation only to unreviewed,
contradictory, or changed areas.

## Review before merging

Verification is proportional to blast radius. `scripts/pre-pr.sh` classifies
every diff through one shared classifier (`scripts/pr-tier.sh`, which the CI
attestation checker also runs against the same diff) and enforces the matching
gate, so the tier is a property of the changed paths and cannot be claimed:

```text
docs-only  *.md files only, with           no reviewer, no owner check
           --docs-only
tooling    leaf owner tests that are not   review OPTIONAL, one focused owner
           bounded-gate content, and       check (plus the cheap ratchet when
           prose that is not compiled      Rust changed); no bounded gate,
           into a binary                   no Clippy
code       everything else                 one fresh review, owner check,
                                           bounded gate, Clippy
```

A non-Markdown file under `docs/` (a fixture, a script, a generated asset) is
not documentation for this classification; only `*.md` is, and
`scripts/pre-pr.sh`'s per-path check uses that same `*.md` predicate. A
`--docs-only` PR additionally needs `scripts/pr-tier.sh`'s full classifier
verdict (`library_changed == false`): the per-path check alone cannot see a
compiled-prose document (`docs/impl/pkg-design/web.md`, a source input to a
test binary despite its extension) or a deletion, both of which the shared
classifier already treats as library tier. The two checks are equivalent in
outcome for `--docs-only`, not merely duplicated pattern text.

Machinery newly fetched from the trusted base needs a one-PR bootstrap arm in
`preflight.yml`, keyed on that PR's number: the base has no copy yet, and
falling back to the PR's own copy would let a change supply the rules that
judge it. None are currently present — remove a bootstrap arm once its PR is
merged, since the trusted-base path works unconditionally from then on.

The classifier fails closed in every direction: an unknown path shape, an
uncomputable diff, and any deletion are all code tier — removing a file takes
away coverage, so it can never be light.
That deliberately keeps `scripts/` and `.github/` — which compile nothing but
gate every future PR — under the full review, and keeps shared test
infrastructure (`tests/common*`, `tests/helpers/`, `tests/fixtures/`,
`tests/golden/`, and any nested module under `tests/`) out of the light tier,
because one file there reaches every suite. Only a leaf owner test, whose
blast radius is the target its own owner check compiles and runs, takes the
light path. Path shape alone is not enough evidence: the test targets
`scripts/test-pr.sh` names *are* the bounded gate, and a document reached by
`include_str!` is a source input to a test binary, so `scripts/pr-tier.sh`
lists both and `scripts/test-pr-workflow.sh` recomputes those sets from the
repository and fails when a list goes stale. Passing `--reviewer` with a log in
a light tier still records a review exactly as the code tier does — the tier
removes the requirement, not the option.

The PR is a publication checkpoint, not a second implementation loop. The
normal code path is exactly:

1. Finish and commit one coherent capability. For Rust under `crates/`, run the
   `align-self-review` skill and the narrow check needed to make the candidate
   reviewable.
2. Run one fresh full-diff review with `scripts/review-bounded.sh` or one fresh
   independent adversarial reviewer.
3. If the review finds issues, verify the complete finding set, fix all valid
   findings in one coherent commit, and record the finding-to-fix ledger. Do
   not ask the reviewer to reread the complete diff.
4. Run `scripts/pre-pr.sh` on the final commit. It runs the specified owner
   check first, then the bounded PR gate and library/binary Clippy for Rust.
   Pass `--findings-fixed` when the review log belongs to the preceding
   reviewed candidate. This closes the ordinary one-review/one-fix cycle
   without pretending that the fix commit was reviewed clean.
5. Push and open the draft with `scripts/open-pr.sh`, wait for CI, then merge.
   Opening the PR does not invalidate or duplicate the pre-open review. Direct
   `gh pr create` is prohibited for agent-driven work.

A complete re-review is required only when the fix changes a public contract
or strategy, changes an IR shape, materially crosses three or more compiler
layers, or responds to a P1 by redesigning the implementation. A local
ownership, cleanup, FFI, ABI, diagnostic, or test correction closes against
the original finding and its owner check. The user may explicitly request a
second review.

**The one-review/one-fix cycle is enforced, not merely stated.** Preflight
treats the first commit in `base..HEAD` as the implementation, uncounted
regardless of its own subject — a branch whose every commit happens to be
fix-titled still has that first commit excluded, so it cannot dodge the
tripwire by simply never having a non-fix commit at all. Every commit after
that whose subject is fix-titled is a review round; at three, preflight fails
unless some commit anywhere after the implementation — fix-titled or not —
both changed an authoritative contract document — a `docs/impl` plan or audit
for a compiler or package capability, or this file for the process machinery
itself, with translated `ja/` mirrors excluded — and carried a
`Closure-Matrix-Reopened: <axis>` trailer. The count spans the whole range and
is never reset by an interleaved non-fix commit. Three rounds of patching
individually reported cells is the signature of a matrix that missed an axis —
the required response is to enumerate that axis, fix the class in one pass, and
commit the updated matrix with that trailer. Measured cost of ignoring this:
24–36 minutes per round, and the two capabilities that ran four and nine rounds
spent 3.5 and 4.75 hours respectively inside the loop. Titling a review-round
commit outside the `fix` prefix excludes only that one commit from the count —
it does not reset any other counted commit — and an author who never uses
`fix` for a review round escapes the tripwire entirely; both are visible,
auditable choices in the history, and reviewers should read either pattern as
the same smell.

Finish ordinary PR-body prose before opening. If a later code push is actually
required, rerun preflight for the new SHA and refresh the existing PR with
`scripts/open-pr.sh --update PR_NUMBER`. There is no separate post-open review
recorder or body-marker workflow.

Review execution follows the progress-monitoring rules above. Review duration
is proportional to useful progress and scope; there is no default wall-clock
cutoff. `scripts/review-bounded.sh` keeps its historical name but stops by
default only after a configured interval with neither log growth nor process
CPU/state progress. An explicit user-supplied maximum duration bounds that one
invocation only. If a review stops without a verdict, record the elapsed time
and last completed area, preserve its useful findings, and continue from the
unfinished scope. Do not treat the missing verdict as CLEAN, and do not restart
the complete review solely because the invocation stopped. Review automation
must not launch builds, tests, benchmarks, or network work; review is
inspection, and verification is selected separately.

### Review log

`scripts/new-review-log.sh [--base REF] [OUTPUT_PATH]` scaffolds a review-log
file: it writes the required `ALIGN_REVIEW_HEAD` (current `HEAD`) and
`ALIGN_REVIEW_BASE` (`git merge-base HEAD <--base>`, default base
`origin/main`) keys, a template
body, and a trailing `ALIGN_REVIEW_VERDICT=FINDINGS` line, then prints both
matching `scripts/pre-pr.sh` invocations — the CLEAN form and the
`--findings-fixed` form for after a later fix commit. It refuses to overwrite
an existing file at `OUTPUT_PATH`. `git status --porcelain` never reports
paths under `.git/`, so an untracked review log placed there does not fail
`scripts/pre-pr.sh`'s clean-worktree check; the scaffolder defaults to a
`.git/align-review-<shortsha>.log` path there (matching
`scripts/review-bounded.sh`'s own default location and extension) and also
accepts any path entirely outside the repository.

### Review operation guardrails

- Run one review for an exact `HEAD`/base pair. Do not launch a duplicate review
  for the same pair while the first is still running.
- A branch ancestry gets one full-diff host review. After findings, validate the
  coherent fix against the finding ledger and changed slice; do not launch a
  second complete-diff discovery pass. `scripts/review-bounded.sh` records
  every started review under the worktree Git directory and refuses a
  descendant full-diff review when an ancestor record exists. Only a
  high-risk redesign may override this with `--reopen-axis <axis>`, and only
  when a commit after the reviewed head changes `CLAUDE.md` or an authoritative
  `docs/impl` plan and carries the exact
  `Closure-Matrix-Reopened: <axis>` trailer.
- A stall stop, explicit user bound, missing machine-readable verdict, or killed
  process means **INCOMPLETE**, never CLEAN. Preserve the log, elapsed time,
  last completed area, and process state.
- Useful log growth, advancing review phases, or accumulating process CPU time
  is evidence to keep a long review running. Repeated identical analysis,
  unchanged zero-CPU process state, scope drift, or orphaned helpers is not.
- A user-supplied maximum duration applies only to the current review
  invocation; it never authorizes polling/restarting the same review or
  chaining another broad review. When that invocation ends, preserve its
  checkpoint and continue only with the unfinished slice.
- Inspect the process and new log output at least once per minute. Stop orphaned,
  duplicate, stalled, or scope-drifting review processes after recording their
  state; do not leave helper processes running after the parent review stops.
- Continue only with the unreviewed, contradictory, or changed slice. A review
  continuation is separate from rerunning owner tests, pre-PR attestation, or
  CI; do not repeat those gates unless the tree or their required inputs changed.
- A broad review rerun requires a high-risk trigger: a P1 redesign,
  public-contract or strategy change, IR-shape change, a material change across
  three or more compiler layers, or an explicit user request. A small
  ownership/cleanup/FFI/ABI fix is reviewed against the original finding and
  changed lines without rereading the unchanged full diff.
- On macOS, a review process at CPU 0 in `_dyld_start` with repeated Xcode cache
  or `xcodebuild` errors is a host stall. Stop it as INCOMPLETE, retain its
  useful static findings, and use CI or an isolated target for verification.

Do not rerun the same broad review or broad test gate on an unchanged tree.
After a bounded review fix, run the smallest owner targets that can detect a
regression in the changed lines; CI remains the final broad gate. Preserve a
successful earlier result when only documentation or review records change.

## Throughput and checkpoint discipline

Implementation progress is measured by independently correct, mergeable
source-and-test checkpoints, not by document length, review-log volume,
formatting churn, or elapsed agent activity.

- After the narrow source-of-truth read, reach a compiling, owner-test-backed
  implementation checkpoint within 60 minutes.
- Before changing an exact public wire/ABI contract, ownership-safety boundary,
  or malformed input path that could panic or miscompile, translate its
  observable `every`/`exact` promises into an owner-test checklist. Exhaustive
  Cartesian coverage is reserved for those externally meaningful or safety-
  critical contracts. An internal representation-preserving refactor reuses
  the cumulative owner suite and adds only tests that discriminate its new
  risk; it does not clone one malformed fixture per variant.
- When changing control-flow/type-inference behavior, cross discriminator reachability,
  alternative completion kind, expected-type availability, source-order permutations,
  and clean versus already-invalid subtrees in the owner matrix. Distinguish runtime joins
  from structural type reconciliation: only fallthrough alternatives contribute to a join;
  discriminator-unreachable alternatives receive no enclosing expectation but remain
  structurally checked; reachable eager-diverging typed wrappers receive any required late
  reconciliation without contributing a runtime value. Preserve the same diagnostic guard
  for immediate and delayed constraints.
- Two hours without new production/test progress triggers an evidence-based
  check of the active blocker, not an automatic split or more design prose.
- For continuous agent-driven milestone work, eight hours should close at
  least one end-to-end capability and 24 hours should leave the planned
  milestone merged or waiting only on an external required check. If it does
  not, preserve the checkpoint and record where the time went: implementation,
  owner tests, review, broad verification, tool/host wait, documentation, or
  repeated planning. Correct the dominant cost before continuing.
- Do not use changed-line count as the progress unit or PR boundary. Prefer a
  larger capability PR over multiple dormant seams that repeat the same
  matrix, review, and broad gates. Above roughly 1,000 hand-written changed
  lines still requires the written capability-boundary proof described above.
- Once the one review cycle and one coherent fix are complete, merge or
  explicitly re-scope. Do not start another general improvement or discovery
  loop inside that PR.

### Claude Code review adapter

- A human starts the dedicated review with `/code-review`.
- When Claude drives the PR flow autonomously, use the model-invocable `review`
  skill or one fresh independent adversarial subagent. Use both only for
  complementary assigned risks or an explicit user request.

### Codex review adapter

- A human starts the dedicated reviewer with `/review`.
- Non-interactive automation may use `codex review --base <branch>`,
  `codex review --uncommitted`, or `codex review --commit <sha>`.
- When Codex drives the PR flow autonomously, use one host-native review or one
  fresh independent adversarial subagent; do not pretend to invoke a user-only
  composer command from inside the turn. Use a second reviewer only under the
  review rules above.
