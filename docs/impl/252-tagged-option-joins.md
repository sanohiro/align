# Tagged value completion and conditional Option joins

Status: implemented, 2026-10-28. Plan71 owns the unchanged
tagged representation, active-payload and defined-value contract. This is a
codegen-quality follow-up found while attributing R109; it does not remove a
bounds check, change a search sentinel, or close the consumer's global census.

## Evidence and strategy

At `4d556508`, an independent three-function `find`/`rfind` plus `else`
reduction emits 504 bytes of arm64 text and 24 conditional byte selections.
The runtime search call returns an ordinary i64. Its temporary Option's
aggregate freeze and aggregate phi prevent LLVM from recovering that scalar
through the join. Simplifying only the sentinel or only the freeze does not
remove the cost.

Keep the common completion boundary before scratch initialization and after
active payload stores. Freeze each top-level field independently, then rebuild
the same aggregate. There are exactly a tag and a storage carrier for a
payload-bearing union; the carrier, including all inactive bytes, is still
frozen as a whole. Tag-only values retain their existing no-freeze path. LLVM's
[element-wise aggregate freeze semantics](https://releases.llvm.org/22.1.0/docs/LangRef.html#freeze-instruction)
make this equivalent to freezing the complete aggregate; no byte is left less
defined, and there is no per-byte expansion or recursive type walk.

For `conditional_option_value`, both arms continue to construct their own
completed Some/None value. Store each into one entry-block scratch slot and load
the result at the join. Ordinary SROA can promote the tag and payload separately.
Do not store an inactive Some payload on the None path or speculate a producer.
The helper's current callers are checked arithmetic, string search and checked
byte-to-typed views. Other aggregate join producers are outside this capability;
their common completion boundary receives the equivalent field-wise freeze.

An IR-only prototype of both changes emits 96 bytes for the three-function
reduction with the same LLVM22 default O2 pipeline. This is motivation, not
production qualification or a cross-platform performance promise.

## Implementation closure matrix

| Boundary | Invariant and owner |
| --- | --- |
| Formation/layout | No type, tag ordinal, byte offset, alignment, payload mapping or semantic identity changes. Existing semantic/LLVM layout twins and tagged identity owners remain. The common completion helper freezes every actual top-level field without unchecked indexing; no-storage values retain their current path. |
| Construction/move-in | Both tag and complete storage are defined before the first scratch store and at publication. Only active Stored payloads are written. Existing `option_none_uses_the_frozen_union_constructor_before_a_join`, `variants_with_omitted_active_payloads_freeze_sibling_storage`, active-store and callback construction owners pin the new spelling and the old invariant. |
| Move-out/nulling/Drop/replacement | No move, cleanup flag or pointer access changes. Nested owned Option/Result/user-sum values must retain active pointer bytes and exactly-once cleanup. Existing optimized nested tagged database owner and native owned-tagged regressions close this boundary. |
| Conditional join | Some/None producers and branch order are unchanged; one slot receives exactly one completed value on each predecessor before the join load. Checked signed/unsigned arithmetic, find/rfind and valid/invalid byte views cross success/failure, scalar widths and consumers in focused IR and native owners. No aggregate phi or scratch survives the optimized scalar-result reductions. |
| Control paths | Existing native owners exercise if/match/else/?, nested arms, joins, loop reuse and early exits. The common completion change cannot move a side effect or read an inactive payload. Explicit native Some/None and overflow/missing controls must produce independent expected outputs. |
| Imports/generics/ABI/cache | Whole/per-unit and ThinLTO preserve results; no MIR/interface/runtime ABI change. Compiler-byte identity already invalidates objects. Existing imported tagged owners and a focused imported reduction close transport; callbacks, task/error records and database rows retain their common frozen formation. |
| Malformed input | Existing type/map validation and diagnostics remain. The freeze C API receives only live LLVM basic values and returns the identical type; field extraction/insertion failures propagate as lowering errors. |
| Allocation/resource | No heap/arena/runtime allocation or retained reference is added. Entry-block scratch is local to the existing function; LLVM promotion is observed for scalar-result reductions. Parameterized native ownership owners retain balanced cleanup. |
| Measurement | Compare identical independently authored search and checked-arithmetic reductions with baseline/candidate compilers on this host, validate outputs, and record object size plus alternating runtime samples. No global sign-count, whole-product latency or universal speedup claim. |

The two changes form one capability: each alone leaves the observed cost, and
both preserve the same Plan71 completion contract. The production boundary is
one backend and its owners, with no new language or library surface.

### Exact owner inventory

The independent strategy review found no semantic defect and requested exact
owner names in place of generic references. The following mapping closes that
finding before production edits; existing coverage is reused.

| Matrix rows | Exact owner |
| --- | --- |
| Formation/layout and malformed shape | `align_codegen_llvm` unit owners `tagged_union_physical_map_omits_zero_values_and_overlays_variants`, `sema_and_codegen_struct_layout_agree`, `physically_equal_option_and_result_keep_distinct_semantic_shells`; the same target's malformed tagged-layout owners |
| Completion, omitted active payloads and store order | `align_codegen_llvm::{option_none_uses_the_frozen_union_constructor_before_a_join,variants_with_omitted_active_payloads_freeze_sibling_storage,tagged_union_constructor_writes_only_the_active_payload}` |
| All conditional callers and promotion | New `align_codegen_llvm::conditional_option_joins_promote_scalar_results` crosses all eight integer widths/signedness choices and checked add/sub/mul, find/rfind and typed-byte-view consumption. Native `align_driver --test checked_arith`, `--test m5 str_find`, `--test m5 str_rfind`, and `--test runway_a2_binary_codec checked_typed_byte_view` retain independent outputs and failure predicates. |
| Nested ownership, replacement, control paths, imports and generics | `align_driver --test owned_tagged_payloads`, including `nested_tagged_payload_executes_exact_pkg_db_shape`, `nested_tagged_public_surface_matches_whole_and_per_unit_builds`, `recursive_move_payloads_match_whole_program_and_per_unit_builds`, and the tag-transition/replacement owners in that target. New `conditional_options_reuse_the_join_slot_across_loop_iterations` executes all three caller families through alternating success/failure on the same loop slot, in whole/per-unit builds. The same imported fixture also passes a native `--thin-lto` CLI run. |
| Historical optimized database return/Drop regression | `align_driver --test pkg_db_q5b2`; no package source or service behavior changes |
| Native callback construction | `align_driver --test pkg_db_callbacks` owners `sqlite_callback_descriptor_and_trampoline_have_the_exact_generated_llvm_shape`, `sqlite_callback_descriptor_and_trampoline_link_in_per_unit_build`, and `sqlite_callback_round_trips_native_values_and_removes_the_exact_identity` |
| Artifact reuse | Existing compiler-byte cache identity remains unchanged; no cache-format owner needs new fixtures. Whole/per-unit native owners above consume the same frozen layout. |

The local measurement additionally compares a no-Option arithmetic control.
The new structural owner is a regression gate; timing is evidence only.

## Qualification and measurements

The new 27-function structural owner fails on the baseline with an aggregate
join around the scalar result and passes on the implementation. All 274 active
backend unit tests pass (nine existing ignored tests), together with the owned
tagged-payload suite, six arithmetic owners, the four find/rfind owners, five
checked-byte-view owners, 14 Q5b2 owners and three selected callback owners.
The existing defined-before-store and omitted-active-payload assertions now
check the field-wise freeze spelling, retaining their tag-only reverse control.

Actual release/baseline objects from both compilers on Apple M1 / macOS arm64 /
LLVM22.1.8 confirm that the three-function search reduction falls from 504 to
96 text bytes (24 conditional byte selections become three scalar selections).
The three-function arithmetic fixture falls from 308 to 188 text bytes.

Seven alternating pairs from `bench/option_joins/run.py`, run after the owner
suites finished, give these medians for 8,100,000 calls per sample:

| Case | Baseline ns | Candidate ns | Baseline / candidate |
| --- | ---: | ---: | ---: |
| Checked i64 add | 18,949,000 | 14,116,000 | 1.342 |
| Checked i64 multiply | 15,516,000 | 13,632,000 | 1.138 |
| Wrapping add control | 8,574,000 | 8,609,000 | 0.996 |

Every sample passes the independent 81-input-pair oracle and timed checksum.
These are local measurements with run-to-run variation, not a cross-platform
latency guarantee. Runtime search semantics, bounds checks and ABI attributes
are unchanged; R109's whole-consumer sign-count acceptance remains external.

Independent code review found one test-execution issue: the new loop regression
used common helpers with unbounded native children. Its complete whole/per-unit
compile/link/run action now uses the existing `owned_fixture` supervisor. The
focused positive owner and `owned_fixture::stalled_linker_is_retired_before_scratch`
pass; production code and the measurement remain unchanged.
