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
- `docs/impl/09-explain-opt.md` through
  `15-pkg-web-plan.md` — focused implementation plans and audits. Read the
  relevant audit before modifying its optimization or safety path.
- `docs/impl/std-design/`, `core-design/`, and `pkg-design/` — shipped and
  planned library surfaces. English files are authoritative; `ja/` mirrors
  must not drift.

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

## Large design authoring gate

Before writing a broad cross-cutting design, create one public-contract ledger
in its design or audit document. Keep that ledger authoritative while drafting.
For every public surface, record the exact type or signature, inputs and
defaults, errors, ownership and lifetime, allocation, compiler/runtime/package
owner, artifact and cache identity, prerequisite milestone, acceptance test,
benchmark, and every source-of-truth or language mirror that must agree.

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
- no milestone consumes a decision or capability scheduled for a later
  milestone;
- `draft.md`, `docs/language-spec.md`, implementation plans, package designs,
  and required language mirrors agree; and
- acceptance tests and benchmarks cover each ledger invariant.

Do not use independent review as the primary completion loop for a design.
When a finding changes a public surface, update the ledger first and propagate
that one decision through all affected documents in one pass.

## Build and verification

The workspace runs end to end from lexer through executable generation.
Use the checks appropriate to the change:

```text
cargo build --workspace
scripts/test-pr.sh
cargo clippy --workspace --all-targets
```

Run the narrow regression target that owns the changed behavior. The full,
expensive corpus is explicit via `scripts/test-full.sh`; it is not the ordinary
PR gate. See `docs/impl/16-test-policy.md` for the test categories and growth
rules.

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
Never infer the publish flow from “build” or “release build.”

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

The PR is not the first correctness pass. A coherent implementation must pass
the pre-PR gate before a draft PR is opened:

1. Finish the intended implementation scope on the branch; do not use a draft
   PR as a scratchpad for basic correctness work.
2. For Rust under `crates/`, run the `align-self-review` skill. Its canonical
   source is `.claude/skills/align-self-review/SKILL.md`.
3. Run a fresh adversarial preflight review of `git diff main...HEAD` and fix
   valid findings locally.
4. Run the focused owner tests, `scripts/test-pr.sh`, and applicable Clippy.
5. Record the HEAD/base-bound clean review log, reviewer, and checks against the
   final commit with `scripts/pre-pr.sh`. Open the draft only through
   `scripts/open-pr.sh`; direct
   `gh pr create` bypasses the local guard and is prohibited for agent-driven
   work. CI rejects an absent or stale HEAD-bound attestation.

Every code PR must still receive independent review after it is opened and
before it is merged:

1. Run the host-native review with `scripts/review-bounded.sh` and a fresh
   independent adversarial reviewer on the final pushed diff.
2. Verify every finding against the code. Apply valid findings and explain
   rejected ones.
3. Batch related valid findings into one coherent follow-up commit whenever
   possible. Do not make one follow-up commit per review comment; separate
   commits are for independent changes or a necessary checkpoint.
4. Rerun `scripts/pre-pr.sh` after any follow-up and refresh the PR attestation
   with `scripts/update-pr-preflight.sh`.
5. Record both clean post-open reviews against the pushed SHA with
   `scripts/record-post-review.sh`. Wait for CI and only then merge.

Review execution follows the progress-monitoring rules above. If a review tool
reaches its configured invocation bound without a verdict, record the elapsed
time and last completed area, preserve its useful findings, and continue from
the unfinished scope. Do not treat the missing verdict as CLEAN, and do not
restart the complete review solely because the bound was reached. Review
automation must not launch
`cargo test --workspace` or `scripts/test-full.sh` for an ordinary PR unless
the change scope explicitly requires that expanded verification.

Do not open and immediately merge a code PR.

### Claude Code review adapter

- A human starts the dedicated review with `/code-review`.
- When Claude drives the PR flow autonomously, use the model-invocable `review`
  skill on the open PR and an independent adversarial subagent.

### Codex review adapter

- A human starts the dedicated reviewer with `/review`.
- Non-interactive automation may use `codex review --base <branch>`,
  `codex review --uncommitted`, or `codex review --commit <sha>`.
- When Codex drives the PR flow autonomously, inspect the PR/base diff and use
  a fresh independent adversarial subagent; do not pretend to invoke a
  user-only composer command from inside the turn.
