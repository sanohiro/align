# Execution, Storage, and Startup Optimization Plan

> **Status:** UMBRELLA DESIGNED; every slice remains unapproved and
> unscheduled. No implementation slice is active.
>
> **Queue position:** `HANDOFF.md` remains the live work queue. `core.codec`
> remains the next language capability. This track consumes no language
> milestone and must not displace that work without an explicit scheduling
> decision.
>
> **Contract boundary:** this document approves no syntax, language semantic,
> library API, CLI, runtime ABI, environment variable, persisted profile, or
> distribution change. A later slice that needs one must first amend its
> authoritative owner with an exact public-contract ledger.

This plan adopts one direction from the 2026-08-31 execution/storage/startup
proposal:

> Reduce bytes moved, simultaneously live working sets, ownership transfers,
> runtime wakes, loaded pages, and hot code footprint before adding instruction
> tricks or source-level tuning controls.

Align already has the semantic facts needed to make many of those decisions:
ownership, regions, effects, explicit parallelism, source shape, static-input
identity, target/profile identity, and backend-agnostic MIR. The missing piece
is not a universal optimizer. It is a disciplined way to expose the decisions
made by existing selectors, introduce shared facts only when a real consumer
needs them, and admit each new strategy from measured evidence.

The original proposal is accepted only with the corrections in this plan. In
particular, virtual `chunks` lowering and byte/work-aware parallel grain already
exist in narrower forms; runtime partitioning and hot/cold guidance are already
recorded deferrals; `std.log` is implemented; and `core.codec` remains next.
This document consolidates those tracks without taking ownership away from
their narrower plans.

---

## 1. Authority and ownership

Read the narrow owner before scheduling a slice:

```text
../../HANDOFF.md
../design-notes.md
../open-questions.md
09-explain-opt.md
10-cache-first-optimization.md
11-parallel-execution-optimization.md
12-pipeline-closure-memory-io-simd-audit.md
13-string-array-allocation-short-input-audit.md
14-llm-inference-focus-audit.md
17-library-boundary-prerequisites.md
19-hir-validation-ledger.md
20-runtime-abi-ledger.md
21-build-perf-plan.md
22-repl-plan.md
23-friction-ledger.md
```

This plan owns only:

- the common execution/storage/startup design rules;
- the ordering and admission gates of this optimization track;
- the cross-slice closure axes; and
- the rule that performance claims require named evidence.

The narrower documents continue to own their existing contracts and shipped
status. A conflict is resolved in favor of the language sources of truth and
then the narrow capability owner, not this umbrella plan. `HANDOFF.md` alone
selects the current task.

## 2. Public-contract freeze ledger

The plan begins with a closed public surface. Future sketches in this document
do not reserve names or authorize implementation.

| Public surface | Decision in this plan | Owner if revisited |
|---|---|---|
| Syntax and annotations | None. No `hot`, `cold`, `flat`, `branchless`, `phase`, `no_rt`, worker, grain, affinity, or prefetch spelling. | `draft.md`, `language-spec.md`, `open-questions.md`, and the friction ledger |
| Evaluation and errors | No change to evaluation order, hard errors, recoverable errors, result order, or trap behavior. | Language specification and the relevant MIR owner |
| Ownership and lifetime | No change to Copy/Move, regions, escape, Drop order, or visible allocation. | Language specification, memory model, and HIR/MIR ledgers |
| Parallel semantics | No implicit parallelism and no change to stable result order or Pure admission. | Language specification and `11-parallel-execution-optimization.md` |
| `core` / `std` / `pkg` types and signatures | None. There are no arguments, defaults, results, or new allocation/lifetime rules to specify. | The future consumer's English design ledger |
| CLI | No command or option. Existing `explain-opt` output is unchanged by this design. | `09-explain-opt.md` |
| Runtime ABI | No key, symbol, signature, tag, or ownership change in the foundation. | `20-runtime-abi-ledger.md` |
| Native text/view input | None. Therefore no new encoding, embedded-NUL, validation-order, or pre-side-effect rule is introduced. | The slice that introduces the boundary |
| Build input | None. Ambient tuning and implicit profile discovery are forbidden. | Cache plan plus the introducing slice |
| Persisted or exchanged format | None. No planner, startup, shape-profile, or static-artifact schema is approved. | The introducing slice's exact format ledger |
| Cache identity | No new identity component in the observation foundation. | Cache plan plus the first codegen consumer |
| Artifact or sidecar | None. Hidden sidecars are forbidden. | Static-input owner and the explicit consumer |
| Distribution | No target, runtime variant, or link policy change. | Release distribution and build-performance owners |
| Performance promise | None. This plan defines measurement discipline, not a numeric guarantee. | The slice-specific evidence record |

