# Read-only view provenance

Status: local capability and read-only text-byte origins are implemented.
Interprocedural capability remains deferred.

## Existing contract and reproduced failures

`draft.md` §3 requires rejection of writes through constant tables and literal
byte views, including rebound views. Mutability of a local changes the view
binding, not the underlying allocation. Explicit owned byte copies remain writable. This is
an implementation repair, not a new type, allocator, ownership model or syntax.

At `177224089629a269bc404f2958b8bfc67b79dcbe`, the following checks successfully
in both source and per-unit compilation even though its target is read-only:

```align
module readonly
S { table: slice<u8> }
fn main() -> i32 {
  mut s := S { table: "xx".bytes() }
  mut view: slice<u8> := s.table
  view[0] = 65
  return 0
}
```

A literal first bound to a `str` local and then converted with `.bytes()` also
loses the origin. A literal byte view passed through `borrow mut slice<u8>` is
accepted. Direct literal-byte element assignment is correctly rejected, and an
owned-array write is accepted. These are compile-only negative witnesses; they
must never execute. Investigation sources and exact verdicts are retained under
`.git/r46-investigation/`. This issue was found while assessing Request 46's
remaining record-array element assignment; that extension has not begun.

The source checker's insert-only `readonly_locals` and checked-HIR's separate
read-only expression walker recognize only selected syntactic wrappers. Neither
is a complete local value-flow analysis. A second, already-recorded failure
domain is laundering an origin through an ordinary function result or a plain
slice parameter (`docs/open-questions.md`, aggregate constants follow-up).

## Capability boundaries

1. **Local origin propagation and explicit writable arguments.** The existing MoveCheck
   projected-fact and storage-generation analysis follows read-only origins through local bindings,
   selected aggregate projections and control flow. It rejects proved read-only
   write destinations and explicit Out/BorrowMut slice arguments. The source
   producer and checked-HIR replay use the same analysis. This is independently
   useful: it closes direct local laundering without changing interfaces or
   requiring a dormant cross-unit producer.
2. **Interprocedural writability obligations.** A subsequent capability supplies
   exact argument-write and result/mutable-output provenance across named,
   imported and higher-order calls. It needs its own reviewed interface record,
   unknown-target rule and cache identity. The local capability must not pretend
   that absence of a known read-only origin proves writable storage. Existing
   ordinary-call laundering remains explicitly unresolved until this capability.
3. Resume Request 46 only after its proposed implementation's write authority is
   established. Do not add a new source write path while its underlying view
   safety remains unqualified.

These separate actual failure domains: an intra-body analysis with no exported
fact, followed by a new cross-unit contract. They are not line-count splits of a
dormant producer/consumer chain. No versioned release or consumer adoption is
part of either capability.

## Local analysis contract

| Surface | Exact rule |
|---|---|
| Fact | A set of read-only storage origins at typed value paths. The empty path denotes the value's own pointed-to storage; element/field payload paths denote contained values and must not taint unrelated writable backing or sibling fields. |
| Origins | ConstArray marks its backing read-only and literal `str` values mark their bytes; mapped file/byte views use the existing read-only classification. Fresh owned array/buffer storage retains its existing writable contract. Text byte views remain read-only even when their source owns the allocation. A shallow array copy preserves contained view origins while replacing only its own backing origin. |
| Propagation | Local/field/tuple/active tagged projections select facts; constructors prefix them. Array-to-slice and subviews preserve backing origins. String/byte view conversions preserve byte origins; `.bytes()` publications also establish the read-only origin specified below. Value-carrying wrappers, if/match/else, Try/map_err and loop breaks join all reaching values. |
| Local assignment | Reuse existing path-local assignment, generation-content updates and CFG joins. Every reaching read-only alternative survives a join; a definite replacement may install a writable view. Backedges cannot hide a read-only assignment that textually follows a write. |
| Precision and termination | Distinguish a collection's backing from its contained views and ordinary disjoint fields. Dynamic element indices may conservatively join. Recursive type/value paths need a finite, explicitly conservative representation; never use an iteration timeout as a clean verdict. |
| Writes | Check indexed/element-field stores, vector stores, map_into and every existing safe intrinsic destination that writes through a view, plus Out/BorrowMut view arguments. Raw/unsafe operations retain their existing explicit unsafe contract. Preserve each operation's existing mutability, no-alias, range and element-type validation. |
| Diagnostics | A proved read-only destination is a compile error at its source operation, explaining that an owned copy is needed. Report one root error per write; do not publish erroneous HIR for native emission. |
| Calls | Do not infer local return writability from unrelated argument descendants. Explicit writable arguments are checked from their completed values. Ordinary-call result/write obligations are boundary 2; the local fact means known read-only, not proved writable. |
| Ownership/allocation | Facts are compiler-only. No source copy, allocation, source nulling, Drop, runtime ABI or execution order changes. Existing move/region/retention analyses remain authoritative. |
| Transport | Recompute after concrete monomorphization and in checked-HIR body replay. No new HIR discriminator, interface field or version in boundary 1. Whole/per-unit local verdicts must agree. |

