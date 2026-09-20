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
                  the accumulator update of built-in sum/reduce/dot-shaped
                  pipeline lowering. It selects LLVM's unordered floating
                  reduction form where such a reduction is recognized. It
                  does not permit reciprocal estimates, approximate library
                  functions, NaN/Inf assumptions, signed-zero erasure or
                  algebra that is not justified by reassociation itself.

Contract          Marks every f32/f64 add, subtract and multiply formed
                  lexically under `contract`; LLVM may contract a multiply and
                  its consuming add/subtract only when every participating
                  operation carries that permission. It does not imply
                  reassociation. Explicit `fma` retains its existing always-
                  fused semantics inside and outside the scope.

Result contract   Both options are result-defined. Every input still produces
                  an IEEE floating value and never aborts. `reassoc` permits
                  an association-dependent rounding, signed-zero result, and
                  NaN payload choice; `contract` permits one fused rounding
                  instead of two and its consequent signed-zero/NaN payload
                  choice. Neither option permits poison for NaN or infinity.
                  Results may differ across targets and optimization profiles
                  only within the named permission. No reproducibility or
                  accuracy bound is promised inside a relaxation scope.

Excluded flags    `nnan`, `ninf`, `nsz`, `arcp`, `afn`, `fast`, and every
                  unnamed or future LLVM fast-math flag are unavailable.
                  Unknown names are errors, not forward-compatible ignores.
                  The compiler never adds a flag not selected by the source
                  scope.

Lexical extent    A scope affects operations whose source expression is inside
                  its body. Nested scopes union their permissions; there is no
                  implicit way to restore a stricter mode within a relaxed
                  scope. A lambda written inside the scope retains the active
                  permissions in its own body wherever it is later invoked.
                  A call to a separately declared function never changes that
                  callee's semantics. A callee uses only scopes in its own
                  source body.

Pipelines         Built-in pipeline arithmetic belongs to the terminal or
                  stage expression that requests it. A `sum` terminal written
                  in `float(reassoc)` may use an unordered reduction. Arithmetic
                  written in an inline lambda inherits the active lexical
                  permission at the lambda's declaration; a named callable
                  does not inherit permissions from the call site. This rule
                  is identical for fused and unfused lowering.

Control flow      The form is a transparent block expression. `return`, `?`,
                  `else`, `match`, loop `break`, cleanup, ownership, regions
                  and evaluation order behave exactly as in a plain block.
                  Relaxation permits reordering of the selected arithmetic
                  nodes, not reordering, erasure, duplication or speculation
                  of calls, loads, stores, traps or other observable effects.

Types             The block may contain mixed float and non-float code and may
                  contain no relaxable operation. Integer, boolean, pointer,
                  comparison, cast, minimum/maximum and explicit vector-lane
                  semantics are unchanged. f32/f64 scalar and vecN<f32/f64>
                  arithmetic use the same permission record.

Ownership         Parser/AST own the option list and block form. align_sema
                  validates options and carries one canonical two-bit
                  FloatMode on a checked-HIR FloatScope node. Every eligible
                  checked arithmetic/reduction node also records the effective
                  mode. HIR validation walks the retained lexical scope stack,
                  recomputes the exact expected mode, and rejects an invented
                  or dropped known bit as well as an unknown bit. align_mir is
                  built only from that authenticated HIR and preserves the
                  exact effective mode on each eligible arithmetic/reduction
                  node. align_codegen_llvm translates only those bits to LLVM
                  `reassoc` and `contract` flags. The formatter owns canonical
                  option order.

Errors            `float()` reports the missing option before checking body
                  errors. Options are validated left to right: the first
                  unknown option is reported before a later duplicate; after
                  all names are known, the first duplicate is reported.
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
                  and missing delimiters. One semantic/MIR owner covers scope
                  entry/exit, nested union, branch/loop joins, early exits,
                  inline-lambda capture, named-call noninheritance, scalar and
                  explicit-vector operations, and checked-HIR rejection of
                  invented, dropped and unknown bits against the retained scope.
                  One LLVM owner proves exact flags on admitted
                  nodes and their absence on strict, excluded and unrelated
                  nodes; optimized structural controls cover unordered sum and
                  contracted multiply-add. One interface owner covers generic
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
flag; it does not mean that the optimizer may choose another one.