`draft.md`, `language-spec.md`, library designs, and their Japanese mirrors do
not change because no normative language or library promise changes. This
internal implementation plan is English-only and has no repository mirror.

A later public or persisted format must fix scalar widths, tags, record order,
unknown-value encoding, malformed-input rejection, validation precedence, and
semantic-to-byte and byte-to-semantic golden vectors before implementation. A
later runtime operation that changes process-global state must also define
overlap exclusion, the failed-second-operation result, exhaustion, failure
cleanup, and Drop restoration order.

## 3. Shipped foundations

The first implementation must extend the current system rather than recreate
it under new names.

| Area | Shipped foundation | Consequence for this track |
|---|---|---|
| Layout | `soa<T>`, field projection, direct JSON-to-SoA paths, and measured cache-density evidence | Report actual touched bytes; never generalize one corpus into an across-language ratio. |
| Pipelines | Fused loops, guarded callable paths, vector-capable shapes, and mask/select reductions | Preserve the distinction between Pure and safe speculative execution. |
| Storage reuse | Unique, dead heap temporaries can donate compatible storage to selected materializers | Expose the existing selection and rejection reasons before adding another reuse model. |
| `chunks` | Direct length/index are virtual; stored and pipeline/parallel consumers still materialize headers | S1 extends the shipped virtual source only to additional nonescaping consumers. |
| Parallel execution | Range kernels, stable count/prefix/scatter, integer reductions, caller draining, task batching, and byte/work-aware grain | Do not add per-element callbacks, per-task locks, or a second scheduler policy vocabulary. |
| Cold parallel path | Below-floor `par_map` and single-task `task_group` avoid pool initialization | Every later execution domain preserves lazy initialization. |
| Memory model | Regions, escape checking, path-local ownership state, exactly-once Drop, explicit heap/arena, and package resources | Planning consumes these proofs; it does not infer a second ownership model. |
| Static input | Explicit compiler-known constructors, content identity, versioned artifacts, and cache separation for owned consumers | Static baking extends a named consumer; it is not general CTFE or a build script. |
| Build | Frontend/object caches, function-level ThinLTO reuse, runtime LTO, instrumented PGO, capability linking, and size profiles | New codegen inputs join existing action identity rather than creating an ambient cache. |
| Inspection | Stable optimization records, source anchors, `explain-opt`, and `alignc size` section/relocation/dependency reporting | Reuse their owners. JSON and a planner output schema remain unapproved. |
| Process measurement | Parent-owned launch/timing machinery exists in the REPL and test harnesses | Reuse lifecycle rules where appropriate; do not share mutable global harness state. |

The remaining opportunities are narrower than the original proposal implied:

```text
no shared vocabulary for current execution/storage decisions
some nonescaping chunks consumers still materialize header arrays
eligible straight-line fixed region scratch uses separate runtime allocations
CPU work and blocking work lack one transitive compiler classification
runtime archive co-location can retain unnecessary code/native dependencies
no parent-observed launch-to-reap startup benchmark family owns the evidence
no bounded Align collection-shape profile is an explicit build input
hot/cold guidance has no exact firing contract
no general mapped static artifact exists outside named consumers
Unix I/O strategies remain consumer-specific measure-first candidates
```

## 4. Design

### 4.1 Semantic boundary

Checked HIR and semantic MIR continue to determine evaluation order, errors,
ownership, regions, cleanup, effects, and parallel admissibility. Planning may
choose only among observationally equivalent implementations. MIR-to-LLVM
remains pure lowering and must not become a second semantic layer.

Conceptually:

```text
source
  -> checked HIR
  -> semantic MIR
       ownership / region / cleanup / effects / source provenance
  -> fact views owned by the selecting capability
  -> execution, storage, or artifact decision
  -> planned MIR node or private side table
  -> LLVM lowering
  -> existing optimization pipeline
```

This is not initially one new pass or crate. Current donation, fusion, and
parallel choices occur at different points, including HIR-to-MIR lowering. S0B
therefore instruments the owner at the point where it makes the decision. A
later refactor may move a selector only when its producer/consumer land in the
same useful capability slice.

### 4.2 Facts are proof states, not optimistic booleans

An inferred property has three logical states:

```text
proved true
proved false
unknown
```

An implementation may encode that as `Proof<bool>`, a dedicated enum, or an
equivalent exhaustive representation. It must not encode unknown as the value
that enables an optimization. Every new HIR/MIR/type/runtime-key variant must
fail closed until its owner classifies it.

Semantic effect and optimization legality are independent axes. At minimum, a
slice that speculates or reroutes work must consider independently:

```text
semantic effect: Pure / Impure / Unknown
may trap
may allocate
may block
may diverge
memory reads, writes, capture, and aliasing
inactive-element execution legality
available scalar/vector lowering
work estimate and its evidence source
```