## Implementation closure matrix

The owning source integration target is `constants_aggregate`; cheap class-level
owners belong to `align_sema` so the bounded gate detects regressions. Boundary
validation mutations belong to `align_mir`'s existing checked-HIR owners.

| Axis | Implementation obligation and owner |
|---|---|
| Formation and origins | Shared analysis accepts only structurally checked type/path records; `readonly_origin_local_matrix` pairs literal/constant/mapped origins with owned copies and writable array/buffer controls. |
| Construction, projections and siblings | `readonly_origin_projection_matrix` covers nested records, tuples where admitted, Option/Result/sums, array elements, clone-versus-shallow-copy distinction and a writable sibling next to read-only text. Unsupported carrier shapes remain rejected by their existing formation checks. |
| Binding, replacement and control | `readonly_origin_control_matrix` covers let/rebind, field/index stores, both branches, match, else, Try/map_err, loop backedges, break/return and diverging operands. Existing local conservative policy must be stated in expected verdicts. |
| Every write sink | `readonly_origin_sink_matrix` enumerates indexed/field stores, vector store, map_into, native view outputs and explicit Out/BorrowMut calls; the closest owned-storage control must pass at each admitted sink. |
| Move-in/out, nulling, Drop and return | Analysis adds no runtime state; existing lifecycle owners remain authoritative. Projection owners verify ownership of backing remains with the original owner and explicit copying makes an independently writable owner. |
| Generic/imported and cache | `readonly_origin_whole_unit_parity` uses concrete generic and imported declarations with local origins; changing/restoring a constant initializer cannot retain a stale accepting frontend artifact. Ordinary-call laundering is explicitly boundary 2. |
| Malformed input and replay | `readonly_origin_checked_hir_replay` starts from writable accepted HIR, substitutes a read-only producer and expects rejection before lowering. Retain independent structural/type/path validation. |
| Termination/cost | Finite monotone fact domain, work only on changed facts, iterative traversal and recursive-shape controls. No benchmark gate: this capability makes no performance promise. |

The marker/lifetime separation and sink inventory below bind the implementation
and author ledger-to-diff pass. The independent strategy
review covers the boundary and the existing-CFG reuse alternative. A new HIR/wire shape or a
changed ownership strategy reopens this matrix. Boundary 1 crosses roughly 1,000 changed handwritten lines including the closure
matrix and parameterized owners. One shared local proof, its replay activation,
and negative/owned-positive pairs must land together: splitting this boundary
would duplicate validation or publish a producer without its consumers.


## Local representation and sink inventory

Reuse `BorrowFact`, projected storage headers, generation contents and the existing
MoveCheck CFG/worklists rather than introducing a second alias/control solver.
A compiler-only static read-only-origin marker travels in the existing projected
fact domain. It names a storage property, never a releasable owner. It is stable
under generation renaming, is never ended, never acts as an invalidation/alias
root, and is filtered before all return, mutable-retention and parallel-transfer
summary conversion. No marker is serialized.

This is NOT blanket reuse of lifetime-root flattening. Lifetime-only capture
retention and whole-owner fallback must not spread the marker into unrelated
result fields. Marker selection follows the exact typed value projection;
lifetime roots retain their current conservative fallback independently.
Ordinary call-result mapping strips the marker until boundary 2 supplies exact
writability provenance; a borrow-summary parameter union is not that proof.
These requirements apply to named, indirect and synthesized callback calls.
`StorageGenerationRef.erase_readonly` marks lifetime-only call edges, including
nested generation content: deleting just a header's fallback marker would let
its content table reintroduce unrelated constant fields. The bit preserves
lifetime identities, propagates through forwarding/renaming, and adds only one
finite alternative per existing generation/path. It is never serialized.
Call-derived alias/provenance obligations remain boundary 2.

