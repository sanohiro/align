# Array builder value provenance

Request 46 reports an owned array replaced by `next.build()` inside a loop
conditional, then rejected as borrowing the iteration-local builder `next`.
Provider scalar, Copy-record and Move-record witnesses reproduce in whole/per-unit
checking. A local-only array read reproduces without any borrowed function call.

The existing contracts in plan17's heap record builder ledger and plan52's
`BuilderElement` row already require consuming build to transfer the buffer and
its element contents to the array. Apply plan132's value-versus-storage distinction
to the linear array-builder family: an owning value carries its explicit element
and region dependencies, not a borrow of the local slot being consumed. A real
borrow of the builder receiver still uses that slot's storage identity. Preserve
all contained lifetimes, read-only origins and byte-validation facts, including
facts that grow across loop backedges. Do not erase an origin merely because a
source is moved or because a destination is an owner.

No syntax, admitted type, MIR, ABI, allocation, Drop strategy, interface format or
ordinary-call summary changes. Array-field assignment, nested Move-field
extraction, general interprocedural authority, K1/plan61 and consumer adoption
remain outside this repair.

## Implementation closure matrix

| Axis | Required closure and owner |
| --- | --- |
| Formation and construction | Keep existing heap scalar/string/closed-record and region RegionPlain admission, malformed type checks, by-value builder rules and borrowed-builder consumption rejection. Existing `m12_array_builder` formation/generic owners remain authoritative. |
| Value versus storage facts | `local_borrow_fact` retains explicit builder facts without inventing a dependency on its own transferred local. `local_storage_roots` and mutable receiver reservations remain unchanged. Cover scalar, vector, mask, fixed-array and fixed-record-array builder variants through `is_array_builder`, including aliases and new bindings. `array_builder_region_siblings_transfer_value_provenance` owns every region descriptor family. Parameterized sema `array_builder_value_transfer_keeps_content_dependencies` discriminates the old false dependency. |
| Move-in/out, source nulling, replacement and Drop | Build consumes the same linear handle and transfers the same buffer/initialized prefix. Existing Move and runtime lowering stay intact. `array_builder_transfer::builder_transfer_whole_and_per_unit` executes two loop iterations and reads scalar, string, Copy-record and Move-record outputs; `m12_array_builder::move_record_push_nulls_source_and_build_deep_drops`, `record_builder_abandonment_all_exit_kinds` and `record_builder_by_value_parameter_return_and_borrow_mut` retain buffer and element Drop coverage. |
| Control paths | Cross direct, block, conditional, match, loop and returned builder/array transfers with replacement and post-loop use. Reuse existing Result/Option `else`, `?`, `map_err` and early-exit ownership owners where no builder value can inhabit that payload type. Builder aggregate, sum and closure storage remain forbidden. |
| Genuine retained dependencies | Pair valid heap/region transfers with borrowed element expiry, expired region, read-only element writes, moved builder reuse and borrowed-builder consumption. Preserve a live external owner through alias/move/build, and continue rejecting the same genuinely dead owner. `array_builder_transfer_retains_real_borrows_and_receiver_reservations` plus existing `readonly_origin_carrier_matrix` own content provenance. |
| Eager evaluation and mutable receivers | Earlier push/append receivers still become invalid when a later eager operand replaces or consumes that builder. Keep receiver storage snapshots and reservation authority. Pre-fix probes also expose a missing pre-action check: push/append exempt their own receiver from post-action validation but never validate it before mutation. Reserve the exact receiver place for locals and borrowed mutable parameters, validate it at the shared Builder action boundary, and retire that action reservation before its own mutation; parameterized replacement/consumption negatives distinguish the real receiver borrow from the transferred value fact. |
| Generic, interface and compilation parity | `array_builder_transfer::builder_transfer_whole_and_per_unit` imports an ordinary builder/array factory and a generic identity path, checking and executing both whole and per-unit modes. Existing builder summaries and serialization remain unchanged; no cache format change. |
| Runtime and allocation parity | Runtime, MIR and LLVM are unchanged. No new source allocation, copying or cleanup policy. Exclusive artifact ownership and one immediately guarded process group bound every transitive compile/link/run helper in `array_builder_transfer`. `builder_transfer_group_cleanup_survives_leader_exit_and_unwind` proves surviving-descendant socket closure and direct-child reap after leader exit and immediate acquisition failure. No performance/resource improvement is claimed, so no benchmark is required. |

This is the existing owned-value provenance strategy applied to a sibling linear
owner. The author matrix-to-diff pass precedes the one fresh preflight review;
no new public or safety contract requires a separate plan review. If the repair
needs a different abstraction or wider origin erasure, reopen this boundary first.
