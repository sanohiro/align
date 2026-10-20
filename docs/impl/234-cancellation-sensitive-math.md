# Cancellation-sensitive elementary math

## Selection and feasibility

AM-R12 needs f32 expm1 for UniPC; AM-R19 needs f64 log1p near zero. All three
consumer registers were reconciled. LLM R92's E1 functions already ship; voice
has no overlapping new request. Consumer source and adoption remain external.

LLVM22.1.8 has neither expm1 nor log1p intrinsics. Its target-library table
recognizes the standard scalar symbols, and vector-function mappings include
both operations for optional providers. Existing libm provides all four scalar
width/function combinations on supported native platforms. Plan75 explicitly
permits scalarized fallback and defers machine-SIMD providers. Requiring an entire
E2 tier or a new SIMD provider before this pair would add an unrelated prerequisite.
The complete operation boundary is both widths and every existing explicit float
vector width, with honest fallback visibility and native ABI validation.

No approximation, exp(x)-1 or log(1+x) rewrite is introduced. No source-level
provider choice, new ownership/error model or cross-target bit identity is added.

## Public-contract ledger

| Surface | Exact contract |
| --- | --- |
| Methods | x.expm1() and x.log1p() on f32/f64 and vec2/4/8/16 of f32/f64, no arguments, result exactly receiver type. No free-function aliases, implicit casts or mixed-lane types. Reserve the standard log1p spelling instead of Plan75's unshipped logp1; no logp1 alias. Other E2 operations remain reserved. |
| Meaning and accuracy | expm1 computes exp(x)-1 and log1p computes log(1+x) without first rounding exp(x) or 1+x to the receiver format. The target libm implements the operation at the receiver width. As in Plan75, no global ULP ceiling, correct-rounding, scalar/vector bit-equality or cross-target identity promise. Target-owned bounded finite/near-zero goldens independently qualify each supported implementation; those test tolerances are not an all-input numerical theorem. NaN sign/payload/quieting unspecified. |
| Special values | Both are total and never abort. Both preserve +0/-0 and map NaN to NaN. expm1(+inf)=+inf, expm1(-inf)=-1; positive overflow=+inf, negative large finite may round to -1. log1p(+inf)=+inf, log1p(-1)=-inf, x<-1 including -inf gives NaN. Tiny results may be subnormal or signed zero according to target underflow behavior. No errno or floating exception status surface. |
| Ownership/effects | Pure Align MathOp, Copy input/result, no Align allocation, backing, lifetime, Drop, retained authority or explicit global state. No temporary heap array for vectors. Existing fp environment assumptions remain. Native libm may affect its errno/exception state as existing target math does; LLVM attributes must not falsely claim memory(none) on direct calls. |
| Scalar lowering | f32 uses exactly float expm1f(float)/log1pf(float); f64 exactly double expm1(double)/log1p(double), standard C calling convention. No widen-to-double-and-narrow. Codegen owns direct libm declaration/call; no new Align runtime wrapper, ABI export or artifact. Existing platform link ownership already supplies libm/system math. The four standard native symbols are fixed ABI rows: validate any same-symbol extern declaration against the exact signature before LLVM publication, even when unused or in a unit without a MathOp. Whole/per-unit paths share the rule; ordinary unexported Align function names are encoded and do not collide. Explicit `--export` definitions of any of these four names are rejected regardless of matching ABI or local MathOp presence, so a wrapper cannot replace the toolchain math implementation. |
| Vector lowering | One MathOp retains the lane-wise operation in HIR/MIR. With no selected vector provider, raw LLVM emits exactly N scalar libm calls plus lane extraction/insertion, with each lane evaluated once in ascending index order. No cross-lane state or hidden parallelism. Source availability does not depend on machine SIMD. These operations do not claim Plan75's E1-only vector-intrinsic structural guarantee. E1 lowering remains unchanged. |
| Explain-opt | Extend existing MathVisibilityRecord operation domain with Expm1/Log1p and ordinal order across all reached supported elementary vector operations. Each source operation keeps one record and tag; all scalar calls for one vector share that tag. Retained scalar calls classify Scalarized, dead/merged calls follow existing EliminatedOrMerged rule; reconstruction shuffles never count as vector math. Provider=None. State/provider/source/reason field presence, row order and single-line escaping remain existing rules. For retained fallback calls set llvm_reason to the exact compiler text `LLVM 22 has no vector intrinsic; emitted lane-wise libm calls`; eliminated records retain no reason. Existing LLVM remarks may describe later transformations but cannot erase this lowering provenance. Report that compiler reason distinctly from optimizer scalarization; no machine-SIMD claim. Normal builds remain quiet. |
| Validation | Receiver kind/width/lanes before arity, as existing methods. Shared MathFn.result_type requires one exact float scalar/vector operand and identical result. Checked HIR/MIR reject forged kind/width/arity/result; malformed metadata rejects before LLVM. Validate native symbols before native declaration; no unsafe source permission is needed for the Pure method. Explicit user extern calls retain existing unsafe behavior. |
| Identity | New exhaustive MathFn variants use existing compiler-build/cache identities. No new HIR/MIR expression variant or interface field/tag. Interface bodies store source text and re-lower it; MathFn is not a binary interface discriminant. The unchanged interface source/text and manifest formats need no version bump. Generic/import and cold/hit/revert owners verify reconstruction under the compiler fingerprint. No ambient provider/config/file discovery or external text boundary. |
| Prerequisites/limits | Existing E1 visibility and native toolchain math linking only. No G1-G3/K1 or machine-provider dependency. Unsupported targets retain existing compiler target boundary. No universal SIMD, throughput, ULP or bit-identical consumer result claim. |
| Acceptance/mirrors | Shared operation/type matrix; source/checked-HIR/MIR negatives; width-exact native ABI/no-allocation IR; IEEE classes and independently rounded finite sample goldens; whole/per-unit/import/generic/cache; optimized visibility on dev/release/fast/small/tiny; 3-target native execution. Update draft/spec/design-notes/Settled, Plan75/68/09/HIR ledger where their exact operation/visibility sets change, guide/examples if they inventory math, queue/HANDOFF and affected registers after merge. |

