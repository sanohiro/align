# M13 Slice 3 design: optimized-IR emission, the vectorization IR-shape suite, and `explain-opt`

Settled 2026-07-11 by a two-lens design review (compiler-integration lens / user-surface +
future-AI-loop lens, integrated by the orchestrator). This is the implementation source of truth
for roadmap M13 Slices 3a and 3b. Context: `07-roadmap.md` M13; adoption boundaries in
`open-questions.md` Open → "External optimization consultation".

## Empirical baseline (probed 2026-07-11, LLVM 19)

- `emit_llvm_ir` builds the module but never runs passes; only `write_object` runs
  `default<O2>` (`run_passes`). "Optimized IR" is a new emission path, not a flag rename.
- Vectorization fires today: `xs.map(dbl).sum()` → `vector.body` + `llvm.vector.reduce`
  (`<4 x i64>` at `x86-64-v3`); `where(big).sum()` → masked `<4 x i1>` if-conversion.
  `scan(0, add)` (loop-carried) correctly does NOT vectorize — a clean negative control.
- Remarks fire today (`-pass-remarks*`) but anchor to `<unknown>:0:0`: codegen emits **zero**
  DILocations and MIR carries **no source spans** (spans are dropped at HIR→MIR). Source
  anchoring is therefore a hard prerequisite of useful remarks, and the single real plumbing
  cost in the slice.
- inkwell 0.9 / llvm-sys 191 expose **no remark-streamer or structured-remark C API**
  (`setupLLVMOptimizationRemarks` is C++-only; nothing in the C API writes remark YAML during
  `LLVMRunPasses`). The only working capture path is the diagnostic handler (Mechanism A below),
  which yields a **flat** `"<file>:<line>:<col>: <message>"` string per remark — no structured
  pass name / RemarkName / args without a C++ shim (deferred, see Deferrals).

## Slice split

- **Slice 3a — optimized-IR emission + the vectorization IR-shape suite.** Self-contained, needs
  neither remarks nor debug info, and delivers the actual LLVM-upgrade gate. Ships first.
- **Slice 3b — debug-loc anchoring + remarks capture + `alignc explain-opt`.** The heavier half:
  MIR line plumbing, opt-in DILocation emission, the process-global remarks handler, and the
  translation surface.

---

## Slice 3a

### `emit-llvm --stage raw|optimized`

Extract the `run_passes("default<O2>", ...)` call from `write_object` into a shared
`run_opt_pipeline(module, tm, pipeline)` helper; `emit_llvm_ir` gains an `optimized: bool` and
calls it before printing. CLI: `--stage raw|optimized`, **default `raw`** (today's semantics —
"what codegen emitted"; `optimized` is the opt-in "what LLVM did" view). Any other `--stage`
value → diagnostic. The `"default<O2>"` string stays the one hardcode until Slice 4 threads the
profile pipeline through the same helper.

### The vectorization IR-shape suite (the LLVM-upgrade gate)

New `crates/align_driver/tests/vectorize_shapes.rs`, asserting textually on **optimized** IR at a
pinned `x86-64-v3` target (a couple of kernels also pinned at `v2`). ★ = empirically verified
during the design review.

| # | Kernel | Assert on optimized IR | Status today |
|---|---|---|---|
| 1 ★ | `xs.map(dbl).sum()` (unknown trip count, int reduction) | `<4 x i64>` + `vector.body` + `llvm.vector.reduce` | vectorizes |
| 2 ★ | `xs.where(big).sum()` (if-conversion, masked) | `<4 x i1>` mask + `<4 x i64>` + `vector.body` | vectorizes |
| 3 ★ | `xs.scan(0, add)` (loop-carried dep) — negative control | NO `vector.body`, no `<N x` | not vectorized (correct) |
| 4 | `xs.where(k).min()` (masked min-reduction) | `<N x i32>` + `llvm.vector.reduce.smin` | verify at impl |
| 5 | `xs.map(f).reduce(1, mul)` | `<N x i64>` + `vector.body` | verify at impl |
| 6 | float `xs.map(f).sum()` — reassoc control | NO `vector.body` without fast-math (documents FP-reduction reality) | verify at impl |
| 7 | `src.map(dbl).map_into(dst)` two-slice — pre-Slice-5 control | today no `noalias` on the fn (confirmed absent): scalar or `vector.memcheck` guard; flips to clean vectorization when Slice 5 lands `noalias` | verify at impl |
| 8 | plain `xs.map(dbl)` materialize (pointer-induction copy) | `<N x i64>` store loop or `llvm.memcpy` | verify at impl |

**Implementation outcome (Slice 3a shipped as #420, 2026-07-11 — suite = `vectorize_shapes.rs`,
12 tests):** kernels 4/5 vectorize (`reduce.smin` / `reduce.mul` over i64); kernel 6 confirmed
the negative prediction (ordered FP sum stays scalar); kernel 8 = vectorized store loop.
Two divergences pinned as reality: **k4 runs over i64** (an i32 slice is not
literal-constructible today — `array<i32>` annotation on a literal is rejected; DX note); and
**k7 diverged from the prediction**: `map_into` ALREADY vectorizes cleanly with zero
`vector.memcheck` — the scoped `!alias.scope`/`!noalias` metadata emitted by the fused
`map_into` lowering is present in raw IR and plausibly contributes, though full inlining +
distinct-alloca provenance may prove non-alias independently (mechanism not isolated; the
non-inlined case is untested). **Consequence: Slice 5's fn-level `noalias` is NOT the unlock for
this pattern — its motivation re-scopes to cross-function / opaque-provenance cases.**

Kernels 1–3 lock now; 4–8 get a one-pass empirical confirmation at implementation time — the
suite pins **reality**, not aspiration (a kernel that doesn't vectorize today becomes a negative
control with a comment, not a wish). Negative controls (3, 6, 7-today) catch spurious
"vectorized everything" regressions across the LLVM upgrade. Mutation-check per house style.

