# Integer vector operators and explicit ties-even rounding

Consumer capability: align-llm R151 and align-media AM-R13. The two
consumers need the existing numeric families at their existing scalar/vector
widths. This does not change `round` or the scalar shift rule. Read all three
request registers before implementation; consumer adoption stays external.

## Public contract

| Surface | Exact behavior |
| --- | --- |
| Integer vector operators | `&`, `|`, `^`, `<<`, `>>` accept `vecN<T>` with `N` in 2/4/8/16 and `T` in i8/i16/i32/i64/u8/u16/u32/u64. Both vector operands have exactly the same element type and lane count. Either operand may instead be a scalar of the exact element type, using the existing arithmetic broadcast and inference rules; operand order is preserved. The result is `vecN<T>`. Unary `~` accepts the same integer vectors and returns the same type. |
| Bit and shift values | Each lane equals its scalar operation. Complement flips the lane's exact width. Shift amounts use only the low log2(width) bits, including negative signed amounts, so oversized amounts never produce LLVM poison. Right shift sign-extends signed elements and zero-fills unsigned elements. Left shift discards high bits with the existing wrapping integer behavior. |
| Floating rounding | `x.round_even()` takes zero arguments on f32/f64 and vec2/4/8/16<f32/f64>, returning precisely the receiver type. It rounds to nearest integral floating value, ties to the even integer. Preserve both signed zeros, infinities and already integral finite values. NaN remains NaN; payload/sign/quieting are unspecified. No integer conversion or overflow abort occurs. Existing `round()` remains ties-away. |
| Effects and ownership | Pure Copy operations. No allocation, borrow, owner transfer, source nulling, replacement cleanup, Drop, runtime resource or native ABI is added. Eager operand evaluation remains source ordered. No errno or floating-point exception channel is exposed. |
| Rejection | Floats, bools, chars, masks, aggregates and unresolved generic parameters do not enter integer bitwise/shift lowering. Mixed lane counts, widths or signedness reject without implicit conversion; ordinary unsuffixed integer literal inference remains supported. `round_even` rejects nonfloating receivers before checking arity, as existing math methods do. Already-invalid children retain normal diagnostics and cannot construct a certified malformed node. |
| Lowering | HIR/MIR preserve existing Unary/Binary and one new MathFn RoundEven discriminator. Vector bitwise/shifts lower to vector LLVM and/or/xor/shl/ashr/lshr with a lane-wise width mask before shifts; complement uses vector xor with all ones. `round_even` lowers to llvm.roundeven at the scalar or vector receiver type. No claim that every target has a single machine instruction. |
| Identity | No source syntax or type change, interface wire field, runtime key, package artifact or user configuration. Interface source AST retains the existing operator/method spellings. Existing compiler-build identity invalidates derived HIR/MIR/cache bytes. Verify imported whole/per-unit and cold/hit source transport. |
| Owners | Sema owns admission and inference; checked HIR and MIR producer validation own exact concrete operator relations; LLVM lowering owns width masking and intrinsic selection. Driver tests own source/native/cache and raw LLVM proofs. |
| Acceptance | Parameterized integer type/lane/operation and broadcast owner with independent scalar values; shift 0/width-1/width/width+1/all-ones and signed negative values. Float scalar/vector round cases include ±0, ±0.5, ±1.5, ±2.5, neighboring representable values, large integers, infinities and NaN. Retain paired `round` ties-away controls. Whole/per-unit execution and generic imported helpers, raw IR masks/intrinsics, malformed HIR/MIR and invalid source domain checks. |
| Performance | No speed, resource reduction or universal machine-SIMD promise. No benchmark gate; raw vector LLVM shape is the structural owner. |

## Closure matrix

