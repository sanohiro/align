# Elementary math lowering and SIMD visibility

Status: implemented 2026-09-21. PR #1141 recorded the first contract, PR #1142
its negative feasibility result, and PR #1152 the corrected contract. The first
contract incorrectly made an
Align-owned, cross-target bit-identical math implementation a prerequisite for
adding the public functions. Align has no general Java/StrictMath-style
cross-target bit-identity policy, and the existing scalar `pow` has no such
promise. This amendment removes that prerequisite while retaining the useful
parts of the issue: the five-function family lands together, explicit vectors
lower as vectors, and pre-instruction-selection LLVM scalarization is
inspectable rather than hidden. Retaining vector LLVM IR is not misrepresented
as proof of final machine SIMD.

The implementation ships all five scalar/vector methods through the exact LLVM
intrinsics and adds the three-state `explain-opt` record. Provider absence is a
supported, visible outcome: the functions remain available, while retained
provider-less vector IR warns that final instruction selection may scalarize it.
Section 9 records the planned provider follow-up and feasibility assessment;
it is not part of the shipped SIMD guarantee.

This plan remains the contract for [issue 1063](https://github.com/sanohiro/align/issues/1063)
and G6 of the [vectorization contract](68-vectorization-contract.md). It is the
public-contract ledger and implementation closure matrix. Evidence baseline:
Align `854815acef2d7e2403532b40d7226aeb4fcbb66a`, LLVM 22.1.8, and align-llm
Request 92.

## 1. Public-contract ledger

```text
Surface           x.exp(), x.exp2(), x.log(), x.log2(), x.log10() on f32,
                  f64, vec2/4/8/16<f32> and vec2/4/8/16<f64>. Each method
                  takes no argument and returns the receiver type. Existing
                  b.pow(e) remains scalar-only. No free-function alias,
                  implicit conversion or source-visible provider selection is
                  added.

Closed set        core.math reserves the IEEE 754-2019 §9.2 elementary family
                  as one design domain rather than reopening the compiler for
                  one client at a time:
                    E1  exp, exp2, log, log2, log10 (this capability)
                    E2  expm1, exp2m1, exp10, exp10m1, logp1, log2p1,
                        log10p1, hypot, rSqrt
                    E3  compound, rootn, pown, pow, powr
                    E4  sin, cos, tan, sinPi, cosPi, tanPi, asin, acos, atan,
                        atan2, asinPi, acosPi, atanPi, atan2Pi, sinh, cosh,
                        tanh, asinh, acosh, atanh
                  Reservation is not availability. Only E1 ships here. A
                  later tier needs its own complete ledger and implementation
                  capability. The current pow surface is unchanged.

Numeric result    Each operation denotes the named real elementary function
                  rounded to the receiver's IEEE binary format by the selected
                  LLVM target implementation. No ULP ceiling, correctly-
                  rounded promise, scalar/vector bit equality or cross-target
                  bit equality is part of this capability. Results may differ
                  in their last bits across LLVM versions, target libraries,
                  vector widths and targets. This is the existing scalar pow
                  precision posture, made explicit for the new family rather
                  than strengthened into a repository-owned math library.

Special values    Every E1 operation is total and never aborts.
                  - exp/exp2: both zeros -> +1; +inf -> +inf; -inf -> +0.
                  - log/log2/log10: +1 -> +0; +0 and -0 -> -inf; +inf ->
                    +inf; every negative nonzero value, including -inf, -> NaN.
                  - Every NaN input produces NaN. NaN sign, payload, quieting
                    and the exact NaN bits are unspecified.
                  Overflow returns +inf. Underflow may return a nonnegative
                  subnormal or +0 according to the selected implementation.
                  Align exposes no errno or floating-point exception status.

Scalar lowering   Scalar receivers lower directly to llvm.exp.*, llvm.exp2.*,
                  llvm.log.*, llvm.log2.* and llvm.log10.*. LLVM may select a
                  target instruction, compiler-rt implementation or platform
                  libm call. Align adds no portable_math.bc, approximation
                  kernel, coefficient table, runtime dispatch or fallback
                  implementation of its own.

Vector lowering   Explicit vector receivers lower directly to the matching
                  LLVM vector intrinsic. Semantics are lane-wise with no
                  cross-lane state. This preserves the operation as a vector
                  at the Align-to-LLVM boundary and gives LLVM or a configured
                  vector-function provider the exact operation identity.
                  Machine SIMD is not universally available: LLVM may legally
                  scalarize the intrinsic when the selected target has no
                  usable vector mapping. Scalarization changes neither the
                  source type nor the lane-wise semantic contract.

SIMD visibility   `alignc emit-llvm --stage optimized` exposes the exact
                  optimized LLVM shape. `alignc explain-opt` accounts for each
                  reached E1 vector operation as eliminated or merged, retained
                  vector IR, or scalarized before instruction selection, and
                  names the available LLVM reason. When no vector provider is configured
                  it also says that final instruction selection may scalarize
                  a retained intrinsic. It never reports machine-SIMD success
                  merely because raw or optimized IR is vector-shaped. Final
                  machine shape belongs to emitted-object disassembly and to
                  any target-provider acceptance test that promises it. Normal
                  builds remain quiet and behavior-preserving. A future vector
                  provider may improve the outcome without changing source
                  semantics; its selection must be explicit, deterministic
                  toolchain configuration and never ambient library discovery.

Unavailable SIMD The public scalar and vector functions remain available when
                  machine SIMD is unavailable. The operation may be
                  scalarized; there is no build failure and no silent claim
                  that SIMD occurred. `explain-opt` exposes the LLVM-level
                  disposition and provider availability; code that requires a
                  particular machine shape additionally inspects the emitted
                  object on its target. Align does not expose a source-level
                  `require_simd` mode in this capability.

Ownership         align_sema owns method/type/arity checking and the exhaustive
                  MathFn classification. align_mir owns preservation and
                  checked-HIR/MIR validation. align_codegen_llvm owns direct
                  intrinsic lowering. align_driver/explain-opt owns the
                  post-optimization visibility record. LLVM and the selected
                  target toolchain own the final scalar or vector math
                  implementation.

Effects           Pure in Align. No Align allocation, I/O, global state,
                  locale, panic, abort, ownership transfer, borrow, lifetime,
                  Drop or cleanup is introduced. Arguments and results are
                  Copy values. Align does not expose errno or floating-point
                  exception flags as an effect channel.

Errors            Invalid receiver kind is diagnosed before arity. A valid
                  receiver with arguments reports arity. Checked HIR/MIR reject
                  an E1 operation whose arity, operand type or result type does
                  not match its MathFn row. Lack of a vector provider is not a
                  language or build error; it is an optimization outcome
                  reported by explain-opt.

Allocation        None in Align IR or runtime. A selected external scalar or
                  vector implementation is outside Align ownership; no hidden
                  temporary array or per-lane heap allocation is permitted by
                  compiler lowering.

Artifact/cache    There is no new embedded bitcode artifact, digest, manifest,
                  runtime ABI row or cache component. MathFn and MIR/interface
                  serialization changes flow through the existing compiler
                  build identity and interface-format versioning rules. A
                  future provider configuration that changes code generation
                  must enter the existing target/toolchain cache identity.

Prerequisite      None from issue 1069 or plan 70. G1-G3 remain prerequisites
                  only for auto-vectorizing ordinary slice loops, not for the
                  E1 surface or explicit-vector lowering.

Acceptance        Section 3 owns surface/type failures, checked-HIR/MIR
                  exhaustiveness, special-value classes, scalar and vector
                  intrinsic spelling, whole/per-unit raw-lowering and result
                  parity, mode-local optimized visibility, absence of a new
                  artifact/runtime ABI surface, and truthful SIMD visibility.
                  It does not pin result bits or a performance number.

Performance       No latency, throughput or universal machine-SIMD promise is
                  made. The structural promise is exact: explicit vectors
                  reach LLVM as vector intrinsics, and the selected optimized
                  outcome is inspectable. A later provider capability may make
                  a target-specific performance promise with its own benchmark
                  and availability ledger.

Mirrors           This plan, plan 68 G6, draft.md float/core.math/SIMD,
                  docs/language-spec.md, docs/design-notes.md,
                  docs/open-questions.md, docs/impl/03-types.md,
                  docs/impl/05-backend-llvm.md, docs/impl/07-roadmap.md,
                  docs/impl/09-explain-opt.md,
                  docs/impl/19-hir-validation-ledger.md and HANDOFF.md.
```

The distinction is deliberate. A `vec4<f32>.exp()` is always one lane-wise
vector operation in Align and in raw LLVM IR. It is not a promise that every
LLVM target owns a native four-lane exponential. The optimizer inspection says
whether vector IR survived and whether a provider is configured; it does not
pretend to replace final object inspection. Align does not manufacture
cross-target result identity to hide that target fact.

### 1.1 Exact visibility record

The inspection path builds one private record for every reached, user-written
explicit-vector E1 operation before optimization and resolves it after the
selected profile pipeline:

```text
MathVisibilityRecord {
  function: ProgramCall
  operation_ordinal: u32
  operation: Exp | Exp2 | Log | Log2 | Log10
  ty: Vec(FloatWidth, LaneCount)
  state: EliminatedOrMerged | RetainedVectorIr | Scalarized
  provider: None | Configured(ProviderId)
  source: Option<MathVisibilitySource>
  llvm_reason: Option<String>
}

MathVisibilitySource {
  file: String
  line: u32
  column: u32
}
```

`operation_ordinal` is one-based within the concrete function in HIR evaluation
order across E1 vector operations only. `FloatWidth` is exactly `F32 | F64`;
`LaneCount` is exactly `2 | 4 | 8 | 16`. `ProviderId` is compiler-owned ASCII
toolchain identity, never an ambient path or soname; this capability produces
only `None`. `llvm_reason` is diagnostic text and never artifact identity. A
missing LLVM reason is rendered with the stable compiler explanation below.

The states are exhaustive. `EliminatedOrMerged` means no independent
result-producing operation with that source identity survives because ordinary
optimization removed it as dead, folded it, or merged it with another
operation. Every reached source record remains in the inventory: when common
subexpression elimination merges calls, the lowest `operation_ordinal` in the
equivalent surviving group owns the `RetainedVectorIr` or `Scalarized`
operation and every other record is `EliminatedOrMerged`, in original ordinal
order. The inspection contract does not guess which LLVM transformation removed
the independent operation.
`RetainedVectorIr` means an LLVM vector intrinsic or vector arithmetic
implementing the operation survives. `Scalarized` means the optimized module
instead contains its per-lane scalar math-call/operation chain. A mixed
surviving vector-and-scalar expansion is `Scalarized`; it cannot be reported as
retained success. A record with no independent surviving operation is the
defined `EliminatedOrMerged` outcome, not an analysis failure. Structurally
invalid type, ordinal, source identity, or an otherwise unclassifiable
surviving operation makes `explain-opt` fail with an internal diagnostic rather
than guess. This analysis occurs before instruction selection and therefore
never claims final machine SIMD.

Records use the same authenticated located-source catalog and escaping rules as
the existing current-plan report. They retain MIR function order and ascending
`operation_ordinal`. In each unit they render after current-plan records and
before LLVM remark records. Default output includes `Scalarized` and every
`RetainedVectorIr` row whose provider is `None`; `--verbose` additionally shows
`EliminatedOrMerged` and provider-backed retained rows. The exact rendered state
spellings are `eliminated-or-merged`, `retained-vector-ir`, and `scalarized`.
Source-less rows never fabricate line 0. Default output replaces all
default-eligible source-less rows with one aggregate after the located math
rows; verbose output renders every source-less row in record order.

The state/provider presence matrix is exhaustive:

| State | Provider | Default | Verbose |
|---|---|---|---|
| `EliminatedOrMerged` | `None` or `Configured` | omitted | exact eliminated-or-merged row |
| `RetainedVectorIr` | `None` | exact provider-absent row | same row |
| `RetainedVectorIr` | `Configured(id)` | omitted | exact provider-backed row |
| `Scalarized` | `None` | exact provider-absent scalarized row | same row |
| `Scalarized` | `Configured(id)` | exact provider-backed scalarized row | same row |

The exact default messages are:

```text
<file>:<line>:<column>: vector math `<operation>` was scalarized before instruction selection — <LLVM reason or "LLVM supplied no reason">; no vector math provider is configured
<file>:<line>:<column>: vector math `<operation>` was scalarized before instruction selection with provider `<provider>` — <LLVM reason or "LLVM supplied no reason">
<file>:<line>:<column>: vector math `<operation>` remains vector IR, but no vector math provider is configured; final instruction selection may scalarize it
```

An eliminated-or-merged row is verbose-only:

```text
<file>:<line>:<column>: vector math `<operation>` has no independent optimized operation; it was eliminated or merged
```

Source-less rows use these exact forms:

```text
+ <N> vector-math visibility record(s) without user source (see --verbose)
  [vector math `<function>` #<ordinal> `<operation>` `<type>`] <state> — <explanation>; source location is unavailable
```

The first line is the single default aggregate. The second is the verbose form
for each source-less row. Its explanation is the corresponding located message
without the source prefix; `<state>` uses the exact spellings above.
`<function>` is the canonical `ProgramCall` spelling, `<ordinal>` is unsigned
decimal without padding, `<operation>` is the lowercase source spelling, and
`<type>` is canonical `vec<N><f32|f64>` syntax such as `vec4<f32>`.

The provider-backed success wording, reserved for a later provider capability,
is verbose-only:

```text
<file>:<line>:<column>: vector math `<operation>` remains vector IR with provider `<provider>`; final machine shape is owned by that provider's object-disassembly contract
```

The filename, function spelling, provider id and LLVM detail follow the existing
single-line escaping rules. There is no unavailable numeric field, estimated
speedup, result-bit sample or source text in the record.

## 2. Provider and fallback policy

| Candidate | Policy |
|---|---|
| LLVM scalar/vector intrinsics | Canonical lowering. They preserve operation identity without making a false final-machine-code promise. |
| Darwin libsystem, libmvec, SVML or SLEEF | Eligible future vector providers. Target/version availability and result bits may differ; that is permitted and must be visible in toolchain identity and explain-opt. |
| Align-owned portable_math.bc | Not built. Cross-target bit identity is not an Align requirement, so owning ten approximation kernels, tables and universal proofs is unjustified. |
| Per-lane compiler-generated scalar calls | Legal LLVM fallback, but never reported as SIMD. The optimized shape and explain-opt record expose it. |
| Ambient provider discovery | Rejected. A host library appearing on PATH or a linker search path cannot silently change optimization or cache identity. |
| Source-level provider selector | Not added. Provider choice is a compiler/toolchain concern; source retains one math operation. |

The previous investigation remains useful evidence: LLVM 22 without a vector
library scalarizes `llvm.exp.v2f32`; Rust `libm` inlining does not form a vector
operation graph; the inspected LLVM libc mathvec revision lacks most of E1; and
SLEEF offers broad SIMD algorithms without cross-CPU bit identity. Under the
amended contract these results mean "report scalarization" or "candidate future
provider", not "withhold the language surface".

## 3. Implementation closure matrix

This capability crosses sema, HIR/interface validation, MIR, LLVM lowering and
the inspection path. Every applicable row must close before implementation is
published.

| Cell | Required behavior | Owner evidence |
|---|---|---|
| Type formation | Five zero-argument methods accept f32/f64 and every float vector; integers, masks, arrays, unconstrained numerics and arguments reject in deterministic order | one sema table plus negative controls |
| HIR exhaustiveness | Every new MathFn has one exact arity/type/result row; malformed checked HIR rejects before MIR | MathFn tripwire plus checked-HIR mutation table |
| MIR preservation | Every E1 operation and scalar/vector type survives lowering, whole-program construction and per-unit/interface replay | parameterized MIR and whole/per-unit owners |
| LLVM scalar lowering | f32/f64 map to the exact scalar LLVM intrinsic with no pow decomposition | raw-IR owner over ten rows |
| LLVM vector lowering | every width/type maps to the exact vector LLVM intrinsic with no compiler-built lane loop, extract/insert chain or temporary array in raw IR | raw-IR owner parameterized by function, width and type |
| Special values | the named zero/infinity/NaN result classes hold for scalar and every vector lane; NaN payload and finite last bits are not compared | runtime classification corpus on supported hosts |
| Optimized vector IR | when LLVM retains vector form, explain-opt reports retained vector IR and optimized IR contains no per-lane scalar call chain; the wording makes no final-machine claim | one deterministic retained-vector fixture |
| Optimizer scalarization | when LLVM expands the operation before instruction selection, explain-opt reports scalarization and never reports vector success; optimized IR supplies the independent truth | one target-independent synthetic analyzer owner plus the LLVM 22 E1 negative fixture where stable |
| Elimination and merging | dead, folded and commoned operations retain their source inventory rows as `EliminatedOrMerged`; a CSE survivor owns exactly one retained/scalarized row and original ordinal order remains stable | synthetic analyzer owners for dead, folded and duplicate live operations |
| Source availability | located rows use exact source messages; source-less default output aggregates their count and verbose output identifies function, ordinal, operation, type and state without line 0 | located/source-less rendering goldens |
| Machine-shape boundary | without a configured provider, retained vector IR reports that final instruction selection may scalarize; a future provider's machine-SIMD promise requires object-disassembly owners | provider-absent explanation owner |
| Provider absence | normal build and execution remain successful and lane-correct without a configured vector provider | explicit-vector runtime owner with provider absent |
| Whole/per-unit | raw intrinsic spelling and runtime results agree across modes; each mode's visibility records agree with its own optimized module, while legal inlining, folding, merging and elimination may produce different classifications | direct whole/per-unit owner with mode-local optimized-IR comparison |
| Cache/toolchain | no E1-specific artifact enters identity; any later provider configuration must change the existing target/toolchain identity | cache identity assertion plus future-provider tripwire |
| Existing math | abs/min/max/sqrt/floor/ceil/round/trunc/fma and scalar pow retain their current semantics and lowering | existing scalar_math and vec_math owners |
| Failure hygiene | malformed IR publishes no object/cache; explanation analysis does not mutate optimized output | mutation owner plus normal/explain byte-parity owner |

One parameterized owner may close every function/type/width cell. The matrix
does not require one fixture per spelling.

## 4. Historical feasibility result

PR #1142 measured four implementation candidates under the superseded
bit-identity contract:

| Candidate | Recorded result | Amended disposition |
|---|---|---|
| LLVM 22 intrinsics | Vector transcendental scalarized without a vector library. | Adopt as canonical lowering; expose the scalarized outcome. |
| Rust `libm` 0.2.16 | Useful accuracy reference, but forced inlining stayed scalar or retained lane extraction/insertion. | No longer a candidate implementation; no Align-owned kernel is needed. |
| LLVM libc mathvec | Incomplete E1 f32/f64 matrix at the inspected revision. | May become a provider when its exact target matrix is sufficient. |
| SLEEF | Broad 1-ULP SIMD algorithms; no cross-CPU bit-identity guarantee. | Eligible provider because cross-target bit identity is no longer promised. |

The disposable probe and its exact revisions remain recorded in PR #1142. They
need not be rerun to add the language surface. A future provider capability
must measure its own exact version, target matrix, accuracy claim, optimized
shape, artifact/link dependency and cache identity.

## 5. PR boundary

The contract amendment is one design PR. After its independent review, the
five E1 functions and their truthful inspection record form one implementation
capability: splitting by function would repeat every enum, validation,
serialization, LLVM and explanation proof. A target-specific vector provider
is a later, independent capability because the functions remain useful and
correct without one. Issue 1064 remains separate: typed byte views have a
different public contract, ownership model and failure domain.

The implementation is slightly above the roughly 1,000-line review threshold
because the five-function/type/width special-value matrix and the strict
producer-to-consumer visibility path close one failure domain. Splitting the
intrinsic producer from its `explain-opt` consumer would leave a dormant,
unverifiable metadata contract and duplicate whole/per-unit and malformed-input
proof; splitting by function would duplicate the same exhaustive enum sweeps.

## 6. Superseded review findings

PR #1141's review findings correctly closed gaps in its proposed
repository-owned artifact: universal proof coverage, malformed-artifact error
precedence, cache invalidation and an unjustified throughput gate. This
amendment removes that artifact and its bit-identity promise, so those closures
remain historical evidence rather than implementation requirements. The new
matrix instead owns intrinsic exhaustiveness and truthful post-optimization
visibility.

## 7. Amendment review closure

The independent reviews of this amendment found five contract gaps. They are
closed here before implementation:

| Finding | Closure |
|---|---|
| Source-less math records had no exact rendering | Section 1.1 now fixes both the default aggregate and per-record verbose form, including field order and state spelling. |
| Common-subexpression elimination could leave a reached call without an independent optimized operation | `EliminatedOrMerged` now covers dead, folded and merged operations; ordinal ownership and the CSE survivor rule are explicit and have matrix coverage. |
| The public visibility promise named only retained and scalarized operations while verbose output admitted eliminated rows | The public promise and mirrors now use the same exhaustive three-state classification, with default/verbose presence rules stated separately. |
| Public mirrors could read as if optimized-IR inspection observed scalarization during instruction selection | Every public mirror now limits `explain-opt` to the pre-instruction-selection disposition, warns that retained provider-less IR may still scalarize, and assigns final machine SIMD to emitted-object inspection. |
| Whole/per-unit coverage required identical optimized visibility even when their legal optimization opportunities differ | Raw lowering and runtime results retain parity; each mode instead validates visibility against its own optimized module. |

## 8. Implementation review closure

The implementation review found two P2 gaps in the inspection producer. Both
are closed as one visibility-integrity class:

| Finding | Closure |
|---|---|
| A synthetic interface function could lend its nonzero statement coordinate to the current unit's filename | Located MIR now carries a diagnostic-only, catalog-authenticated user-source fact per function. Math inventory publishes a coordinate only when that fact is present; synthetic interface bodies remain source-less even when LLVM debug coordinates exist. |
| Captured LLVM scalarization reasons never reached `MathVisibilityRecord` | The inspection producer now correlates explicit scalarization remarks by exact authenticated file, line and column. Unrelated or differently located remarks are ignored; absence retains the stable `LLVM supplied no reason` fallback. |

## 9. Planned follow-up: vector providers and machine-code verification

Status: planned investigation, recorded 2026-09-21.
Provider selection, integration and machine-code qualification remain unimplemented.
This section records direction and acceptance questions, not a new public
contract or an extension of issue 1063's completed acceptance boundary.

### Feasibility assessment

The engineering assessment is that a bounded machine-SIMD guarantee is
practical: qualify a named provider version for exact CPU features, functions,
element types and vector widths. Cross-target bit identity is not necessary
for this guarantee. Existing vector math implementations make integration and
verification a credible route without developing Align-owned approximation
kernels. This is a feasibility judgment, not evidence that Align's LLVM 22
pipeline already supports every required combination.

[LLVM documents vector-library mappings](https://llvm.org/docs/Vectorizers.html)
for providers including SLEEF, libmvec and Darwin libsystem.
[SLEEF's support matrix](https://github.com/shibatch/sleef#supported-environment)
lists AVX2 and AArch64 AdvSIMD as mainline implementations and Linux/macOS
support. Its current SSE2 status is experimental, so AVX2 evidence cannot
qualify Align's default x86-64-v2 baseline. These upstream capabilities justify
probes; they do not establish function completeness, ABI compatibility or
intrinsic-to-provider mapping in Align's pinned toolchain.

The recommended first scope is the five E1 operations on explicit float
vectors. Ordinary scalar-loop auto-vectorization remains dependent on loop
legality, aliasing, control flow and the vectorizer's decisions; qualifying a
math provider does not guarantee arbitrary loops will vectorize. A wide vector
may use several narrower SIMD calls. The intended guarantee concerns vector
execution, not a single instruction or an unmeasured speedup.

### Investigation and implementation sequence

1. **Measure a complete candidate matrix.** Start with SLEEF as a shared
   candidate for macOS/Linux AArch64 and Linux x86_64; compare platform
   providers where coverage or integration requires it. Pin provider and LLVM
   revisions, target triple/features, profile and link mode. Probe all five
   functions, f32/f64 and vec2/4/8/16, including widths smaller and larger than
   the provider ABI. Record supported, unsupported and unverified combinations
   separately, with reproducible commands and retained objects. Check the
   existing special-value contract and mixed ordinary/exceptional lanes.
2. **Specify one integration capability from the evidence.** Before production
   changes, extend the public ledger and closure matrix with exact deterministic
   provider configuration/defaults, availability and failure behavior, artifact
   packaging/licensing, ABI and link ownership, cache identity, whole/per-unit
   behavior, and diagnostic semantics. Verify how explicit LLVM vector
   intrinsics reach provider calls; upstream auto-vectorizer support alone is
   insufficient. Keep unavailable configurations usable under the existing
   fallback contract and distinguish configuration errors from unavailable SIMD.
3. **Qualify final code, including the provider.** Inspect emitted objects and
   the linked implementation or pinned provider artifact. Check that admitted
   operations reach actual SIMD implementations rather than per-lane scalar
   math calls; a vector symbol name or an unrelated SIMD instruction is not
   proof. Account for width splitting, dynamic dispatch and exceptional-input
   paths. If a provider uses scalar repair paths, record that limitation before
   deciding what guarantee the combination can carry. Add negative controls
   for absent providers, unsupported features and scalarized calls, plus
   bounded runtime owners for special values and representative finite inputs.
4. **Publish only qualified coverage.** Integrate provider selection, lowering,
   evidence-backed diagnostics and owner tests together as one useful
   capability. Specify how configured, qualified and unavailable/unverified
   coverage are distinguished; the current IR-stage report cannot certify an
   arbitrary final executable. LLVM/provider upgrades must rerun the affected
   qualification matrix. Benchmark only if making an explicit performance
   claim, separately from structural SIMD and numerical correctness checks.

Completion means that each advertised combination has reproducible final-code
evidence and each unsupported or unverified combination has truthful visibility.
The follow-up may narrow its supported matrix based on evidence; it must not
silently widen the shipped numerical contract or equate provider presence with
machine-SIMD success. Any new public promise follows the repository's design
review gate and updates the mirrors listed in Section 1 before implementation.