---

## Slice 3b

### Debug-loc anchoring (prerequisite)

- MIR: parallel per-block `stmt_lines: Vec<(u32, u32)>` (line, col; `(0,0)` = none) populated at
  HIR→MIR lowering from the HIR node span. Touches the block builder + stmt-push sites, NOT the
  `Stmt` enum. Partial population (pipeline / loop / call sites) already anchors every remark
  that matters — pipelines lower from one spanned HIR expression.
- Codegen: inkwell's DI builder covers everything (`create_debug_info_builder(true, ...)` — the
  `true` stamps the required `"Debug Info Version"` module flag — one `DIFile`/`DICompileUnit`
  per source, one `DISubprogram` per fn, `set_current_debug_location` per lowered stmt).
- **Opt-in only**: debug-loc emission runs only under `explain-opt` (and a future `-g`), so
  normal builds and the 3a IR-shape baseline stay byte-identical to today.
- Self-review note: a new `Block` field is a struct-shape change — audit the MIR printer and
  every exhaustive `Block` construction site.

### Remarks capture (Mechanism A — the only C-API path)

1. Once per process (`std::sync::Once`): `LLVMParseCommandLineOptions` with
   `-pass-remarks=.*  -pass-remarks-missed=.*  -pass-remarks-analysis=.*` — sets the cl::opt
   globals that make `OptimizationRemarkEmitter` actually emit. Process-global: keep strictly
   behind the `explain-opt` path; the IR-shape suite must never enable it.
2. `LLVMContextSetDiagnosticHandler` on the inkwell context (`Context::raw()`); in the handler
   keep severity `LLVMDSRemark`, take ownership of `LLVMGetDiagInfoDescription` (must
   `LLVMDisposeMessage`), collect.
3. Run `run_opt_pipeline`; the handler fires synchronously; translate afterwards.

### Command surface

**`alignc explain-opt <file>`** — a new inspection verb parallel to `emit-llvm`/`emit-mir`; NOT a
`build --explain` alias (One way; it produces a report, not an executable). Runs front end →
MIR → codegen (+debug-loc) → `default<O2>` with capture. Output, in the compiler's existing
diagnostic voice (`file:line:col: message`, lowercase, terse, backticked code, remedy inline):

- Default: the **missed/actionable** records on data-path constructs, one line each; a one-line
  success summary (`12 pipelines fused, 8 vectorized (2 not)`); a trailing bucket count
  (`+ 41 other LLVM remarks (see --verbose)`).
- `--verbose`: itemized Passed records; untranslated remarks as raw passthrough explicitly
  marked `[llvm <pass>/<name>]` — a machine string is never dressed as an Align diagnostic;
  suppressed internal-location remarks surface here labeled compiler-internal.
- Exit code: `0` = compiled + report produced (missed optimizations are NOT errors); `1` =
  compile error / bad args. Miss-counts never affect the exit code in v1 — the CI
  count-regression gate is a separate deferred mechanism and coupling now would prejudge it.

### Translation contract

- v1 scope: **`loop-vectorize`** Passed + Missed (full translation of the enumerable miss
  reasons — this is THE core, pipelines are loops); **`inline`** Missed filtered to
  pipeline-critical callees (lifted lambdas / `$clos` / `$fnval` — identifiable from the message
  callee name); **`slp-vectorize`** Passed (feeds the summary only). Everything else → the
  bucket. Inline/slp pattern reliability is verified at implementation time; if unreliable they
  drop to the bucket (honest scoping), loop-vectorize is the hard requirement.
- **Keying reality:** the C API yields a flat message string, so the v1 table matches on
  message patterns of the pinned LLVM 19 — contained in one module, re-verified at the LLVM
  upgrade. The ideal structured keying (`(pass, RemarkName)`, version-stable) needs the C++
  shim — deferred with record. Each table row carries our own stable `reason_code`
  (`MayAlias | CostModel | CallNoVectorForm | ReductionNotRecognized | TooLargeToInline |
  Recursive | UnknownTripCount | Unspecified`), so the human text and the future JSON never
  depend on LLVM prose.
- **Honesty rule (load-bearing):** the message asserts only what the remark justifies. Concrete
  mappable cause (aliasing / no-vector-form / unrecognized reduction / inline cost) → cause +
  concrete Align remedy. Vague cause (`cost model`) → say only that; **never** upgrade a
  cost-model decline into an aliasing story; no fabricated suggestions.
- Example renderings (the voice to match):

```text
app.align:42:9: not vectorized — the pipeline's source and destination may be the same array;
  the compiler can't prove they don't overlap. Write the result into a distinct `out` array.
app.align:71:9: not vectorized — the compiler judged it not worthwhile here (short pipeline or
  cheap per-element work); nothing to change in the source.
app.align:80:9: not vectorized — the pipeline calls a function with no vector form (`sin`), so
  the loop can't be widened. Use a vectorizable operation, or accept the scalar loop here.
app.align:95:9: not vectorized — the combine here wasn't recognized as associative, so its
  iterations can't run in parallel. Use `reduce` with a recognized combiner (`+`, `*`, `min`, `max`).
```

### Anchoring policy (Nothing-hidden applies to our own leaks)

explain-opt speaks only about spans the user wrote. Runtime/std/FFI locations (`align_rt_*`) →
suppressed from the default report, counted into an `N remarks in library/runtime code` line,
raw under `--verbose`. Inlined locations → re-anchor to the outermost **user-source** frame of
the `inlinedAt` chain; whole-chain-internal → suppress. Compiler-generated constructs with no
user span (thunks, the Result-main wrapper) → suppress; never fabricate a span.

### The remark data model (build first, render second)