At a view write, inspect the selected backing header, not flattened contained
view lifetimes. A known inline/owned allocation stays writable even when its
stored str elements are read-only. An unknown/view header carrying a read-only
origin rejects. Constant tables and mapped-byte producers publish analysis-only
property headers without local release owners; this preserves their alternative
when joined with a writable generation. EscapeCheck retains its existing
allocation/region classification. Known owned string/buffer byte storage cannot inherit a marker
from unrelated aggregate siblings. Mapped views remain read-only independently
of their arena lifetime. The finite existing origin/header/content domain and
CFG worklists own loop convergence and alias propagation; no new recursive path
expansion, traversal cutoff or numeric widening rule is introduced.

The initial source guard inventory is `check_place`, vector `.store`,
`map_into`, `rng.shuffle`, process `OutBytes`, and named-call `Out` slice checks.
The shared MoveCheck sink inventory must include their HIR forms:
`AssignIndex`, `AssignElemField`, `AssignElem`, `VecStore`, `ArrayMapInto`,
`RandShuffle`, `ProcessLive` inputs classified `OutBytes`, plus explicit
Out/BorrowMut parameters for direct and indirect calls. Owned-only native buffer
receivers do not admit a read-only view and keep their existing type/ownership
checks. Pipeline/intrinsic destination inventory must be checked against every
HIR variant with caller-supplied view output before implementation is complete.

A function result or a closure capture transported into another function is an
interprocedural boundary, even within one source file. Boundary 1 does not
invent a writable guarantee there. Its closest positive/negative pairs use local
origins and exact explicit writable argument modes; boundary 2 owns ordinary
parameter writes, callable captures/results and imported effect/provenance
records. No new immutable-qualified public type is proposed.

## Plan review closure: local retained carriers

The independent review identified a missed local transfer axis: stateful builders
and opaque/materialized containers can retain read-only *payload views* despite
owning fresh writable storage. Extend the same projected fact with typed carrier
content edges; do not label the carrier's owned allocation read-only. This is a
correction to the local closure matrix, not a change of ownership strategy or
an extra public boundary.

| Existing storage initializer / local family | Required transfer and owner |
|---|---|
| FixedLiteral / Aggregate / Forwarded | Constructors prefix selected child facts; field/tuple/tagged/element reads select them. A pooled local array is still writable allocated storage; ConstArray alone marks its backing read-only. `readonly_origin_projection_matrix` covers writable backing containing literal str and independent sibling storage. |
| BuilderElement | ArrayBuilderPush joins the value's fact into the destination builder's element-content edge; Append joins only the source's element facts, not its backing flag. Build transfers those contents to the result's element edge under fresh writable backing. Moves/rebinds retain the same conservative content dependencies. `readonly_origin_carrier_matrix` crosses push/append/build, intervening aliases/moves and loop mutation. Literal and independently cloned string elements both publish read-only bytes; explicit byte materialization supplies the writable controls. |
| JsonDecoded | Borrowed JSON record/array/union/scanner results derive their view-bearing payloads from the input byte origin. New record/array backing is writable; deep owned decode has no input-backed string leaves, but its text still publishes read-only bytes. `readonly_origin_carrier_matrix` covers literal versus owned-input twins, aggregate/array/union projection, and an owned-decoding control with explicit byte materialization for writes. Existing mapped-view source owners cover the independent origin. Scanner materialization is rejected by the existing J5 owner; scalar reductions expose no byte view, and callback escape is boundary 2. |
| JsonDocElements / CarrierSource | JsonDoc carries a typed input-byte-content fact, separate from the document owner's storage. Get/At preserve it in their direct json.doc result (including Missing); AsStr/Key expose it as byte-view origin through OptionSome; Elems gives fresh array backing with borrowed document-content facts per element. The same classifier inventories other opaque carriers that expose retained views. `readonly_origin_carrier_matrix` covers get/at/as_str/key/elems chains and scalar-only reads. |
| PipelineElement / PartitionElement / SoaColumns / GroupAggregation | Existing element-preserving transformations copy/move the element facts into the exact result column/element path, never the input collection's backing flag. Group/dictionary borrowed keys retain their selected key-view origin. Deep owned leaf copies clear only those copied leaf origins. Callback-produced values and callback mutations remain the explicit interprocedural boundary; do not guess their behavior from all argument descendants. `readonly_origin_carrier_matrix` covers stage-free copies, partition/column/group-key and dictionary projection where admitted. |
| CloneIn / FreshEmpty | A copied storage allocation has fresh backing; preserve any shallow-copied view contents. Explicit byte-element copies produce writable array storage. Deep string/owned decoding copies detach source ownership but their text-byte views remain read-only. Buffer copies retain their existing writable-byte contract. Empty construction contributes no read-only content. Closest shallow/deep copy pairs are mandatory. |
| CallSummary / UnknownView / Missing | Ordinary call provenance stays boundary 2. Missing local materializer or retained-carrier semantics is an implementation error to resolve from the exhaustive storage-variant inventory, never an excuse to drop known local origins. Unknown local views retain conservative input-derived read-only content where the existing operation borrows those inputs. |