`Pure` alone never proves totality, allocation freedom, inactive-lane safety,
nonblocking behavior, or a vector form. Vector availability is not a member of
an inactive-lane-safety enum. Unknown higher-order targets and unaudited native
boundaries remain unknown.

The existing canonical semantic effect remains the sole effect authority; an
optimization view references it rather than serializing a second copy. Before
any other fact is consumed, its slice fixes the producer and conservative join
for direct calls, recursive SCCs, generic monomorphs, function values,
higher-order targets, imports, audited runtime keys, and FFI.

No broad `FnOptSummary` or `KernelFacts` record lands ahead of a consumer. The
first slice introduces only the fields consumed by the current selection it
observes. Cross-unit serialization begins only when an imported fact changes a
caller's plan.

Existing `align_mir::source_shape` names and canonical type-graph machinery
remain authoritative. A pipeline-specific view, if needed, uses a distinct
name such as `KernelSourceKind`; it must not shadow or fork `SourceShapeView`.

### 4.3 Semantic classes and optimization actions stay separate

MIR-owned semantic classes constrain planner actions. They are not choices in
one strategy enum:

```text
explicit execution class
  sequential / explicit parallel / source-resolved foreign operation

semantic storage class and role
  source, output, or scratch x the existing provenance lattice and allocation mode

optimization action
  preserve / fuse / guard / virtualize / donate / pack / select bounded kernel
```

An ordinary sequential pipeline cannot become parallel, a heap allocation
cannot become region-owned, and an ordinary call cannot become a shaped foreign
operation merely because a cost model prefers it. Those boundaries are already
fixed by source semantics, checked HIR, MIR, or a consumer contract.
Storage reasoning consumes the existing `Static` / `Frame` / `Arena(k)`
provenance lattice, fixed-frame and heap allocation modes, and owner generation;
it does not flatten them into a new storage enum.

The shared vocabulary describes independent questions inside those boundaries:

| Decision axis | Example current answers | Later answers require |
|---|---|---|
| Execution strategy | fused or guarded serial; range, stable count/prefix/scatter, or reduction inside an explicit parallel class | The same semantic class, exact legality proof, and fallback |
| Storage action by role | preserve borrowed/owned storage, donate an eligible output, or virtualize a temporary source | Ownership/allocation/Drop closure; no change to semantic storage authority |
| Materialization | none, required boundary, required escape, required layout conversion | Exact reason and owner test |
| Runtime selection | compile-time selected, or a bounded runtime selector among named candidates | Exact selector inputs; never a claim about the path a future run took |
| Runtime domain | caller-only or current shared worker path | Transitive blocking facts and measured domain policy |
| Artifact strategy | preserve the consumer's current representation | An explicit consumer, exact schema, validation, identity, and lifetime |

An internal decision record may carry a decision kind, selected strategy,
stable reason, source anchor, and separately named estimates. Storage decisions
are an ordered role list because one pipeline can have distinct source, output,
and scratch storage. A record distinguishes a compile-time selection from a
bounded runtime selector and must not report that a pool initialized, a grain
was used, or a candidate ran when those depend on runtime values. The exact
enum, field presence, order, and unavailable-value rule belong to the first
output consumer's ledger. No dormant strategy variant is predeclared.

There is no aggregate performance score. Useful independent observations
include materialization count, owned-allocation count, estimated bytes read and
written, donation selection/rejection, stable-order strategy, domain/grain
reason, vector/guard reason, and evidence source. Estimates are explanations,
not claims of cache residency, hit rate, page faults, or speedup.

### 4.4 Nothing hidden

The optimizer may remove compiler-internal storage or a provenance-proved
unbound temporary, copy, materialization, runtime call, or wake when
equivalence is proved. It does not erase a named or aliased materialization or
cross a source-declared storage boundary. It may not silently add a
source-visible allocation, recoverable failure, parallel region, ownership
transfer, blocking boundary, external artifact, or process-global policy.

Working-set co-location is scoped to one owner and phase. It never means one
process-global arena. Global packing would couple lifetimes, contention,
false-sharing, and cross-core ownership.

### 4.5 Identity

The following identities remain distinct:

```text
semantic interface
  public type, parameter mode, region, effect, and ownership contract

optimization import
  only facts required to plan an importing caller

codegen action
  existing complete codegen/ThinLTO action identity from 10-cache-first-optimization.md
  + versioned digest of only the imported optimization facts this action consumes
```

S0B observation does not change object identity. Located source anchors are
diagnostic data and remain outside normal semantic/codegen identity.

If a private implementation fact later changes caller codegen, the imported
summary receives an exact schema version and digest and participates in every
existing codegen/ThinLTO action that consumes it, as defined by
`10-cache-first-optimization.md`. That
invalidates dependent codegen without pretending source compatibility changed.
Whole-program, per-unit, cold-cache, and cache-hit compilation must select the
same normalized plan.