v1 builds a `Vec<OptRecord>` and renders the human view from it — that single discipline makes
the recorded AI-loop follow-ons pure extensions. Fields: `kind`
(`Vectorized|NotVectorized|Fused|NotFused|Inlined|NotInlined|Hoisted|Other`), `verdict`
(`passed|missed|analysis`), `pass`, `reason_code` (above), `construct`
(`{pipeline|loop|fn, construct_id}`), `source_span`, `message` (rendered), `suggestion`
(optional), `llvm_detail` (raw; `--verbose` only). `--format json` later = a second printer over
the same vector; the optimization score = `group_by(kind, verdict).count()` over it; CI gates =
a count diff between two runs. None require schema or CLI breaks.

### One-way boundary vs the deferred M8 frequency lints

One rule: **knowable from Align's own IR without LLVM → check-time lint; requires the LLVM pass
verdict → explain-opt.** Where both could speak (e.g. an invariant call in a loop), the lint
owns it and explain-opt stays silent — no double reporting. explain-opt never fires during a
plain `build`.

### Implementation outcome (Slice 3b shipped — `m13-slice3b-explain-opt`)

Built to the contract above; the C-API remarks path and DILocation anchoring both work. The real
LLVM-19 remark strings captured from the design's probe kernels (`x86-64-v3`) and keyed on:

```text
probe.align:2:33: vectorized loop (vectorization width: 4, interleaved count: 1)      # passed
probe.align:1:33: loop not vectorized: cannot prove it is safe to reorder floating-point operations
probe.align:2:40: loop not vectorized: value that could not be identified as reduction is used outside the loop
probe.align:2:33: 'dbl' inlined into 'run' with (cost=-15030, threshold=337) ...      # inline passed
probe.align:6:3:  align_rt_print_i64 will not be inlined into align_main ...           # runtime inline miss
probe.align:4:8:  Stores SLP vectorized with cost -2 and with tree size 2             # slp passed
<unknown>:0:0:    'align_main' inlined into 'main' ...                                 # compiler-internal
```

Deviations from the design, all documented in code:

- **The "Debug Info Version" module flag is stamped manually** (`module.add_basic_value_flag`) —
  inkwell's `create_debug_info_builder(true, …)` does **not** stamp it (the design's claim was
  inaccurate); without it the verifier strips all debug metadata and no remark anchors.