Builder/dictionary mutation must enqueue every dependent result/projection when
its retained content grows, including a build/get expression visited earlier in
source order on a loop backedge. Use local identity/content dependencies for
mutable opaque carriers and generation-alias evidence where two live bindings
can address the same object. No mutation callback is silently treated as local
when its behavior is actually behind the deferred call-summary boundary.

Author closure: the review's P1 is addressed by the table above and the mandatory
`readonly_origin_carrier_matrix`; the implementation-to-owner pass must audit all
`StorageContentInitializer` and `StorageVariantPolicy` arms against this table.


## Existing-CFG strategy review obligations

The independent alternative-strategy inspection identified the following traps;
these are explicit implementation obligations, not optional precision work:

- `BorrowFact.direct` applies to descendants. Seed backing markers and retained
  payload markers only in their proper typed positions; do not freeze fresh
  collection backing from its elements.
- `storage_roots` field/tuple/index fallbacks conservatively name whole owners.
  Preserve those lifetime roots, but select static origin markers through exact
  projected value facts instead of copying unrelated siblings.
- `borrow_fact_one` adds pipeline/closure capture roots for liveness even when
  the result does not depend on the capture. Exclude static origin markers from
  those lifetime-only edges.
- Filter markers before `exact_borrow_mut_source_indices` and mutable-retention
  export collect into Option, so a marker cannot turn an exact summary into an
  unavailable fallback. Return/parallel summaries and imported replay carry no
  marker either. Ordinary call result mapping does not propagate it.
- Every root invalidation, alias-overlap, generation-end and owner-Drop query
  excludes the non-owning static property. `ended`/`live`/rename operations have
  exhaustive explicit handling; static origins cannot be diagnosed as expired
  owners or create false overlap between unrelated constant views.

`readonly_origin_projection_matrix` must include both orders of readonly and
writable sibling fields and an owned array of readonly str elements whose slots
remain writable. `readonly_origin_carrier_matrix` retains builder/JSON/grouping
coverage. Existing borrowed-retention/alias and summary owners are required
alongside the focused constants owner to detect a static marker leaking into
lifetime behavior. The shared HIR replay reuses the same corrected MoveCheck;
remove the separate syntactic readonly walker as an authority only when replay
covers all its original sinks and malformed counterparts.


## Author implementation closure

- `BorrowRoot::ReadOnly`, `BorrowFact::flatten_lifetimes`, projected/header
  construction and `indexed_generation_content` own local facts. Materializers,
  builders and grouping preserve selected payload paths while keeping allocation
  backing independently writable. The local/projection/carrier matrices cover
  literal and owned twins, both field orders, dynamic record and builder siblings,
  shallow copies, partitions, JSON records/arrays/unions/documents/keys/elements,
  grouping/dictionaries, and deep owned decode.
- Existing MoveCheck CFG joins, generation renaming, assignments and completed
  snapshots own replacement and backedges. The local/control matrices cover
  field replacement, later loop reassignment, conditional alternatives, match,
  value-carrying break and transparent-block stores. Carrier Option and
  Result/map_err/Try cases share the same projected transfer. Existing
  `return_provenance` control/cleanup owners cover early exits and diverging
  operands without adding a parallel control solver.