## Implementation closure matrix

| Axis | Implementation and exact owner |
| --- | --- |
| Formation/type and error order | MathFn Expm1/Log1p result_type/source_name/float classification; sema method resolution/check_scalar_math; `cancellation_math_rejects_invalid_receivers_arity_and_aliases` plus the shared numerical fixture cover f32/f64, 2/4/8/16, wrong scalar/vector kind and arity; MIR grammar mutation owns invalid physical widths/lanes. The existing concrete-float constraint stays unchanged; a generic Copy helper transports concrete math results. |
| IR and serialization | Existing MathOp passes preserve the exhaustive operation enum; sweep every MathFn match and E1 predicate consumer. validate_hir_tests MathOp mutation owner and MIR producer closed grammar reject malformed operand/result/arity; cache round-trip proves new ops survive imports/generic materialization. No unrelated enum sweep. |
| Native ABI and FFI collisions | One codegen scalar libm signature authority shared by whole-program/per-unit native preflight and scalar/vector lowering. Codegen owner checks all four exact signatures, C convention, each lane's original width, call count/order, no allocation, and wrong same-symbol extern argument/result/arity twins before LLVM declarations. Reject ABI-compatible and incompatible explicit exports in whole/per-unit modes and in units without a MathOp; compatible extern declarations and ordinary unexported same-name functions remain positive controls. The existing callable_preflight reservation loop enforces all four identities. Existing linker/target owners retain libm closure. |
| Control/ownership | Existing Copy MathOp semantics through local/field/index/if/match/else/?/map_err/loop/early return and Pure parallel closures; `cancellation_math_copy_control_paths_and_pure_parallel_closures` checks both branches, Option/Result propagation, loops, record/index receivers, map_err and Pure par_map in both frontends. Existing MathOp validation treats discarded results identically to used ones. No move/nulling/Drop path or view provenance changes. |
| Values | Independently generated high-precision finite goldens, checked at two precisions, contain input and expected IEEE bits with exact classification or bounded sample ULP tolerance; signed zero/NaN/inf/domain boundary/tiny signed inputs/large finite/overflow and mixed exceptional lanes. One invariant-level native owner crosses widths/lanes and whole/per-unit builds. Actual media seeded/reference adoption remains external. |
| Visibility | Inventory and lowering share ordinal eligibility; per-source metadata marks scalar math calls only. explain_opt owner crosses functions, op ordering with E1, scalar/vector/dead/merged/mixed-lane inputs, all profiles and absent provider, with exact canonical records and no invented SIMD claim. Existing E1 retained-vector control must still pass. |
| Resources/local qualification | Reuse ArtifactStage and bounded child lifecycle; share fixture with cross-target object/C oracle export if needed. Run matching macOS/Linux ARM64/Linux x86 native numerical/ABI owners before push. No benchmark unless a performance promise is later introduced. |

