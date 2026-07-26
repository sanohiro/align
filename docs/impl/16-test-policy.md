# Test execution policy

The test suite has two distinct jobs:

1. prove on every change that the compiler still builds and its vertical path
   still works;
2. protect individual implementations with deep, expensive regression checks.

Those jobs must not share one mandatory command.

## Audit baseline

The July 2026 audit found:

- 159 `align_driver` integration-test binaries containing 2,159 tests;
- 10 differential tests that each compile and run 150–200 generated programs;
- frontend and formatter fuzz loops with 10,000–12,000 seeds;
- 14 driver test binaries using real sockets and 32 using the filesystem;
- runtime tests that include TLS, timeouts, process control, fd-leak cycles,
  cryptographic cost cases, and performance probes;
- a router benchmark with eight paired trials in the ordinary PR workflow.

Most of these tests have a valid regression role. Their accumulation into one
default gate did not.

## Ordinary PR gate

Run:

```text
scripts/test-pr.sh
```

This gate is deliberately fixed and bounded:

1. build the workspace, including the runtime archive;
2. run the explicit deterministic library-test list for the compiler crates and
   the small interface/formatter integration targets;
3. run `align_driver/tests/m0.rs`, which covers source checking, HIR-to-MIR
   lowering, native object emission, linking, execution, and a rejected
   program.

CI additionally builds release compiler/runtime artifacts and compiles and runs
`examples/hello.align` on Linux x86-64, Linux ARM64, and Apple Silicon. This is
the cross-platform packaged-command smoke path.

The ordinary gate does not run:

- the full driver regression corpus;
- differential fuzzers;
- real network, TLS, filesystem, timeout, process, or fd-leak suites;
- performance or scaling measurements;
- repeated concurrency and artifact-staging stress tests.

The compiler library list is intentionally explicit in `scripts/test-pr.sh`.
Adding a new workspace crate does not silently add its tests to every PR; the
new crate must first be given an intentional owner and gate classification.

## PR sequencing and review cost

The draft PR is a review-ready checkpoint, not the place to discover basic
correctness problems. Before opening a draft for a code change, finish the
coherent implementation, run the applicable self-review checklist, run a fresh
adversarial preflight on the local diff, and pass the focused owner target plus
the bounded PR gate. `scripts/pre-pr.sh` records those checks against the exact
HEAD, and `scripts/open-pr.sh` is the required agent path for opening the draft.
CI rejects a missing or stale attestation. This keeps obvious ownership,
malformed-input, ABI, and cross-stage omissions out of the external review cycle.

After the draft is opened, run the required host-native and independent reviews
on the final pushed diff. Batch related review fixes into one follow-up commit
where possible; do not create one commit per finding. A review command has a
15-minute watchdog implemented by `scripts/review-bounded.sh`. If it has not
produced a verdict, the wrapper terminates the whole review process group and
reports a timeout rather than waiting without a stopping point or repeatedly
starting the full review. Ordinary review automation must not promote `cargo test
--workspace` or `scripts/test-full.sh` into the PR path without an explicit
scope justification.

`scripts/record-post-review.sh` publishes the clean host and independent review
as a SHA-bound GitHub status after the PR exists. The required status prevents a
later push from inheriting an earlier review result.

## Change-specific verification

The author must run the narrow regression targets that own the changed
behavior. Examples:

```text
cargo test -p align_driver --test par_map
cargo test -p align_driver --test fuzz_differential
cargo test -p align_driver --test m11_http_server
cargo test -p align_runtime --lib http_client
cargo test -p align_runtime --lib par_map
```

Optimization work that changes a performance path, is covered by a `MEASURE-FIRST`
audit, or makes a performance or resource claim runs its named benchmark or
measurement probe. A correctness-only change that does not alter a performance
path uses its owner target and the bounded code gate. Network, filesystem, timeout,
process, and fd work runs the corresponding real-resource target in an
unrestricted environment. A test should be added to an existing owner target
when possible; do not create another cross-cutting integration matrix for a
unit-level rule.

## Selection procedure

Choose the smallest test set that covers the changed boundary, then expand only
when the change crosses another boundary:

1. Identify the owner: documentation, one private implementation helper, a
   public FFI/ABI surface, compiler lowering, or a resource boundary.