- **`stmt_lines` is populated at statement / block-value granularity, not per sub-expression.**
  `lower_expr` is a very large, deeply recursive function; setting the span inside it (even one
  field write, or a save/restore wrapper) grew its debug-build frame enough to overflow the 2 MB
  test thread on machine-generated deep expressions (`expr_depth`). Setting the span in the shallow
  `lower_stmt`/`lower_block` instead anchors every pipeline / loop / call site (a single-expression
  function body's pipeline lowers from one block-value expression) with zero hot-path cost. Line
  tracking is populated only in located mode; a normal build's `push` is byte-identical to before,
  and the block-value's `emit-llvm`/`emit-mir` output is unchanged (verified).
- **`reason_code` gained `FpReorder`** — the ordered-FP-reduction decline is a distinct, mappable
  cause with a concrete remedy (integer reduction), so it earns its own honest code rather than
  collapsing into `ReductionNotRecognized`.
- **Inline misses → bucket (not actionable).** Every Align pipeline lambda inlines in practice, so
  no lambda-inline-miss string was observed to key on; v1 buckets inline misses (runtime callees to
  a library/runtime sub-bucket) rather than key on an unverified pattern. The lambda-inline-miss →
  actionable-record path is deferred until a real string exists.
- **`slp-vectorize` passed feeds the summary only**; the bare redundant `loop not vectorized`
  decline (LLVM also emits a specific-reason remark) is deduped into the bucket, not double-reported.

Default view = the missed/actionable one-liners + a one-line success summary (`N loop(s)
vectorized (M not); K call(s) inlined; J store group(s) SLP-vectorized`) + a trailing bucket count;
`--verbose` adds the raw `[llvm …]` passthrough (library/runtime, other, and compiler-internal
sub-lists). Exit 0 when compiled+reported, 1 on compile error / bad args / unreadable file. The
report is built as a `Vec<OptRecord>` and rendered second, so `--format json` / score / CI gates
stay pure extensions. A test-only note: `common::build_and_run`'s post-lowering work was moved into
its own frame (`emit_link_run`) so the deep MIR lowering it drives keeps its 2 MB-thread margin — a
behaviour-preserving robustness fix that helps every deep-lowering test, not just explain-opt.

## S0B extension: current compiler decisions

> **Status:** IMPLEMENTED against the accepted exact design.
>
> **Authority:** This section is the exact S0B public-output and implementation
> ledger required by `31-execution-storage-startup-plan.md`. The earlier Slice
> 3a/3b contract continues to own LLVM remarks and optimized-IR emission.

### Scope and public-contract ledger

S0B reports three existing decision families needed by the first bounded
consolidation candidate. It observes the selector at the point that already
makes the decision; it does not reconstruct a plan from emitted MIR, LLVM IR,
runtime behavior, or optimization remarks.

```text
chunks representation
  direct count / direct indexed subview / owned header materialization

materializing-pipeline output storage
  reuse one eligible source buffer / allocate the existing output storage

explicit par_map execution form
  range materialization / direct integer range reduction / sequential collector
```

Fusion, LLVM vectorization, runtime pool initialization, the range chosen for a
particular execution, physical traffic, allocation counts, elapsed time,
runtime-capability linking, region-frame packing, blocking domains, and future
S1--S6 strategies are not S0B records. LLVM decisions retain the existing
remark owner and rendering below the new current-plan lines.

| Surface | Exact S0B contract | Owner and acceptance |
|---|---|---|
| CLI | The only command remains `alignc explain-opt <file> [--verbose|-v] [--target-cpu baseline|native|CPU]`; S0B adds no command, option, environment input, profile, or ambient discovery | Existing argument parsing and exit rules remain unchanged. No otherwise-ignored token acquires S0B meaning. |
| Output | English UTF-8 text on stdout, with the exact prefix, ordering, and fixed reason sentences below; diagnostics remain on stderr | `align_driver::explain`; exact golden rows cover every admitted state/reason and multi-unit order. |
| Record model | One ephemeral `PlanRecord` per admitted selector decision actually reached and consumed by lowering, with exact enums, no-row cases, and field-presence rules below | `align_mir` located lowering. No public language type, JSON, wire format, persistence, or reader is added. |
| Semantics | Observation never changes evaluation, errors, ownership, cleanup, allocation mode, explicit parallel semantics, or the selector result | Existing chunks, donation, and `par_map` owners plus MIR/object identity controls. |
| Source | A record refers to the user-written selector expression only when the driver can bind its span to the same real source catalog that produced the checked HIR. No synthetic interface location is dressed as user source | Located lowering, `SourceMap`, and the walk-owned catalog below; absent anchors use the exact default and verbose rules below. |
| Whole/per-unit | The CLI keeps its shipped bottom-up per-unit walk. An internal whole-program located route must normalize to the same decision records for bodies whose selector inputs are equally visible; source fields compare only when both routes authenticate them | `per_unit_surface` plus a new current-plan parity owner. Imported bodies remain opaque in the per-unit MIR and retain their existing conservative work hint. |
| Cache and artifact identity | Plan records are located diagnostic data. They never enter HIR/MIR implementation hashes, interfaces, frontend/object/ThinLTO keys, object bytes, link inputs, or runtime capability discovery | Existing cache namespaces plus explicit empty-cache and generated-object identity owners. |
| Runtime and ABI | No runtime key, symbol, signature, argument, result, ownership rule, or global state changes | Runtime ABI inventory stays byte-for-byte unchanged. S0B never reports which runtime branch actually executed. |
| Allocation | The record vector and rendered text allocate only in `explain-opt`; ordinary lowering/build paths construct no record storage | Located-mode allocation owner and normal-build no-record assertion. No performance promise is made. |
| Errors | Missed/rejected optimizations are report rows and keep exit 0. Compile, codegen, bad-argument, and internal malformed-record failures keep exit 1 and publish no partial stdout | Existing `run_explain_opt` buffering plus the validation rule below. |

There is no persisted or exchanged format, so scalar-width, byte-order, tag,
and decoder-golden rules do not apply. The internal Rust enums are exhaustive;
unknown or incompatible variants fail report validation rather than selecting
an optimistic strategy.

### Exact record model

Located MIR carries a private side table. Unlocated lowering always carries an
empty table and does not allocate a backing buffer for it. The conceptual
record is exact:

```text
PlanRecord {
  function: ProgramCall
  construct_ordinal: u32
  kind: PlanKind
  state: PlanState
  strategy: PlanStrategy
  reason: PlanReason
  source: Option<PlanSource>
}

PlanSource {
  file_id: u32
  span_lo: u32
  span_hi: u32
  line: u32
  column: u32
}
```

`PlanKind` is exactly `Chunks | BufferDonation | ParMap`.
`PlanState` is exactly
`Selected | Rejected | RuntimeSelected | NotApplicable | Unavailable`.
`PlanStrategy` and `PlanReason` contain only the rows admitted by the three
tables below. Source availability is the orthogonal `Option<PlanSource>` field,
not a fabricated selector verdict. The enums do not reserve future planner
strategies.

`function` is the canonical concrete MIR callable. Generic monomorphs therefore
remain distinct, and an imported declaration with no body emits no record.
`construct_ordinal` is one-based within that concrete function across all three
kinds. Collection assigns a monotonically increasing private index at each
reached decision point in the existing HIR lowering order. After collection,
records are stably sorted within each function by `(source-key, kind-rank,
collection-index)`. `source-key` orders every `Some(PlanSource)` before `None`;
anchored keys order by `(file_id, span_lo, span_hi)`, and all `None` keys are
equal. Kind rank is `Chunks`, `BufferDonation`, `ParMap`. Ordinals are then
assigned in that total order. Functions retain MIR function order and units
retain the existing dependency-first walk. The collection index is never
rendered or hashed.

All anchored rows for one concrete HIR body must resolve to that body's one
source file; mixed present `file_id` values within one function are malformed.
Route-local numeric file IDs are not cross-route identity: parity resolves them
through each route's source catalog to the same walk-owned source name before
comparison.

The source span is the `chunks` expression, the materializing terminal
(`to_array`, `scan`, `sort`, `sort_by_key`, or sequential `par_map`), or the
`par_map` expression, respectively. `span_lo` and `span_hi` are byte offsets;
`line` and `column` are the one-based values returned by
`SourceFile::line_col(span_lo)`, the same projection used for diagnostics and
LLVM debug locations. `file_id` and raw span offsets are not printed.
`function` renders as the canonical concrete callable spelling, and the
filename is the existing per-unit debug basename. No record contains source
text, estimated speedup, a runtime result, or LLVM prose.

The per-unit driver builds a private `LocatedPlanSourceCatalog` from the same
load/tokenize walk that creates the checked HIR. Before each located lowering,
it sizes the catalog to the current `SourceMap`, initializes every entry as
`NonHirInput`, then replaces file IDs from the load-owned Align-unit registry
with `User { source_name, exact_source }` and IDs from the compiler-owned
interface registry with `SyntheticInterface`. Duplicate/conflicting
classification is malformed. Later SourceMap additions extend the catalog with
`NonHirInput` before any later unit lowers; this includes file-backed SQL and
other static inputs interleaved before a subsequent interface source. The full
Align source bytes are retained only for this located command.

A `PlanSource` is present only when the HIR span names a `User` entry and that
entry's name and source bytes still equal the current `SourceMap` entry
byte-for-byte. `SyntheticInterface` deliberately yields `source = None`. A plan
span that names `NonHirInput`, a missing entry, an out-of-bounds span, or a
name/content mismatch is a malformed internal record. This catalog is
compiler-owned diagnostic provenance, is never serialized, and is absent from
ordinary lowering, interfaces, hashes, and artifacts.

Its owned internal shape is exact:

```text
LocatedPlanSourceCatalog {
  files: Vec<LocatedPlanSourceOrigin>
}

LocatedPlanSourceOrigin =
  User { source_name: Box<str>, exact_source: Box<str> }
  | SyntheticInterface
  | NonHirInput
```

The vector index is the `FileId`; a missing index is malformed. `Box<str>` owns
UTF-8 bytes for the duration of one report and is dropped with the walk. The
driver lends the catalog immutably to located MIR lowering; it adds no lifetime
to `PlanRecord` and crosses no FFI, wire, or cache boundary.

S0B accepts no new text input and does not change path admission or source-file
decoding. It renders only a debug basename that the existing command has
already admitted, using the lossy and escaping projection fixed below; no text
crosses a new native or persisted boundary before validation or output.

One lowering-owned site may first receive a general chunks-materialization
reason and then a more specific immediate-consumer reason. The collector keeps
exactly one `(HIR-site, kind)` record by a lowering-private site token that is
discarded before `PlanRecord` publication, and replaces the general reason with
the specific reason. A duplicate record, missing replacement target, or
incompatible replacement marks the table malformed. Separate kinds at one
expression are not duplicates: for example, a materialized `chunks` source and
the `par_map` consuming it each
produce their own record. LLVM remarks never suppress plan records, and plan
records never suppress LLVM remarks.

Located construction also retains one `align_mir`-private certification copy
of the complete normalized record table and the private catalog-malformed bit.
`Program::default()` and ordinary unlocated lowering are uncertified; only a
located lowering owner can construct the certified state, and downstream crates
can neither replace it nor clear a malformed catalog result. Final validation
requires that certified state and structural equality with its copy before
checking individual records. The copy is allocated only beside located records
and is erased with them from MIR text, implementation hashes, caches,
interfaces, LLVM, objects, and runtime keys. This makes deletion, insertion,
field mutation, authenticated-`Some` to source-less-`None` stripping, or
rebuilding the public semantic fields on a default program fail closed without
reconstructing decisions from MIR or LLVM.

After the complete per-unit walk and before any LLVM invocation or output, the
driver validates every state/strategy/reason combination, the total sort and
one-based ordinal sequence, the single-source-per-function rule, source-catalog
provenance, present source bounds, and exact `(line, column)` equality with
`SourceFile::line_col(span_lo)`. Input/read and compiler errors retain
their existing precedence and publish their existing diagnostics instead of
inspecting partial plan tables; an otherwise successful empty walk retains the
existing `alignc: no units to analyze` failure. On a successful nonempty walk,
malformed plan data precedes accumulated warnings and every codegen error: it
returns exit 1, writes exactly
`alignc: cannot explain current plan: malformed record\n` to stderr, invokes no
LLVM pipeline, and publishes no stdout. After valid plan data, existing warnings
are emitted, then LLVM/codegen runs unit by unit into the existing buffered
stdout. A later codegen error keeps exit 1 and no stdout, with existing warning
and codegen stderr behavior. This is an internal compiler failure, not a source
diagnostic.

### Chunks representation selector

Every `ArrayChunks` HIR expression whose lowering reaches and consumes its
representation decision produces exactly one record. A source, chunk-size, or
enclosing eager operand that terminates before that point produces no record.
The existing direct-parent special cases remain the complete virtual set; every
other reached use constructs the existing owned `array<slice<T>>` header array.

| State | Strategy | Reason | Exact condition and rendered explanation |
|---|---|---|---|
| `Selected` | `virtual-count` | `direct-len` | Direct receiver of `len`: “the direct `len` consumer needs only the chunk count” |
| `Selected` | `virtual-index` | `direct-index` | Direct receiver of one index: “the direct index consumer needs only one borrowed subview” |
| `Selected` | `materialized-headers` | `parallel-consumer` | Immediate source of `par_map`, including direct `par_map(...).sum()`: “the current explicit-parallel consumer reads an owned header array” |
| `Selected` | `materialized-headers` | `pipeline-consumer` | Immediate source of any other synchronous pipeline stage or terminal: “the current synchronous pipeline consumer reads an owned header array” |
| `Selected` | `materialized-headers` | `stored-or-boundary` | Every remaining use, including binding, return, call argument, control-flow value, or aggregate storage: “the chunks value crosses a stored, returned, call, or control-flow boundary” |

Consumer-reason precedence is the table order. `direct-len` and `direct-index`
are recognized before descending into the child and therefore never emit a
materialization row. For a materialized expression, `parallel-consumer`
precedes `pipeline-consumer`, which precedes `stored-or-boundary`. This
classification changes only the explanation; the existing direct-parent test
continues to choose representation.

### Materializing-pipeline donation selector

Every call to the existing materializing collector that reaches its allocation
decision produces exactly one donation record. A source, stage operand, `scan`
initializer, `sort_by_key` capture, `par_map` capture, or other terminal capture
that terminates first produces no donation row and no output allocation. The
decision helper returns the state/strategy/reason tuple and the existing
allocation branch consumes that same tuple; reporting cannot reimplement the
predicate. Each caller passes its terminal HIR span into the collector solely
for the located side table; the unlocated call path does not construct or copy
it. Reasons are tested in this exact first-match order:

| Precedence | State | Strategy | Reason | Exact condition and rendered explanation |
|---:|---|---|---|---|
| 1 | `NotApplicable` | `arena-output` | `arena-owned-output` | An arena is active: “the output is arena-owned, so source-buffer reuse does not apply” |
| 2 | `NotApplicable` | `fresh-output` | `unsupported-source-or-stage-shape` | Zip, struct view, chunks headers, Move/nonuniform scalar, another non-donatable source representation, or a stage outside the current in-place-safe set: “this source or stage shape cannot reuse the source buffer” |
| 3 | `Rejected` | `fresh-output` | `source-not-unique-dead` | No individually owned, unbound, provably dead heap temporary is available: “the source is not a unique dead heap temporary” |
| 4 | `Rejected` | `fresh-output` | `layout-mismatch` | Source and result scalar size/alignment are not identical: “source and result element layouts are not identical” |
| 5 | `Unavailable` | `fresh-output` | `measurement-disabled` | The existing exact value `ALIGN_BUFFER_DONATE=off` disabled the otherwise eligible choice: “the measurement override disabled an otherwise eligible reuse; this is not the default plan” |
| 6 | `Selected` | `reuse-source-buffer` | `eligible-unique-source` | Every existing eligibility check passed: “a unique dead heap source with an identical layout is reused as the result buffer” |

`scan` remains eligible because the accumulator is the terminal rather than a
pipeline stage; its output index equals its input index. The decision helper
matches every currently admitted source and stage class exhaustively. A new
class cannot compile until this contract gains its fail-closed row and the
selector consumes that row; it cannot inherit the selected case.

When `ALIGN_BUFFER_DONATE=off` is present, stdout begins with this exact line
before any unit header or record, even if the current program has no eligible
donation site:

```text
current-plan measurement override: buffer donation is disabled; donation rows are not the default plan
```

Other values retain the shipped default and emit no banner. Located lowering
already bypasses frontend/MIR memoization, and the existing cache owner keeps
the object cache disabled for this measurement override. S0B adds no toggle and
does not change that cache rule.

### Explicit `par_map` form selector

Every `ArrayParMap` expression whose lowering reaches and consumes its form
decision produces exactly one record. A source, stage operand, or callable
capture that terminates before that point produces no row. The record describes
the compile-time form and, for a range kernel, says that the runtime still
chooses within its existing bounded caller-only/shared-pool policy. It never
claims that a pool was initialized, a threshold was crossed, or a particular
range executed.

| State | Strategy | Reason | Exact condition and rendered explanation |
|---|---|---|---|
| `RuntimeSelected` | `range-reduce` | `direct-integer-sum` | The existing direct, unstaged integer `par_map(...).sum()` specialization fires: “the runtime chooses caller-only or shared-pool range reduction from the input length, element layouts, conservative work hint, and process-lifetime worker availability” |
| `RuntimeSelected` | `range-materialize` | `supported-range-kernel` | The existing `par_map_parallelizable` predicate and input-element formation accept the source/stages: “the runtime chooses caller-only or shared-pool range materialization from the input length, element layouts, conservative work hint, and process-lifetime worker availability” |
| `Rejected` | `sequential-collect` | `unsupported-source-representation` | The source cannot form the range-kernel input element: “the source representation has no current range-kernel form, so the explicit operation uses the sequential collector” |
| `Rejected` | `sequential-collect` | `unsupported-stage-or-value-shape` | Input formation succeeds but the shared parallelizable predicate rejects a stage/value shape: “a stage or value shape has no current range-kernel form, so the explicit operation uses the sequential collector” |

The direct-reduction condition is evaluated first and is exactly the existing
outer empty-stage `sum`, empty-stage `par_map`, integer result, and supported
direct input test. The ordinary materializing path then tests input formation
before the shared `par_map_parallelizable` predicate solely to select one of the
two rejection explanations; either refusal uses the existing sequential
collector. The helper that supplies the record also supplies the
lowering branch, so reporting cannot drift from generated MIR.

The compiler-generated work hint remains the shipped `1 | 2 | 4` value and is
not printed as a runtime outcome. Process-lifetime worker availability is the
runtime's cached `available_parallelism()` result (or the existing test-only
override in runtime owner tests); reporting neither initializes that cache nor
reads or prints its value. A missing imported callable body continues to
contribute the conservative value `1`; S0B adds no interface field or cross-unit
summary. Whole/per-unit parity compares the selected form. A separate owner
asserts that the per-unit MIR keeps the conservative imported-body hint and
that no report field claims otherwise.

