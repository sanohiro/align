# Codec view field placement

At merged `41f3edaecfff55d531680925e0be2f9a150fb4b7`, source checking accepts
ordinary record fields containing `codec.batch` and all four typed columns,
directly or through Option/Result. Whole/per-unit MIR lowering rejects the same
program at `type_placement_metadata_is_valid`. The field leaf whitelist omits
those five Copy descriptors. Plan201 recorded the Result-field symptom as a
separate formation defect; direct fields and every column share its cause.

Implement the existing [codec carrier contract](core-design/codec.md#type-region-and-placement-closure).
Admit the five existing leaves in the checked-HIR field predicate. Preserve the
recursive field/payload, nominal-id, inline-cycle, stream, C-layout and collection
guards. Do not admit a new carrier or relax the producer's Result tag proof.
No source syntax, ownership strategy, type/interface format, ABI or native code
changes. All views still borrow the validated input and retain its generation.

## Implementation closure matrix

| Axis | Implementation and exact owner |
| --- | --- |
| Formation and canonical carriers | `PlacementValidator::field_type_ok` admits exactly the batch and four column leaves. `align_mir::validate_hir_tests::codec_view_field_placement_matrix` crosses all five with direct, Option, both Result arms and nested record/tagged placement, then lowers through whole/per-unit entrypoints. Existing scalar and field recursion remain authoritative. |
| Invalid placement and malformed HIR | That same owner injects graph-valid array, slice and C-layout field placements and requires the complete lowering boundary to reject. `core_codec::codec_view_field_lifetime_and_placement_refusals` retains source collection/extern refusals. Existing `codec_hir_requires_exact_envelopes_discriminators_and_exclusive_encoder_places` and `producer::codec_tests` retain exact operation/result/discriminator and tag-guard checks. |
| Construction, copies, return and generic/imported boundaries | Extend `core_codec::codec_batch_result_survives_imported_generic_cache_replay` with all five views in records, nested tags, imported generic construction, shared projected matching and exact accessor values. Run whole-program execution and per-unit cold/hit interface replay under its existing bounded owned fixture. |
| Lifetime, replacement, Drop and validation observation | Copy descriptors own no storage; their record fields retain input authority. `core_codec::codec_view_field_lifetime_and_placement_refusals` crosses all five direct/Option/Result fields with backing mutation, owner replacement and local-owner escape. Reuse complete codec observation and `pkg_frame` owners for backing transfer, detached copies, reopening, cleanup and downstream use. |
| Branches, loops and error paths | Existing imported generic replay retains if/loop/map_err, successful/failed open and both Result payload arms; record-held success/error matching extends it. Existing `validated_codec_observation_control_matrix`, `validated_codec_observation_completion_matrix` and whole/unit parity retain joins, eager exits and changed-byte refusal. No control-flow or diagnostic-order rule changes. |
| Allocation, ABI and serialization | Native code, layouts, canonical tags and interface records are unchanged. Existing complete `core_codec` and `pkg_frame` targets retain native zero-copy/accessor and owner-cleanup behavior. No new performance/resource claim or benchmark. |

This closes an already-reviewed field contract using the existing validated
observation and Result-producer strategy. The author matrix pass precedes the
five-leaf edit; one fresh full-diff preflight review checks the implementation
and parameterized owners. The capability is below 1,000 handwritten lines.
No consumer adoption or consumer repository implementation is included.