| Axis | Implementation closure and test owner |
| --- | --- |
| Formation and inference | Extend finish_binary plus the stack-safe single-check numeric operand context, unary checking, numeric method dispatch and exhaustive MathFn helpers. Reuse the arithmetic broadcast relation rather than a second integer coercion rule. Source owner crosses vector/vector, vector/scalar, scalar/vector, exact typed operands, unsuffixed literals, expected vector result, nested expressions, mismatches and already-invalid expressions. |
| Checked HIR | Extend unary_result/binary_result and shared MathFn result_type. Reject forged float/mask/non-numeric vectors, invalid widths/lanes, wrong result, operand mismatches and wrong arity before MIR. Use existing parameterized validation owners rather than one fixture per variant. |
| MIR whole and partition | Extend producer Rvalue::Un/Bin relation checks and shared MathOp grammar. Preserve valid broadcast operands as scalars until backend splatting. The same parameterized owner proves whole/partition agree and rejects malformed relations. |
| LLVM | Extend integer-only gen_vec_bin allow-list and vector unary handling; mask shift operands with the lane-width constant splat. Keep float operation rejection explicit and no add fallback for a new unsupported op. Add RoundEven to exhaustive intrinsic dispatch. Raw IR owner checks all bitwise/shift spellings and scalar/vector roundeven overloads. |
| Values and control | Copy results flow through existing local construction, assignment, return, if/match/else, loops and function calls. A focused compiled owner includes branch/loop uses and repeated assignments. No new owning representation exists, so move/null/Drop/cleanup obligations are unchanged and require no new allocation fixture. |
| Source and imported parity | Native matrix runs whole/per-unit; an imported concrete/generic helper and persistent cold/hit cache fixture exercise AST rechecking and method/operator preservation. No new interface discriminator is assumed without auditing the actual codec. |
| Documentation | Update draft.md, condensed language spec, design notes and the relevant Settled entries; numeric/SIMD English guides and Japanese mirrors, core-design/hash.md intrinsic surface, HIR ledger, backend/roadmap references that make exact operator/method promises. Record implementation completion once. |

This is one numeric capability for one shared consumer kernel, with two complete
existing operator families and no new ownership strategy. It must not absorb E2
math-provider work, JSON field-addressing or mutable parallel output. Those have
distinct producer/ABI/ownership boundaries. The complete boundary and its size
are justified in the source-to-ledger extraction below.

Author-side extraction and one fresh independent strategy review precede code.
After the candidate compiles and owner tests pass, run align-self-review, one
fresh full-diff review and final-SHA preflight. Consumer source, fixtures, pins
and Git state stay unchanged; update only their request registers after merge.

## Exact owners and strategy-review closure

The fresh inspection-only numeric_strategy_review found one P2 gap: abstract
owner labels did not meet the exact-test binding requirement. The following
bindings close that finding before implementation; no public strategy changed.

- `vec_simd::integer_vector_bitwise_shift_type_lane_matrix` replaces the old
  `bitwise_and_shift_on_vectors_is_rejected` owner with all integer widths,
  signedness and lane counts; `vector_bitwise_invalid_domain_is_rejected`
  preserves float, mask, logical, width/lane/signedness and malformed-child
  negatives. Unary vector admission is only `~`, never vector negation.
- `vec_simd::integer_vector_broadcast_evaluates_operands_once_in_order` uses
  distinguishable side effects, both broadcast positions, nested expressions
  and an expected vector result. It detects accidental execution duplication
  in operand evaluation; the same actual-source owner runs whole/per-unit.
- `scalar_math::all_math_discriminators_keep_whole_and_per_unit_results` gains
  RoundEven. `round_even_scalar_vector_ieee_values` asserts exact bits for
  both input zero signs, -0.5 -> -0.0 and neighboring negative zero-rounding
  cases; NaNs use classification without a payload promise. It retains
  `round()` ties-away controls and baseline native whole/per-unit execution.
- `vec_simd::integer_vector_ir_masks_shift_counts` uses nonconstant function
  parameters, checks that the lane-width-mask result feeds shl/ashr/lshr,
  and covers both broadcast positions plus negative/all-ones count execution.
  `scalar_math::round_even_llvm_overloads` pins scalar and vector intrinsic
  types; native execution above separately proves selection/linking.
- `validate_hir_tests` extends the existing `math_round_case` discriminator
  fixture with `math_round_even_case`; `numeric_completion_hir_relations`
  explicitly owns forged unary/binary/MathOp operand/result/width/lane/arity.
- `producer::tests::math_producers_share_complete_arity_input_and_result_rules`
  adds RoundEven. LLVM crate owners
  `producer_vector_numeric_relations_reject_forged_operands_and_operators` and
  `producer_vector_numeric_relations_reject_invalid_numeric_type_shapes`
  admit the new integer relations while preserving every float/mask/logical/
  malformed-result/type control.
