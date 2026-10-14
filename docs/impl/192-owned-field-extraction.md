# Owned record field extraction (voice V002, LLM R36)

## Evidence and boundary

Voice's tokenizer retains an `Entries { entries: array<Entry> }` wrapper because
it cannot transfer the owned array into its lookup record. `Entry` owns a string
key. LLM R36 asks for the same operation on owned arrays and complete nested
Move records. Its other request, whole-field replacement, is already supplied
by Plan65. Provider copies of scalar-array, owned-record-array, nested-record
and tuple-field witnesses all reject the existing partial-move allowlist.
All three registers and relevant consumer source were inspected; consumer code,
pins and acceptance runs remain external.

Extend the existing depth-one record-field move to every already-admitted
owning field with a complete valid Drop plan. Use the existing type ownership,
selected-source transfer, projected generation directory and typed field nulling.
This is one complete capability, not a new ownership mode or extraction API.
R137's indexed shared projection, R132/R142's partial exclusive borrowing,
R46/R147 and media AM-R01's field-element writes, and media AM-R02's JSON schema
remain separate. Plan52's interprocedural writability prerequisite still applies
to new write surfaces; this capability adds no such surface.

## Public-contract ledger

| Surface | Exact rule | Owner |
| --- | --- | --- |
| Expression and input | Existing `owner.field : T` in a consuming context, where owner is a bound owning record local or ByValue parameter and field is one direct member. T is any field type already admitted by ordinary formation with a valid recursive Drop plan that needs Drop. Copy field reads retain their existing behavior. No arguments, defaults, syntax, public function or nominal type are added. | shared semantic eligibility; source and malformed-HIR owner |
| Transfer and result | Ordinary binding, assignment, constructor, by-value call and return obtain exactly T. Evaluate in source order; complete all operands required by the enclosing action before transferring a selected bound field. Read the selected value before nulling exactly that field to its own typed zero. Its sibling values remain owned by the source record. No whole-parent transfer or implicit clone occurs. | completed-action MoveCheck, MIR source transfer and exact-layout owner |
| Readability | Mark exactly the direct field moved. Reusing it, any descendant, or the parent as a whole rejects; distinct sibling values remain readable, including reads below an unmoved sibling. Existing same-type replacement and complete-owner rules remain authoritative; this capability does not introduce field-by-field initialization of a partially moved record. | source move/read/control matrix |
| Allocation and cleanup | Extraction allocates nothing and adds no runtime entry. Transfer the existing individual-versus-arena cleanup bit from the parent to the extracted value before source nulling, including a path-dependent bit; do not clear the parent bit while it still owns siblings. Recursive parent Drop sees the zero field; the extracted owner drops its live contents once. Empty fixed/dynamic values follow their existing type-based Move and Drop rules. Replacement preserves the existing uniform aggregate allocation mode. | counted heap/arena/join/early-exit lifecycle owner |
| Lifetime and storage | An allocated header keeps its exact generation while its release place transfers from the selected parent projection to the destination. Existing dynamic aliases follow that generation and are invalid after its new owner ends it; consumed inline fixed storage instead rematerializes at the destination and ends its source generation. Contained borrowed text/views retain their original dependencies. Unknown or legacy non-storage lifetime facts keep their existing conservative rejection; no new interprocedural ownership/result-projection summary is inferred. | Plan19 projected-storage authority and alias/return owners |
| Borrow/alias refusal | A borrowed parameter, borrowed match projection, already-moved source, conflicting eager receiver reservation or duplicate selected Move source cannot be consumed. A genuine dependent resource remains governed by its existing live-parent rule. No partial mutable borrow or write authority is created by extraction. | owning/borrowed twins and eager operand permutations |
| Formation and exclusions | Do not widen record/array/tuple/native placement. Preserve rejection of consuming a deeper `owner.inner.field` path and indexed record-element extraction under their current rules. A complete direct nested record or tuple is a normal T and must not be excluded by a leaf allowlist. | exact family sweep and unchanged forbidden-path controls |
| Errors/effects | Failures above are compile diagnostics before emission; malformed checked records reject without panic. No recoverable runtime error or new diagnostic precedence is introduced. The transfer itself is Pure; child evaluation keeps its existing effects and termination behavior. | source negatives, checked-HIR/MIR mutation owners |
| Representation/identity | Existing HIR Field, MovedKey::Field, MIR NullStructField, aggregate type layouts, cleanup-bit ABI and interface records suffice. Their meanings remain typed source selection and source clearing. Compiler identity invalidates private artifacts; public type fingerprints retain their complete existing definition graph. No native/wire format, cache option or ambient input is added. | whole/per-unit, generic import, cold/hit and producer validation |
| Sources and prerequisites | Existing Plan19 storage/action contract, Plan17 partial-field nulling, Plan73/164/165 fixed owners and Plan190 tuple fields. Update draft.md, language-spec.md, design-notes.md and the Settled implementation account, plus the narrow Plan17/19/08 records and English/Japanese memory guide where this public capability is described. | author extracted-obligation pass; independent strategy review |

## Implementation strategy

Use one shared eligibility query derived from `DropPlan::is_valid()` and
`needs_drop()`, rather than copying lists of string, tagged and handle cases.
Both immediate consuming expressions and deferred completed-source actions use
it; MIR source nulling and malformed producer validation ask the same query.
The existing field formation validator remains a prerequisite, so an invalid
Drop plan or forbidden placement cannot become admitted by the new query.