Curly quotation marks in the three selector tables delimit the exact
explanation text; the quotation marks themselves are not emitted.

### Rendering and deterministic order

An anchored record is one physical line with this exact prefix:

```text
<file>:<line>:<column>: current plan `<function>` #<ordinal> <kind>: <state> `<strategy>` — <explanation>
```

`<kind>` is `chunks`, `buffer-donation`, or `par-map`; `<state>` is lowercase
`selected`, `rejected`, `runtime-selected`, `not-applicable`, or `unavailable`.
The strategy spelling and explanation are the exact table cells above. The
canonical function spelling is made only from validated source identifiers and
compiler-owned ASCII separators, none of which can contain a backtick, newline,
or carriage return. The filename uses the existing lossy-UTF-8 debug basename
and additionally escapes backslash, LF, and CR as `\\`, `\n`, and `\r`,
respectively, so one record stays one physical line; colons remain literal and
the two trailing numeric fields disambiguate them. Ordinals are unsigned
decimal without leading zeroes.

For each unit the output order is below. Current-plan functions retain MIR
function order; records within each function use ascending
`construct_ordinal`.

```text
existing unit header, when the walk has more than one unit
current-plan records in normalized order, with anchored rows and source-less
  default aggregates or verbose details at their normalized positions
existing LLVM missed/actionable records
existing verbose LLVM passed/raw records, when requested
existing LLVM success summary and bucket output
```

