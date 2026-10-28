# Guarded codec Option joins

Status: implemented, 2026-10-28.

Plan252 established the representation-preserving entry-slot join. Six codec
accessors still use a separate aggregate-phi helper: batch name/kind and the
four column element types. An independent six-function reduction retains six
aggregate phis and emits 1,060 arm64 text bytes with explicit exports on the
Plan252 compiler. An IR-only slot prototype removes those phis; production
qualification follows below. This is a compiler-quality improvement to the
existing core.codec contract, not a consumer-request acceptance claim.

## Strategy and implementation closure matrix

Unify both conditional Option helpers in one closure-taking helper. Its
condition and payload type are already validated. Allocate one entry-block
slot; invoke the payload builder only in the Some block, construct the same
completed Some/None values, store on both predecessors and load at the join.
The three existing eager producer families pass their already-computed value
through a closure. Codec callers keep their existing delayed loads. Plan71's
layout and completion rules, Plan60's validated observations, and Plans201/204's
nominal types and carrier rules remain unchanged. This reuses Plan252's reviewed
strategy and needs the author matrix pass plus one implementation review.

| Boundary | Implementation and exact acceptance owner |
| --- | --- |
| Type formation, validation, representation | No HIR/MIR variant, source type, ABI, layout, tag, payload map or freeze change. Existing backend `producer_equations_preserve_guarded_absence_and_scalar_view_siblings`, malformed producer owners and tagged layout owners remain in the full backend unit target. |
| Construction and guarded access | New `codec_options_keep_payloads_guarded_and_promote_scalar_consumers` crosses i64/f64/bool/str/name/kind. Raw IR requires each payload address and load in its Some block; optimized IR refuses aggregate joins for scalar consumers. The unchanged baseline must fail the promotion assertion. No payload address or load moves before range success. |
| Move-in/out, source nulling, Drop, replacement | All codec results are existing Copy values; no owner, pointer provenance, cleanup or allocation changes. `core_codec::codec_view_field_lifetime_and_placement_refusals`, `codec_views_keep_the_input_generation_live_and_stay_out_of_invalid_carriers`, and `validated_codec_observation_safe_execution` retain source invalidation and detached-copy boundaries. |
| Values and malformed indices | `core_codec::codec_encoder_and_views_round_trip_all_four_kinds`, `codec_all_four_golden_decodes_at_every_base_alignment`, and `codec_llvm_uses_keyed_runtime_rows_and_alignment_one_inline_loads` retain exact values, negative/upper-bound failure, byte order, string views and alignment-1 loads. Wire validation and invalid-envelope precedence are untouched. |
| Control paths, imports, generics, cache | `core_codec::codec_batch_result_survives_imported_generic_cache_replay` covers returned views, if/loop/map_err, Some/None and cold/hit replay in the existing bounded fixture. `codec_types_cross_whole_program_and_per_unit_interfaces` and the complete `pkg_frame` target retain downstream whole/per-unit use. No interface/cache format changes. |
| Existing eager callers and loop reuse | Backend `conditional_option_joins_promote_scalar_results` retains all 27 reductions. `owned_tagged_payloads::conditional_options_reuse_the_join_slot_across_loop_iterations` runs checked arithmetic, search and byte views across Some/None loop iterations in whole/per-unit builds under its existing supervisor. |
| Allocation and measurement | No runtime or heap allocation. The private slot is allocated at function entry and receives a completed value on every incoming edge. Compare the same independently authored six-function fixture, explicit export roots, release profile and baseline target using actual baseline/candidate objects. Report local text size only; no throughput, universal stack-elimination or whole-product speed claim. |

The change is confined to one existing lowering helper and its callers. Manual
codec column-projection joins and Result producers are outside this boundary;
they have different control-flow or validation responsibilities. No public
contract or safety strategy changes, so specifications and language mirrors do
not need edits.

## Qualification

The six-case owner fails on the Plan252 baseline because the integer consumer
retains an aggregate phi, and passes with the unified helper. Moving the payload
builder above the branch in a temporary compiler mutant fails the independent
address-order assertion; that mutant emits IR only and is never used for native
execution. The production source is restored before qualification.

On Apple M1 / macOS arm64 / LLVM22.1.8, actual release-profile, baseline-target
objects from the same `bench/codec_joins/kernel.align` and six explicit exports
contain 1,060 versus 460 bytes of `__text` (600 fewer, 56.6%). The exports include
wrapper code; the earlier IR-only prototype had no explicit export wrappers.
The optimized six consumers have no aggregate Option phi. The bool consumer
retains an existing one-byte scratch associated with sub-byte payload storage;
this capability does not change that distinct representation path.

The final owner bundle is the complete backend unit target, complete
`core_codec` and `pkg_frame` driver targets, and the bounded conditional-Option
loop owner. The ordinary final-SHA preflight runs that bundle, the bounded gate
and Clippy before publication. No native throughput claim or consumer adoption
is attached to this local code-size result.