- `update_mutable_collection_contents`, `apply_mutable_call_effects` and
  `apply_builtin_mutation_action` cover ordinary and transparent worklists after
  operands complete. The sink matrix pairs all admitted scalar write forms with
  owned storage, including SIMD, map_into, shuffle, direct/indirect writable
  parameters and native process output. SoA stores share the indexed collection
  action; their owned backing remains writable while contained text is tracked.
- `readonly_origin_lifetime_separation` pins non-overlap, non-expiration,
  exact empty mutable-retention export, and ordinary call selection over both
  plain and generation-backed disjoint siblings. `borrowed_params`,
  `return_provenance` and `imported_mutable_retention` retain ownership authority.
- `readonly_origin_whole_unit_parity` covers imported constant initializers and
  concrete generic writable-argument checks in both frontends. No interface
  field or cache identity changed; existing constant/interface invalidation
  owners remain applicable. `readonly_origin_checked_hir_replay` independently
  mutates accepted HIR, pairs a rejected write with an accepted read, and proves
  structural validation still succeeds before the shared body proof rejects.
- No runtime, ABI, allocation, cleanup or source-evaluation-order changes. The
  removed syntactic source/HIR walkers have one shared replacement. Negative
  witnesses are compile-only; existing owned-storage runtime owners remain the
  execution controls. The finite generation/path domain and existing deep CFG
  owners remain the termination proof, with no numeric analysis cutoff.


## Review finding closure

The first full code review identified two local transfer corrections under the
existing strategy. Both are covered by the revised owner matrices:

- Fixed-to-slice and range conversions retain the allocation's fixed
  `ArrayElement` content paths. `collection_generation_content` selects from
  the generation descriptor as well as the receiver type; a view with an unknown
  offset joins backing slots while preserving field paths. Index reads,
  materialization and builder append share that normalization.
  `readonly_origin_view_conversion_matrix` crosses fixed/dynamic storage,
  full/offset views and direct/copy/builder consumers with read-only text views
  and explicit owned-byte-copy twins.
  Checked-HIR replay includes the same fixed-to-slice producer mutation.
- Reduce/scan callback results do not inherit a source element's read-only
  property from a lifetime union. An empty reduce can return its initial
  accumulator directly, so that local alternative remains. Scan emits only
  post-callback values. `readonly_origin_accumulator_matrix` covers both
  terminals, projected sources, writable initial values and the read-only
  empty-reduce initial value. Initial field paths apply only to static properties:
  callbacks may swap fields, so lifetime roots still flatten conservatively.
  The accumulator owner pins the exported two-parameter lifetime union after a
  swapping reducer. Callback writability provenance remains boundary 2.

The view-conversion owner also rejects writes through `as_str().bytes()` after
either literal bytes or an owned byte copy. An explicit `.to_array()` after that
text publication supplies the independently writable twin. `storage_roots`
obtains a collection view's static backing property from its selected header,
independently of retained element origins. Owned array backing stays writable
without erasing read-only text payload paths.


## Static descriptor origin closure

Compiler-owned descriptor ID, SQLite SQL and PostgreSQL SQL views are static
read-only storage. `StaticDescriptorView` must seed the same local read-only
origin as a literal, independently of its raw descriptor pointer. The trusted
bridge's unsafe pointer access does not authorize an ordinary safe indexed
write to that storage. This closes a local producer omission; ordinary function
result laundering remains boundary 2.

| Axis | Implementation / owner |
|---|---|
| Formation / all producers | Preserve descriptor shape, offset and trusted-origin validation. `readonly_static_descriptor_origin_matrix` covers offsets 16/32/48 through the actual three descriptor operations. |
| Projection / replacement / writable copies | Reuse the local projected-fact engine. The owner crosses direct, record and slice views, readonly writes, readers and explicit owned copies. No new allocation or copy is inserted. |
| Generic / imported / whole-per-unit | Concrete generic bridge bodies retain the origin after instantiation; `readonly_static_descriptor_whole_unit_parity` checks the three operations through both frontends. |
| Malformed HIR / replay | `readonly_static_descriptor_checked_hir_replay` substitutes bytes from a checked StaticDescriptorView for a writable slice input while keeping its valid descriptor pointer/offset. Structural validation must still pass and body replay must reject only the write. |
| Control / ownership / ABI | Existing local control, carrier, lifetime-separation and call-summary owners remain authoritative. No IR shape, runtime operation, lifetime summary, interface byte or ABI changes. |