The global donation-override banner, when required, precedes the first unit
header. Planner records are identical under default and `--verbose`; verbose
does not change their anchored wording or order. A program with no admitted
record and no donation override preserves the existing stdout byte-for-byte.

Every admitted selector originates in user HIR, but a per-unit monomorph parsed
from a synthesized interface deliberately has no authenticated user anchor. A
replay route invoked without a `LocatedPlanSourceCatalog` also withholds all
anchors; it may not guess from a bare `SourceMap`. These cases preserve the
selector's state, strategy, and reason with `source = None`. Default output
emits one exact aggregate line in the record's normalized position:

```text
+ <N> current-plan record(s) without user source (see --verbose)
```

Consecutive source-less records are one aggregate; an anchored record ends the
aggregate. Verbose output replaces each aggregate with one line per record:

```text
  [current plan `<function>` #<ordinal> <kind>] <state> `<strategy>` — <explanation>; source location is unavailable
```

The absent-anchor rule does not alter the selected MIR strategy. Source-less
records sort after anchored records within their function under the total key
above. A synthetic or unauthenticated source never becomes line 0 in the
default diagnostic voice; a supplied but mismatched catalog fails validation
instead of becoming unavailable.

### Whole-program, generic, cache, and generated-code identity

The CLI remains per-unit because that is the linked-build truth. Owner tests
also lower the same checked bodies through whole-program located mode. After
normalizing away unit headers and per-unit callable linkage, records with the
same visible selector inputs must agree exactly on concrete function, ordinal,
kind, state, strategy, and reason. Source agrees on walk-owned name and span when
both routes authenticate it. When either route has only a synthetic or absent
catalog entry, parity compares the decision with source normalized to
unavailable and separately requires `None` on the unauthenticated route. An
imported body is not a visible input to its dependent unit: its existing
conservative parallel work hint is tested as an explicit per-unit unavailable
fact, not replaced with the body visible only to whole-program lowering.