A future shape profile is an explicit, versioned, content-addressed build
input. Its digest affects only the action that consumes it. Target/profile
choices affect object/link identity, never type identity. Absence, mismatch,
unsupported target, and malformed input all select the documented fail-closed
path before codegen side effects.

### 4.6 Runtime policy stays bounded

The AOT compiler selects either a complete strategy or a bounded runtime
selector. The runtime may execute a bounded range claim, choose among
preapproved CPU variants, or apply a measured threshold. Runtime worker count
or host load does not enter artifact identity. The runtime does not become a
general adaptive optimizer or accept ambient tuning.

## 5. Priority classes

These labels express admission order, not bug severity.

| Class | Work | Admission rule |
|---|---|---|
| P0 | Startup baseline and observation of current decisions | May be promoted first. Observation changes no codegen. |
| P1 | Extend virtual `chunks`; introduce exact speculation facts only with their first consumer | One structural win, one fallback, and a complete closure matrix per slice. |
| P2 | Fixed straight-line region frames; measured runtime partition; blocking-domain selection; bounded shape input; hot/cold guidance; consumer-owned static artifacts; Unix I/O portfolio | Requires a named workload or consumer and predeclared evidence gate. |
| P3 | User-kernel multiversioning; cleanup/code-layout refinements; `io_uring`; freestanding/minimal target | Remains deferred until evidence defeats the simpler shipped path. |

P0 makes current choices observable. It must not first create a broad fact
model and then search for consumers.

## 6. Roadmap

The wave number states dependency, not a reserved PR or current task. Only the
P0 entries are candidates for promotion. S1 and later remain unscheduled until
their individual gates are met.

### S0A — Parent-measured startup baseline

**Status:** ELIGIBLE FOR A SLICE LEDGER; not scheduled.

Create a benchmark family that measures from the parent immediately before
process creation through child observation/reap. An in-program clock is not a
launch-to-reap startup measurement because it excludes process launch, loader,
relocation, CRT, entry-wrapper, and reap cost.

Initial fixtures use only shipped capabilities:

```text
empty i32 entry
empty Result entry
argv-consuming entry
first primitive output
first arena allocation/reset
below-floor par_map
first pool-using par_map
two pool-using operations in one child
single-task task_group
small embedded read-only table first touch
```

A mapped-artifact fixture joins only with S5. It must not be a dormant S0A
format producer.

The evidence row records, separately where available:

```text
parent-observed launch-to-reap wall time
child user and system CPU time
minor and major page faults
voluntary and involuntary context switches
peak resident set
```

Executable/section size, requested/final native dependencies, and any mapped-
image inspection are untimed companion records bound to the same artifact
digest. The timed child is not paused, probed, or given extra runtime exports
to obtain them.

Unavailable metrics are unavailable, never zero. The implementation ledger
must fix the record format and integer widths before a machine-readable result
is persisted.

Every run binds the source revision, target triple, CPU identity, OS/kernel,
toolchain, optimization/size profile, linker, libc/link policy, strip policy,
runtime capabilities, executable digest, fixture digest, and explicit
environment. The harness invokes absolute paths and does not infer tuning from
the caller's environment. Existing measurement-only compiler toggles are
explicitly absent unless the named experiment is testing one.

The slice ledger also fixes each fixture's exact source, stdout/stderr and exit
oracle, clock and units, timeout, warmup and iteration count, balanced schedule,
whether descendant usage is included, counter overflow behavior, and the
precedence of spawn failure, timeout, signal, wrong output, and partial metric
collection. A failed sample is never silently dropped into a successful
distribution.

Fresh-child samples measure process startup. Candidate/control order is
balanced, raw samples are retained, and a distribution is reported; a minimum
sample is not an adoption statistic. Warm-page-cache and qualified cold-cache
claims are distinct. A cold-cache claim is accepted only on a controlled Linux
host whose storage identity and eviction procedure are part of the evidence.
macOS remains a native development lane unless a slice explicitly admits it.

The control is built under an explicitly recorded equivalent target, linker,
libc/link, and strip policy. Static and dynamic binaries are never compared as
if link policy were an implementation result.

CI owns structural correctness, not timing thresholds. Pool initialization is
checked by a separate runtime owner; adding a probe to a timed artifact would
change the link closure being measured:

```text
entry forms preserve exit/error behavior
below-floor parallel and single-task task_group avoid pool initialization
capability-free fixtures do not gain an unowned constructor or dependency
the parent distinguishes spawn failure, child failure, signal, and success
```

Reuse parent-process lifecycle rules from the REPL or test harness only after
the owner proves compatible descriptor, process-group, timeout, and cleanup
semantics. Do not share mutable global harness state.

No S0A implementation begins until its exact slice ledger and lifecycle
closure matrix are complete. A new process-lifecycle strategy receives an
independent design review before implementation.