The pre-fix compiler accepts the three static-view write witnesses. Keep them
compile-only, with no generated executable run. The source/replay owner must
fail when the new origin seed is removed. This local closure is independently
useful and does not claim the deferred interprocedural proof.

## Read-only text-byte closure

`draft.md` §12 requires `str` and owned text to remain valid UTF-8. Rebinding
`text.bytes()` as a mutable slice cannot authorize changing that text's bytes.
The compile-only owned-string witness at `2e2d8d51` accepts a write of 255 in
both frontends. It must never execute. Explicit `.bytes().to_array()` remains a
writable owned copy. Text returned by an ordinary function is subject to the
same rule when its `.bytes()` conversion occurs locally.

This closure covers `StrBytes`, independently of the text's literal, owned,
borrowed, decoded or native-getter origin. It preserves the operation's existing
`UnknownView` classification and source lifetime roots. `borrow_sources_inner`
adds `BorrowRoot::ReadOnly`; `form_storage_completion` retains those roots in
the selected typed fallback header. The ownerless constant/mapped-view shortcut
would incorrectly discard text-owner lifetimes. Existing projections, completed
operands, replacement and CFG joins preserve the marker. Primitive materializing
copies detach their byte contents from the original text.

The native byte getters are a different contract. In particular, plan 37 and
Request 61 require writable buffer views for a borrowed numeric-stream owner
without an added allocation or copy. Do not infer that these getters share the
text invariant, or substitute copying for that accepted capability. Their
existing native/owner contracts remain unchanged by this repair.

| Axis | Implementation / owner |
| --- | --- |
| Formation / origins | `StrBytes` adds the marker while retaining every source root. `readonly_text_bytes_matrix` crosses local/field/range/sibling projections, reads, rejected writes and explicit copies. No type, HIR, MIR or ABI change. |
| Safe destinations | `readonly_origin_sink_matrix` covers owned text and copied-byte twins at indexed stores, Out, direct/indirect BorrowMut, SIMD store, shuffle, native OutBytes and map_into. Existing type, mutability, range and no-alias checks remain authoritative. |
| Backing versus contents | Local/carrier/projection/conversion owners distinguish cloning text from materializing writable byte elements. Descriptor-slot and disjoint raw-byte sibling writes remain valid; reduce/scan and lifetime-summary controls retain that distinction. |
| Replacement / control | `readonly_origin_control_matrix` covers literal/owned-text origins through existing branches, Try/map_err, loops and completed operands. |
| Ownership / source expiry | `readonly_text_bytes_matrix` retains owned-text escape and replacement rejection with independent copy controls. `copied_text_bytes_are_writable_after_source_expiry` executes copied-byte writes after text expiry in both frontends. Existing m5 str_bytes owners retain zero-copy reads and lifetime rejection. No runtime allocation, source nulling, Drop or replacement behavior changes. |
| Generic / imported / whole / per-unit | `readonly_text_bytes_whole_unit_parity` checks local/imported concrete generic bodies, reads, writes and explicit copies. No interface/cache field changes. |
| Checked HIR | `readonly_text_bytes_checked_hir_replay` substitutes borrowed/owned text byte producers for a writable slice input. Structural validity must pass before shared body-fact replay rejects the write. Existing projected and static-descriptor replay owners remain covered. |
| Native contract separation | Existing `struct_handle_fields::borrowed_handle_receivers_preserve_nested_and_optional_owners` and `consumer_borrow_boundaries::derived_view_mutation_preserves_disjoint_owner_facts` remain unchanged and must pass: text protection must not freeze native buffer views or remove their allocation-free mutation path. |
| Cost / deferred boundaries | Reuse finite projected facts and the existing worklist; no new analysis, iteration bound or performance promise. Ordinary slice argument/result writability is boundary 2. |

A writable byte allocation validated by `as_str()` has a distinct observation
invariant: a later write through an older byte alias must expire the validated
text while leaving ordinary byte aliases usable. That separate failure domain
is not closed by marking subsequent `StrBytes` views read-only; its local
observation repair is specified in [plan 57](57-validated-text-observation-plan.md).
Hidden callee validation/write effects remain interprocedural work.
