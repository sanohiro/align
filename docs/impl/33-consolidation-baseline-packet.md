# Consolidation baseline execution packet

> **Status:** READY FOR V0 after the transition in
> [`32-post-xml-consolidation-plan.md`](32-post-xml-consolidation-plan.md).
> This packet specifies qualification using existing tools. It adds no code,
> fixture, CLI, machine-readable report format, performance guarantee, or S0
> implementation approval. No measurements have been performed for this packet.

## 1. First session

1. Confirm the XML PR is merged and its owner has stopped. Resolve whether the
   intended release completed or was explicitly deferred. Do not choose a
   release version or publish anything from this packet.
2. Record the immutable baseline and available evidence using §2. Work from a
   clean isolated checkout of that source revision, not the release owner's
   mutable worktree. Read its `HANDOFF.md` and `scripts/known-failures.txt`.
3. Qualify R0, then R1 and R2 in §3. Reuse exact applicable completed evidence
   rather than rerunning the XML owner's verification. Missing LLVM, native
   linking, backend execution, or service support is a blocker, never PASS.
4. Record R3–R5 from existing evidence or identify the smallest missing owner
   run. This table selects a corpus; it is not a command to run every full suite.
5. Inspect the relevant existing reports and metric limits in §4. Produce a
   bounded candidate list: at most one proposed C0 boundary and at most one
   residual optimization candidate, with evidence or explicitly missing
   evidence. Record an explicit no-change disposition if neither is justified.
6. Run the §5 AI pilot if an already available model can be pinned. Otherwise
   record its missing lane and proceed with compiler qualification.
7. Hand off the V0 record plus the concrete missing measurement/decision rows
   to S0A and S0B design. Do not start an implementation cleanup during V0.

Preparation is complete enough to start these steps without another general
roadmap discussion. The release identity and host observations are execution
inputs, not facts to fabricate in advance.

## 2. Baseline and evidence record

Keep one private Markdown record and raw logs in a named directory outside the
checkout. This is an operator notebook, not a stable interchange format. Record:

| Field group | Required contents |
|---|---|
| Baseline | XML merge commit; actual release tag and resolved commit if released; qualification source commit; distinguish their roles |
| Artifact | Absolute compiler path, compiler digest, adjacent runtime/package/cache asset identities, distribution archive digest, target; missing provenance means unqualified artifact |
| Build lane | Distributed dist/PGO asset or local optimized source build; never aggregate the two; source tests exercise their locally built compiler libraries |
| Host | OS/kernel, CPU/architecture, memory, toolchain and LLVM versions, linker and native library identities, relevant power/load condition |
| Inputs | Corpus paths and content digests, declared package sources, invocation/working directory, target CPU, profile, link/strip policy, explicit cache state and measurement toggles |
| Results | Row/test ID, actual command, exit status, stdout/stderr log paths, expected oracle, backend/service execution evidence, reused evidence provenance or new run duration |
| Measures | Value and unit, observed versus estimated, source of observation, availability reason, raw samples and statistic when supplied by an owner |
| Disposition | Qualified; known baseline defect; new defect; unavailable prerequisite; missing instrument; not applicable, with a concrete reason and next owner |

Record only relevant allowlisted build/measurement settings; do not dump the
environment or credentials. Keep a source-built runtime paired with its source
compiler. Do not overwrite installed release assets or mix a benchmark probe
runtime into the release lane. Use fresh independent caches for cold claims;
a cache hit is a distinct recorded case, not an unexplained faster sample.

First capture evidence already produced for the baseline: exact local XML owner
results, CI outcome, and the latest applicable nightly verdict. A green test
whose backend path returned early is not native execution evidence. A release
smoke proves packaging, while an owner test linked to local compiler crates
proves the source lane. Neither result silently stands for the other.

All new test execution follows `16-test-policy.md`: at most 30 minutes for a
selected run and 15 minutes per binary, with shared concurrency control where
the existing runner provides it. Do not split one oversized mandatory suite
into consecutive invocations to evade the budget. Preserve completed phases
and fix/narrow a budget failure. Monitor actual progress at least once a minute.
No full-workspace test command, new CI job, or benchmark threshold is added.

## 3. Fixed initial corpus and correctness owners

Paths are relative to the bound baseline checkout. Test function names identify
the existing oracle; they are not permission to rewrite that oracle. If the
release changes a named path/test, record the mapping at the bound revision
before execution. Never silently substitute a different case.

