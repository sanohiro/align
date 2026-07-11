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
