# Portable elementary math

Status: design merged in PR #1141; E1 implementation deferred after the
candidate feasibility probes in §3.1 failed the vector/identity closure. Resume
only with a kernel source and proof package that closes every §3 cell; do not
land a partial intrinsic or scalar-libcall surface. This plan remains the
contract for [issue 1063](https://github.com/sanohiro/align/issues/1063) and G6 of the
[vectorization contract](68-vectorization-contract.md). This document is the
public-contract ledger, feasibility gate and implementation closure matrix.
Evidence baseline: Align `854815acef2d7e2403532b40d7226aeb4fcbb66a`, LLVM
22.1.8, and align-llm Request 92.

The issue's original implementation — map five new `MathFn` variants directly
to LLVM intrinsics — is rejected. On the measured Apple M1 baseline, LLVM 22
lowers a vector transcendental to scalar libcalls plus lane extraction and
insertion when no vector library is configured. That is slower than the scalar
loop and differs by platform library. A public intrinsic name would therefore
promise neither vector execution nor reproducible results.

The existing `str_prims.bc` path is not itself the answer. Its four admitted
rows are dependency-free leaf predicates, each bounded to 200 LLVM
instructions and admitted only after a paired performance measurement. Accurate
`f64` exp/log kernels need range-reduction tables and substantially larger
bodies. Folding them into that guarded set would silently discard the
admission predicate settled by plan 70. Portable math gets a distinct mandatory
artifact and a distinct proof.

## 1. Public-contract ledger

```text
Surface           x.exp(), x.exp2(), x.log(), x.log2(), x.log10() on f32,
                  f64, vec2/4/8/16<f32> and vec2/4/8/16<f64>. The five
                  methods take no argument and return the receiver type.
                  Existing b.pow(e) remains scalar during this capability; it
                  moves to the same portable policy before vector pow ships.
                  No free-function aliases or implicit conversions are added.

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
                  Reservation is not availability. Only E1 names type-check in
                  this capability. Each later tier needs its own complete
                  ledger, kernel evidence and implementation PR. The current
                  scalar pow does not imply the E3 accuracy or determinism
                  contract and is not renamed or removed here.

Numeric result    Round-to-nearest, ties-to-even is the sole arithmetic mode;
                  Align exposes neither a mutable rounding mode nor floating-
                  point exception flags. Each E1 result is at most 1 ULP from
                  the correctly rounded binary32/binary64 value, including
                  subnormal outputs. The scalar result, every lane of every
                  explicit vector width and both supported baseline targets
                  are bit-identical for the same input bits.

Special values    Every E1 operation is total and never aborts.
                  - exp/exp2: both zeros -> +1; +inf -> +inf; -inf -> +0.
                  - log/log2/log10: +1 -> +0; +0 and -0 -> -inf; +inf ->
                    +inf; every negative nonzero value, including -inf, ->
                    the canonical quiet NaN.
                  - Every quiet or signaling NaN input -> the canonical quiet
                    NaN, 0x7fc00000 for f32 and 0x7ff8000000000000 for f64.
                  NaN sign/payload and signaling state are deliberately not
                  preserved. Overflow returns +inf. Underflow returns the
                  correctly signed nonnegative subnormal or +0 selected by the
                  numeric-result rule. There is no errno or observable
                  floating-point status side effect.

Vector contract   Vector evaluation is lane-wise in lane order and has no
                  cross-lane state. Optimized release/fast lowering at the
                  default baseline target contains vector arithmetic for every
                  admitted width and no scalar exp/exp2/log/log2/log10 libcall,
                  no per-lane kernel call, and no surviving extract/insert
                  chain. Dev builds retain identical values but make no
                  instruction-shape promise. Ordinary slice-loop
                  auto-vectorization remains plan 68's G1-G3 corpus, not an E1
                  acceptance condition.

Implementation    One repository-owned scalar source defines each f32/f64
                  kernel and is compiled to `portable_math.bc` with the same
                  rustc and target triple as alignc. MIR carries the semantic
                  MathFn operation; LLVM lowering emits calls to those exact
                  definitions. Scalar calls and the lane maps used for explicit
                  vectors are merged before optimization. The optimizer may
                  scalarize machine vectors, but the optimized IR must satisfy
                  the vector contract. Kernel arithmetic uses one fixed
                  operation graph: explicit `llvm.fma` where the algorithm
                  requires fused rounding, otherwise strict IEEE operations;
                  no fast-math, ambient libm call, target feature branch,
                  runtime dispatch, host table generation or excess-precision
                  intermediate is allowed.

Feasibility       Section 3 is a fail-closed prerequisite, not a benchmark to
                  waive. Before the surface is implemented, one disposable
                  spike must prove all ten scalar kernels and every explicit
                  vector type on aarch64 and x86-64 baseline optimized IR. It
                  must also provide a complete, independently checked error
                  proof for every finite input and an operation-graph proof of
                  cross-target bit identity; a sampled conformance corpus is
                  regression evidence, never the proof. If any cell fails, E1
                  is deferred and no partial intrinsic/libcall surface lands.
                  The spike and proof tools are not retained as a recurring
                  compiler test suite.

Ownership         align_sema owns method/type/arity checks and the exhaustive
                  MathFn classification. align_mir owns preservation and
                  checked-HIR/MIR validation of the operation and type.
                  align_driver owns construction, embedding, digesting and
                  fail-closed loading of `portable_math.bc` plus target/layout
                  validation. align_codegen_llvm owns pure lowering to the
                  artifact entry points and optimized vector-shape owners. The
                  kernel source owns range reduction, tables, special values
                  and the exact operation graph.

Effects           Pure. No allocation, I/O, global initialization, TLS,
                  locale, errno, floating-point environment read/write, panic,
                  abort or runtime host state. Arguments are Copy values and
                  results are Copy values. There is no borrow, lifetime, Drop,
                  cleanup or ownership transfer.

Errors            Invalid receiver kind is diagnosed before arity. A valid
                  receiver with arguments reports arity. Artifact construction
                  failure fails the alignc build. At compilation, absent,
                  malformed, wrong-triple, wrong-datalayout, incomplete-symbol,
                  externally dependent or semantically mismatched bitcode is a
                  compiler error before object/cache publication; unlike the
                  optional string rt-LTO optimization, portable math never
                  falls back to host libm. Validation order is: embedded byte
                  presence and digest; bitcode parse; target triple; data
                  layout; exact exported-symbol manifest and signatures;
                  forbidden definitions/globals/attributes; undefined-symbol
                  closure; then module verification. The first failing phase is
                  the reported error, including when later phases are also
                  invalid.

Artifact/cache    `portable_math.bc` is separate from `str_prims.bc` and from
                  the optional `--rt-lto` digest. It contains only the E1
                  kernels, private helpers and constant tables. The driver
                  embeds its bytes. The existing executable-byte
                  `compiler_build_id` therefore changes for every kernel,
                  table, operation-policy or manifest edit and invalidates all
                  unit codegen keys, including units without E1. The artifact's
                  versioned content digest is still checked against the baked
                  manifest for stale/corrupt-artifact diagnostics; it is not a
                  second selective cache-key component. Whole-program and
                  per-unit builds consume identical bytes. The complete set of
                  definitions is internalized after linking, and no portable
                  math symbol remains undefined in an emitted object.

Prerequisite      Issue 1069 parts 1-3 are merged. The plan 70 runtime-effects
                  registry and its rt-LTO admission predicate are reused as
                  patterns but are not widened. Plan 68 G1-G3 are not a
                  prerequisite for the explicit-vector contract.

Acceptance        Section 4 owns surface/type failures, checked-HIR/MIR
                  exhaustiveness, scalar/vector special values, ULP corpus,
                  cross-target golden bits, whole/per-unit parity, artifact
                  rejection, cache isolation, optimized IR shape and the
                  absence of host-libm symbols. One parameterized owner closes
                  each family of cells; there is no test per input or width.

Performance       No latency or throughput number is promised, so no benchmark
                  is an admission or recurring correctness gate. The optimized
                  IR shape owner proves only the stated structural contract:
                  vector arithmetic with no scalar math/kernel call or
                  extract/insert chain. A later numerical performance promise
                  requires its own ledger and reproducible benchmark protocol.

Mirrors           This plan, plan 68 G6, draft.md float/core.math/SIMD,
                  docs/language-spec.md, docs/design-notes.md,
                  docs/open-questions.md, docs/impl/03-types.md,
                  docs/impl/05-backend-llvm.md, docs/impl/07-roadmap.md,
                  docs/impl/19-hir-validation-ledger.md and HANDOFF.md. Public
                  mirrors change only after the feasibility gate passes; this
                  candidate does not claim that E1 is shipped.
```

The accuracy contract is one ULP, not "whatever the platform libm returns."
Correctly rounded implementations may be used as references, but importing an
implementation does not import its build system, dynamic rounding mode, errno,
exception flags or target dispatch. The repository owns the exact source and
tables that define Align's result.

## 2. Rejected shortcuts

| Shortcut | Rejection |
|---|---|
| Emit `llvm.exp.*` / `llvm.log.*` | Without a configured vector library LLVM 22 lowers vector calls to scalar libcalls and lane shuffles. Host libm also breaks cross-target bit identity. |
| Configure Darwin libsystem / libmvec / SVML / SLEEF by target | Availability, version and result bits differ by host. This can only become an explicit nondeterministic opt-in under a later contract. |
| Add the kernels to `str_prims.bc` | Accurate kernels do not satisfy the settled leaf/200-instruction admission predicate. Math is mandatory semantics; string rt-LTO is optional optimization with fallback. |
| Vendor an upstream math library and its test suite | LLVM libc's relevant f32/f64 support headers alone are large and dependency-rich; SLEEF does not currently promise cross-CPU bit identity. Upstream sources are algorithm evidence, not a substitute for Align's closed artifact and proof. |
| Exhaust all f32 inputs in every PR | It consumes the detector budget without detecting integration regressions better than a fixed boundary/hard-case corpus. Exhaustive or proof-tool evidence is an admission artifact for a changed kernel, not a recurring compiler gate. |
| Sample random inputs without recording them | It is irreproducible and cannot bind a kernel revision. The checked corpus is deterministic and content-addressed. |
| Ship f32 first under the final names | It makes the same source program type-dependent in an arbitrary way and leaves the promised f64/vector contract dormant. E1 lands as one capability or is deferred. |

## 3. Feasibility gate

The gate runs before implementation and records its commands, compiler/LLVM
versions, artifact digest and results in this document. The disposable spike
may live outside the repository; only the evidence and the eventual reviewed
kernel source land.

| Cell | Required evidence |
|---|---|
| Source closure | The candidate dependency graph contains no allocator, panic/abort path, libc/libm symbol, TLS, mutable global, target intrinsic or dynamic rounding-mode query. Tables are immutable literal bits. |
| Artifact closure | `llvm-nm --undefined-only` is empty for the merged kernel closure; the exported manifest is exactly ten scalar entries before internalization. |
| Universal accuracy | For f32, an exhaustive all-input run against an independently correctly rounded reference; for f64, an independently checkable range-reduction and polynomial error certificate covering every finite input interval, with every approximation/rounding term and table bit included. The certificate checker and exact tool versions are recorded. Named special values compare exact bits. A corpus alone cannot close this cell. |
| Target identity | A machine-checked operation-graph audit proves that every admitted operation has target-independent IEEE bits: explicit fused versus unfused nodes, fixed-width integer operations, immutable table bits, preserved subnormals and no excess precision. aarch64 and x86-64 execution of the corpus is a negative control, not the universal proof. |
| Vector identity | The scalar and vector forms are generated from one typed operation graph. A structural checker proves node-for-node lane isomorphism for widths 2/4/8/16 and both element types; corpus execution checks the proof plumbing. |
| Optimized shape | Release and fast optimized IR on both baselines has vector arithmetic and has no scalar math/kernel call or surviving per-lane extract/insert chain for every explicit-vector method. |
| Size/build cost | Record bitcode bytes, alignc binary delta and clean/incremental driver build time. There is no numeric performance promise or threshold; this evidence only decides whether embedding the artifact is proportionate. |

The recurring corpus has four bounded parts: all named special/exact values; every
range-reduction boundary and its two adjacent representable values; the
published hard cases for the adopted algorithms; and a fixed-seed stratified
sample by sign/exponent/fraction. Its checked input-bit manifest and expected
result bits are content-addressed. Increasing a sample count without adding a
new invariant is not coverage and is rejected as test inflation. It detects
integration regressions after admission; it never substitutes for the
universal proof above. The exhaustive f32 run and f64 certificate checker rerun
only when the kernel operation graph, coefficients, tables or proof-tool
identity changes.

### 3.1 Recorded candidate results (2026-09-20)

The first feasibility pass is negative. One failed mandatory cell rejects a
candidate; running the remaining architecture matrix or a throughput benchmark
would add no decision evidence.

| Candidate | Evidence | Result |
|---|---|---|
| LLVM 22 transcendental intrinsics | The plan-68 Apple M1 probe lowered `llvm.exp.v2f32` to scalar `_expf` calls plus lane insertion/extraction when no vector library was configured. | Rejected: fails optimized shape and target-independent implementation. |
| Rust `libm` 0.2.16, soft-float path | Upstream assigns 1 ULP to all five f32/f64 E1 families against MPFR and provides exhaustive f32/high-iteration f64 tooling. A local Rust 1.96.1/LLVM 22.1.8 fat-LTO probe expanded four `libm::expf`/`logf` lanes. Default inlining retained four scalar calls. Raising LLVM's inline threshold to 10000 removed the calls, but `expf` remained 626 lines of scalar IR with no vector type; `logf` retained 12 `extractelement`/`insertelement` operations. | Rejected: useful accuracy reference, but fails the mandatory explicit-vector shape even after non-default forced inlining. |
| LLVM libc mathvec at `f28f0baf` | The generic portable vector tree currently supplies f32 `expf` and `logf` only. `logf` is a scalar `cpp::map`; `exp2`, `log2`, `log10` and every f64 E1 vector kernel are absent. The separate scalar correctly-rounded implementation is dependency-rich and does not close the vector surface. | Rejected for E1: incomplete type/function matrix and scalar-map shape. |
| SLEEF at `7623d6cf` | SLEEF supplies broad 1-ULP SIMD algorithms, but upstream issue 187 still records deterministic results across CPU architectures as unsupported. Its target-specific vector-extension implementations are precisely the result-identity variation this contract excludes. | Rejected as the guaranteed default: fails cross-target bit identity. It remains eligible only for a future explicit nondeterministic opt-in. |

The disposable local probe used `/tmp/align-math-spike` and
`/tmp/align-math-spike-target*`; it is not a repository test or artifact. Its
essential commands were:

```text
CARGO_TARGET_DIR=/tmp/align-math-spike-target \
  cargo rustc --manifest-path /tmp/align-math-spike/Cargo.toml \
  --release --lib -- --emit=llvm-ir

CARGO_TARGET_DIR=/tmp/align-math-spike-inline-target \
RUSTFLAGS='-Cllvm-args=-inline-threshold=10000' \
  cargo rustc --manifest-path /tmp/align-math-spike/Cargo.toml \
  --release --lib -- --emit=llvm-ir
```

The host was `aarch64-apple-darwin`; rustc was 1.96.1
(`31fca3adb283cc9dfd56b49cdee9a96eb9c96ffd`) with LLVM 22.1.8. No benchmark
was run: both Rust-libm shapes had already failed the deterministic IR gate.
The next admissible candidate must bring a truly vector operation graph for all
ten kernels plus its universal error/identity proof, not another request to
raise an inline threshold or test budget.

## 4. Implementation closure matrix

This matrix becomes actionable only after every §3 cell passes. A row may cite
one parameterized owner; it does not require one test per spelling, width or
input.

| Cell | Required behavior | Owner evidence |
|---|---|---|
| Type formation | Five zero-argument methods accept f32/f64 and every float vector; integers, masks, arrays, unconstrained numerics and arguments reject in deterministic order | one sema table plus checked-HIR replay |
| IR exhaustiveness | Every new MathFn survives serialization/checking and reaches MIR/codegen; every exhaustive classifier is updated | existing variant tripwire plus one MIR shape owner |
| Proof binding | the admitted operation graph, coefficients, tables, exported manifest and proof-tool identity hash to the exact implementation inputs; any change invalidates admission before build publication | digest owner over the proof record and kernel inputs |
| Scalar values | Exact special cases and <=1 ULP finite corpus for ten entries | one runtime conformance owner over the content-addressed corpus |
| Vector values | Every vector width/type is lane-bit-identical to scalar | the same conformance owner parameterized by width |
| Vector shape | aarch64/x86-64 baseline optimized IR contains no forbidden call/extract/insert shape | one cross-target IR owner over generated cases |
| Artifact validation | truncation, wrong target/layout, missing/extra symbol, undefined dependency, mutable global and digest mismatch reject before publication in the ledger's fixed order | mutation table over one valid fixture plus one all-invalid fixture that reports only the earliest phase |
| Whole/per-unit | results, optimized shapes and symbol closure agree; only units containing E1 consume the artifact digest | direct whole/per-unit owner |
| Cache | kernel source/table/policy/manifest changes alter the embedded compiler bytes and therefore miss every unit through `compiler_build_id`; exact source and binary revert restores identity | cache edit/revert owner with E1 and non-E1 units |
| Profiles/options | dev is value-correct; release/fast satisfy shape; small/tiny stay value-correct; `--rt-lto` on/off cannot change bits | one profile/option table |
| Existing math | abs/min/max/sqrt/floor/ceil/round/trunc/fma and scalar pow retain their current semantics and lowering | existing scalar_math and vec_math owners; no duplicate fixture |
| Failure hygiene | no host math symbol or new dynamic library enters emitted objects; malformed artifact publishes no cache/object | symbol owner plus failed-build residue check |

## 5. PR boundary

The feasibility evidence and this contract form one design PR. Implementation,
if admitted, is one capability PR because the public surface has no useful
state without the mandatory artifact, scalar/vector parity and both target
proofs. This is intentionally not combined with issue 1064: typed byte views
have a different public contract, ownership model and failure domain. Within
1063, all five E1 functions stay together to avoid repeating the artifact,
cache and target proof five times.

## 6. Design-review closure

The fresh review of candidate `6153efd1` found four P2 contract defects. The
fix commit closes the complete set without a second full-diff review:

| Finding | Closure |
|---|---|
| A bounded corpus did not prove the universal 1-ULP promise | f32 now requires an exhaustive all-input admission run; f64 requires an independently checked full-domain error certificate; the recurring corpus is explicitly only a regression owner. |
| Multiply-invalid artifact precedence was unspecified | the ledger fixes eight validation phases and the matrix adds one combined-invalid earliest-error owner. |
| Selective cache hits contradicted the embedded compiler build identity | every unit now invalidates through the existing executable-byte `compiler_build_id`; the artifact digest is validation metadata, not a selective key. |
| The throughput rejection was unquantified and noise-dependent | the benchmark gate is removed. No throughput number is promised; optimized IR structure is the sole stated performance-shape contract. |