| Row | Program/fixture and oracle | Starting verification | What it qualifies |
|---|---|---|---|
| R0 | `examples/hello.align`: stdout `42\n`, empty stderr, exit 0; `bench/binary_size/progs/empty.align`: empty streams, exit 0 | Released compiler `build`, execute each output, `size`; source owner `capability_linking` | Actual entry/primitive output and dependency baseline, not startup timing |
| R1 | `pkg_csv::canonical_decode_runs_whole_and_per_unit`; exact output already asserted by the owner | Focused source-owner command below | Real canonical CSV package -> typed SoA -> field aggregates, whole/per-unit behavior |
| R2 | `pkg_frame::frame_i64_join_is_stable_and_whole_per_unit_equivalent`; codec inputs produce the exact stable five-pair result | Focused source-owner command below | Canonical codec/frame composition, bounded output and stable ordering |
| R3 | `chunks::direct_chunks_consumers_are_semantically_equivalent`, `buffer_donate::donation_on_and_off_execute_identically` | Run the named filtered owners only when comparable baseline evidence is absent | Existing virtualization/reuse behavior and negative controls; no new selector |
| R4 | `examples/task_group.align`: stdout `305\n`, empty stderr, exit 0; `par_map::par_map_pure_function`; runtime `par_map_cold_start` | Example via released artifact; existing task/parallel owners for missing evidence | Explicit parallel semantics and separate cold-pool structural evidence; warm probes do not measure process startup |
| R5 | Existing `apps/db/main.align`, `m11_http_server` owners | Applicable existing DB/web qualification; run `scripts/db-verify-local.sh` only if a changed DB boundary or missing DB qualification requires it | Existing multi-module/service consumer behavior; do not invent a new network application |

The initial R0 commands below run from a fresh output directory. Bind
`BASELINE_REPO` and `BASELINE_ALIGNC` to absolute paths recorded in §2; the latter
must retain its matching adjacent distribution assets. These names are shell
variables, not new compiler environment inputs. The `build` output stem is the
source stem in the current directory. Use an empty directory to avoid collisions.

```bash
"$BASELINE_ALIGNC" build "$BASELINE_REPO/examples/hello.align" --profile release --target-cpu baseline
./hello
"$BASELINE_ALIGNC" size "$BASELINE_REPO/examples/hello.align" --profile release --target-cpu baseline
"$BASELINE_ALIGNC" build "$BASELINE_REPO/bench/binary_size/progs/empty.align" --profile release --target-cpu baseline
./empty
"$BASELINE_ALIGNC" size "$BASELINE_REPO/bench/binary_size/progs/empty.align" --profile release --target-cpu baseline
```

Capture and check each command's status/streams against the table, not just
the terminal's last exit code. `size` may build again; it is an untimed companion.
Build flags, object identity, and native link evidence must agree before two
reports describe the same artifact. Do not time `alignc run` as executable
startup: it includes compiler work. R4 uses the same build/execute procedure
with `examples/task_group.align` and output `task_group`.

For missing source-lane R1/R2 evidence, run from the isolated baseline checkout,
using its matched LLVM 22 environment. Build the runtime/workspace once if that
checkout has no completed build, then use the two existing filters:

```bash
scripts/cargo.sh build --workspace
scripts/cargo.sh test -p align_driver --test pkg_csv canonical_decode_runs_whole_and_per_unit -- --exact
scripts/cargo.sh test -p align_driver --test pkg_frame frame_i64_join_is_stable_and_whole_per_unit_equivalent -- --exact
```

Require the selected test actually to exist and execute (one selected test,
no ignored/filtered-away substitute), and establish backend/linker availability
before accepting its native subpaths. Test compilation/build time is recorded
separately from generated-program execution. A pre-existing failure is retained
with its owner; do not disable it to qualify the baseline.

R3 filters follow the same command form with target `chunks` and filter
`direct_chunks_consumers_are_semantically_equivalent`, then target
`buffer_donate` and filter `donation_on_and_off_execute_identically`. R4's cold
owner belongs to package `align_runtime`, test target `par_map_cold_start`.
These are selected follow-ups, not additions to every PR gate.

XML itself uses its just-completed implementation owner's evidence. Do not
predeclare its final test target or copy its XML fixtures before its PR lands.
The existing XML design remains the exact semantic oracle.

## 4. Measurement inventory and next-design inputs

Before running any benchmark, inspect its script at the bound baseline for
build paths, profile, probes, environment, output and sample policy. Run
source-building benchmarks in a disposable source lane, serially with respect
to shared target/runtime outputs. Do not let probe features contaminate normal
artifacts. Repository Cargo work uses `scripts/cargo.sh`; if a legacy benchmark
cannot run under the qualified environment, record the harness prerequisite
instead of quietly measuring a different toolchain.

| Observation | Existing owner or command | Limit and next action |
|---|---|---|
| Image bytes, sections, dependencies | R0 `size`; `capability_linking`; `bench/binary_size/README.md` | Historical BEFORE/AFTER numbers and simulated legacy linking are not a comparison to the post-XML baseline. S3 needs current requested and final dependency closure. |
| JSON-to-SoA throughput | `bench/json_soa/README.md` and `run.sh prepare native` -> digest-bound `run.sh native` | Preserve the existing external work-directory, sealed-input, target and evidence policy. Native macOS is development evidence, not accepted Linux evidence. |
| Frame build/probe/output cost | `bench/frame_join/run.sh` | Direct runtime ABI benchmark, not end-to-end source compilation. Pair with R2's actual package path; scratch/output byte formula is not RSS. |
| Parallel cost and chunks headers | `bench/par_map/run.sh threshold`, `width`, `chunks` | Probe-runtime/cdylib evidence only. Warm pool, scalar/stride/runtime-header cases do not establish startup or S1 source-consumer benefit. |
| Build/edit/RSS | `bench/function_incremental/run.sh`; `bench/build_pipeline/run.sh` | Existing mechanism comparisons, not generic release-vs-candidate benchmarks. Preserve their existing thresholds; V0 does not make them mandatory. |
| Fusion and vector shape | `deep_pipeline`, `vectorize_shapes`, existing `explain-opt` and optimized LLVM output | A structural observation can reject an unsafe transformation; vectorization count alone is not a performance score. |
| Process startup | No existing qualified family supplies all S0A observations | S0A remains a new exact-ledger capability. Do not use an in-program clock or warm parallel probe as a substitute. |
| Decision explanations | Existing `explain-opt`, donation/chunks/parallel owner assertions | Record which current decision/rejection facts are missing. S0B observes actual owners; it must not infer a planner decision from LLVM remarks. |
| Physical traffic, RSS, faults, allocation | Operation-specific counters or qualified host observation, where available | Distinguish actual counters, estimates, and unavailable values. Never infer physical traffic or residency from static IR alone. |

