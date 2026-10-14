# AoS record-array subslice producer certification

Media AM-R06 reports a rejected fallible cache consumer returning an owned Stage.
The two exact source modules and three logs in media issue7 match their published
hashes. The source pair overlaid on media src at 314e431af88dfc4949655e4e9b1cff15b5585cb6
reproduces the original diagnostic with Align 77acc7afc71df9898bf71020ceec4df573129414.
That is a reconstructed dependency context; the record/named-Stage variants have
logs only. A GPU-free provider reduction needs neither a loop nor a native resource:
pass an owned record-array slice to a fallible helper, then return an owned record.
The scalar-array and incoming-slice controls pass.

Plan44 already admits contiguous AoS record-array slicing, including existing
Move records, with shared provenance and no owner transfer. Sema and lowering
implement that contract, but the MIR SubSlice producer relation omits
DynStructArray(_, Aos). Its invalid dependency reaches the final owned return.
Extend that exact relation to the existing record id, Slice(Struct(id)) result
and Struct(id) stride type. Keep grounded read dependencies and every selected
field path. Fixed struct arrays already become MakeSlice before SubSlice;
SoA and specialized array sources remain excluded. No new syntax, type formation,
view authority, ABI, allocation, cleanup, interface format or safety strategy.

## Implementation closure matrix

| Axis | Implementation and owner |
| --- | --- |
| Formation and exact producer relation | SubSlice checks AoS source, exact record id, borrowed slice result, element stride, and i64 start/length. MIR mutation owner crosses Copy/Move records and whole/partition certification; wrong record/element/result/layout/base/bounds and uninitialized backing reject. Existing source/HIR formation remains unchanged. |
| Borrowed source and selected content | Reuse read_source and selected paths, preserving shared authority, owned-string physical leaves and source dependencies. Source owner rejects escaped/dead local views and consuming a shared Move element; existing move_slice_mir_gate remains the malformed projection/authority owner. |
| Construction, moves, nulling, replacement and Drop | No lowering or runtime change. Driver record_subslice_producer owner executes builder-produced Copy and Move records, independent returned buffer storage, replacement/discard and success/early-error cleanup, with live-allocation and source payload observations. A slice creates no allocation or owner transfer. |
| Controls | Driver owner uses explicit ranges, empty and repeated/resliced views, if/match/loop, ?, else, map_err and early error while the source stays live. Existing move_record_slices controls retain evaluation order and bounds semantics. No new CFG or error rule. |
| Generic, imports, caches and native parity | Imported generic record factory/consumer, whole/per-unit and cold/hit frontend replay. Existing canonical serialization and compiler identity apply. Compile the reconstructed media source after repair without GPU linkage; real image/cleanup acceptance remains consumer-owned. |
| Scope and performance | One exact validator repair with safety-negative and executable owners. No benchmark or application performance claim. No consumer source, compiler pin or adoption changes. |

This follows Plan44's existing public and safety contract. Complete the author
matrix-to-diff pass and align-self-review, then one fresh independent preflight
review. A separate new strategy review is unnecessary unless the repair needs a
different provenance abstraction. No normative language or library prose changes.

Sibling read-only inspection found llm prompt_tree_snapshot already preserving
a named slice of built Entry records, and voice tokenizer building owned Entry
records. Neither selects broader write/extraction authority; their existing
view and owner contracts remain unchanged.


## Author matrix-to-diff closure

The sole production change adds the missing AoS source relation to SubSlice;
read_source, selected-field traversal, graph grounding and every cleanup action
are unchanged. The parameterized MIR owner crosses Copy/Move records, exact
stride/result/source/bounds, columnar retyping and an uninitialized source in
whole/partition certification. The native owner executes generic/imported
Copy and owned-string records through fallible factories and repeated views,
partial errors, owned buffer return, replacement and discard in whole/per-unit
and cold/hit replay modes. View observations allocate and free nothing; final
requested live bytes are zero. The pre-fix compiler rejects this owner at the
same producer-return diagnostic as media.

All seven existing move_record_slices owners pass, including formation,
projection, source lifetime/invalidation, eager controls, allocation cleanup
and interface replay. The reconstructed original media source now passes MIR
certification and emits all nine native unit objects. Native GPU linkage, image parity and consumer pin/adoption are
not claimed. The self-review checks the source/HIR/lowering receiver domain,
exact AoS relation, read-only retained provenance, selected Move-content paths,
unchanged ABI/IR/Drop and malformed-producer controls. No language extension or
general aggregate-provenance capability is included.