Generic instances emit one row per concrete callable. Imported generic bodies
instantiate in the consuming unit under the existing source/interface rules;
their rows use that concrete function identity but `source = None`, because the
format-9 interface carries body text without an authenticated producer path or
coordinate mapping. Whole-program lowering of the producer's real source may
anchor its corresponding monomorph. No source mapping or plan record is added
to the interface.

No record is serialized with checked HIR. A replay that needs anchors must
carry the private catalog captured by the same load/tokenize walk; byte-for-byte
name/source disagreement with its `SourceMap` is the malformed-record failure
above. A replay without that catalog still reconstructs the same decisions but
sets every `source` to `None`. It never trusts numeric file IDs alone.

`Program`'s private located side table is deliberately excluded from every
canonical graph walk, implementation/interface hash, MIR text printer, runtime
key inventory, capability scan, and LLVM lowering match. Clearing only that
table from a located program must leave raw LLVM, optimized LLVM after
normalizing LLVM's existing diagnostic metadata, and emitted object bytes
identical. Running `explain-opt` before a normal build must neither create a
frontend/object/ThinLTO cache entry nor change the later build's hit/miss
counters or artifact digest. Comment/whitespace changes may move located rows
but retain their existing exclusion from ordinary codegen identity.

Normal lowering installs no collector in `BuilderCtx`, performs no selector-
record construction, and leaves the side-table vector empty. The selector
helpers return the same decision enum in both modes; located mode alone copies
that already-made decision into the table. This keeps report collection from
becoming a second selector and prevents a new source/type/MIR variant from
falling through to an optimistic record.

### Implementation closure matrix

S0B is intentionally one capability PR even though its hand-written diff is
expected to exceed roughly 1,000 lines. The selector-owned decisions, located
source catalog, fail-closed validation, renderer, and parity/identity owners
form one proof boundary: none is a useful stable consumer without the others.
Splitting that dormant producer-to-consumer chain would duplicate record-shape,
source-authentication, and codegen-identity proof while allowing intermediate
commits that can collect but not safely publish (or publish records they did
not authenticate). Keeping the boundary whole therefore lowers integration
risk and leaves distinct LLVM-remark behavior unchanged.

