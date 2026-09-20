# Scoped floating-point relaxation

Status: design candidate for
[issue 1082 Part 2](https://github.com/sanohiro/align/issues/1082). This document
is the public-contract ledger and implementation closure matrix. Evidence
baseline: Align `1a446e5e4c38e241be7c140f6ee7747e39429110`, LLVM 22.1.8.

Align keeps floating-point evaluation ordered by default. The missing
capability is an explicit source scope in which the programmer may separately
permit reassociation and contraction. This is an observable language choice,
not an optimizer heuristic: it changes permitted result bits and must survive
generic and interface-carried body serialization.

## 1. Public-contract ledger

```text
Surface           float(reassoc) { body }
                  float(contract) { body }
                  float(reassoc, contract) { body }
                  The form is a block expression and has the block's type.
                  At least one option is required. `reassoc` and `contract`
                  are the complete option set. Source order is accepted and
                  the formatter writes the canonical order shown above.

Default           Outside a float scope, every operation retains today's
                  strict IEEE lowering: no reassociation, no contraction, no
                  fast-math flag. Existing source and result bits are
                  unchanged.

Reassoc           Permits reassociation only on f32/f64 add, subtract and
                  multiply operations formed lexically in the scope, including
                  `ArraySum`, `ArrayDot`, `VecSum`, `VecSumWhere`, and `VecDot`
                  arithmetic. A generic
                  `reduce` is only its callable body and receives no terminal-
                  site mode. The permission selects LLVM's unordered floating
                  reduction form where such a reduction is recognized. It
                  does not permit reciprocal estimates, approximate library
                  functions, NaN/Inf assumptions, signed-zero erasure or
                  algebra that is not justified by reassociation itself.

Contract          Authenticates every f32/f64 add, subtract and multiply formed
                  lexically under `contract`, but never emits LLVM's raw
                  `contract` fast-math flag. That flag is consumer-controlled
                  and could let a permitted add absorb a strict multiply.
                  Instead, before LLVM optimization, Align recognizes a direct
                  multiply operand of an add/subtract only when both checked
                  operations carry `contract` and lowers that use to the
                  existing explicit `fma` operation. The four scalar/vector
                  shapes are `a*b+c`, `c+a*b`, `a*b-c`, and `c-a*b`; subtraction
                  uses an exact sign inversion on one FMA operand. Array and
                  fixed-vector dot use the same authenticated fused
                  product/accumulator step. A multiply reached only through a
                  local, load, call or later LLVM inlining is not a recognized
                  pair and remains separate; permission does not promise that
                  every candidate contracts. Explicit source `fma` retains its
                  existing always-fused semantics inside and outside the scope.

Result contract   Both options are result-defined. Every input still produces
                  an IEEE floating value and never aborts. `reassoc` permits
                  an association-dependent rounding, signed-zero result, and
                  NaN payload choice; `contract` permits one fused rounding
                  instead of two and its consequent signed-zero/NaN payload
                  choice. Neither option permits poison for NaN or infinity.
                  Results may differ across targets and optimization profiles
                  only within the named permission. Reassociation may compose
                  when LLVM inlining exposes independently permitted
                  operations. Contraction is decided from authenticated MIR
                  pairs before LLVM optimization and never arises merely from
                  later inlining. No reproducibility or accuracy bound is
                  promised for a permitted expression.

Excluded flags    `nnan`, `ninf`, `nsz`, `arcp`, `afn`, `fast`, and every
                  unnamed or future LLVM fast-math flag are unavailable.
                  Unknown names are errors, not forward-compatible ignores.
                  The compiler never adds a flag not selected by the source
                  scope.

Lexical extent    A scope affects operations whose source expression is inside
                  its body. Nested scopes union their permissions; there is no
                  implicit way to restore a stricter mode within a relaxed
                  scope. Every named, inline, lifted or escaping function body
                  starts strict. A lambda written inside a scope does not
                  inherit it; relaxing lambda arithmetic requires a visible
                  `float(...)` scope inside that lambda body.
                  A call site never adds flags to operations in a separately
                  declared callee; that body uses only its own source scopes.
                  For reassociation, the scope is a permission boundary rather
                  than an optimization-isolation boundary: after body import or
                  inlining, independently permitted operations may compose and
                  a strict participant remains a barrier. Contraction is
                  stricter: Align forms an explicit FMA only from a MIR-visible
                  multiply/add-subtract pair whose two authenticated modes both
                  permit it. It emits no raw LLVM `contract` flag, so a strict
                  producer cannot be absorbed by a relaxed consumer after
                  inlining.

Pipelines         Built-in pipeline arithmetic belongs to the terminal or
                  stage expression that requests it. A `sum` terminal written
                  in `float(reassoc)` may use an unordered reduction. Inline and
                  named callable bodies both start strict; each must contain
                  its own visible scope for relaxed arithmetic. Fusion/inlining
                  does not change which source operations received permission.

Control flow      The form is a transparent block expression. Plain blocks,
                  `if`, `match`, `else`, `?`, `map_err`, loops and value-carrying
                  `break`, `return`, `arena`, `unsafe`, and task-group blocks
                  may occur inside it; the effective mode enters each child
                  and is restored on every normal, join, early-exit, error, and
                  malformed-input path. Cleanup, ownership, regions and
                  evaluation order behave exactly as in a plain block.
                  Relaxation permits reordering of the selected arithmetic
                  nodes, not reordering, erasure, duplication or speculation
                  of calls, loads, stores, traps or other observable effects.

Types             The block may contain mixed float and non-float code and may
                  contain no relaxable operation. Integer, boolean, pointer,
                  comparison, cast, minimum/maximum and explicit vector-lane
                  semantics are unchanged. f32/f64 scalar and vecN<f32/f64>
                  arithmetic use the same permission record.

Ownership         The wrapper's type, value category, ownership, effect and
                  region are
                  exactly its body's. Formation allocates no value and moving
                  a result into or out of the wrapper transfers the body result
                  exactly once. `region_of`, `tracks_region`, local-slice and
                  escape analysis, MoveCheck, effect scan, replay/clone,
                  depth/finalization, checked-HIR validation and MIR production
                  all recurse through the body. Source nulling, Drop,
                  replacement and return therefore occur exactly where the
                  same plain block would perform them; the wrapper adds none
                  and cannot hide one. Parser/AST own the option list and
                  block form. align_sema
                  validates options and carries one canonical two-bit
                  FloatMode on a checked-HIR FloatScope node. Every eligible
                  checked arithmetic/reduction node also records the effective
                  mode. HIR validation walks the retained lexical scope stack,
                  recomputes the exact expected mode, and rejects an invented
                  or dropped known bit as well as an unknown bit. align_mir is
                  built only from that authenticated HIR and preserves the
                  exact effective mode on each eligible arithmetic/reduction
                  node. align_mir selects only authenticated contraction pairs
                  and represents them with the existing explicit FMA operation.
                  align_codegen_llvm translates `reassoc` to that LLVM flag and
                  lowers the selected pairs to `llvm.fma`; it never emits raw
                  `contract`. The formatter owns canonical option order.

Errors            `float()` reports the missing option before checking body
                  errors. Sema scans the complete option list before the body.
                  If any name is unknown, it reports the first unknown in
                  source order, regardless of an earlier or later duplicate.
                  Only when every name is known does it report the first
                  duplicate's second occurrence. Thus both
                  `reassoc,reassoc,bogus` and `bogus,reassoc,reassoc` report
                  unknown `bogus`; `reassoc,contract,reassoc` reports duplicate
                  `reassoc` at its second occurrence.
                  Missing comma/parenthesis/brace remains a parser error at the
                  first missing token. A valid no-op scope is accepted.

Effects           Compile-time arithmetic permission only. The scope allocates
                  nothing, owns nothing, performs no I/O, creates no region,
                  changes no inferred Pure/Impure result and has no runtime
                  enter/exit action. It grants no permission to move effects.

Interface         Interface format 15 is unchanged. Its generic-template body
                  tag 1 and concrete-inline body tag 2 already carry the exact
                  UTF-8 source string; a `float(...)` scope therefore enters
                  those existing bytes and is parsed and checked again by the
                  consumer. No FloatMode field or function summary is encoded.
                  Plain imported declarations need no summary because caller
                  scopes do not affect callees. Unknown options fail consumer
                  checking before imported HIR or MIR publication.

Artifact/cache    The source hash changes for a scope in the current unit. The
                  existing body source string changes generic and eligible
                  concrete-body interface bytes and therefore their interface
                  hash. Format 15's body tag, policy version, source length,
                  UTF-8 bytes and extern sequence remain byte-for-byte the plan
                  74 encoding; there is no new tag, field or scalar width.
                  Compiler executable bytes enter `compiler_build_id`,
                  invalidating affected whole-program and per-unit object keys.
                  No ambient environment variable or build flag selects mode.

Prerequisite      Plan 68 G4 Part 1 is merged. Plans 69 and 74 are merged so
                  the intended reductions and interface-carried bodies have
                  stable owners. No milestone or portable-math implementation
                  is a prerequisite. Issue 1064 may consume this surface only
                  after its implementation merges.

Acceptance        One syntax/formatter owner covers the three canonical forms,
                  either source option order, empty/unknown/duplicate options
                  both unknown/duplicate relative orders and missing
                  delimiters. One parameterized semantic/MIR owner covers scope
                  entry/restoration through a plain block, `if`, `match`,
                  `else`, `?`, `map_err`, loop joins, value-carrying `break`,
                  `return`, `arena`, `unsafe`, and task-group blocks, including
                  normal, joining, early-exit, error and malformed paths,
                  strict named/inline/lifted/escaping function roots, explicit
                  scopes inside lambda bodies, scalar and
                  all five floating sum/dot reduction variants (`ArraySum`,
                  `ArrayDot`, `VecSum`, `VecSumWhere`, and `VecDot`), and
                  checked-HIR rejection of
                  invented, dropped and unknown bits against the retained scope.
                  One transparent-wrapper owner covers Copy, Move, borrowed and
                  arena-backed results through construction, move-in, move-out,
                  source nulling, Drop, replacement and return, and proves all
                  region/effect/escape/replay/depth/finalization passes recurse
                  into the body exactly once.
                  One MIR/LLVM owner proves exact `reassoc` flags, the complete
                  authenticated FMA-pair product, absence of raw `contract`,
                  and absence of fusion when either producer or consumer is
                  strict; optimized structural controls cover unordered sum
                  and explicit fused multiply-add. One interface owner covers generic
                  and concrete-body source round trips, consumer rechecking,
                  whole/per-unit parity and cache invalidation. Runtime numeric
                  controls prove
                  strict source remains bit-identical and relaxed NaN/Inf inputs
                  remain defined; they do not pin one relaxed result bit.

Performance       No latency, throughput, vector width or instruction-count
                  number is promised. No benchmark, align-llm build, full
                  nightly suite or exhaustive numeric corpus is a provider
                  gate. Structural optimized-IR/object assertions may prove
                  only that the requested permission reaches LLVM and enables
                  an unordered reduction/FMA on the owner fixture. The issue's
                  microbenchmarks and real-client targets remain optional
                  consumer measurements after adoption.

Mirrors           This plan, draft.md, docs/language-spec.md,
                  docs/design-notes.md, docs/open-questions.md, plan 68 G4,
                  docs/impl/02-frontend.md, docs/impl/04-mir.md,
                  docs/impl/05-backend-llvm.md, docs/impl/07-roadmap.md,
                  docs/impl/19-hir-validation-ledger.md and HANDOFF.md.
```

The complete operation/option product is fixed here. A dash means no fast-math
flag or special lowering; it does not mean that the optimizer may choose one.
`contract` below always means an Align-selected explicit FMA, never LLVM's raw
flag.

| Source operation | strict | `reassoc` | `contract` | both |
|---|---|---|---|---|
| f32/f64 or vec fadd/fsub | — | raw operation carries `reassoc` | raw operation stays unflagged; an immediate multiply operand becomes explicit FMA only when both modes permit | unmatched raw operation carries `reassoc`; an authenticated pair becomes explicit FMA carrying `reassoc` |
| f32/f64 or vec fmul | — | raw operation carries `reassoc` | raw operation stays unflagged; an authenticated direct consumer may replace that use with explicit FMA | unmatched raw operation carries `reassoc`; an authenticated direct consumer may replace that use with explicit FMA carrying `reassoc` |
| array-pipeline floating `sum` (`ArraySum`) | ordered adds/reduction | adds and reduction carry `reassoc`; reduction is unordered | unchanged ordered adds/reduction; no multiply exists to contract | adds and reduction carry `reassoc`; reduction is unordered |
| array-pipeline floating `dot` (`ArrayDot`) | ordered products/adds/reduction | products, adds, and reduction carry `reassoc`; reduction is unordered | ordered accumulation uses authenticated explicit FMA steps | authenticated FMA accumulation carries `reassoc` and may reduce unordered |
| fixed-vector floating `sum` (`VecSum`) | ordered lane adds/reduction | lane adds and reduction carry `reassoc`; reduction is unordered | unchanged ordered lane adds/reduction; no multiply exists to contract | lane adds and reduction carry `reassoc`; reduction is unordered |
| fixed-vector masked floating `sum_where` (`VecSumWhere`) | ordered selected-lane adds/reduction | selected-lane adds and reduction carry `reassoc`; reduction is unordered | unchanged ordered selected-lane adds/reduction; no multiply exists to contract | selected-lane adds and reduction carry `reassoc`; reduction is unordered |
| fixed-vector floating `dot` (`VecDot`) | ordered lane products/adds/reduction | products, adds, and reduction carry `reassoc`; reduction is unordered | ordered lane accumulation uses authenticated explicit FMA steps | authenticated FMA accumulation carries `reassoc` and may reduce unordered |
| explicit `fma` | existing fused call | unchanged | unchanged | unchanged |
| fdiv/frem, comparison, conversion, min/max, math call | — | — | — | — |
| integer/bool/char/pointer/memory/control operation | — | — | — | — |

LLVM's `reassoc` flag permits composition according to the participating
instructions' flags and carries no lexical scope identity. LLVM's `contract`
flag does not provide the same two-sided boundary: permission on a consuming
add/subtract can absorb an unpermitted multiply. Align therefore emits no raw
`contract` flag. Its pre-LLVM contraction selection checks both authenticated
modes and emits an explicit FMA only for that use. This table promises
permission, not that every eligible source pattern will be selected.

## 2. Rejected alternatives

| Alternative | Rejection |
|---|---|
| Relax floating point by default | Breaks reproducible programs and the settled ordered-default rule. |
| A compiler flag such as `--fast-math` | Ambient configuration changes program semantics without a visible source boundary and cannot compose per kernel. |
| One bundled `fast` level | Hides which guarantees are relinquished and would invite LLVM's poison-producing `nnan`/`ninf` flags. |
| `sum_reassoc`, `dot_fast`, and similar terminals | Creates a second surface per operation, leaves hand-written loops and future reducers unanswered, and does not compose contraction independently. |
| Function-only annotation | Is too coarse for mixed strict/relaxed numeric work and makes a call site unable to see the local semantic boundary. |
| Caller mode copied onto a callee's strict operations | Makes an unannotated function depend on its caller and invalidates separate compilation. Each callee operation keeps only its own permission. Reassociation may still compose after inlining because LLVM flags do not carry scope identity; contraction is selected before that point. |
| Raw LLVM `contract` on relaxed add/subtract/multiply | The consuming add/subtract alone can authorize fusion with a strict multiply, so flags on both source operations do not enforce the lexical boundary. Align instead checks both authenticated modes and emits an explicit FMA for the selected use. |
| Optimization barriers or forced `noinline` at every scope/function edge | Would make lexical scopes isolation regions, inhibit reassociation, and require a second call ABI. Strict operations already stop reassociation through missing flags; pre-LLVM pair selection makes contraction safe without a barrier. |
| A strict inner scope that removes an outer permission | Requires mode subtraction and makes one expression's arithmetic contract depend on nesting accidents. Move strict work to a separate function instead. |
| `nnan` or `ninf` | A NaN or infinity would become poison, adding hidden undefined behavior to a language where floats never abort. |
| Throughput thresholds in the provider gate | They test a host and optimizer cost model, not the source contract, and would repeat the waste identified in earlier performance issues. |

## 3. Implementation closure matrix

| Boundary | Required closure | Owner |
|---|---|---|
| Lex/parse/format | reserved `float`; option tokens remain identifiers; block-expression precedence; canonical formatting | parser/formatter owner |
| Checked formation | canonical two-bit FloatScope record plus effective mode on eligible nodes; exact validation order; no-op body admitted | semantic owner |
| Scope authentication | HIR validator recomputes the lexical union and requires exact equality on every eligible node; invented, dropped and unknown bits reject | checked-HIR mutation owner |
| Transparent analysis | the FloatScope wrapper has exactly its body's type, value category and Pure/Impure result; effect scan, replay/clone, HIR depth/finalization, validation and MIR production each visit the body exactly once; the expression-variant sweep tripwire requires every such pass to classify FloatScope | parameterized semantic/HIR/MIR structural owner |
| Ownership lifecycle | Copy, Move, borrowed and arena-backed body results preserve construction, move-in, move-out, source nulling, Drop, replacement and return exactly as a plain block; the wrapper adds no owner, allocation, null or cleanup | MoveCheck/Drop driver owner with struct, sum, Option and Result carriers |
| Region and escape | `region_of`, `tracks_region`, local-slice/view provenance and escape checking return the body's exact facts; arena-backed views cannot become static or escape through the wrapper, while valid borrowed returns retain their roots | region/escape owner over stack, arena, heap, static and borrowed origins |
| Scope propagation | retained HIR scope enters and restores across plain blocks, `if`, `match`, `else`, `?`, `map_err`, loops, value-carrying `break`, `return`, `arena`, `unsafe` and task-group blocks on normal, branch-join, loop-join, early-exit, error and malformed-input paths; no path attaches mode to a later strict operation; MIR receives only authenticated effective modes | parameterized semantic/HIR/MIR control-flow owner |
| Lambdas/calls | every named/inline/lifted/escaping body validates from a strict root; only a FloatScope retained inside that body changes its operations; named/direct/indirect/imported callee operations receive no caller flags; optimized owners show reassociation may compose among independently permitted caller/callee operations while a strict participant blocks it; later LLVM inlining never creates a new contraction | semantic/HIR/MIR/interface/LLVM owner |
| Scalar arithmetic | f32/f64 add/sub/mul receive only selected `reassoc`; div/rem/comparison/cast/min/max and explicit source fma do not gain unrelated flags; raw `contract` is absent everywhere | LLVM owner |
| Vector arithmetic | vecN<f32/f64> follows the same table for every admitted width | LLVM owner |
| Reductions | every existing floating sum/dot reduction variant — `ArraySum`, `ArrayDot`, `VecSum`, `VecSumWhere`, and `VecDot` — follows its exact product/add/reduction row above; unordered reduction appears only under `reassoc`; strict and contract-only controls remain ordered; integer instances remain unchanged | parameterized HIR/MIR/LLVM owner over all five variants |
| Generic reduce | reducer callable starts strict like every function; only a FloatScope written inside its body may mark its arithmetic; enclosing terminal scope is not inherited | semantic/MIR/LLVM negative owner |
| Contraction | scalar and vector `a*b+c`, `c+a*b`, `a*b-c`, and `c-a*b` become the existing explicit FMA only when the immediate multiply and consumer both carry authenticated `contract`; subtraction uses exact operand negation; strict-producer/relaxed-consumer, relaxed-producer/strict-consumer, local/load/call producers and post-LLVM inlining remain unfused; no raw LLVM `contract` flag is emitted; `contract` alone does not set `reassoc` | parameterized MIR/LLVM positive-and-negative owner |
| Effects and traps | calls, memory operations, bounds/division traps and cleanup retain source order and receive no fast-math permission | MIR/LLVM negative owner |
| Generic bodies | exact source scope survives template formation, interface round trip and monomorphization; consumer derives the same mode | interface/sema owner |
| Concrete bodies | plan 74 eligible source preserves the scope when emitted `available_externally`; producer and consumer optimized forms agree | interface/codegen owner |
| Malformed HIR/interface | known invented/dropped bits, unknown bits and mode on an ineligible node fail before MIR/codegen; unknown source options fail imported-body checking before cache publication | HIR/interface validator owner |
| Whole/per-unit | identical source has the same semantic mode and runtime-defined behavior in both compilation modes | driver owner |
| Numeric controls | strict exact-bit regression; relaxed NaN/Inf remains a defined float; no result pin inside the allowed envelope | driver owner |

One parameterized owner may close multiple rows. No row requires a benchmark.

## 4. Review finding ledger

| Review | Finding | Closure |
|---|---|---|
| `d4a2d307` independent design review | P1: checking only known bits and eligible types lets malformed HIR attach a valid relaxed mode to a strict source operation after the scope is discarded | Retain `FloatScope` in checked HIR; recompute the exact lexical union during HIR validation; reject both invented and dropped known bits before MIR. Reopened closure axis: float-mode provenance. |
| `1d9838b9` reopened full review | P1: lifting separates a lambda body from its declaring FloatScope, so a copied root mode is unauthenticated. P2: direct ArrayDot product/reduction semantics, arbitrary option parsing and unknown/duplicate precedence were incomplete. | Reopen the matrix around lifted-declaration provenance. Global HIR validation derives a unique target root mode from the parent lambda expression and validates nested lifted bodies from that map. Add the exact dot product/add/reduction row, parse option identifiers before sema, and make any unknown outrank duplicates. |
| `ae9983bb` reopened full review | P1: LLVM flags have no scope identity, so equally flagged operations can combine across sibling scopes or an inlined function boundary | Reopen lowered-rewrite composition. Define scopes as operation-permission boundaries, not optimization-isolation regions. Caller mode never marks strict callee operations. The reassociation half remains: independently permitted operations may compose while a strict participant blocks the rewrite through flag intersection. The later `a8d2e264` finding supersedes the contraction half. |
| `09338df4` reopened full review | P1: pipeline HIR stores lifted target names rather than parent lambda expressions, so declaration-site mode still cannot be authenticated. P2: the grammar prevented the promised sema diagnostic for `float()` | Remove declaration-site inheritance entirely: every function and lambda body starts strict and must contain its own FloatScope. This deletes the cross-function provenance mechanism and treats all callable forms uniformly. Parse an optional identifier list so sema owns the empty-list diagnostic. |
| `23b3be10` reopened full review | P1: the Reassoc prose still named generic `reduce`, contradicting strict callable roots and the complete operation table | Remove generic `reduce` from terminal-site relaxation. Its arithmetic lives only in the reducer function and is relaxed only by a scope written inside that body. Add the explicit negative matrix row. |
| `1381360f` reopened full review | P1: the reduction matrix named only the array-pipeline forms and could leave the three distinct fixed-vector HIR/MIR variants without mode semantics or an owner | Reopen the reduction-variant axis. Enumerate `ArraySum`, `ArrayDot`, `VecSum`, `VecSumWhere`, and `VecDot` separately in the complete product and bind one parameterized HIR/MIR/LLVM owner to all five. |
| `69949b68` reopened full review | P1: the new transparent expression wrapper had no ownership/region pass closure. P2: control-flow restoration omitted several block forms, and plan 12 still directed a future dot toward a rejected terminal-specific fast surface. | Reopen the transparent-wrapper axis. Add exact type/effect, ownership lifecycle, region/escape and exhaustive control-flow rows with parameterized owners; require every HIR analysis to recurse exactly once and the variant sweep to pin that classification. Replace plan 12's `fast dot` direction with this lexical permission scope. |
| `a8d2e264` reopened full review | P1: LLVM contraction is authorized by the consuming add/subtract, so tagging both operations does not prevent a relaxed consumer from absorbing a strict multiply | Reopen the contraction-lowering axis. Never emit raw LLVM `contract`; select only a direct MIR-visible multiply/add-subtract pair whose two modes are authenticated, lower that use to existing explicit FMA, and keep unmatched or later-inlined operations separate. Add all four scalar/vector shapes, all dot variants and both asymmetric strict/relaxed negative controls. |
| `545454b7` reopened full review | P2: the normative zip/map/sum example split multiplication and accumulation across a lifted lambda, so the new two-sided contraction rule would not fuse it | Replace the example with direct `xs.dot(ys)` and `a * b + c` pairs. No public contract, strategy or IR shape changes. |

## 5. PR boundary

The public design and the implementation are two PRs. The design must receive
one independent adversarial review before implementation because it adds syntax
and observable arithmetic semantics. The implementation is one capability:
parser through LLVM and imported-body rechecking are a strict producer/consumer
chain, and splitting them would leave either dormant syntax or consumer body
source with no matching semantic validation. If the implementation exceeds roughly 1,000 hand-written lines,
the larger boundary remains preferable because it avoids two temporary mode
representations and duplicates neither validation nor proof.