This is one useful math-pair capability, not a dormant provider layer. Any >1000
handwritten-line boundary is justified by the shared width/lane/ABI/visibility
proof across both cancellation-sensitive operations, rather than splitting
formation from its consumers. Independent boundary review follows an author
ledger-to-prose and matrix pass; no production implementation precedes it.

## Author ledger and implementation-site pass

The narrowed sources are Plan75 sections1/2/9, MathFn/result_type, check_scalar_math,
MIR MathOp grammar/producer, LLVM scalar call lowering and visibility
inventory/resolve, driver explain.rs, and interface generic_body plus unit cache
compiler_fingerprint. The interface stores generic source text; no numeric
MathFn code appears in its schema. All MathFn consumers are in sema hir/lib,
MIR lib/validate_hir/producer/tests, codegen lib and driver explain/tests.
The two concrete operations are a complete useful surface without waiting for
all E2 functions or a target-specific machine-SIMD guarantee. The previous
consumer assessment overstated that dependency; this ledger supersedes that
assessment, while preserving E1's vector-intrinsic promise and the broader
provider project's independent acceptance.

Primary technical evidence: LLVM llvmorg-22.1.8 Intrinsics.td lacks both names;
LLVM22 VecFuncs.def maps standard expm1[f]/log1p[f] to optional vector providers.
The native system math prototypes are width-specific. Standard math entrypoints
are explicitly identified rather than guessed from ambient library discovery.
No global accuracy theorem, external-provider packaging change or unqualified
SIMD claim is needed. Native sample qualification remains required before push.

## Independent boundary review

Fresh inspection accepted the useful pair and numerical/provider boundary, with
one P2 identity-ownership gap: explicit Align exports must not replace fixed
libm symbols, even with matching ABI. The ledger and native owner matrix now
reserve all four names against exports, including units without MathOp, and
retain compatible extern/unexported-name controls. This closes the same native
identity strategy rather than adding a provider or runtime layer. The compiler
lowering reason remains protected from later LLVM remark attachment by the
visibility owner. No production implementation preceded the review.


## Author closure and qualification

- `MathFn.result_type`, `check_scalar_math`, checked-HIR and MIR's existing
  `MathOp` grammar own both new discriminators. `hir_body_validator_rejects_malformed_elementary_math_rows`
  and `math_producers_share_complete_arity_input_and_result_rules` extend the
  existing exhaustive owners rather than create parallel validators.
- `math_libm.rs` is the single four-symbol width/signature authority. Native
  extern validation and callable preflight reserve it before publishing LLVM
  declarations; `call_scalar_libm_math` emits scalar or ascending lane calls.
  `scalar_libm_math_abi_and_vector_fallback` checks all three target triples,
  exact physical signatures, 124 width-exact calls and malformed externs before
  publication. `cancellation_math_reserves_native_exports_but_accepts_matching_externs`
  checks both frontends, matching/mismatching exports, compatible externs and
  encoded ordinary definitions with no local MathOp.
- `math_libm_cases.rs` is one numerical oracle shared by native driver execution
  and the three-target object exporter. Its 106 sample inputs expand to 582
  width/lane observations; expected bits come from independent Decimal exp/ln
  at 600 and 900 digits with direct integer nearest-even IEEE conversion.
  `math_libm_goldens.py` reproduces that qualification without native libm.
  Finite samples allow four ULP, signed zero/infinities are exact, NaN is a
  class. This is a bounded sample qualification, not a universal accuracy bound.
- `cancellation_math_independent_ieee_samples_whole_and_per_unit`, the control
  owner above and `cancellation_math_generic_cache_replays_operation_changes`
  cover imports, generic Copy transport and cold/hit/change/revert reconstruction.
  `all_math_discriminators_keep_whole_and_per_unit_results` retains sibling math.
- `math_visibility_node` shares ordinal allocation between intrinsic and native
  call lowering. `scalar_libm_visibility_keeps_ordinals_and_compiler_provenance`
  checks exact operation/type/source/order/provider rows, dead/retained outcomes,
  no false vector claim, reason overwrite resistance and unchanged ordinary IR
  across all five profiles. The CLI owner checks both output detail levels and
  exact source-located fallback wording; existing E1 visibility owners remain.
- This roughly 1,400-line capability is mostly the shared independent numerical
  oracle, negative ABI/native owners and normative mirrors. Splitting the pair
  or the width/lane domains would duplicate one native identity/visibility proof
  and leave partial semantics for the same useful consumer capability.