The selected source remains `(root, direct field ordinal)`. Existing Move keys
already distinguish these fields. A deeper read checks the availability of its
first field rather than treating an unrelated partial move as a whole move;
deep consuming paths remain rejected. Resource dependency and eager reservation
checks are retained at the same transfer action.

Reuse projected headers and release places to transfer only the field's owned
allocated generations and materialize any inline leaves at the destination.
Audit `direct_storage_move_source`, `install_projected_storage`, completed-value
snapshots, and legacy-root detachment together: passing only a containing root
must not transfer or end an unrelated sibling. Preserve the directory's full
historical release identities through subsequent replacement, return and Drop.
Do not erase contained borrowed roots while detaching a former release owner.
The same projection must feed EscapeCheck and checked-HIR semantic replay.

MIR reads the parent cleanup bit at extraction, including a joined runtime bit,
then forwards it through existing value/return/control cleanup transport. It
zeros only the exact field after a reached transfer. The enclosing parent's
cleanup bit remains unchanged. LLVM already derives NullStructField's zero from
the exact field type; broaden eligibility only with producer validation and
layout owners. Deferred fixed literals initialize their inline slot even at zero length, so a
containing record has the same construction proof as a direct fixed binding. No untyped pointer/length store or new runtime mechanism is needed.

## Closure matrix and exact owners

| Axis | Implementation closure | Owner |
| --- | --- | --- |
| Formation / type families | Valid complete Drop plan across every currently admitted owning field family: dynamic scalar/text/record/specialized headers, Move fixed arrays/records, nested Move records, tuples and existing string/tagged/handle/resource leaves. Copy and invalid definitions do not qualify for nulling. | `align_sema --lib owned_field_extraction` shared predicate/source family sweep; `align_codegen_llvm --lib partial_field_nulling` complete layout/invalid-type sweep |
| Construction / move-in / move-out / return | Local binding, field/whole assignment, struct/tuple/tagged construction, direct/imported/indirect calls and explicit/tail return select one complete field. Later sibling reads see the pre-action source; duplicate source or later source replacement cannot escape validation. | `owned_field_extraction::field_extraction_values_and_control`; sema eager-source matrix; existing completed-action owners |
| Nulling / Drop / replacement | Exact field zero, unchanged adjacent scalars/owners, no new allocation, exactly-once deep cleanup. Heap, arena and joined runtime mode survive extraction, replacement of a destination, and parent/destination scope exits in either order. | `owned_field_extraction::field_extraction_allocation_lifecycle`; MIR cleanup-bit/nulling assertions; existing `owned_field_replacement` target |
| If / match / else / ? / map_err / loop | Selected-arm moves and ordinary fallthrough joins, early return/error/break, eager terminating later operands and loop re-entry follow the existing atomic transfer and cleanup rules. No transfer or destination publication after a non-fallthrough operand. | `field_extraction_values_and_control`, `field_extraction_allocation_lifecycle`; existing staged constructor/call and Result owners |
| Backing / alias / sibling / return | Dynamic alias follows moved backing; parent release cannot free transferred storage; destination Drop/replacement does invalidate it. Distinct sibling generation and nested value reads remain live. Inline Copy/Move backing, borrowed-content arrays, caller-region return and local owner/arena escape remain distinguished. | `owned_field_extraction::field_extraction_authority_matrix`; `return_provenance::{storage_generation_move_replacement_escape_matrix, storage_generation_interprocedural_allocation_parity_matrix, storage_generation_nonwritable_carrier_matrix}` |
| Borrowed or incomplete source | Both Borrow modes, borrowed sum projections, moved field/parent, selected descendant, duplicate/overlapping action sources and dependent-resource owners reject. Deep/indexed extraction and partial mutable borrows remain outside this boundary. | parameterized `field_extraction_authority_matrix`; sema direct/deferred twins |
| Malformed HIR / MIR | Independently replay source transfer eligibility and selected field; reject missing/wrong root, bad ordinal/type/definition, invalid recursive Drop, forged borrowed ownership and inappropriate nulling in unreachable/partition-peer blocks. | `align_mir --lib owned_field_extraction_checked_hir`; `align_codegen_llvm --lib partial_field_nulling` |
| Generics / interface / cache | Concrete generic record fields, imported helpers and function values preserve exact ownership/type/mode. Whole/per-unit and cold/hit cache results agree without new interface facts. | `owned_field_extraction::field_extraction_imports_and_cache`; existing interface and cleanup transport owners |
| Resource cost | Transfer adds no allocation or native call; fixture owners count only their explicit initial storage. No throughput or latency promise. | counted lifecycle and LLVM nulling owner; no benchmark gate |

Use one bounded driver target with exclusive scratch/process ownership and
parameterized cases, not one test file per matrix cell. Reuse existing owners
when they discriminate the unchanged boundary. Update the obsolete direct nested
Move-record rejection test; retain the deeper-path negative. The new-admission source witnesses must fail on retained merged baseline
`20a9dcd2` and succeed only with the complete capability. Baseline checks also
confirm that refilling a moved field currently rejects and that the unrelated
sibling-depth read is conservatively rejected; the former stays excluded and
the latter belongs to this selected-field readability closure.

The capability may exceed 1,000 hand-written changed lines because formation,
source/action authority, cleanup transport, release ownership and malformed-record
proof must agree. A guard-only producer or nulling-only consumer would be dormant
or unsafe, while separating layout from lifecycle repeats the same proof. One
end-to-end PR provides the useful consumer boundary; deeper/indexed extraction
and exclusive field authority are independently deferred failure domains.