### S0B — Current-decision observation

**Status:** DESIGN-DEPENDENT; not scheduled.

Instrument existing selectors at their actual decision points and collect
only the facts they already use. Generated code must remain byte-for-byte
equivalent after normal nondeterminism normalization.

The first visible consumer is the existing `explain-opt` command. Before code
lands, `09-explain-opt.md` must define the exact record kind, presence rules,
row and construct-ordinal order, reason precedence, unavailable values,
source-anchor absence, default/verbose behavior, and whole/per-unit parity. It
must distinguish LLVM remarks from planner decisions and compile-time selected,
rejected, bounded-runtime-selected, not-applicable, and unavailable states,
including duplicate-suppression rules. This plan does not approve JSON or a
new option.

The same ledger fixes how existing owner-controlled measurement toggles, such
as the donation disable control, affect decisions and cache isolation. S0B adds
no new environment input and never presents a toggle-selected record as the
default plan.

S0B is complete only when:

```text
each admitted current selector emits exactly its specified decision record
whole-program and per-unit normalized decisions agree where both located routes exist
repeat located reports agree after deterministic normalization
unknown imported facts fail closed
located reporting does not alter ordinary object identity
new source/type/MIR/runtime-key variants cannot fall into an optimistic default
report collection changes neither MIR selection nor generated code
located reporting remains ephemeral and leaves normal cache eligibility and counters unchanged
```

No cross-unit optimization summary is introduced until a later caller-side
selector consumes it. If any S0B collection work enters ordinary compilation
rather than the located reporting route, the slice must predeclare and pass a
compiler wall-time/RSS guard; the preferred foundation leaves normal builds
untouched.

### S1 — Extend the shipped virtual `chunks` source

**Status:** MEASURE-FIRST; not scheduled.

Extend existing direct virtualization to additional nonescaping synchronous
pipeline and explicit-parallel consumers that still materialize
`array<slice<T>>` headers. This is not a lazy collection model and does not add
a source type.

The eligible path proves:

```text
positive chunk size selects the virtual path
nonpositive chunk size preserves the existing canonical empty result before division or pointer work
the chunks value is consumed directly and synchronously
it is not stored, returned, captured, sent, retained, or passed opaquely
base owner generation and region roots remain available
the consumer already accepts borrowed slice elements
stable order and final partial-chunk behavior are unchanged
trap, error, effect, and callable order are unchanged
```

The kernel derives each header in SSA without wrapping the tail calculation:

```text
start      = checked_mul(chunk_index, chunk_size)
remaining  = element_count - start       // after proving start <= element_count
length     = min(chunk_size, remaining)
end        = start + length               // proved <= element_count
byte_offset = checked_mul(start, stride)
```

The implementation fixes the integer-width conversions and validation
precedence before emitting a pointer. `min(start + chunk_size, element_count)`
is not accepted because Align integer addition wraps. Ineligible paths retain
the current materialization. Owners cover empty, exact-multiple, partial-tail,
fixed-frame arrays and literals, frame-backed slices, arena/heap/static bases,
sequential and supported parallel consumers, imported callables, all control
exits, whole/per-unit parity, and maximum count/chunk/stride arithmetic. The
positive owner checks absence of the header-array allocation/runtime
materializer; the fallback owner checks unchanged escape and lifetime behavior.

The shipped direct lowering and its prior measurement are the baseline. S1
must demonstrate incremental benefit on a still-materializing consumer rather
than claim invention of virtual chunks.

### S2 — Straight-line fixed region frames

**Status:** CONSUMER- AND EVIDENCE-GATED; not scheduled.

Coalesce a cluster of fixed-size scratch allocations into one aligned region
frame only when all allocations:

```text
belong to the same region, task, and ownership domain
have compile-time checked size and alignment
execute on the same straight-line path
are separated by no branch, loop edge, early exit, recoverable failure,
or observable side effect
have no intervening operation, other than the cluster allocations, that is not
proved total, nonallocating, nonblocking, and nondiverging
use the existing generated-MIR `ArenaAlloc` path, whose public runtime ABI is conservatively zeroed
need no individual Drop beyond the region's admitted cleanup class
escape only through views already admitted by that region
have a checked total size and target-layout offset calculation
```

The initial slice admits only an immediately consecutive semantic allocation
cluster, apart from the proved compiler-internal operations above. The frame
allocation occurs at the first original allocation point, and the original
evaluation and first-terminal-failure order is unchanged. No pointer is
published before the checked layout and allocation succeed. Branch-local,
loop-local, dynamically sized, fallible, individually dropped, or
cross-task storage stays on the current path. Compiler-selected `Uninit` frames
are deferred until a later design supplies the exact fact producer, runtime ABI,
per-subrange first-write coverage, Drop safety, and every native-read boundary.

