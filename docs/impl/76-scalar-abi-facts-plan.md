# Scalar ABI facts at program call boundaries

Status: implementation candidate for
[issue 1075](https://github.com/sanohiro/align/issues/1075). This document is
the public-contract ledger and implementation closure matrix. Evidence
baseline: Align `0aab8796457defcd11b0586fed5c8f5a0499e2ac`, LLVM 22.1.8.

Align currently lowers narrow scalar parameters and results without stating
their extension convention. That is not merely missed optimization. LLVM
classifies `zeroext` and `signext` as ABI attributes and requires them to agree
on the definition or declaration and the call site. This capability therefore
cannot be implemented only in `declare_fn` and `declare_imported_fn`: the same
type-derived record must reach every direct and indirect Align call.

This plan deliberately rejects three claims from the issue's proposed boundary
table. An `i8` boolean in aggregate storage is a memory-load fact, not a
function parameter or result. A fieldless enum currently crosses the Align ABI
as the ordinary enum aggregate, not as its integer tag. Giving either row a
function attribute would describe a value that is not present at that boundary.
The proposed `char` range is also false for integer-cast values. They receive
explicit dispositions in section 2 rather than meaningless or unsound tests.

## 1. Public-contract ledger

```text
Surface           Align-owned program ABI only: stored cores, imported per-unit
                  declarations, noncapturing function-value thunks, closure
                  thunks, generated parallel callbacks, direct calls, indirect
                  closure calls and task-trampoline calls. Externally visible
                  per-unit `pub` definitions still use this Align ABI. Native C
                  entry/export shells, foreign C calls, RawCall, runtime
                  declarations and runtime calls retain their independently
                  owned ABIs.

Canonical rule    scalar_boundary_facts(Ty, transport) is the sole derivation.
                  It returns an extension convention derived from the semantic
                  Ty and selected LLVM transport, never from source spelling
                  or an observed operand. Aggregate, pointer, view, float and
                  unit types have no scalar facts.

Type table        bool / direct i1       zeroext
                  u8, u16, u32           zeroext
                  i8, i16, i32           signext
                  char / direct i32      zeroext
                  u64, i64               none
                  f32, f64               none
                  aggregate enum         none
                  aggregate cleanup ret  none on the aggregate return

Char range        None. Integer-to-char casts are defined as the low 32-bit
                  code-point representation and currently admit values outside
                  Unicode's scalar domain. Even the conservative envelope
                  [0, 0x110000) would therefore be false and could create LLVM
                  poison. `char` receives only its truthful unsigned extension
                  convention. Tightening cast semantics is a separate language
                  decision, not an ABI optimization precondition.

Agreement         ABI extension facts are attached to both sides: every stored
                  definition or imported declaration and every corresponding
                  direct or indirect call site. Return facts are emitted only
                  when the selected LLVM return transport is that scalar.
                  Parameter facts are emitted only when the selected parameter
                  transport is that scalar. Hidden environment parameters,
                  cleanup proxies, byval values and aggregate result carriers
                  are indexed from the physical signature and never inherit a
                  neighboring source parameter's fact.

Native shells     A direct non-Unit/non-Result source `main` is itself the C
                  entry and receives no Align scalar facts, including no
                  `signext` on its i32 return. Generated C `main` and named
                  export wrappers likewise receive none. Their calls into an
                  encoded/internal Align body do receive the body's canonical
                  Align call-site facts. An externally linked per-unit `pub`
                  definition is not a native shell: its consumer is an Align
                  unit and both units derive the same facts.

Range semantics   No function-boundary range is emitted by this capability.
                  Stored booleans and enum tags are not scalar call-boundary
                  values, and `char` lacks a truthful range under the settled
                  cast implementation. This avoids both invented facts and an
                  otherwise unnecessary raw LLVM attribute constructor.

Direct calls      Ordinary MIR direct calls, direct calls with dynamic cleanup,
                  generated adapters whose semantic signature can carry a
                  scalar transport, parallel kernels and wrapper-to-body calls
                  use one call-site helper. A foreign declaration found in the
                  program-call table is excluded before facts are attached.

Fixed adapters    The SQLite callback target is fixed to aggregate
                  `DbFunctionArgs -> Result<DbValue, string>`, resource Drop is
                  fixed to `raw -> Unit`, and generated C-main bodies accept an
                  aggregate argv or no parameter and return Unit/Result. None
                  can carry this capability's scalar transport, so they remain
                  explicit no-op exclusions rather than invoking the helper.

Indirect calls    Function-value and closure calls derive facts from their
                  checked `param_tys`, `ret_ty`, `FnSignatureFacts` and physical
                  env-ABI offset. Both ordinary and dynamic-cleanup forms are
                  covered. A non-fallible task trampoline also calls a closure
                  thunk indirectly and derives its return fact from the task's
                  checked result Ty; Unit has no result fact and a fallible
                  task's aggregate `Result` has none. RawCall is excluded
                  because it denotes a native pointer whose ABI is not owned
                  by Align.

Return cleanup    `ReturnCleanupAbi::DynamicBit` returns `{value, i1}`. LLVM
                  parameter attributes cannot describe a nested aggregate
                  member, so no extension fact is put on that aggregate
                  return. Its value and cleanup extraction semantics are
                  unchanged. Parameters of the same function still receive
                  their facts. `ReturnCleanupAbi::None` scalar results receive
                  the table's return fact.

Ownership         align_codegen_llvm owns derivation and emission. align_sema
                  continues to own scalar validity; align_mir continues to own
                  checked call signatures. No source syntax, HIR/MIR variant,
                  interface field, runtime row or library surface changes.

Effects           Compile-time metadata only. No allocation, lifetime, borrow,
                  Drop, cleanup, error, side effect or runtime state changes.
                  Generated object code keeps the existing Align ABI while
                  making its narrow-scalar extension convention explicit.

Errors            A semantic scalar whose selected physical transport is not
                  the expected integer width is a codegen error before module
                  verification or artifact publication. A parameter/type/mode
                  count mismatch remains rejected by callable preflight before
                  attribution. Range construction cannot truncate a bound.

Artifact/cache    Emitted LLVM and object bytes change. The executable-byte
                  compiler_build_id therefore invalidates all affected whole-
                  program and per-unit codegen keys. Interface format and
                  canonical function identities do not change because the
                  facts are deterministically re-derived from serialized types
                  and transport metadata.

Prerequisite      Plans 69 and 71 are merged. This capability is independent of
                  runtime-effects plan 70 and does not widen its registry.
                  Issue 1064 may consume this capability after it merges.

Acceptance        One parameterized codegen owner checks the complete direct
                  scalar table on definitions, declarations and call sites;
                  negative rows prove no attribute leaks to i64, floats or
                  aggregates. One per-unit owner checks imported caller/callee
                  agreement. One indirect owner checks the env offset and both
                  ordinary and cleanup forms. One spawn fixture checks the
                  non-fallible task trampoline's indirect scalar return and the
                  fallible aggregate negative case. Existing wrapper/parallel
                  owners gain structural assertions only where they exercise a
                  scalar program call. Boundary-value execution covers signed
                  minima, unsigned maxima, bool values and char envelope
                  endpoints on the native target. LLVM module verification
                  remains the final structural control.

Performance       No latency, throughput or instruction-count promise is made,
                  so no benchmark and no align-llm build is a provider gate.
                  The issue's 763-mask and 13-to-10-instruction observations
                  are consumer measurements to repeat after adoption, not
                  correctness tests. A bounded optimized-IR negative control
                  may assert that the motivating bool caller/callee masks are
                  absent; it must not compile or profile the external client.

Mirrors           This plan, docs/impl/05-backend-llvm.md,
                  docs/impl/07-roadmap.md and HANDOFF.md. The language contract
                  does not change, so draft.md and docs/language-spec.md do not.
```

The rule follows LLVM's ABI distinction precisely: `zeroext` and `signext`
affect the target ABI and must match at both ends. See the LLVM 22
[parameter-attribute contract](https://llvm.org/docs/LangRef.html#parameter-attributes).

## 2. Dispositions and rejected shortcuts

| Proposal | Disposition |
|---|---|
| Add `zeroext` only to `bool` declarations | Rejected. It leaves call sites ABI-inconsistent and repeats the omission for every other narrow scalar. |
| Add attributes only in `declare_fn` and `declare_imported_fn` | Rejected. LLVM requires ABI attributes at call sites too, including indirect calls. |
| Mark stored `bool` i8 as a function `range` | Rejected. The i8 is a memory representation, not the i1 parameter/result. A future load-facts capability may attach load metadata where storage provenance proves it. |
| Mark a fieldless enum parameter with integer `range` | Rejected. The current parameter is an enum aggregate. Tag-load facts or an enum ABI representation change are separate capabilities. |
| Mark `char` with `range(i32 0, 0x110000)` | Rejected. Valid integer-to-char casts can produce any 32-bit pattern, so the attribute would turn accepted values into poison. |
| Put a range on a dynamic-cleanup aggregate return | Rejected. LLVM cannot attach an integer range to the nested value or cleanup member through an aggregate return attribute. |
| Add `llvm.assume` at each callee entry | Rejected. It does not define the caller's ABI extension convention and duplicates facts as instructions. |
| Run an align-llm benchmark in CI or preflight | Rejected. The provider contract is structural and semantic; external instruction counts are consumer-owned follow-up evidence. |

These dispositions reduce the original six conceptual rows to the four kinds
that actually cross an Align call boundary as scalars: boolean, unsigned narrow
integer, signed narrow integer and char. The integer widths are parameterized,
not separate hand-written cases or separate test binaries.

## 3. Implementation closure matrix

| Boundary / state | Formation and emission | Owner proof |
|---|---|---|
| Stored Align definition | derive from `Function.params`, modes, cleanup transport and `ret`; attach at physical ordinals; exclude direct C `main` | direct-table IR owner including char's no-range control, malformed transport negative control, direct-main negative assertion |
| Imported declaration | derive from `ImportedFn` through the same helper | per-unit caller/callee IR comparison |
| Direct MIR call | derive from callable-preflight `ProgramSignature`; exclude extern rows | direct-table IR owner checks call operands and return |
| Direct cleanup call | parameter facts only; aggregate return gets none | owned-result fixture checks params and negative return |
| Function-value thunk definition | env has no fact; explicit parameters start at physical ordinal 1; scalar return follows cleanup transport | indirect owner inspects thunk definition |
| Function-value thunk to target | target signature facts on the internal direct call | indirect owner inspects thunk body call |
| Closure thunk definition/call | same env offset; captured tail uses lifted target signature at its physical call | capture fixture inspects definition and lifted call |
| Indirect ordinary call | derive from checked `param_tys` / signature, offset by env; scalar result fact | indirect owner checks call-site attributes |
| Indirect cleanup call | parameter facts offset by env; aggregate result gets none | indirect owned-result fixture |
| Task-trampoline indirect call | non-fallible scalar `R` derives the return fact from `GeneratedId::Task.result`; Unit and fallible aggregate results get none | spawn owner checks bool/char/narrow-int returns plus fallible negative control |
| Native export / entry shell | generated wrapper definition and direct C `main` receive no Align scalar facts; wrapper-to-Align-body call follows the Align signature | existing export/main fixtures assert the shell exclusion and the body-call inclusion |
| Generated parallel program call | derive from the recorded stage/terminal signature, not the current SSA value | existing generated-parallel fixture gains focused assertion |
| Fixed aggregate/pointer adapters | SQLite callback, resource Drop and generated C-main calls have no eligible scalar transport; do not invoke the helper | signature-shape audit and existing adapter owners |
| Foreign/runtime/raw call | no program-scalar helper invocation | negative scan covers representative C, runtime and RawCall sites |
| Whole/per-unit | identical facts from identical semantic types; no interface field | paired optimized-IR owner and existing interface round trip |
| Target baselines | LLVM verifies the same IR contract; native boundary values run on each required platform | owner target in normal platform matrix; no separate benchmark job |

Author-side matrix-to-diff closure requires every program-function `build_call`
and `return_transport::build_indirect_call` site either to use the canonical
helper or to carry an explicit foreign/runtime/native exclusion. A repository
search is part of the owner test only when it checks a stable wrapper API; a
source-line count is not a correctness test.

This capability crosses the repository's approximate 1,000-line explanation
threshold because the reviewed ledger, closure matrix and one parameterized IR
owner ship with the single ABI rule. Splitting declarations, direct calls,
generated adapters or indirect calls would leave an ABI-inconsistent
intermediate state and duplicate the same semantic-type-to-transport proof.
They are one failure domain and have no independently safe consumer boundary.

## 4. Verification bundle

The narrow owner is the `align_codegen_llvm` scalar-boundary test filter. It
builds tiny in-memory fixtures and inspects emitted IR; it does not invoke the
full workspace suite or any benchmark. A single `main_abi` driver owner executes
the same boundary values through whole-program and per-unit builds. Existing
wrapper and parallel paths reuse the canonical helper rather than gaining new
test binaries.

```text
owner             scripts/cargo.sh test -p align_codegen_llvm scalar_boundary -- --nocapture
execution         scripts/cargo.sh test -p align_driver --test main_abi scalar_boundary_values_survive_whole_and_per_unit_calls -- --nocapture
structural        LLVM module verification in every emitted fixture
platform          existing codegen platform matrix
benchmark         none
external client   pending consumer-owned mask/instruction recount
```

Candidate verification on 2026-09-20: the codegen owner passed in 0.03 seconds
(one test, 221 filtered out), and the whole/per-unit execution owner passed in
1.03 seconds. Compilation took 16.88 seconds and 18.05 seconds respectively.
No benchmark or broad test suite was run. The helper owner also rejects a
semantic `bool` mapped onto a physical `i8` function or call before module
verification, pins the environment offset, and proves that a dynamic-cleanup
aggregate return receives no nested scalar result attribute.

The implementation is complete when the matrix is closed, the focused owner
passes, the standard bounded code gate passes, and the merged issue comment
records the external recount as remaining work rather than pretending it was a
provider acceptance gate.

## 5. Design-review closure

The first independent review of candidate `3b7eef67` found one P1: the matrix
covered MIR indirect calls but omitted the separate generated task-trampoline
call to a closure thunk. The complete root-cause audit found three owned
`return_transport::build_indirect_call` families: non-fallible/fallible task
trampolines, ordinary MIR indirect calls and cleanup-bearing MIR indirect
calls. This revision adds the task family to the surface, derivation rule,
acceptance set and matrix, and changes the author-side inventory from only
`build_call` to both call builders. `build_native_indirect_call` remains the
intentional RawCall/native exclusion. A fresh design review is required before
implementation because the finding was ABI-correctness severity.

That fresh review of candidate `1e4c07fa` found one P2: a direct i32-returning
source `main` is the C entry itself, not an adapter, so the stored-definition
row was ambiguous about `signext`. The complete native-surface audit separates
three shells from Align-owned boundaries: direct C `main`, generated C `main`
and named export wrappers receive no scalar facts; an export wrapper's call to
its internal core does. Per-unit `pub` linkage remains Align-owned and is not
excluded. The surface, agreement rule and matrix now state that distinction,
and existing entry/export fixtures own the positive and negative assertions.

The implementation review of candidate `b4996e02` found one P1: the proposed
`char` range was unsound because the settled integer-to-char codegen preserves
any low 32-bit pattern. The root-cause audit confirmed that literals are Unicode
scalars but `gen_cast` performs an unchecked integer cast, and accepted values
then cross every ordinary direct, indirect and per-unit boundary. This revision
reopens the scalar-fact matrix, removes the range field and raw LLVM constructor
entirely, retains only `zeroext` for `char`, and adds both an IR no-range control
and whole/per-unit execution of `0xffffffff as char`. Because this P1 changes
the implementation strategy, the revised candidate requires one fresh review.
