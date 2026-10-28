# Checked codec column projection joins

Status: implemented, 2026-10-28.

Plan253 makes single-condition codec Options promotable. Column projection has
a distinct two-stage admission path: inspect the kind only after an admitted
ordinal, then form the view only after exact kind equality. Its four projections
still retain aggregate Option phis. The independently authored four-function
`len` reduction emits 1,052 arm64 text bytes on the Plan253 compiler. An IR-only
prototype of the strategy below emits 464 bytes, with no aggregate phi or
alloca. Actual compiler objects and owner results must qualify implementation.

## Strategy and implementation closure matrix

Keep ordinal checking and the guarded kind-byte load. Join their decision as
one scalar boolean: false on the out-of-range edge, the exact-kind comparison
on the inspected edge. Pass that boolean to the existing `conditional_option_with`
helper. Its lazy payload closure reads data/aux descriptors and forms the same
column view only on success. This removes the separate Option constructor/join
while preserving both admission stages, evaluation order and the existing
Plan71 completion boundary. There is no new source, HIR/MIR, wire, interface,
ownership or native ABI contract. This reuses the reviewed Plan252/253 strategy;
the author matrix pass and one preflight implementation review are required.

| Boundary | Implementation and exact owner |
| --- | --- |
| Formation and validation | Preserve the exact four kind/result-payload pairs and malformed-result diagnostics in `gen_codec_batch_column`. Backend `producer_equations_preserve_guarded_absence_and_scalar_view_siblings` and the complete backend target retain producer/type validation. No type or IR variant is added. |
| Two-stage admission and construction | New backend `codec_column_projections_keep_both_guards_and_promote_consumers` crosses all four kinds. Raw IR requires the kind address/load in the ordinal-admitted block and data/aux addresses/loads in the exact-kind Some block; optimized consumers retain no aggregate phi. The old compiler must fail promotion, and deliberate eager-read mutants must fail guarded-address assertions. |
| Values and invalid inputs | New bounded native `core_codec::codec_column_projection_admission_matrix` crosses all four accessors with every source column, negative/upper-bound/extreme indices and an empty batch. Independent expected lengths distinguish zero/mismatch/absence. Existing golden all-base-alignment and four-kind round-trip owners retain exact bytes and source values. |
| Source lifetime, moves, nulling, Drop and replacement | Results remain Copy descriptors into the same validated input generation, with no owner or allocation. Existing `codec_view_field_lifetime_and_placement_refusals`, `codec_views_keep_the_input_generation_live_and_stay_out_of_invalid_carriers`, and codec observation owners retain mutation, replacement and escape refusals. Encoder and buffer cleanup are untouched. |
| Control paths, imports, generics and cache | New bounded native owner executes the matrix through imported helpers in whole/per-unit builds. Existing `codec_batch_result_survives_imported_generic_cache_replay` covers Option/Result records, if/loop/map_err, returns and cold/hit reuse. Complete `pkg_frame` retains downstream joins. Only pre-check operands cross into the payload closure; an inspected-only value cannot bypass the decision join without its own SSA merge. The scalar decision phi has exactly the range-failed and inspected predecessors; both Option arms continue to initialize the helper's entry slot. |
| Existing shared helper callers | Existing six-case codec accessor owner, 27-case conditional Option owner and bounded whole/per-unit alternating Option loop remain unchanged and run with the backend/driver bundle. No allocation or placement is added outside the established helper. |
| Measurement | Compare identical independently authored four-function fixtures, explicit exports, release profile and baseline CPU using actual baseline/candidate objects. Scope the result to local text size; no throughput, universal size, consumer adoption or bounds-removal claim. |

The stable six single-condition consumers shipped separately under Plan253;
this capability closes the two-stage ordinal/kind failure domain. Other manual
tagged joins and sub-byte bool storage remain outside this change. Specifications,
language mirrors, runtime ABI and persisted formats do not change.

## Qualification

The four-kind backend owner fails on the baseline's aggregate join and passes
on the implementation. Two valid-IR compiler mutants, moving the kind read
before ordinal admission and moving the data descriptor read before the final
admission branch, each fail the corresponding guarded-address assertion. The
mutants emit IR only; no unsafe native program is executed. Production source
is restored before qualification.

The new bounded native matrix passes on both baseline and candidate in whole
and per-unit builds. Each executes 84 accessor/index/input cases: seven ordinals
cross four accessors and three batches (two rows, zero rows with four columns,
and no columns). These independent expected lengths preserve absence, exact
kind equality and a valid zero-length column.

Actual objects from identical explicit exports, release profile and baseline
CPU on Apple M1 / macOS arm64 / LLVM22.1.8 contain 1,052 versus 464 bytes of
`__text`, including export wrappers: 588 fewer bytes, or 55.9%. The benchmark
source is `bench/codec_columns/kernel.align`. This is local code-size evidence,
not runtime throughput or a universal result.

The final owner bundle is the complete backend unit target, complete
`core_codec` and `pkg_frame` targets, and the existing bounded conditional-Option
loop owner. Final-SHA preflight runs the bundle, bounded gate and Clippy before
publication. No consumer adoption or acceptance target is changed.
