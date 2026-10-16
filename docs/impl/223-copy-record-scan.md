# Copy record prefix scans

Status: implemented; owner verification and preflight review close this capability.

The existing `scan` collector already has typed record loads, accumulator slots,
calls and output buffers. Source admission still forbids every record input and
accumulator. This capability composes those existing mechanisms. No current
consumer source uses `.scan`; this is existing-library composition, not a claim
of consumer adoption. All three request registers remain available and their
pending ownership requests are outside this boundary.

## Public contract ledger

| Surface | Exact contract |
| --- | --- |
| Signature and domain | Existing `source.scan(init, f) -> array<A>`, exactly two required arguments, no defaults; `init: A`, `f: (A, E) -> A`. Preserve every previously admitted Copy accumulator, including `()`, sums and `str`. Additionally admit an already-formed independent Copy record A without root `align(N)`, using Plan220's complete graph and ordinary collection formation. Root tuples and Move/view/callable/raw/resource-bearing record accumulators remain excluded. Input E admits existing Copy record pipeline elements, including view-bearing records, under the same by-value call rule as `reduce`; source formation and Move-input rejection do not widen. |
| Sequence and effects | Source and stage captures evaluate in written order, then init once, then terminal captures once. The fused loop visits surviving elements in source order and calls f exactly once per survivor with the previous accumulator and complete E. It stores each returned A; init is never emitted. Empty/no-survivor input yields an empty array and calls f zero times. Ordinary sequential effects and the existing Pure requirement of surrounding parallel callbacks remain. Named, lifted, inferred and generic/imported functions use ordinary call rules. |
| Result representation | Complete A values form a canonical owned AoS array for record A. Existing fixed/dynamic/slice/SoA sources and admitted map/where/project stages apply. No hidden field projection, clone, new layout or record reinterpretation. |
| Lifetime and ownership | Inputs and captures stay reserved through evaluation and callback execution. When the complete output element graph is independent, result contents retain no source/init/stage/terminal borrow; the fresh array header and its heap/explicit-arena allocation generation remain tracked. For potentially borrowed old scalar/sum outputs, retain existing conservative lifetime facts, including captured text and source views. Copy alone is not an independence proof. Result move, replacement, return, Drop and arena escape checks remain ordinary array rules. Elements add no ownership, source nulling or per-element Drop. |
| Allocation and errors | Existing collector capacity, typed stride/alignment, checked size arithmetic, heap/arena allocation and temporary-source cleanup apply, including empty and zero-sized records. Output allocation is visible in scan; callback allocations remain visible in the callback. No allocation parity or speed improvement is promised. Invalid source/accumulator/call mode/effect/metadata diagnoses before LLVM; OOM/size overflow retains the existing hard error. Existing validation order is source/stages, init/domain, callable. |
| Implementation and identity | Sema and checked-HIR share one accumulator-to-result classifier; record materialization shares the independent record rule with callback sorting. Independent output content selection is one graph-based predicate used by region, storage-content and borrow analyses without bypassing completed-header/generation checks. Existing collector and ordinary record call ABI lower the result. No new IR variant, runtime ABI/effect, interface field, cache format, wire/native-text boundary or global state. Existing nominal identities and independently minted monomorph matching remain authoritative. |
| Sources and prerequisites | Shipped record collection, Plans220/221/222 and Plan19 delegation. Update draft, digest, design-notes, Settled, Plans12/19 and the core array-pipeline English/Japanese domain/lifetime prose. HANDOFF records this capability once. No new milestone or consumer-owned changes. |

## Implementation closure matrix

