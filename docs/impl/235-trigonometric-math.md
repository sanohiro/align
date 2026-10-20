# Scalar and explicit-vector trigonometry

## Selection and feasibility

Media AM-R14 needs sin/cos for independent rotary validation; AM-R20 needs
quadrant-aware atan2 before integer-degree rasterization. All three consumer
registers were reconciled. LLM's E1 math and voice's named rotary fields already
ship; no additional trig request or public native-helper use was found there.
Media's current `guide_draw` declares compatible C `atan2(double,double)`.
Consumer source, compiler pins, raster pipelines and adoption stay external.

LLVM22.1.8 has width-overloaded scalar/vector sin, cos and atan2 intrinsics.
A direct LLVM probe emits every f32/f64 scalar and vec2/4/8/16 form on macOS ARM64,
Linux ARM64 and Linux x86_64. The probe also identifies combined sin/cos lowering:
Linux uses sincos[f] output-pointer helpers; Darwin uses __sincos[f]_stret with a
two-float C result. These identities belong in native collision qualification.
The three operations are a complete useful capability; other E4 functions are
independent reservations. No K1, new provider or complete E4 prerequisite applies.

Primary references: LLVM22 LangRef's [sin](https://releases.llvm.org/22.1.0/docs/LangRef.html#llvm-sin-intrinsic),
[cos](https://releases.llvm.org/22.1.0/docs/LangRef.html#llvm-cos-intrinsic) and
[atan2](https://releases.llvm.org/22.1.0/docs/LangRef.html#llvm-atan2-intrinsic)
rows; the installed SDK math.h declares Darwin's two-result helpers. Target
libm supplies implementation and argument reduction; Align adds no coefficients,
polynomial kernel, manually reduced argument or y/x rewrite.

## Public-contract ledger

| Surface | Exact contract |
| --- | --- |
| Methods | x.sin(), x.cos(), y.atan2(x) on f32/f64 and vec2/4/8/16 of either float. Unary methods take no arguments; atan2 takes exactly one argument with precisely the receiver type. Results have that same type. Radians throughout; atan2 order is y then x, with range [-pi, pi]. No implicit vector broadcast, width conversion, aliases, free-function overload or provider option. Ordinary unsuffixed literals retain existing receiver-guided inference. |
| Numeric meaning | Target implementation of sine, cosine and quadrant-aware arctangent at the receiver format. Atan2 is not division followed by atan: it must preserve overflow/underflow, signed axes and quadrants. Sin/cos argument reduction belongs to the target libm, including very large finite inputs. Plan75's existing precision posture applies: no global ULP ceiling, correct-rounding, scalar/vector bit identity or cross-target bit identity promise. Independently rounded sample goldens qualify concrete targets, not all inputs. |
| Sin/cos special values | Total, never abort. sin(+0)=+0, sin(-0)=-0; cos(+0)=cos(-0)=+1. Either infinity or NaN gives NaN; finite sin/cos remain in [-1,1]. NaN payload/sign/quieting unspecified. Existing target subnormal behavior remains. |
| Atan2 special values | Either NaN gives NaN. For y=+0/-0 and x with positive sign including +0/+inf, result is +0/-0; with negative x including -0/-inf, result is +pi/-pi. For nonzero finite y and x=+0/-0, result has y's sign and magnitude pi/2. Finite nonzero y with x=+inf gives signed zero; with x=-inf gives signed pi. Infinite y with finite x gives signed pi/2. Both infinite give signed pi/4 for positive x and signed 3pi/4 for negative x. Sign follows y; pi multiples are rounded by the selected receiver-width target implementation. |
| Ownership/effects | Pure Align MathOp; Copy operands/results, no allocation, borrow, lifetime, backing, mutation, Drop or new runtime surface. Existing IEEE environment assumptions apply. No exposed errno or FP exception state. Operands are evaluated once, receiver then argument. Vectors are lane-wise, with no cross-lane state or hidden parallelism. |
| Lowering | One MathOp becomes the exact same-width llvm.sin, llvm.cos or llvm.atan2 scalar/vector intrinsic, without afn, nnan, ninf, nsz, reassociation or new fast-math flags in any profile. No compiler lane loop or scalar widening. Target selection may scalarize vectors or combine corresponding sin/cos into a native helper; neither changes source availability. Existing platform math linking supplies these functions. No Align runtime wrapper, embedded library or ambient provider discovery. |
| Native identities | One codegen authority extends the fixed math identity table with sinf/sin/cosf/cos/atan2f/atan2, exact C float/double unary or binary signatures. Validate every same-symbol extern before LLVM function publication, even unused/no local MathOp. Compatible declarations remain usable under existing unsafe extern rules; wrong width/arity/result rejects. Explicit exports cannot replace any fixed math identity, regardless of ABI or whether this unit reaches math. Ordinary encoded Align names remain valid. |
| Combined native identities | Also reserve sincosf/sincos and __sincosf_stret/__sincos_stret against replacement exports. On supported Linux, sincos[f] has void(float-or-double, ptr, ptr); both pointers use the existing raw-pointer ABI. On supported Darwin ARM64, __sincos[f]_stret takes one exact float/double and returns a two-member homogeneous float aggregate in the platform C ABI. Compatible source C-record return shapes may flatten fixed numeric fields to exactly two same-width float members, with the same size and HFA return class; nominal record identity is irrelevant to the native ABI. Wrong widths, counts, padding, physical return class or argument types reject. A source extern for a platform-specific helper on another supported platform rejects before publication; no cross-platform availability is promised. Source calls remain unsafe. The compiler never synthesizes these helper declarations; LLVM owns their selection. |
| Explain-opt | Extend the existing MathVisibilityRecord operation domain with Sin/Cos/Atan2. Source order/ordinal, source location, provider and reason presence, escaping and three-state classification remain Plan75/09. Each source vector operation has one tag on its intrinsic; argument values and reconstruction instructions are not operations. RetainedVectorIr with provider=None means LLVM may scalarize at instruction selection; it is not machine-SIMD evidence. Scalarized and EliminatedOrMerged retain their existing rules. No Plan234 no-intrinsic reason is attached to these operations. Normal builds remain quiet. |
| Validation and errors | Receiver kind before arity, then exact argument type; invalid children return the existing error sentinel. MathFn.result_type owns unary/binary exact float widths and lanes for checked-HIR and MIR replay. Malformed type/arity/result rejects before LLVM publication. Native collision rules precede declarations. |
| Identity and prerequisites | Existing compiler fingerprint and target/toolchain identity own changed code. No new HIR/MIR expression variant, source syntax, interface wire field, runtime ABI export, provider config or cache component. Imported generic bodies are source reconstructed and rechecked. Existing E1 visibility and target math linkage suffice. Unsupported targets retain the compiler's current boundary. |
| Acceptance/mirrors | Both source frontends, checked-HIR/MIR grammar, exact raw vector intrinsic shapes and native identity twins, independent special/finite goldens and 3-target execution, operand order, imports/generic/cache, control/Pure parallel paths and all-profile visibility. Update draft/spec/design-notes/Settled, Plans75/68/09/19 and exact inventories, EN/ja SIMD guide, queue/HANDOFF; affected registers after merge. No benchmark without a performance promise. |

## Implementation closure matrix

| Axis | Implementation and owner |
| --- | --- |
| Formation and replay | Extend MathFn and source method lookup/check_scalar_math; shared exact-type grammar admits two atan2 operands. Extend `math_producers_share_complete_arity_input_and_result_rules`, `hir_body_validator_rejects_malformed_elementary_math_rows`, and `all_math_discriminators_keep_whole_and_per_unit_results`; new `trigonometry` source owner covers widths/lanes, receiver-before-arity, mixed widths/lanes, masks/integers and no broadcast. |
| Lowering/ABI | Generalize existing elementary intrinsic call to an operand slice; preserve existing E1 metadata and one tag per vector operation. `trigonometry_intrinsics_and_native_identities` checks all operations/types and target families, no new fast flags, no hidden allocation, standard and combined native compatibility/replacement negatives before declaration, matching extern and encoded-name positives. C-record/fixed-array HFA positives and wrong-size/width twins discriminate physical compatibility from mere record tags. |
| Values/native execution | `trigonometry_independent_ieee_samples_whole_and_per_unit` uses immutable input/expected IEEE bits independently generated at two high precisions with direct nearest-even IEEE rounding. Include signed axes, all quadrants, all infinity combinations, NaNs, tiny/subnormal and large finite inputs, near-pi boundaries, near integer-degree transitions and mixed exceptional lanes. Reuse the fixture for native object qualification on all three platforms; the C observer checks expected bits/tolerances and never calculates trig. A same-input sin+cos consumer exercises combined helper lowering. |
| Evaluation/control/effects | `trigonometry_copy_control_paths_and_pure_parallel_closures` and `trigonometry_evaluates_receiver_then_argument_once` cover observable receiver-before-argument evaluation, each once, if/match/else/?/map_err/loops/early exits, field/index results and Pure parallel use. Existing Copy semantics retain no origin or cleanup state; move/nulling/Drop are not changed. |
| Imports/generics/cache | `trigonometry_imports_generics_and_cache` runs source reconstruction, generic Copy transport, cold/hit/changed-operation/reverted-operation cases through the package cache; the numeric owner separately executes whole/per-unit source reconstruction. No new interface representation. |
| Visibility | `trigonometry_visibility_across_profiles` crosses dev/release/fast/small/tiny with mixed E1/Plan234/trig source order, unary/binary/scalar/vector/dead inputs and exact ordinals/provider/reason. Existing E1 and Plan234 controls must retain their state and reason. `trigonometry_reports_intrinsic_disposition_in_every_profile` uses the existing supervised CLI fixture. |
| Consumer evidence | Check/run owned copies of AM-R14/20 minimal sources. The 1,099 guide angle values can be compared as a bounded provider probe on the qualified Linux host; the 30 raster fixtures and full 81-frame guide remain consumer-owned adoption. Preserve any host-specific mismatch as evidence rather than claiming a universal bit contract. |
| Resources | Every new compile/link/run owner uses the existing exclusive ArtifactStage and owned_fixture process-group deadline. Generated object export has exclusive directory acquisition and failed-export cleanup. Reuse shared caches; remove raw objects after retaining compact qualification receipts. |

This boundary combines all three operations with their existing visibility and
native consumers. Splitting an enum producer from lowering/validation would not
produce usable math. A change exceeding 1,000 handwritten lines is justified by
one shared width/lane/IEEE/native-ABI proof rather than repeating that proof and
full gates in three operation PRs. The author ledger-to-prose and matrix pass,
then fresh independent strategy review, precede production changes.

## Independent boundary review and native feasibility

The fresh inspection-only strategy review found no actionable issues. It
checked the complete ledger against existing native/C-record, MathFn and
visibility owners before production implementation. The author extracted
normative obligations and mapped them to the closure matrix first.

The LLVM feasibility probe executes 251 scalar and combined-helper observations
on all three platforms against 204 independent sample goldens; the largest
observed finite sample distance is 1 ULP. A separate read-only provider probe
matches all 1,099 consumer degree bit patterns on Linux ARM64 and x86_64. macOS
has 14 last-bit differences from that pinned Linux packet. This is concrete
evidence for retaining the no-cross-target-bit-identity contract, not a compiler
regression or consumer adoption.

## Implementation qualification

The immutable corpus contains 293 sample rows. Decimal series at 600 and 900
significant digits independently agree after direct nearest-even IEEE rounding.
The C observer supplies exact input bits and checks fixed expectations without
calling native trig; signed zero and cos(+-0)=1 are exact, finite sin/cos remain
within [-1,1], and other finite samples use a four-ULP qualification tolerance.
This tolerance is a test criterion for these samples, not a public error bound.

The Align numeric owner executes 1,541 observations through scalar and every
explicit vector width in both source frontends. Exported Align objects pass the
same observer on macOS ARM64, Linux ARM64 and Linux x86_64. The original AM-R14
and AM-R20 minimal packets pass check, per-unit check and native execution.
An Align atan2-and-degrees source probe reproduces the 1,099-row consumer packet
exactly on both Linux targets; macOS retains the same 14 last-bit differences
observed in the native feasibility probe. The 30 raster fixtures, 81-frame guide,
rotary fixture migration and consumer pin remain external acceptance.

The author matrix-to-diff pass closes the listed formation, native-identity,
control, cache, visibility and resource owners. Existing E1/Plan234 owners stay
in the verification bundle because they share the intrinsic-call helper and
fixed native identity authority. No benchmark is required: this capability
adds no throughput, SIMD or resource-improvement promise.