V0 need not run this entire inventory. For the proposed C0/O0 boundary, select
only the rows that can resolve its question. Keep default-runtime results
separate from probe results. Preserve the exact supplied samples/statistics;
an instrument that lacks balanced candidate/control evidence may locate a
cost but cannot justify adoption under plan 31.

The S0A design handoff names baseline artifacts, missing metrics, qualified
host, and existing process-lifecycle owner to examine. Its exact fixtures,
timeout/cleanup behavior, sample/format contract, and tests remain required
before implementation. The S0B handoff names existing selectors, actual
producer/consumer locations, missing record states, source-anchor coverage,
whole/per-unit routes, and generated-code identity controls. Do not request
another broad architecture survey in place of filling these bounded ledgers.

## 5. AI pilot protocol

The pilot reuses existing semantic oracles; it does not start a benchmark
platform. Freeze a pilot bundle before asking a model to solve it: exact task
texts, input/output oracles and test IDs, allowed specification excerpts and
package sources, compiler artifact, model/provider revision or returned model
identity, all exposed sampling settings, and the fixed repair budget. An
unpinnable model is labeled non-comparable across runs.

Start with six tasks below, one initial answer plus at most two repair answers
per task. Each repair receives only that task's compile/test diagnostics; no
reference solution or compiler patch is supplied. V1 repeats the identical
bundle and model settings in a fresh context. Keep initial and repaired success
separate. This small pilot is diagnostic evidence, not a language-wide score.

| ID | Task to freeze using the named existing fixture's exact inputs and oracle | Oracle owner |
|---|---|---|
| A1 | Decode the R1 projected/quoted CSV and compute/print its specified columns | `pkg_csv::canonical_decode_runs_whole_and_per_unit` |
| A2 | Encode the R2 integer columns and return/print the bounded stable join pairs | `pkg_frame::frame_i64_join_is_stable_and_whole_per_unit_equivalent` |
| A3 | Join string keys containing embedded NUL and non-ASCII bytes through a wrapper | `pkg_frame::frame_string_join_preserves_byte_equality_through_an_indirect_wrapper_call` |
| A4 | Compute the existing uneven final-chunk sums with the specified chunk size | `chunks::chunks_count_and_per_chunk_sum` |
| A5 | Launch captured-value tasks, wait, and print their combined result | `task_group::multiple_capturing_tasks`, plus R4's executable control |
| A6 | Build the exact primitive/builder log records and explicitly flush the writer | `std_log::logger_emits_exact_text_and_builder_records` |

Preparing a task means extracting its input, required observable behavior and
allowed package scaffold from the bound owner, without exposing its solution.
Keep this private task bundle outside compiler tests. Preserve the complete
generated answers and diagnostics. A missing provider does not authorize a new
account, paid service, or external consumer modification. Initial scoring can
remain unavailable while V0's compiler work proceeds.

For every attempt record compilation, oracle success, repair count, final code
size, diagnostics, and any observed materialization/allocation decisions.
Unmeasured fields remain unavailable. These constant/small fixtures are
correctness tasks: do not time them for Rust/C comparisons or count accidental
constant folding as successful vectorization. Promote a correct solution to a
runtime-input workload only in a separately frozen measured experiment.

Classify each failure as model misunderstanding, documentation/diagnostic gap,
implementation gap against an existing contract, missed optimization, deliberate
restriction, or evaluation-infrastructure failure. Correctness denominators
include failed solutions; performance comparisons include only solutions with
the same verified semantics and inputs. No aggregate score mixes the two.

## 6. V0 output and continuation

V0 produces the private evidence record, a bounded C0 candidate, any justified
O0 candidate, and the S0A/B handoff inputs in §4. Missing metrics and unavailable
AI/service lanes have owners and explicit implications. A failed R1/R2 native
qualification blocks claims about that source path, but does not require
discarding valid R0 evidence or restarting a completed independent row.

Record a capability-level status once in the live handoff. Do not paste raw
measurements, per-command journals, or operational PR/SHA narration there.
Continue into the next unfinished bounded design/verification task; S0/S1
implementation must still pass its exact owning ledger and ordinary code gates.