| Source operation | strict | `reassoc` | `contract` | both |
|---|---|---|---|---|
| f32/f64 or vec fadd/fsub | — | `reassoc` | `contract` | `reassoc contract` |
| f32/f64 or vec fmul | — | `reassoc` | `contract` | `reassoc contract` |
| built-in floating `sum` reduction | ordered call | unordered `reassoc` call | ordered call | unordered `reassoc` call |
| explicit `fma` | existing fused call | unchanged | unchanged | unchanged |
| fdiv/frem, comparison, conversion, min/max, math call | — | — | — | — |
| integer/bool/char/pointer/memory/control operation | — | — | — | — |

LLVM treats `reassoc` and `contract` as rewrite permissions shared by all
instructions participating in a rewrite. Tagging every eligible operation in
the source scope therefore permits a local multiply-add/subtract contraction
without creating a cross-scope candidate. This table deliberately promises
permission, not that LLVM will perform a particular rewrite.

## 2. Rejected alternatives

| Alternative | Rejection |
|---|---|
| Relax floating point by default | Breaks reproducible programs and the settled ordered-default rule. |
| A compiler flag such as `--fast-math` | Ambient configuration changes program semantics without a visible source boundary and cannot compose per kernel. |
| One bundled `fast` level | Hides which guarantees are relinquished and would invite LLVM's poison-producing `nnan`/`ninf` flags. |
| `sum_reassoc`, `dot_fast`, and similar terminals | Creates a second surface per operation, leaves hand-written loops and future reducers unanswered, and does not compose contraction independently. |
| Function-only annotation | Is too coarse for mixed strict/relaxed numeric work and makes a call site unable to see the local semantic boundary. |
| Caller mode inherited by a callee | Makes a function's result depend on its caller's compilation context and invalidates separate compilation. |
| A strict inner scope that removes an outer permission | Requires mode subtraction and makes one expression's arithmetic contract depend on nesting accidents. Move strict work to a separate function instead. |
| `nnan` or `ninf` | A NaN or infinity would become poison, adding hidden undefined behavior to a language where floats never abort. |
| Throughput thresholds in the provider gate | They test a host and optimizer cost model, not the source contract, and would repeat the waste identified in earlier performance issues. |

## 3. Implementation closure matrix

| Boundary | Required closure | Owner |
|---|---|---|
| Lex/parse/format | reserved `float`; option tokens remain identifiers; block-expression precedence; canonical formatting | parser/formatter owner |
| Checked formation | canonical two-bit FloatScope record plus effective mode on eligible nodes; exact validation order; no-op body admitted | semantic owner |
| Scope authentication | HIR validator recomputes the lexical union and requires exact equality on every eligible node; invented, dropped and unknown bits reject | checked-HIR mutation owner |
| Scope propagation | retained HIR scope nests through all branch/block/loop/value positions; returns and early exits do not leak mode after the scope; MIR receives only authenticated effective modes | semantic/MIR owner |
| Lambdas/calls | inline lambda records declaration-site mode; lifted/escaping forms agree; named/direct/indirect/imported callees do not inherit caller mode | semantic/MIR/interface owner |
| Scalar arithmetic | f32/f64 add/sub/mul receive selected flags; div/rem/comparison/cast/min/max and explicit fma do not gain unrelated flags | LLVM owner |
| Vector arithmetic | vecN<f32/f64> follows the same table for every admitted width | LLVM owner |
| Reductions | built-in sum and dot-shaped fused lowering select unordered reduction only under `reassoc`; strict negative control remains ordered | MIR/LLVM owner |
| Contraction | eligible multiply plus consuming add/sub carry `contract`; optimizer fixture contains fused operation; `contract` alone does not set `reassoc` | LLVM owner |
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

## 5. PR boundary

The public design and the implementation are two PRs. The design must receive
one independent adversarial review before implementation because it adds syntax
and observable arithmetic semantics. The implementation is one capability:
parser through LLVM and imported-body rechecking are a strict producer/consumer
chain, and splitting them would leave either dormant syntax or consumer body
source with no matching semantic validation. If the implementation exceeds roughly 1,000 hand-written lines,
the larger boundary remains preferable because it avoids two temporary mode
representations and duplicates neither validation nor proof.