- Driver target `numeric_completion_cache`, test
  `numeric_methods_import_generic_whole_per_unit_and_cache`, imports concrete
  numeric helpers through an ordinary generic identity wrapper and checks
  cold/hit execution and artifact reuse. No generic integer-operator bound
  is added. Its command is `scripts/cargo.sh test -p align_driver --test
  numeric_completion_cache`; numeric source owners run with `--test vec_simd
  --test scalar_math` and two test threads.

The strategy reviewer confirmed that interface bodies carry source text and
compiler cache identity hashes executable bytes: no new wire-format field or
version bump is required for this operation discriminator. The proposal has no
ownership, ABI or LLVM safety redesign finding. The names above bind each obligation to its executable owner; final verification
evidence belongs in the SHA-bound preflight record.

## Author source-to-ledger extraction

The existing arithmetic control `fn f(x:i32,v:vec2<i32>)->vec2<i32> = x + v`
rejects on the release base because the exact vector result hint reaches the
scalar left operand. Author Gate 5 inspection also rejected diagnostic replay:
truncation cannot restore inference, locals or lifted-function state. A focused
continuation of the independent strategy review requires the following single-
check strategy before implementation:

- Exact expression expectations keep their existing behavior. A separate
  checker-only numeric operand context carries a scalar candidate and an optional
  vector candidate; it is not a language type or an interface field.
- Literals use the scalar candidate; contextual array literals use the vector
  candidate. Intrinsically typed values retain their actual type. Final binary
  relations and the enclosing result expectation remain exact.
- Only value-producing block, branch and loop-break edges inherit this context.
  Conditions, local initializers and lambda bodies do not. Branches and breaks
  still reconcile to one exact result type; a scalar/vector branch mix rejects.
- A generic bare return slot obtains its shape from its determining arguments
  under this context. Repeated bound slots retain exact argument checking. A
  return-only slot uses the vector candidate when available, otherwise scalar.
  Ordinary exact-context generic calls retain expected-return-first ordering.
- Heap-owned binary continuation frames visit every operand once. Delete all
  binary diagnostic checkpoints, truncation and replay, including the previous
  RHS retry. No general speculative checker transaction is introduced.

The evaluation owner crosses direct/generic scalar-left, vector literals,
contextual wrappers, source order, expected versus inferred results, existing
arithmetic and invalid children. `numeric_completion_cache::numeric_operand_context_checks_each_source_once`
pins branch and loop completion, repeated generic parameters, and exactly one
lifted function per source lambda. Existing scanner/generic owners preserve ordinary exact
context ordering. Copy representation, runtime ABI and interface format remain
unchanged.

This complete numeric boundary now includes the inference seam required by both
broadcast directions. Tests and normative mirrors exceed 1,000 handwritten
lines. Keeping them with the operator producer/validator/backend chain avoids
shipping a dormant operator or repeating the same inference proof and gates.

The widened contextual scalar-left arithmetic owner also exposes the existing MIR
division dispatch selecting its guard from the left operand instead of the
result. Select scalar versus vector guards by the exact result shape.
`vec_simd::scalar_left_vector_division_preserves_lane_guards` closes signed and
unsigned extrema, all element/lane widths, / and %, INT_MIN/-1 wrapping, and
whole/per-unit zero-lane aborts. The existing vector guard itself is unchanged.

## Completion-context review closure

F1: soft numeric context must also supply the structural result type of a proven
non-fallthrough control expression. Plain/float/arena/named-arena/unsafe/task-group
blocks, if (including a diverging condition), both match forms (including a
diverging scrutinee), and loops without reachable breaks use the candidate only
when forming that result. Dead children remain independently checked; no source
is replayed. Direct process termination remains Unit in eager operands, and
fallthrough Unit or mixed branch/break result types still reject. No Never type,
IR shape or ownership policy is introduced.

`numeric_completion_cache::numeric_diverging_completion_preserves_context` owns
both operand positions, scalar/vector contexts, generic identity, absent versus
available context and invalid children. Terminating-return cases execute whole
and per-unit; infinite loops are checked/emitted but not run. This extends the
existing values/control matrix cell and closes the review finding in one fix.