S2 may promise fewer runtime allocation calls for the admitted cluster and an
exact layout with no unaccounted padding. Reduced metadata traffic, working
set, cache misses, RSS, or elapsed time are measured outcomes, not consequences
asserted by construction.

The closure matrix must include hard-allocation failure, zero size,
initialization class, alignment/size overflow, every early-exit form around an
ineligible cluster, replacement and return paths, nested regions,
whole/per-unit lowering, and allocation/cleanup parity.

### S3 — Measured runtime capability partition

**Status:** OPEN-QUESTION GATED; not scheduled.

Resume the existing runtime-staticlib feature-split question only after a
closure report demonstrates retained co-location that matters for a named
binary through deployment size, supply-chain surface, or startup pressure.
Build the report from:

```text
RuntimeKey and UnkeyedRuntimeKey ownership
compiler capability reachability
runtime-to-runtime calls
native-library dependencies
object/archive strongly connected components
the emitted link command and requested native-library closure
final section, relocation, and needed-library evidence
```

Select the smallest partition that removes the measured closure. Candidate
areas are observations, not prescribed crates. Keep one exhaustive symbol/ABI
registry, one target rule, deterministic capability selection, and identical
whole/per-unit/runtime-LTO closure. A row has one implementation owner; a
partition may not duplicate symbols, allocators, process-global state, or ABI
tables.

The slice updates `20-runtime-abi-ledger.md`, the release/build owners, and the
open question before implementation. The emitted link command is the primary
proof of requested libraries; final `DT_NEEDED` or platform equivalent is
outcome evidence, not a substitute. If archive/member or codegen-unit changes
do not remove the measured dependency or size cost, retain the current runtime.

### S4 — CPU and blocking execution-domain decision

**Status:** WORKLOAD-GATED; not scheduled.

The existing bounded blocking claim-loop patterns and the current shared
parallel pool are the starting points. First prove a transitive `may_block`
fact across visible bodies, generated wrappers, imported summaries, generic
instantiations, higher-order targets, and audited runtime/native keys. Unknown
routes to the conservative domain or rejects the specialized plan; it never
routes optimistically to a CPU pool.

Only then compare bounded policies for a named mixed workload. Any new domain
keeps lazy initialization, bounded queues/claims, caller participation rules,
stable result order, first-error behavior, nested work behavior, and complete
shutdown/Drop semantics. The slice must define concurrent initialization,
failed second initialization, exhaustion, task panic/hard-error containment,
and process teardown before changing global runtime state.

No worker count, affinity, grain, or domain annotation enters source. If a
single pool with improved batching wins the preregistered evidence gate, S4
closes with that result instead of forcing a split.

### S5 — First consumer-owned zero-parse artifact

**Status:** CONSUMER-GATED; not scheduled.

Generalize the shipped static-input/artifact substrate only for a named
consumer whose repeated validation or parse cost is measured. `core.codec` is
a possible future consumer, not an implicit prerequisite or an authorization
to change its public wire contract.

The consumer ledger must fix:

```text
source input and explicit build declaration
path/text encoding, embedded-NUL behavior, and pre-side-effect validation
semantic schema and complete reachable structural type graph
canonical byte format, widths, tags, order, alignment, and endianness
producer/toolchain/target/schema identity and content digest
inline versus external-artifact rule
mapping/open/validation order before any observable side effect
malformed/truncated/oversized/unsupported rejection precedence
ownership, borrowed-view lifetime, mapping owner, cleanup, and Drop
whole/per-unit/cache-hit and distribution behavior
semantic-to-byte and byte-to-semantic golden vectors
fallback when mapping or the target format is unavailable
```

No host pointer, padding byte, source span, filesystem mtime, or unstable map
order enters the canonical artifact. External artifacts are explicit build
outputs, never hidden sidecars. A mapped artifact is read-only until a later
ledger proves a writable ownership model.

### S6 — Bounded shape input and hot/cold guidance

**Status:** PROFILE-SCHEMA AND CORPUS GATED; not scheduled.

A shape input is admitted only when one selector has a demonstrated decision
that static facts cannot make well. It is explicit, bounded, versioned,
content-addressed, target/profile bound, and absent by default. The exact
schema, merge rule, count saturation, discriminator coverage, validation
precedence, and cache identity are fixed before implementation.

Initial candidates are length/filter-selectivity/element-byte buckets, never
raw user data. A mismatched or malformed profile fails according to its exact
ledger before codegen output is published. There is no ambient search path and
no runtime online optimizer.

Hot/cold work begins as a deterministic suggestion over a reviewed corpus. It
does not automatically change struct layout because explicit layout is
settled. The firing contract names the access evidence, minimum confidence,
source anchor, false-positive corpus, and reason. Existing LLVM PGO hotness may
inform code layout, but it is not an Align collection-shape profile. Advice
derived entirely from MIR belongs to the check-time lint owner and is not
duplicated as an `explain-opt` planner decision.

