# Mutable-borrow storage authority

Plan194 exposed ordinary call producer refusals that predate field writes. On
`9227651f`, heap dynamic arrays of integers and str execute through a whole-place
exclusive helper, while the same arrays allocated in an arena fail MIR producer
certification. Fixed str-array record fields fail independently of arena placement.
The twelve exact root/record/fixed, integer/str, heap/arena witnesses are retained
in the provider investigation. These are two consumers of an incomplete separation
between readable contents and authority to mutate the selected storage.

The existing contracts in draft.md and Plan17 section 2 already admit the calls.
This capability changes producer verification, not source syntax, type admission,
borrow generation semantics, allocation, cleanup, ABI or interface format. Shared
places, overlapping call/task accesses, expired views and partial exclusive Move
fields remain subject to their existing source rules. Function-value mutable-output
precision and K1 remain separate; fixing producer authority cannot invent a more
precise semantic output summary.

## Proof boundary

A canonical borrowed descriptor must retain its exact slot, physical type, path,
mode and callable identity proofs. Authenticate payloads at each typed selected
leaf instead of applying the whole aggregate's producer state to every leaf. A
borrowed str requires readable text, not ownership or permission to mutate its
bytes. Complete construction and initialization remain mandatory for plain and
empty values; absence of protected leaves is not proof of initialized storage.

A mutable call additionally requires exclusive authority for its actual storage
and every owned component it can replace or destroy. Use the existing complete
DropPlan inventory for allocation/owner paths and the existing protected-leaf
inventory for nested text/callable/native/owned payloads. Numeric owning buffers
must be covered even when the protected-leaf inventory is empty. Every owning
String/native payload retains its exclusive requirement; merely changing a Move
parameter's payload requirement to READ is forbidden.

Owned collection buffers use the existing buffer-provenance equation family,
independently of element content. Extend that family through the actual typed
allocation pointer and owning-header construction, slot/value forwarding and joins.
A validated fresh ArenaAlloc/SoaAlloc grants writable exclusive buffer access in
this lane, never individual ownership or a move/drop capability. Its ordinary
payload/cleanup provenance remains Shared. HeapAllocBuf keeps its existing owned
proof. Propagate buffer dependencies without upgrading shared parameters, copied
borrowed descriptors, static data, unknown producers or unresolved Copy-view call
results. Buffer root seeds must still be founded by the real typed producer and
all required inputs; a cycle cannot certify itself. The same distinction applies
to a nested owned buffer and a root buffer.

Canonical selected payload queries reuse the typed producer graph, including
parameter seeds and every writer, with the descriptor prefix followed by the
selected leaf path. Fixed array elements use their established construction and
Element graph. Preserve Plan170's strict anonymous construction, pooling,
all-path initialization, typed cardinality and malformed writer obligations;
this is not permission to reconstruct arbitrary array writes as construction.
Borrowed element/fixed-element descriptors retain their existing stable-place,
bounds and field authentication. They gain no new exclusive source admission.

One call validator serves all four direct/indirect and cleanup/no-cleanup MIR
forms, local and imported contracts, whole-program and per-unit publication,
unused peer validation and emission. Validation remains fail-closed for every
missing/wrong path, producer, type, mode, cleanup identity and malformed graph.
No proof is serialized or trusted from a cached claim; the compiler identity
invalidates prior checked artifacts as usual.

## Implementation closure matrix

| Axis | Implementation boundary and owner |
| --- | --- |
| Read versus write authority | `borrowed_content_calls::values_and_storage` crosses dynamic root/nested record, fixed record, scalar/str/mixed borrowed+owned content, heap/arena and local/borrowed/exclusive forwarding. Native whole/per-unit reads observe repeated writes. Existing `dynamic_array_fields::imports_and_cache` becomes a positive for the repaired direct text-call cases; its indirect-output limitation stays explicit. |
| Complete owner graph | `mutable_borrow_call_authority_matrix` in MIR tests requires readable str plus exclusive String/native/owning numeric and text-buffer siblings, including fixed arrays and sum payloads in already-admitted calls. Mutate a formerly owned sibling to a shared/unreadable producer while keeping the selected text valid; whole/partition validation must reject. No protected-leaf-empty shortcut. |
| Allocation provenance | The same MIR matrix separates fresh heap, anonymous/named arena, caller region, shared parameters, copied shared headers and static/unknown pointers. Typed count/element/region/layout and producer identity corruptions reject. Arena writable authority never satisfies Move/free requirements; existing arena/owned return and cleanup owners retain that negative. |
| Construction and malformed data | Existing fixed-scalar construction owners plus `mutable_borrow_call_authority_matrix` cover fixed str/empty/mixed construction, absent or wrong writers, forged path/type/slot, missing or duplicate definition, ungrounded cycles, Out reads and shared-to-exclusive upgrades. Keep malformed anonymous construction and late/extra writers rejected. |
| Call forms and modes | Direct, cleanup-carrying, indirect and indirect-cleanup MIR twins; source direct/imported/concrete-generic helpers and existing allowed function-value cases. Borrow, BorrowMut, ByValue and Out requirements stay distinct. A mutable Copy slice authenticates its header, not the pointed-to bytes. |
| Mutation and control | `borrowed_content_calls::control_and_lifecycle` covers indexed/whole-field/root replacement, if/match/else/?/map_err, loop entry/back-edge/exit, early return and a terminating operand. Existing source lifetime/alias owners reject expired inputs and overlapping argument generations. |
| Move/Drop/allocation parity | The same native owner measures actual alloc/free and live-byte deltas across unchanged borrow, successful/failing replacement, arena exit, moved/extracted owners and return. Caller ownership and cleanup bits persist; source nulling and Drop stay on existing paths. No performance claim or benchmark. |
| Interfaces and replay | `borrowed_content_calls::imports_and_cache` checks imported and concrete-generic helpers in whole/per-unit native builds and cold/hit/type-edit/restore caches. Existing borrowed_params, fixed_array_fields, fixed_scalar_borrows and owned_borrowed_composition owners close adjacent public paths; interface summary and bounded compiler owners cover replay. |
| Validation phase | Forged call/owner cases reject during full and pre-partition producer validation before object emission, including an unused peer. No new HIR/IR field, runtime ABI, public interface record or package surface. |

This is one proof capability: selected readable content without complete owner
and buffer authority would be unsound, and an unused buffer producer without its
call consumer fixes no client. If the implementation plus malformed/native owners
exceeds roughly 1,000 hand-written lines, keep that proof together to avoid a
second temporary authority system and duplicate integration proof. Reuse existing
owner matrices rather than cloning one fixture for every table cell.

Before coding, complete the author obligation extraction and obtain one fresh
independent adversarial review of the authority separation and capability boundary.
Before code review, every applicable cell must name its implementation and passing
owner, or record a justified independent limitation without broadening this proof.