| Axis | Exact implementation closure | Owner evidence |
|---|---|---|
| Formation and validation | Exhaustive enums admit only the three kind tables; `align_mir` alone owns the private certification and catalog-malformed state, Default/unlocated programs are uncertified, located programs (including an empty program) are certified, and downstream crates cannot jointly replace records and proof; state/strategy/reason compatibility, reached-decision presence, total ordering, ordinals, exhaustive source-catalog provenance, spans, derived coordinates, and duplicates validate before LLVM or output | Rust privacy is the compile-time tripwire for external Program construction; unit-level default/located-empty/deletion/field/catalog invalid-record matrix, user/interface/non-HIR interleaving owner, coordinate-corruption matrix, plus an enum/selector coverage tripwire. |
| Construction | Chunks, donation, and `par_map` helpers return the decision consumed by their existing lowering branch; located mode records that same value | MIR structural positives/negatives for every table row. |
| Move-in / move-out / source nulling | No Align value or ownership bit enters a record. Donation still transfers/nulls the existing source owner only on `reuse-source-buffer` | Existing `buffer_donate` MIR and execution differential, including bound and escaping results. |
| Replacement and return | General chunks materialization may be replaced only by the same site's more-specific consumer reason; returning/storing chunks remains materialized | Direct/stored/call/return/control-flow chunks rows and existing lifetime owners. |
| Drop and cleanup | Record vectors are ordinary compiler-owned Rust values; generated cleanup, runtime drops, and early-exit cleanup are unchanged | MIR/object identity plus existing chunks/donation/parallel cleanup tests. |
| `if`, `match`, `else`, `?`, `map_err`, loops, early exits | A selector whose decision point is reached on any supported control path receives one row; termination during source/stage/terminal formation receives none, and path structure does not duplicate a reached decision or change cleanup | Parameterized located fixture spanning branch/loop/result forms, pre-decision termination at every formation phase, and matching generated MIR. |
| Malformed/future input | Unknown representation/stage/value shapes select the exact fail-closed rejection/not-applicable row; incompatible internal records fail before stdout | Hand-constructed HIR/MIR boundary owners and invalid-record renderer tests. |
| Generic monomorphization | Each concrete body has independent canonical function identity and stable ordinal; imported interface monomorphs have unavailable source rather than a synthetic anchor; no template-only phantom row | Imported/local generic formation fixture, whole/per-unit decision comparison, and explicit source-presence difference. |
| Interface serialization | No plan/source field enters format-9 interfaces; missing imported bodies retain work hint 1 without a new summary | Interface byte identity, synthetic-interface no-anchor owner, and per-unit imported-callback owner. |
| Whole-program/per-unit | Same-visible-input decisions agree; authenticated anchors agree only where both routes own real source; per-unit section order remains dependency-first; single-unit header remains absent | Extended `per_unit_surface` owner and direct normalized decision/source-presence comparison. |
| Checked-HIR replay | Plans are reconstructed from checked HIR; anchors additionally require the original private located-source catalog, a mismatch fails before output, and absence produces source-less rows | Replay decision parity, catalog-present anchor parity, catalog-absent unavailability, and mismatched-source rejection owners. |
| Cache and ThinLTO | Located mode remains ephemeral, bypasses lowering memoization and persistent caches, and changes no action identity or counters | Empty-cache explain-then-build sequence plus existing cache namespace owners. |
| Runtime ABI and ownership provenance | No runtime symbol or ABI row changes; records contain no pointer, owner, region, descriptor, or runtime observation | Runtime ABI inventory equality and no-new-key structural assertion. |
| Allocation parity | Ordinary compilation has an empty, unallocated record buffer; the selected MIR allocation is identical with collection on/off | Normal/located MIR decision comparison and object-byte identity. |
| Target/profile | Existing target CPU reaches LLVM remarks; current-plan rows are target-independent and `explain-opt` remains fixed at `default<O2>`; range rows name process-lifetime worker availability without reading it | Baseline/native named-target output parity for plan rows, plus one-worker/multi-worker runtime owners; LLVM rows may differ honestly. |
| Explanation, order, and source absence | Exact one-line grammar, reason explanation in both anchored and source-less verbose rows, reason precedence, total source-option/ordinal order, override banner, duplicate rule, and absent-anchor aggregation | Golden default/verbose, repeat-run, multi-unit, collision, mixed anchored/source-less, and all-source-less fixtures. |

### Acceptance and proportional verification

The implementation owner must, at minimum:

1. Add one reduced fixture per selector table row and mutation-check that each
   selector's generated MIR changes if its decision helper is inverted. Add
   explicit no-row owners for termination during source, stage, `scan` init,
   `sort_by_key` capture, `par_map` capture, and other terminal formation.
2. Prove exact default/verbose bytes, repeat determinism, generic identities,
   multi-unit order, single-unit header absence, total mixed-source ordering,
   imported-generic and replay source absence, and no partial output on
   malformed internal records. Cross malformed records with warnings and
   codegen failure to pin the precedence above. Corrupt zero/stale coordinates
   and interleave a file-backed non-HIR static input before a later interface
   file to close catalog indexing. Golden verbose source-less rows retain the
   table's exact explanation.
3. Compare whole-program and per-unit normalized decisions for all equal-input
   rows, compare authenticated anchors only where both routes own them, and
   separately prove conservative imported-body parallel work.
4. Prove located collection on/off produces the same selected MIR and object,
   and that explain-then-build leaves cache entries, counters, interface bytes,
   runtime keys, and executable/object digests unchanged.
5. Cover `sort` and `sort_by_key` donation rows and capture termination. Reuse
   `direct_chunks_consumers_are_semantically_equivalent`,
   `donation_on_and_off_execute_identically`, the structural chunks/donation
   owners, `par_map_pure_function`, `par_map_cold_start`, and the existing
   one-worker/multi-worker runtime selectors, `explain_opt`, and
   `per_unit_surface` owners where they would fail for this slice.
6. Run the normal code-tier preflight. There is no timing benchmark because S0B
   makes no performance or resource-improvement claim.

S0B changes a public reporting contract and spans `align_mir` plus the driver,
but adds no language, library, runtime, FFI, ownership, or persisted-format
surface. `draft.md`, `language-spec.md`, runtime ABI ledgers, and bilingual
library designs therefore do not change. On implementation, the English and
Japanese toolchain guides must both say that `explain-opt` reports compiler
storage/execution choices as well as LLVM optimization remarks; examples need
not enumerate this internal schema.

## Deferrals (recorded)

- The C++ remark shim (structured `(pass, RemarkName, args)` keying) — revisit at the LLVM
  upgrade; requires LLVM dev headers + a `cc` build step.
- `--format json`, the itemized optimization score (needs MIR-side counters), CI
  count-regression gates — the AI-loop follow-on (consultation digest).
- `--fn <name>` scoping filter (thin extension; add when needed).
- Auto-fix / apply-suggestion (explain-opt explains, never rewrites).
- Profile-guided ranking (`!prof`, PGO hotness) — Slice V / M14.
- Remark persistence / on-disk DB — the future gate diffs two fresh runs.
- Deep SLP/LICM/GVN translation — bucket until a consumer needs them.
- Custom pass pipelines — explain-opt reports what `default<O2>` did; re-tuning is Slice 4+,
  justified BY these reports (the "no custom pass order until remarks justify one" rule).

## Risk ranking (low→high)

1. `emit-llvm --stage` — trivial refactor of an existing call.
2. IR-shape suite — expressible today, 3/8 verified; one empirical pass for the rest.
3. MIR line plumbing + DILocation — bounded plumbing; opt-in protects the baseline.
4. Remarks capture — process-global cl::opt state, `unsafe extern "C"` handler + C-string
   ownership, test-harness isolation. Highest, but no new deps for v1.