### S7 and later — independently admitted candidates

The following are separate failure domains and never one catch-all PR:

- user-kernel multiversioning after code-size/startup and workload evidence;
- cleanup/hot-code layout after the large-Drop/build-performance owners expose
  a remaining regression;
- `sendfile`, `splice`, additional mmap/advice/transfer strategies, direct I/O,
  or `io_uring` behind a named `std`/`pkg` consumer and its existing semantic
  surface; and
- a freestanding/minimal target only with an explicit target, runtime subset,
  startup object, allocator/error/entry contract, distribution artifact, and
  conformance matrix.

`io_uring` is never a universal backend. Unsupported filesystems, alignment,
cancellation, partial completion, resource exhaustion, and fallback must be
owned by the consumer before it can win a measured strategy portfolio.

## 7. Cross-cutting implementation closure

Before changing ownership, cleanup, FFI, ABI, an IR variant, or three or more
compiler layers, the scheduled slice replaces each applicable `required` cell
below with implementation locations and exact owner tests. A non-applicable
cell records why. Do not begin code from this template alone.

S0A and S0B remain separate failure domains in the closure record:

| Closure axis | S0A startup baseline | S0B current-decision observation |
|---|---|---|
| Formation and validation | Exact fixture, artifact, environment, schedule, and metric availability | Exact current-selector and record-state coverage |
| Construction | Parent launch, observation, sample construction, and terminal classification | Observation only at the existing decision seam |
| Move-in / move-out / source nulling | N/A for Align values; every parent fd/process handle has one owner | N/A; no new Move value or serialized summary |
| Replacement and return | Sample/result replacement and parent return | Located report construction only |
| Drop and cleanup | Child, process group if any, fd, timeout, and reap closure | Report scratch cleanup; ordinary compiler cleanup unchanged |
| Branch, early exit, and malformed input | Spawn failure, timeout, signal, wrong output, missing metric, partial observation | Selected, rejected, runtime-selected, not-applicable, unavailable, and malformed internal fact |
| Generic and interface serialization | N/A; no language interface or generic value | Existing imported-fact parity only; no new serialization |
| Whole-program / per-unit / cache behavior | Exact fixture build policy is bound to the evidence row | Supported located routes agree; located mode remains ephemeral and normal cache eligibility/counters stay unchanged |
| Runtime ABI and ownership provenance | Unchanged; measurement probes stay out of the timed artifact | Unchanged |
| Allocation parity | Harness allocations are bounded outside the timed child contract | Observation changes neither selected MIR nor generated allocation |
| Target/profile behavior | Platform metric availability and unavailable reasons are exact | Normal target/profile selection is observed, not changed |
| Explanation and reason | Every failed or unavailable sample is typed | Exact reason precedence, row order, source absence, and duplicate suppression |

Later slices fill their own cells before implementation:

| Closure axis | S1 virtual chunks | S2 region frame | S3 runtime partition | S4 domain | S5/S6 artifact or profile |
|---|---|---|---|---|---|
| Formation and validation | Chunk-size and source eligibility | Size/alignment/init-class/cluster eligibility plus intervening-operation totality | Registry and closure graph | Blocking fact provenance | Schema/input validation |
| Construction | SSA slice header | One checked frame and offsets | Unique symbol/archive owner | Task/domain record | Canonical producer/decoder |
| Move-in / move-out / source nulling | Base and borrowed-header provenance | Each subobject's ownership bit | Runtime owner not duplicated | Captures and task result | Mapping/profile owner and views |
| Replacement and return | Escaping chunks fallback | Frame-ineligible fallback | Link replacement parity | Result/error return | Mapped view cannot outlive owner |
| Drop and cleanup | Base cleanup unchanged | Region reset and individual-Drop exclusion | Allocator/global/runtime Drop unique | Queue/pool shutdown | Unmap/close/temp cleanup |
| `if` / `match` / `else` / `?` / `map_err` | Order and fallback through joins | Cluster rejection across edges and unchanged first failure | Initialization errors | First error and cancellation | Validation precedence before effects |
| Loop, branch join, early exit, malformed input | Direct use versus escape | No cross-edge packing or hoist across trap/allocation/block/divergence | Malformed registry/report | Nested work/exhaustion | Truncated/oversized/unknown tags |
| Generic and interface serialization | Imported callable/source shape | Generic layout | Registry parity | Transitive imported fact | Structural graph and schema digest |
| Whole-program / per-unit / cache behavior | Required | Required | Required | Required | Required |
| Runtime ABI and ownership provenance | Unchanged unless separately ledgered | Arena owner | Exact ledger update | Domain state and callbacks | Mapping/native owner if any |
| Allocation parity | Header allocation absent only on admitted path | One frame only on admitted path | No duplicate runtime state | Bounded queues/tasks | Bounded decode/map/profile storage |
| Target/profile behavior | Same semantics | Target layout checked | Link-command and final-image evidence | Target capability fallback | Target/schema/endian fallback |
| Explanation and reason | Selected versus exact fallback | Selected versus exact fallback | Measured retained closure | Fact and policy reason | Source/default/profile/artifact reason |