2. For code changes, run the focused owner target first. For a private runtime
   helper, use a filtered library test such as
   `cargo test -p align_runtime --lib par_map` rather than the entire runtime
   library test binary. For documentation-only changes, always run
   `git diff --check`, then run the relevant consistency or render check when
   one exists; do not invent a code-test target for prose.
3. Run the ordinary PR gate required for the change. Rust code changes use
   `scripts/test-pr.sh` as the bounded test gate, with the standard workspace
   build and applicable Clippy checks still required. Documentation-only changes
   need only `git diff --check` plus any relevant consistency or render check.
4. Add a broader target only when the changed behavior is not exercised by the
   owner target, crosses crate/ABI/linker boundaries, changes scheduling or
   resource semantics, or is unusually broad.
5. Use `scripts/test-full.sh` only for an unusually broad change, preparation
   for a versioned release, or an explicit full-regression request.

Do not run a whole crate or the full workspace by reflex after a narrow change.
Do not repeat a target already covered by `scripts/test-pr.sh` unless the
focused invocation selects an additional behavior. Record the reason for every
expanded target and distinguish a product failure from a host permission,
network, toolchain, or dependency-linking limitation.

## Relevance and cost rule

Test cost is a reason to narrow the selection, not a reason to skip a meaningful
check. An additional target is meaningful when all of these are true:

- it executes the changed boundary or a direct consumer of its contract;
- it can fail for a plausible regression introduced by the change;
- it adds information not already supplied by an earlier target; and
- its result has a clear interpretation and stopping point.

If one of those conditions is false, do not run the target merely because it is
nearby in the same crate or suite. If all are true, run it even when it is
expensive: soundness, ABI/FFI, scheduler, resource-boundary, cross-platform,
release, and measurement checks can justify their cost.

Classify verification by scope rather than by a machine-specific wall-clock
threshold:

```text
bounded   the ordinary PR gate or a small deterministic owner target
focused   one owner regression, filtered library test, or named probe
expanded  multiple owner/resource targets needed for a crossed boundary
full      scripts/test-full.sh for broad work, release preparation, or an explicit request
```

For every focused, expanded, or full target added beyond the ordinary gate,
record four facts in the PR description or handoff: the changed boundary, the
plausible failure it protects against, why a smaller target is insufficient,
and why the result is worth its cost. If a target is omitted because it is
duplicative or unrelated, record that briefly when the omission could otherwise
look surprising. A host limitation is evidence about the environment, not a
product result, and must be reported separately.

### Scope matrix

| Change scope | Minimum verification | Add only when |
| --- | --- | --- |
| Documentation or policy | `git diff --check` | add the relevant consistency or render check when one exists |
| Private helper or local analysis | one filtered owner test; Rust changes also use the bounded gate `scripts/test-pr.sh` (including its workspace build) and applicable Clippy | the helper crosses a crate, ABI, linker, scheduler, or resource boundary |
| Compiler, runtime, or FFI code | focused owner test, `scripts/test-pr.sh`, and applicable Clippy | the focused target does not exercise the changed contract or the change is broad |
| Optimization or concurrency | owner correctness test; a named benchmark/probe for a changed performance path, a `MEASURE-FIRST` audit, or a performance/resource claim; `scripts/test-pr.sh` for code changes | add fresh-process, cross-platform, stress, or repeatability coverage only when that behavior is part of the changed contract or claim |
| Broad refactor or versioned release | the bounded gate and the affected owner targets | run `scripts/test-full.sh` when the scope or release process warrants it |

Run each selected target once per unchanged environment. Repeat only when the
target is inherently statistical, tests fresh processes or hosts, is
nondeterministic by design, or follows a code/environment change. Do not rerun
a command solely to obtain a more comforting green result, and do not use a
full-workspace command as a substitute for identifying the owner target.

## Full regression

Run the retained full corpus explicitly when the change is unusually broad or
before a versioned release:

```text
scripts/test-full.sh
```

The full corpus is not a mandatory ordinary-PR or push gate. A focused
regression test remains required for a bug or optimization that needs it, but
that requirement does not promote the entire historical corpus into every
change's critical path.

## Growth rule

Every new integration test must name the boundary or regression it protects.
Prefer, in order:

1. a unit test beside the implementation;
2. one focused regression in the existing owner target;
3. an end-to-end test only when the failure crosses crate, ABI, linker,
   runtime, process, or protocol boundaries.

Load, throughput, scaling, repeated-race, differential-fuzz, and resource-leak
checks are explicit change-specific tests, never ordinary smoke tests.