| Axis | Implementation and exact owner |
| --- | --- |
| Formation and complete graph | Replace `scan_accumulator_scalar` with a shared result classifier preserving its old scalar/sum domain; use the shared independent record materialization branch for records. `scan_record_domain_and_metadata` in checked-HIR tests covers accepted records and rejected view/Move/callable/raw/overaligned/invalid/inactive/zero-length graph variants, init/output/result/callback/capture/mode metadata. Reuse `producer_plain_copy_type_graph`. Existing delegated Unit/sum positives remain; replace the obsolete all-record negative with a genuinely excluded record. Sema's obsolete all-record negatives become source admission owners. |
| Construction and typed execution | `scan_records::record_scan_order_layout_and_replay` executes complete mixed-width/nested/fixed/tuple/tagged record values, Copy record and scalar inputs, fixed/dynamic/slice/SoA/map/where/project sources, empty/singleton/multiple survivors, and exact prefix values. Reuse the shared physical allocation/header mutation owner from Plan220; the ordinary collector must produce canonical record headers and exact typed accumulator/output stores. |
| Move, source nulling, replacement, return and Drop | `scan_records::record_scan_lifetimes_and_cleanup` executes heap/arena results, temporary sources, repeated materialization, moved/replaced/discarded/returned arrays and zero-sized records. Measure balanced live bytes/allocation/free after helper return, using existing native counter fixtures. Only array spines transfer; no element Drop strategy changes. Run with buffer donation enabled and disabled. |
| Result facts versus call reservations | The same lifetime owner accepts independent scalar/record output after local text sources and stage/terminal captures expire or their owners are replaced. It rejects expired borrowed-str/sum output, arena-result escape, invalidated output-array views, source replacement during init and captured-view invalidation before use. `escape_initializer_value` excludes independent scan element contents; `region_of` uses only allocation region for that case. `borrow_fact_one` omits stage/node capture additions only after completed-value/generation snapshots have been honored; the scan-specific inner fact and fallback source selection return no element lifetimes only under the same proof. All traversal, source snapshots and callback-effect logic stay intact. |
| Control and early exits | Lifetime/native owner covers if/match/else/?/map_err/loop joins and early source/init exits. Reuse `current_plan_capture_termination` and existing scan capture-order tests; extend only where record accumulators change the executed path. No store/call after termination. |
| Generic/import/replay and siblings | Mixed-record owner uses generic record helpers and imported functions under whole-program and per-unit compilation, then executes a cold/hit cache replay. Keep `body_ty_matches` at semantic compatibility seams rather than comparing separately minted nominal IDs. Run `mmv2`, `lambda`, `unit_values`, `array_materialize`, `buffer_donate`, changed HIR/MIR owners, bounded gate and separate-target Clippy. No benchmark is required. |

This is one usable boundary: record input/output admission, checked-HIR,
independent-result provenance and executable lifecycle coverage ship together.
It introduces no dormant producer or new ownership rule. The expected
handwritten change is below 1,000 lines.

## Author ledger-to-boundary pass

Baseline source rejects record input before examining the record accumulator.
A separate scalar baseline returning a numeric scan whose lambda reads a local
text capture also rejects with a borrowed-local-storage diagnostic. Inspection
finds conservative source/capture unions in region selection, storage-content
initialization and borrow-result fallback. `pipeline_accumulator_fact` already
honors callback return summaries but its initial fact and generic capture unions
must not reintroduce authority absent from a completely independent A. A shared
predicate must derive A from the actual result type and confirm the scan's stored
element/result equation, then use `independent_copy_type` over every reachable
field and payload. It must not treat the array owner itself as independent or
return early before completed generation/snapshot handling.

The lowered collector already uses `collected_array_type`, typed allocation and
ordinary aggregate calls, including record input reconstruction from SoA. Its
scan strategy borrows original HIR captures (Plan222's address-identity lesson),
so no synthetic HIR or additional lowered capture cache is needed. Output
formation and malformed metadata are checked independently of that lowering.

The ledger covers each changed argument/result, evaluation order, errors,
ownership, allocation and artifact identity. There is no new canonical encoding,
CLI/configuration, reflection or connection-global state to specify. The result
domain extension does not relax the settled Copy-only fold restriction.

Independent pre-implementation strategy review found no P1/P2 issue. It checked
the public domain, complete typed collector, all result-fact sites, fresh header
generations, call reservations, checked-HIR and the planned lifecycle/replay owners.

## Author matrix-to-diff closure

`scan_accumulator_result` delegates record materialization to the same independent
record classifier as callback sorting, retaining old Unit/sum/str admission.
Checked-HIR independently checks Copy arguments, ordinary formation, semantic
init/callback compatibility and canonical output. `scan_record_domain_and_metadata`
mutates output/init/graph/call/capture/mode/source records, including inactive and
zero-length borrowed fields; delegated owners preserve old scalar/sum behavior.
`record_scan_uses_typed_accumulator_and_output` pins exact aggregate call operands,
accumulator store, output store, allocation and canonical header in both modes.

`independent_scan_contents` derives the actual result element and checks its
stored equation plus complete graph. The five analysis consumers (region,
storage initializer, generic capture additions, scan accumulator facts and source
fallback) use that proof without changing traversal or completed snapshots.
The new native owner accepts expired/replaced source/capture owners only for
independent results and rejects borrowed scalar/sum escapes, source/capture
invalidation during evaluation, arena result escape and invalidated scalar/record
result-array views. Fresh generation and call reservations remain separately live.

The two native owners execute complete mixed prefix states, all admitted source
forms, generic/imported helpers and cold/hit per-unit replay, heap/arena and
zero-size paths, result transfer/replacement/Drop and normal control joins. Exact
observable init/callback traces pin empty/no-survivor frequency and survivor order.
Allocation/free/live-byte checks close after lifecycle helpers return; the one
remaining numeric result owner has exactly its expected live buffer. The existing
physical storage mutation and capture-order/early-exit owners close unchanged
collector mechanics. No lowering, runtime ABI or serialized representation changes.