One parameterized invariant owner may close many cells. Reuse an existing test
when it would fail for the changed defect. When a review finds one missed
variant or cleanup edge, audit the entire slice for that root-cause class and
repair the matrix before patching individual lines.

## 8. Evidence and adoption

Correctness is never traded for a benchmark result. Each performance slice
first closes:

```text
semantic equivalence
ownership and exactly-once cleanup
whole-program/per-unit/cache parity
target and malformed-input fallback
bounded resource behavior
explanation of selection and rejection
```

Only an explicit performance or resource promise requires a benchmark. Before
measurement, the slice records:

```text
named workload and why it represents the consumer
baseline and candidate revisions/artifact digests
host, target, toolchain, linker, profile, and runtime capability identity
primary and guard metrics
sample count, warmup, ordering, statistic, and outlier rule
minimum material benefit and maximum admitted regression
revert or defer decision when the gate is missed
```

Use balanced candidate/control measurement and report independent metrics. A
strategy is adopted only when the primary metric meets its preregistered gate,
guard metrics stay within their bounds, binary size/startup costs are counted,
and the simpler fallback remains correct. Microbenchmarks guide a selector;
they do not justify a global default for unrelated workloads.

Ordinary CI checks deterministic correctness and structural invariants. Host-
sensitive performance evidence is retained separately and is not converted
into a flaky timing gate.

## 9. Scheduling rules

1. `HANDOFF.md` continues to name `core.codec` as the next language capability.
   This track remains independent and consumes no milestone.
2. S0A and S0B are independently useful capabilities and may be promoted
   separately. Neither requires S1.
3. S0B cannot begin until the exact `09-explain-opt.md` extension is reviewed.
4. S1 is the first structural candidate after current decisions are observable,
   but only for a still-materializing named consumer.
5. S2 and S3 are independent. Schedule either only from its own evidence and
   closure matrix.
6. S4 requires transitive blocking facts and a mixed workload before a runtime
   topology decision.
7. S5 belongs to its first public consumer. It does not pull that consumer
   ahead in the language roadmap.
8. S6 requires a selector that static facts demonstrably cannot choose and an
   exact explicit-input schema.
9. S7 candidates remain deferrals until individually promoted.
10. A wave that changes a public contract, ABI, persisted format, or safety
    strategy updates its authoritative ledger first and receives a fresh
    adversarial design review before code.

## 10. Rejected forms

This track explicitly rejects:

- source tuning annotations or a second parallel/ownership vocabulary;
- a universal optimizer pass or runtime adaptive scheduler;
- a process-global packed arena;
- treating Pure as total, nonallocating, nonblocking, or inactive-lane safe;
- boolean facts whose default enables an optimization;
- predeclared planner variants, summaries, or cache schemas without consumers;
- package-name compiler special cases;
- automatic AoS/SoA or hot/cold public-layout changes;
- branchless lowering when masked-off work can trap, allocate, block, diverge,
  or change evaluation;
- one opaque performance score;
- hidden profile discovery, sidecars, or build scripts;
- an unvalidated native-memory-dump artifact;
- eager all-in-memory I/O as a universal policy;
- `io_uring` as a cross-platform default; and
- static-versus-dynamic or unlike-target startup comparisons presented as an
  optimization result.

## 11. Relationship to align-LLM

The align-LLM audit remains evidence about external consumers, not permission
to put planner vocabulary into Align source. Model loading, KV caches,
continuous batching, paged attention, device placement, speculative decoding,
and request scheduling belong to the engine or package that owns them.

This track may supply only selective substrate with independent value, such as
lower materialization, explicit static artifacts, bounded blocking
classification, or explainable plan selection. No `tensor`, model-loader,
attention, token-scheduler, or GPU-runtime surface is added to core for this
plan.

## 12. North star

The successful endpoint is not a large planner API. It is ordinary Align code
whose compiler can explain and prove that it:

- moves only required bytes;
- materializes only at ownership, layout, or escape boundaries;
- reuses owned storage only when provenance permits;
- groups scratch storage by one owner and phase;
- keeps cold binaries free of unused runtime closure;
- sends work to a domain only from proved facts;
- starts without unowned eager initialization;
- consumes static data without repeated parse work when a consumer owns that
  contract; and
- records why each faster path was selected or rejected.

The source language stays smaller while the implementation becomes more
predictable, measurable, and explainable.
