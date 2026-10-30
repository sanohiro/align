# Plan 258: nightly release qualification

The 2026-10-29 nightly (run 40961291102) exposed stale owner expectations,
false-positive lifetime/publication diagnostics and runner disk exhaustion.
Restore the existing contracts before the next versioned release. Plans 240
and 242 remain authoritative; this work adds no writable result, field-relational
interface summary, runtime ABI or language surface.

## Implementation closure matrix

| Boundary | Required closure | Implementation and owner |
| --- | --- | --- |
| Returned storage candidates | Preserve every selected source, including indexed borrowed elements and headerless resource references. Missing/incompatible identity must remain unknown; exact caller lifetime does not imply writable identity. Never erase unmatched fallback dependencies. | Escape fallback formation/resolution; `struct_handle_fields::borrowed_buffer_views_follow_optional_array_and_generic_sources`, `indexed_field_views::returned_candidate_closure`, `pkg_db_a1`, `pkg_db_q4b`, parameterized sema fallback owner |
| Local, arena and ended sources | Local or arena storage cannot escape; missing, cyclic and ended evidence fails closed. Copy/move and source replacement retain their existing release and lifetime roots. | Existing `return_provenance`, `struct_handle_fields`, indexed-field candidate and sema storage owners, with paired caller/local fallback controls |
| Validated content publication | Preserve the ordinal-only summary's conservative domains: a text field may validate bytes of a numeric sibling. Repeated independent writes use separate numeric/text schemas; mixed aggregate calls and stale UTF-8/codec negatives remain covered. No publication precision change. | Fixture qualification only; `borrowed_content_calls`, `dynamic_array_fields`, `indirect_mutable_outputs`, `completed_call_publications`, `imported_publication_domains`, `call_boundary_views` |
| Control and calls | Direct, generic, imported and indirect calls; `if`, `match`, `else`, `?`, `map_err`, loops and early exits use the same facts. Whole-program and interface compilation agree; malformed checked HIR rederives the contract. | Existing owners above and `call_boundary_*` matrices; no new serialized field |
| Authority qualification | Ordinary returned/captured view leaves remain read-only. No-rebind calls preserve descriptor authority; numeric element writes do not end a descriptor. Keep lifetime and task-conflict controls distinct from read-only refusals. | `assignment_target_liveness`, `return_provenance`, `task_progress`, `sort_comparator`, `imported_mutable_retention`, `array_truncate` |
| Existing producer contracts | JSON bounded arity and frozen XML union construction owners must assert the shipped behavior, including exact tag/payload association. | `m5_json_bounded`, `std_xml` |
| Nightly capacity | Retain all test binaries, stable inventory/sharding, named failures, two-way manifest judgement and the existing deadlines. Avoid duplicate build-profile generations and unnecessary debug/incremental artifact retention. | Nightly build/cache configuration; `scripts/test-pr-workflow.sh`, measured artifact footprint and a complete nightly run |
| Fixed C-field cache cost | Keep the 392-call native ABI inventory in both compilation modes. The cold/warm/width/count/revert cache cycle exercises the changed integer record and an unchanged float control, including all 28 register-pressure calls per revision, within its existing 60-second process-group deadline. | `c_layout_raw_fields::{fixed_numeric_fields_cross_c_storage_and_values,fixed_numeric_field_cache_tracks_width_count_and_revert}` |

Type formation, construction, move-in/out, source nulling, replacement, Drop,
allocation and native ABI remain unchanged. Existing lifecycle/allocation owners
must continue to pass; no benchmark is required because no performance promise
changes. DB integration runs locally against disposable PostgreSQL services
before push. Diagnostic qualification changes preserve the original safety axis
with a valid positive control and a discriminating negative control, rather than
accepting any diagnostic or suppressing failing cases.

This is one release-readiness repair boundary: the full-suite detector must be
able to run and accurately judge the contracts it tests. Production fixes retain
the existing safety strategy and require one author matrix-to-diff pass and one
fresh full-diff preflight review. Record any strategy change here before coding
and independently review it first.

The independent strategy pass confirmed the existing boundary with these exact
constraints: keep the baseline call lifetime cap even for an unknown call-result
header; retain all candidate fallbacks and unknown identity; propagate selected
ended/unknown content independently of that lifetime; accept headerless exact
lifetime without inventing identity; and keep missing/cyclic evidence closed.
The resolver owner crosses Caller/Frame/Arena with clean/ended/unknown contents.
Indexed-call owners cross borrowed, by-value, local and arena storage. The
review rejected filtering numeric siblings from publication: a string can view
their `as_bytes()` result, which an ordinal-only summary cannot distinguish from
a static string setter. Fixture qualification follows that existing limitation.
